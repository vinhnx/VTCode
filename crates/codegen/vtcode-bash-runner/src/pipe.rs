//! Async pipe-based process spawning with unified handle interface.
//!
//! This module provides helpers for spawning non-interactive processes using
//! regular pipes for stdin/stdout/stderr, with proper process group management
//! for reliable cleanup.
//!
//! Inspired by codex-rs/utils/pty pipe spawning patterns.

use hashbrown::HashMap;
use std::io::{self, ErrorKind};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};
use bytes::{Bytes, BytesMut};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::process::{ChildTerminator, ProcessHandle, SpawnedProcess};
use crate::process_group;

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

const RELIABLE_OUTPUT_CHANNEL_CAPACITY: usize = 128;

/// Read from an async reader and send chunks to both compatibility and
/// optional lossless output channels. The compatibility broadcast is always
/// independent: callers that only use the legacy receiver must never be able
/// to stop the child-process pipe from being drained.
async fn read_output_stream<R>(
    mut reader: R,
    output_tx: broadcast::Sender<Bytes>,
    mut reliable_output_tx: Option<mpsc::Sender<Bytes>>,
) where
    R: AsyncRead + Unpin,
{
    // Read directly into a BytesMut so each chunk can be frozen without a
    // separate Vec-to-Bytes copy.
    let mut buf = BytesMut::with_capacity(65_536);
    loop {
        buf.clear();
        match reader.read_buf(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                let chunk = buf.split_to(n).freeze();
                let _ = output_tx.send(chunk.clone());
                if let Some(sender) = reliable_output_tx.as_ref().cloned()
                    && sender.send(chunk).await.is_err()
                {
                    // A lossless consumer is optional. Once it goes away,
                    // continue draining the child stream for compatibility
                    // subscribers instead of deadlocking the child.
                    reliable_output_tx = None;
                }
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
#[must_use = "spawn options must be passed to a spawn function"]
pub struct PipeSpawnOptions {
    /// The program to execute.
    program: String,
    /// Arguments to pass to the program.
    args: Vec<String>,
    /// Working directory for the process.
    cwd: std::path::PathBuf,
    /// Environment variables (if None, inherits from parent).
    env: Option<HashMap<String, String>>,
    /// Override for `argv[0]` (Unix only).
    arg0: Option<String>,
    /// Stdin mode.
    stdin_mode: PipeStdinMode,
    /// Enable the bounded lossless stream used by spoolers and
    /// `wait_with_output`. Legacy callers should leave this disabled so an
    /// unconsumed reliable receiver cannot apply backpressure.
    lossless_output: bool,
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
            lossless_output: false,
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

    /// Enable the lossless output stream for a consumer that drains it.
    pub fn lossless_output(mut self, enabled: bool) -> Self {
        self.lossless_output = enabled;
        self
    }
}

/// Spawn a process using regular pipes, with configurable options.
async fn spawn_process_internal(opts: PipeSpawnOptions) -> Result<SpawnedProcess> {
    if opts.program.is_empty() {
        anyhow::bail!("missing program for pipe spawn");
    }

    #[cfg(unix)]
    let owned = crate::process_owner::OwnedCommand::from_environment(
        opts.program.as_ref(),
        opts.args.iter(),
        opts.arg0.as_deref().map(std::ffi::OsStr::new),
    )?;

    #[cfg(unix)]
    let mut command = match &owned {
        Some(owned) => {
            let mut command = Command::new(&owned.executable);
            command.args(&owned.arguments);
            command
        }
        None => Command::new(&opts.program),
    };
    #[cfg(not(unix))]
    let mut command = Command::new(&opts.program);

    #[cfg(unix)]
    if owned.is_none()
        && let Some(ref arg0) = opts.arg0
    {
        command.arg0(arg0);
    }

    #[cfg(unix)]
    #[expect(
        unsafe_code,
        reason = "detach_from_tty only calls setsid/setpgid via safe nix wrappers to detach the child from the controlling terminal; it is a pure process-group operation with no undefined behavior"
    )]
    // SAFETY: `pre_exec` runs the closure in the forked child before `exec`,
    // so only async-signal-safe operations are permitted. `detach_from_tty`
    // calls `setsid()` (falling back to `setpgid(0, 0)` on EPERM) through safe
    // `nix` wrappers — both are async-signal-safe POSIX calls that perform no
    // allocation, no locking, and no mutable aliasing of process memory.
    unsafe {
        command.pre_exec(process_group::detach_from_tty);
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

    #[cfg(unix)]
    if let Some(owned) = &owned {
        // Environment filtering remains authoritative; add only this private
        // lifetime endpoint after applying the admitted command's environment.
        command.env(crate::process_owner::OWNER_SOCKET_ENV, &owned.owner_socket);
    } else {
        command.args(&opts.args);
    }
    #[cfg(not(unix))]
    command.args(&opts.args);

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
    let pid = child.id().ok_or_else(|| io::Error::other("missing child pid"))?;

    #[cfg(unix)]
    let process_group_id = pid;

    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let (writer_tx, mut writer_rx) = mpsc::channel::<Vec<u8>>(128);
    let (output_tx, _) = broadcast::channel::<Bytes>(256);
    let initial_output_rx = output_tx.subscribe();
    let (reliable_output_tx, reliable_output_rx) = mpsc::channel::<Bytes>(RELIABLE_OUTPUT_CHANNEL_CAPACITY);
    let reliable_output_enabled = opts.lossless_output;

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
        let reliable_output_tx = opts.lossless_output.then(|| reliable_output_tx.clone());
        tokio::spawn(async move {
            read_output_stream(BufReader::new(stdout), output_tx, reliable_output_tx).await;
        })
    });

    let stderr_handle = stderr.map(|stderr| {
        let output_tx = output_tx.clone();
        let reliable_output_tx = opts.lossless_output.then(|| reliable_output_tx.clone());
        tokio::spawn(async move {
            read_output_stream(BufReader::new(stderr), output_tx, reliable_output_tx).await;
        })
    });
    drop(reliable_output_tx);

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
        process_id: pid,
        output_rx,
        reliable_output_rx,
        reliable_output_enabled,
        exit_rx,
    })
}

