use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use rexpect::{
    process::wait::WaitStatus,
    session::{Options, spawn_with_options},
};
use tracing::{debug, warn};
use tui_term::vt100::Parser;

use crate::config::PtyConfig;
use crate::tools::types::VTCodePtySession;

#[derive(Clone)]
pub struct PtyManager {
    workspace_root: PathBuf,
    config: PtyConfig,
    inner: Arc<PtyState>,
}

#[derive(Default)]
struct PtyState {
    sessions: Mutex<HashMap<String, PtySessionHandle>>,
}

struct PtySessionHandle {
    master: Box<dyn MasterPty + Send>,
    child: Mutex<Box<dyn Child + Send>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    parser: Arc<Mutex<Parser>>,
    reader_thread: Mutex<Option<JoinHandle<()>>>,
    metadata: VTCodePtySession,
}

impl PtySessionHandle {
    fn snapshot_metadata(&self) -> VTCodePtySession {
        let mut metadata = self.metadata.clone();
        if let Ok(size) = self.master.get_size() {
            metadata.rows = size.rows;
            metadata.cols = size.cols;
        }
        if let Ok(parser) = self.parser.lock() {
            metadata.screen_contents = Some(parser.screen().contents());
        }
        metadata
    }
}

pub struct PtyCommandRequest {
    pub command: Vec<String>,
    pub working_dir: PathBuf,
    pub timeout: Duration,
    pub size: PtySize,
}

pub struct PtyCommandResult {
    pub exit_code: i32,
    pub output: String,
    pub duration: Duration,
    pub size: PtySize,
}

impl PtyManager {
    pub fn new(workspace_root: PathBuf, config: PtyConfig) -> Self {
        let resolved_root = workspace_root
            .canonicalize()
            .unwrap_or(workspace_root.clone());

        Self {
            workspace_root: resolved_root,
            config,
            inner: Arc::new(PtyState::default()),
        }
    }

    pub fn config(&self) -> &PtyConfig {
        &self.config
    }

    pub fn describe_working_dir(&self, path: &Path) -> String {
        self.format_working_dir(path)
    }

    pub async fn run_command(&self, request: PtyCommandRequest) -> Result<PtyCommandResult> {
        if request.command.is_empty() {
            return Err(anyhow!("PTY command cannot be empty"));
        }

        let mut command = request.command.clone();
        let program = command.remove(0);
        let args = command;
        let timeout = clamp_timeout(request.timeout);
        let work_dir = request.working_dir.clone();
        let size = request.size;
        let start = Instant::now();

        let result = tokio::task::spawn_blocking(move || -> Result<PtyCommandResult> {
            let mut cmd = std::process::Command::new(&program);
            cmd.args(&args);
            cmd.current_dir(&work_dir);
            cmd.env("TERM", "xterm-256color");
            cmd.env("COLUMNS", size.cols.to_string());
            cmd.env("LINES", size.rows.to_string());

            let options = Options {
                timeout_ms: Some(timeout),
                strip_ansi_escape_codes: false,
            };

            let mut session = spawn_with_options(cmd, options)
                .with_context(|| format!("failed to spawn PTY command '{program}'"))?;

            let mut output = String::new();
            let collected = session
                .exp_eof()
                .context("failed to read PTY command output")?;
            output.push_str(&collected);

            let status = session
                .process
                .wait()
                .context("failed to wait for PTY command to exit")?;
            let exit_code = wait_status_code(status);

            Ok(PtyCommandResult {
                exit_code,
                output,
                duration: start.elapsed(),
                size,
            })
        })
        .await
        .context("failed to join PTY command task")??;

        Ok(result)
    }

    pub fn resolve_working_dir(&self, requested: Option<&str>) -> Result<PathBuf> {
        let requested = match requested {
            Some(dir) if !dir.trim().is_empty() => dir,
            _ => return Ok(self.workspace_root.clone()),
        };

        let candidate = self.workspace_root.join(requested);
        let normalized = normalize_path(&candidate);
        if !normalized.starts_with(&self.workspace_root) {
            return Err(anyhow!(
                "Working directory '{}' escapes the workspace root",
                candidate.display()
            ));
        }
        let metadata = fs::metadata(&normalized).with_context(|| {
            format!(
                "Working directory '{}' does not exist",
                normalized.display()
            )
        })?;
        if !metadata.is_dir() {
            return Err(anyhow!(
                "Working directory '{}' is not a directory",
                normalized.display()
            ));
        }
        Ok(normalized)
    }

