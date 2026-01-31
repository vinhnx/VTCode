//! Async pipe-based process spawning with unified handle interface.
//!
//! This module provides helpers for spawning non-interactive processes using
//! regular pipes for stdin/stdout/stderr, with proper process group management
//! for reliable cleanup.
//!
//! Inspired by codex-rs/utils/pty pipe spawning patterns.

use std::collections::HashMap;
use std::io::{self, ErrorKind};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::process::{ChildTerminator, ProcessHandle, SpawnedProcess};
use crate::process_group;

#[cfg(target_os = "linux")]
use libc;

/// Terminator for pipe-based child processes.
struct PipeChildTerminator {
    #[cfg(windows)]
    pid: u32,
    #[cfg(unix)]
    process_group_id: u32,
}

impl ChildTerminator for PipeChildTerminator {
    fn kill(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            process_group::kill_process_group(self.process_group_id)
        }

        #[cfg(windows)]
        {
            process_group::kill_process(self.pid)
        }

        #[cfg(not(any(unix, windows)))]
        {
            Ok(())
        }
    }
}

/// Read from an async reader and send chunks to a broadcast channel.
async fn read_output_stream<R>(mut reader: R, output_tx: broadcast::Sender<Vec<u8>>)
where
    R: AsyncRead + Unpin,
{
    let mut buf = vec![0u8; 8_192];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                let _ = output_tx.send(buf[..n].to_vec());
            }
            Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

/// Stdin mode for pipe-based processes.
#[derive(Clone, Copy)]
pub enum PipeStdinMode {
    /// Stdin is available as a pipe.
    Piped,
    /// Stdin is connected to /dev/null (immediate EOF).
    Null,
}

/// Options for spawning a pipe-based process.
#[derive(Clone)]
pub struct PipeSpawnOptions {
    /// The program to execute.
    pub program: String,
    /// Arguments to pass to the program.
    pub args: Vec<String>,
    /// Working directory for the process.
    pub cwd: std::path::PathBuf,
    /// Environment variables (if None, inherits from parent).
    pub env: Option<HashMap<String, String>>,
    /// Override for argv[0] (Unix only).
    pub arg0: Option<String>,
    /// Stdin mode.
    pub stdin_mode: PipeStdinMode,
}

impl PipeSpawnOptions {
    /// Create new spawn options with default settings.
    pub fn new(program: impl Into<String>, cwd: impl Into<std::path::PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: cwd.into(),
            env: None,
            arg0: None,
            stdin_mode: PipeStdinMode::Piped,
        }
    }

    /// Add arguments.
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Set environment variables.
    pub fn env(mut self, env: HashMap<String, String>) -> Self {
        self.env = Some(env);
        self
    }

    /// Set arg0 override (Unix only).
    pub fn arg0(mut self, arg0: impl Into<String>) -> Self {
        self.arg0 = Some(arg0.into());
        self
    }

    /// Set stdin mode.
    pub fn stdin_mode(mut self, mode: PipeStdinMode) -> Self {
        self.stdin_mode = mode;
        self
    }
}

/// Spawn a process using regular pipes, with configurable options.
async fn spawn_process_internal(opts: PipeSpawnOptions) -> Result<SpawnedProcess> {
    if opts.program.is_empty() {
        anyhow::bail!("missing program for pipe spawn");
    }

    let mut command = Command::new(&opts.program);

    #[cfg(unix)]
    if let Some(ref arg0) = opts.arg0 {
        command.arg0(arg0);
    }

    #[cfg(target_os = "linux")]
    let parent_pid = unsafe { libc::getpid() };

    #[cfg(unix)]
    unsafe {
        command.pre_exec(move || {
            process_group::detach_from_tty()?;
            #[cfg(target_os = "linux")]
            process_group::set_parent_death_signal(parent_pid)?;
            Ok(())
        });
    }

    #[cfg(not(unix))]
    let _ = &opts.arg0;

    command.current_dir(&opts.cwd);

    // Handle environment
    if let Some(ref env) = opts.env {
        command.env_clear();
        for (key, value) in env {
            command.env(key, value);
        }
    }

    for arg in &opts.args {
        command.arg(arg);
    }

    match opts.stdin_mode {
        PipeStdinMode::Piped => {
            command.stdin(Stdio::piped());
        }
        PipeStdinMode::Null => {
            command.stdin(Stdio::null());
        }
    }
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command.spawn().context("failed to spawn pipe process")?;
    let pid = child
        .id()
        .ok_or_else(|| io::Error::other("missing child pid"))?;

    #[cfg(unix)]
    let process_group_id = pid;

    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let (writer_tx, mut writer_rx) = mpsc::channel::<Vec<u8>>(128);
    let (output_tx, _) = broadcast::channel::<Vec<u8>>(256);
    let initial_output_rx = output_tx.subscribe();

    // Spawn writer task
    let writer_handle = if let Some(stdin) = stdin {
        let writer = Arc::new(tokio::sync::Mutex::new(stdin));
        tokio::spawn(async move {
            while let Some(bytes) = writer_rx.recv().await {
                let mut guard = writer.lock().await;
                let _ = guard.write_all(&bytes).await;
                let _ = guard.flush().await;
            }
        })
    } else {
        drop(writer_rx);
        tokio::spawn(async {})
    };

    // Spawn reader tasks for stdout and stderr
    let stdout_handle = stdout.map(|stdout| {
        let output_tx = output_tx.clone();
        tokio::spawn(async move {
            read_output_stream(BufReader::new(stdout), output_tx).await;
        })
    });

    let stderr_handle = stderr.map(|stderr| {
        let output_tx = output_tx.clone();
        tokio::spawn(async move {
            read_output_stream(BufReader::new(stderr), output_tx).await;
        })
    });

    let mut reader_abort_handles = Vec::new();
    if let Some(ref handle) = stdout_handle {
        reader_abort_handles.push(handle.abort_handle());
    }
    if let Some(ref handle) = stderr_handle {
        reader_abort_handles.push(handle.abort_handle());
    }

    let reader_handle = tokio::spawn(async move {
        if let Some(handle) = stdout_handle {
            let _ = handle.await;
        }
        if let Some(handle) = stderr_handle {
            let _ = handle.await;
        }
    });

    // Spawn wait task
    let (exit_tx, exit_rx) = oneshot::channel::<i32>();
    let exit_status = Arc::new(AtomicBool::new(false));
    let wait_exit_status = Arc::clone(&exit_status);
    let exit_code = Arc::new(StdMutex::new(None));
    let wait_exit_code = Arc::clone(&exit_code);

    let wait_handle: JoinHandle<()> = tokio::spawn(async move {
        let code = match child.wait().await {
            Ok(status) => status.code().unwrap_or(-1),
            Err(_) => -1,
        };
        wait_exit_status.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut guard) = wait_exit_code.lock() {
            *guard = Some(code);
        }
        let _ = exit_tx.send(code);
    });

    let (handle, output_rx) = ProcessHandle::new(
        writer_tx,
        output_tx,
        initial_output_rx,
        Box::new(PipeChildTerminator {
            #[cfg(windows)]
            pid,
            #[cfg(unix)]
            process_group_id,
        }),
        reader_handle,
        reader_abort_handles,
        writer_handle,
        wait_handle,
        exit_status,
        exit_code,
        None,
    );

    Ok(SpawnedProcess {
        session: handle,
        output_rx,
        exit_rx,
    })
}