/// Spawn a process using regular pipes (no PTY), returning handles for stdin, output, and exit.
///
/// Copies the borrowed arguments, environment, and optional `arg0` into owned
/// spawn options. `arg0` overrides Unix `argv[0]`; other platforms ignore it.
///
/// # Errors
/// Returns launch or pipe setup failures with process context.
///
/// # Example
/// ```ignore
/// use vtcode_bash_runner::pipe::spawn_process;
/// use hashbrown::HashMap;
/// use std::path::Path;
///
/// let env: HashMap<String, String> = std::env::vars().collect();
/// let spawned = spawn_process("echo", &["hello".into()], Path::new("."), &env, None).await?;
/// let output_rx = spawned.output_rx;
/// let exit_code = spawned.exit_rx.await?;
/// ```
pub async fn spawn_process(
    program: &str,
    args: &[String],
    cwd: &Path,
    env: &HashMap<String, String>,
    arg0: Option<&str>,
) -> Result<SpawnedProcess> {
    let opts = PipeSpawnOptions {
        program: program.to_string(),
        args: args.to_vec(),
        cwd: cwd.to_path_buf(),
        env: Some(env.clone()),
        arg0: arg0.map(str::to_owned),
        stdin_mode: PipeStdinMode::Piped,
        lossless_output: false,
    };
    spawn_process_internal(opts).await
}

/// Spawn a process using regular pipes, but close stdin immediately.
///
/// This is useful for commands that should see EOF on stdin immediately.
/// Borrowed inputs and Unix `arg0` have the same semantics as [`spawn_process`].
///
/// # Errors
/// Returns launch or pipe setup failures with process context.
pub async fn spawn_process_no_stdin(
    program: &str,
    args: &[String],
    cwd: &Path,
    env: &HashMap<String, String>,
    arg0: Option<&str>,
) -> Result<SpawnedProcess> {
    let opts = PipeSpawnOptions {
        program: program.to_string(),
        args: args.to_vec(),
        cwd: cwd.to_path_buf(),
        env: Some(env.clone()),
        arg0: arg0.map(str::to_owned),
        stdin_mode: PipeStdinMode::Null,
        lossless_output: false,
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
    use assert_fs::TempDir;

    fn find_echo_command() -> Option<(String, Vec<String>)> {
        #[cfg(windows)]
        {
            Some(("cmd.exe".to_string(), vec!["/C".to_string(), "echo".to_string()]))
        }
        #[cfg(not(windows))]
        {
            Some(("echo".to_string(), vec![]))
        }
    }

    #[tokio::test]
    async fn test_spawn_process_echo() -> Result<()> {
        let Some((program, mut base_args)) = find_echo_command() else {
            return Ok(());
        };

        base_args.push("hello".to_string());

        let env: HashMap<String, String> = std::env::vars().collect();
        let spawned = spawn_process(&program, &base_args, Path::new("."), &env, None).await?;

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

    #[cfg(unix)]
    #[tokio::test]
    async fn test_spawn_process_detaches_from_tty() {
        let dir = TempDir::new().expect("tempdir");
        let env: HashMap<String, String> = HashMap::new();

        let spawned = spawn_process("sh", &["-c".into(), "echo ok".into()], dir.path(), &env, None)
            .await
            .expect("spawn");

        let exit_code = spawned.exit_rx.await.unwrap_or(-1);
        assert_eq!(exit_code, 0);
    }
}