    pub fn create_session(
        &self,
        session_id: String,
        command: Vec<String>,
        working_dir: PathBuf,
        size: PtySize,
    ) -> Result<VTCodePtySession> {
        if command.is_empty() {
            return Err(anyhow!("PTY session command cannot be empty"));
        }

        let mut sessions = self
            .inner
            .sessions
            .lock()
            .expect("PTY session mutex poisoned");
        if sessions.contains_key(&session_id) {
            return Err(anyhow!("PTY session '{}' already exists", session_id));
        }

        let mut command_parts = command.clone();
        let program = command_parts.remove(0);
        let args = command_parts;

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(size)
            .context("failed to allocate PTY pair")?;

        let mut builder = CommandBuilder::new(program.clone());
        for arg in &args {
            builder.arg(arg);
        }
        builder.cwd(&working_dir);
        builder.env("TERM", "xterm-256color");
        builder.env("COLUMNS", size.cols.to_string());
        builder.env("LINES", size.rows.to_string());

        let child = pair
            .slave
            .spawn_command(builder)
            .context("failed to spawn PTY session command")?;
        drop(pair.slave);

        let master = pair.master;
        let mut reader = master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = master
            .take_writer()
            .context("failed to take PTY writer")?;

        let parser = Arc::new(Mutex::new(Parser::new(size.rows, size.cols, 0)));
        let parser_clone = Arc::clone(&parser);
        let session_name = session_id.clone();
        let reader_thread = thread::Builder::new()
            .name(format!("vtcode-pty-reader-{session_name}"))
            .spawn(move || {
                let mut buffer = [0u8; 4096];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => {
                            debug!("PTY session '{}' reader reached EOF", session_name);
                            break;
                        }
                        Ok(bytes_read) => {
                            if let Ok(mut parser) = parser_clone.lock() {
                                parser.process(&buffer[..bytes_read]);
                            }
                        }
                        Err(error) => {
                            warn!("PTY session '{}' reader error: {}", session_name, error);
                            break;
                        }
                    }
                }
            })
            .context("failed to spawn PTY reader thread")?;

        let metadata = VTCodePtySession {
            id: session_id.clone(),
            command: program,
            args,
            working_dir: Some(self.format_working_dir(&working_dir)),
            rows: size.rows,
            cols: size.cols,
            screen_contents: None,
        };

        sessions.insert(
            session_id.clone(),
            PtySessionHandle {
                master,
                child: Mutex::new(child),
                writer: Mutex::new(Some(writer)),
                parser,
                reader_thread: Mutex::new(Some(reader_thread)),
                metadata: metadata.clone(),
            },
        );

        Ok(metadata)
    }

    pub fn list_sessions(&self) -> Vec<VTCodePtySession> {
        let sessions = self
            .inner
            .sessions
            .lock()
            .expect("PTY session mutex poisoned");
        sessions
            .values()
            .map(PtySessionHandle::snapshot_metadata)
            .collect()
    }

    pub fn close_session(&self, session_id: &str) -> Result<VTCodePtySession> {
        let handle = {
            let mut sessions = self
                .inner
                .sessions
                .lock()
                .expect("PTY session mutex poisoned");
            sessions
                .remove(session_id)
                .ok_or_else(|| anyhow!("PTY session '{}' not found", session_id))?
        };

        if let Ok(mut writer_guard) = handle.writer.lock() {
            if let Some(mut writer) = writer_guard.take() {
                let _ = writer.write_all(b"exit\n");
                let _ = writer.flush();
            }
        }

        let mut child = handle.child.lock().expect("PTY child mutex poisoned");
        if child
            .try_wait()
            .context("failed to poll PTY session status")?
            .is_none()
        {
            child.kill().context("failed to terminate PTY session")?;
            let _ = child.wait();
        }

        if let Ok(mut thread_guard) = handle.reader_thread.lock() {
            if let Some(reader_thread) = thread_guard.take() {
                if let Err(panic) = reader_thread.join() {
                    warn!(
                        "PTY session '{}' reader thread panicked: {:?}",
                        session_id, panic
                    );
                }
            }
        }

        Ok(handle.snapshot_metadata())
    }

    fn format_working_dir(&self, path: &Path) -> String {
        match path.strip_prefix(&self.workspace_root) {
            Ok(relative) if relative.as_os_str().is_empty() => ".".to_string(),
            Ok(relative) => relative.to_string_lossy().replace("\\", "/"),
            Err(_) => path.to_string_lossy().to_string(),
        }
    }
}

fn clamp_timeout(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

fn wait_status_code(status: WaitStatus) -> i32 {
    match status {
        WaitStatus::Exited(_, code) => code,
        WaitStatus::Signaled(_, signal, _) | WaitStatus::Stopped(_, signal) => -(signal as i32),
        WaitStatus::StillAlive
        | WaitStatus::Continued(_)
        | WaitStatus::PtraceEvent(_, _, _)
        | WaitStatus::PtraceSyscall(_) => 0,
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}