/// Spawn a process using regular pipes (no PTY), returning handles for stdin, output, and exit.
///
/// # Example
/// ```ignore
/// use vtcode_bash_runner::pipe::spawn_process;
/// use std::collections::HashMap;
/// use std::path::Path;
///
/// let env: HashMap<String, String> = std::env::vars().collect();
/// let spawned = spawn_process("echo", &["hello".into()], Path::new("."), &env, &None).await?;
/// let output_rx = spawned.output_rx;
/// let exit_code = spawned.exit_rx.await?;
/// ```
pub async fn spawn_process(
    program: &str,
    args: &[String],
    cwd: &Path,
    env: &HashMap<String, String>,
    arg0: &Option<String>,
) -> Result<SpawnedProcess> {
    let opts = PipeSpawnOptions {
        program: program.to_string(),
        args: args.to_vec(),
        cwd: cwd.to_path_buf(),
        env: Some(env.clone()),
        arg0: arg0.clone(),
        stdin_mode: PipeStdinMode::Piped,
    };
    spawn_process_internal(opts).await
}

/// Spawn a process using regular pipes, but close stdin immediately.
///
/// This is useful for commands that should see EOF on stdin immediately.
pub async fn spawn_process_no_stdin(
    program: &str,
    args: &[String],
    cwd: &Path,
    env: &HashMap<String, String>,
    arg0: &Option<String>,
) -> Result<SpawnedProcess> {
    let opts = PipeSpawnOptions {
        program: program.to_string(),
        args: args.to_vec(),
        cwd: cwd.to_path_buf(),
        env: Some(env.clone()),
        arg0: arg0.clone(),
        stdin_mode: PipeStdinMode::Null,
    };
    spawn_process_internal(opts).await
}

/// Spawn a process with full options control.
pub async fn spawn_process_with_options(opts: PipeSpawnOptions) -> Result<SpawnedProcess> {
    spawn_process_internal(opts).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_echo_command() -> Option<(String, Vec<String>)> {
        #[cfg(windows)]
        {
            Some((
                "cmd.exe".to_string(),
                vec!["/C".to_string(), "echo".to_string()],
            ))
        }
        #[cfg(not(windows))]
        {
            Some(("echo".to_string(), vec![]))
        }
    }

    #[tokio::test]
    async fn test_spawn_process_echo() -> anyhow::Result<()> {
        let Some((program, mut base_args)) = find_echo_command() else {
            return Ok(());
        };

        base_args.push("hello".to_string());

        let env: HashMap<String, String> = std::env::vars().collect();
        let spawned = spawn_process(&program, &base_args, Path::new("."), &env, &None).await?;

        let exit_code = spawned.exit_rx.await.unwrap_or(-1);
        assert_eq!(exit_code, 0);

        Ok(())
    }

    #[tokio::test]
    async fn test_spawn_options_builder() {
        let opts = PipeSpawnOptions::new("echo", ".")
            .args(["hello", "world"])
            .stdin_mode(PipeStdinMode::Null);

        assert_eq!(opts.program, "echo");
        assert_eq!(opts.args, vec!["hello", "world"]);
        assert!(matches!(opts.stdin_mode, PipeStdinMode::Null));
    }
}
