use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use avt::Vt;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use shell_words::join;
use tracing::{debug, warn};

use crate::config::PtyConfig;
use crate::sandbox::SandboxProfile;
use crate::tools::types::VTCodePtySession;

#[derive(Clone)]
pub struct PtyManager {
    workspace_root: PathBuf,
    config: PtyConfig,
    inner: Arc<PtyState>,
    sandbox_profile: Arc<Mutex<Option<SandboxProfile>>>,
}

#[derive(Default)]
struct PtyState {
    sessions: Mutex<HashMap<String, Arc<PtySessionHandle>>>,
}

struct PtyScrollback {
    lines: VecDeque<String>,
    pending_lines: VecDeque<String>,
    partial: String,
    pending_partial: String,
    capacity: usize,
}

impl PtyScrollback {
    fn new(capacity: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            pending_lines: VecDeque::new(),
            partial: String::new(),
            pending_partial: String::new(),
            capacity: capacity.max(1),
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        let text = String::from_utf8_lossy(chunk);
        for part in text.split_inclusive('\n') {
            self.partial.push_str(part);
            self.pending_partial.push_str(part);
            if part.ends_with('\n') {
                let complete = std::mem::take(&mut self.partial);
                let _ = std::mem::take(&mut self.pending_partial);
                self.lines.push_back(complete.clone());
                self.pending_lines.push_back(complete);
                while self.lines.len() > self.capacity {
                    self.lines.pop_front();
                }
                while self.pending_lines.len() > self.capacity {
                    self.pending_lines.pop_front();
                }
            }
        }
    }

    fn snapshot(&self) -> String {
        let mut output = String::new();
        for line in &self.lines {
            output.push_str(line);
        }
        output.push_str(&self.partial);
        output
    }

    fn pending(&self) -> String {
        let mut output = String::new();
        for line in &self.pending_lines {
            output.push_str(line);
        }
        output.push_str(&self.pending_partial);
        output
    }

    fn take_pending(&mut self) -> String {
        let mut output = String::new();
        while let Some(line) = self.pending_lines.pop_front() {
            output.push_str(&line);
        }
        if !self.pending_partial.is_empty() {
            output.push_str(&self.pending_partial);
            self.pending_partial.clear();
        }
        output
    }
}

struct PtySessionHandle {
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    terminal: Arc<Mutex<Vt>>,
    scrollback: Arc<Mutex<PtyScrollback>>,
    reader_thread: Mutex<Option<JoinHandle<()>>>,
    metadata: VTCodePtySession,
}

impl PtySessionHandle {
    fn snapshot_metadata(&self) -> VTCodePtySession {
        let mut metadata = self.metadata.clone();
        if let Ok(master) = self.master.lock() {
            if let Ok(size) = master.get_size() {
                metadata.rows = size.rows;
                metadata.cols = size.cols;
            }
        }
        if let Ok(terminal) = self.terminal.lock() {
            let contents = terminal.text().join("\n");
            metadata.screen_contents = Some(contents);
        }
        if let Ok(scrollback) = self.scrollback.lock() {
            let contents = scrollback.snapshot();
            if !contents.is_empty() {
                metadata.scrollback = Some(contents);
            }
        }
        metadata
    }

    fn read_output(&self, drain: bool) -> Option<String> {
        let mut scrollback = self.scrollback.lock().ok()?;
        let text = if drain {
            scrollback.take_pending()
        } else {
            scrollback.pending()
        };
        if text.is_empty() { None } else { Some(text) }
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
            sandbox_profile: Arc::new(Mutex::new(None)),
        }
    }

    pub fn config(&self) -> &PtyConfig {
        &self.config
    }

    pub fn set_sandbox_profile(&self, profile: Option<SandboxProfile>) {
        if let Ok(mut slot) = self.sandbox_profile.lock() {
            *slot = profile;
        }
    }

    fn current_sandbox_profile(&self) -> Option<SandboxProfile> {
        self.sandbox_profile
            .lock()
            .ok()
            .and_then(|value| value.clone())
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
        self.ensure_within_workspace(&work_dir)?;
        let workspace_root = self.workspace_root.clone();

        let sandbox_profile = self.current_sandbox_profile();
        let result = tokio::task::spawn_blocking(move || -> Result<PtyCommandResult> {
            let timeout_duration = Duration::from_millis(timeout);
            let (exec_program, exec_args, display_program) =
                if let Some(profile) = sandbox_profile.clone() {
                    let command_string =
                        join(std::iter::once(program.clone()).chain(args.iter().cloned()));
                    (
                        profile.binary().display().to_string(),
                        vec![
                            "--settings".to_string(),
                            profile.settings().display().to_string(),
                            command_string,
                        ],
                        program.clone(),
                    )
                } else {
                    (program.clone(), args.clone(), program.clone())
                };

            let mut builder = CommandBuilder::new(exec_program.clone());
            for arg in &exec_args {
                builder.arg(arg);
            }
            builder.cwd(&work_dir);
            set_command_environment(&mut builder, &display_program, size, &workspace_root);

            let pty_system = native_pty_system();
            let pair = pty_system
                .openpty(size)
                .context("failed to allocate PTY pair")?;

            let mut child = pair
                .slave
                .spawn_command(builder)
                .with_context(|| format!("failed to spawn PTY command '{display_program}'"))?;
            let mut killer = child.clone_killer();
            drop(pair.slave);

            let reader = pair
                .master
                .try_clone_reader()
                .context("failed to clone PTY reader")?;

            let (wait_tx, wait_rx) = mpsc::channel();
            let wait_thread = thread::spawn(move || {
                let status = child.wait();
                let _ = wait_tx.send(());
                status
            });

            let reader_thread = thread::spawn(move || -> Result<Vec<u8>> {
                let mut reader = reader;
                let mut buffer = [0u8; 4096];
                let mut collected = Vec::new();

                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(bytes_read) => {
                            collected.extend_from_slice(&buffer[..bytes_read]);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => {
                            return Err(error).context("failed to read PTY command output");
                        }
                    }
                }

                Ok(collected)
            });

            let wait_result = match wait_rx.recv_timeout(timeout_duration) {
                Ok(()) => wait_thread
                    .join()
                    .map_err(|panic| anyhow!("PTY command wait thread panicked: {:?}", panic))?,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    killer
                        .kill()
                        .context("failed to terminate PTY command after timeout")?;

                    let join_result = wait_thread.join().map_err(|panic| {
                        anyhow!("PTY command wait thread panicked: {:?}", panic)
                    })?;
                    if let Err(error) = join_result {
                        return Err(error)
                            .context("failed to wait for PTY command to exit after timeout");
                    }

                    reader_thread
                        .join()
                        .map_err(|panic| {
                            anyhow!("PTY command reader thread panicked: {:?}", panic)
                        })?
                        .context("failed to read PTY command output")?;

                    return Err(anyhow!(
                        "PTY command timed out after {} milliseconds",
                        timeout
                    ));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let join_result = wait_thread.join().map_err(|panic| {
                        anyhow!("PTY command wait thread panicked: {:?}", panic)
                    })?;
                    if let Err(error) = join_result {
                        return Err(error).context(
                            "failed to wait for PTY command after wait channel disconnected",
                        );
                    }

                    reader_thread
                        .join()
                        .map_err(|panic| {
                            anyhow!("PTY command reader thread panicked: {:?}", panic)
                        })?
                        .context("failed to read PTY command output")?;

                    return Err(anyhow!(
                        "PTY command wait channel disconnected unexpectedly"
                    ));
                }
            };

            let status = wait_result.context("failed to wait for PTY command to exit")?;

            let output_bytes = reader_thread
                .join()
                .map_err(|panic| anyhow!("PTY command reader thread panicked: {:?}", panic))?
                .context("failed to read PTY command output")?;
            let output = String::from_utf8_lossy(&output_bytes).to_string();
            let exit_code = exit_status_code(status);

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
        let sandbox_profile = self.current_sandbox_profile();

        let (exec_program, exec_args, display_program) = if let Some(profile) = sandbox_profile {
            let command_string = join(std::iter::once(program.clone()).chain(args.iter().cloned()));
            (
                profile.binary().display().to_string(),
                vec![
                    "--settings".to_string(),
                    profile.settings().display().to_string(),
                    command_string,
                ],
                program.clone(),
            )
        } else {
            (program.clone(), args.clone(), program.clone())
        };

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(size)
            .context("failed to allocate PTY pair")?;

        let mut builder = CommandBuilder::new(exec_program.clone());
        for arg in &exec_args {
            builder.arg(arg);
        }
        builder.cwd(&working_dir);
        self.ensure_within_workspace(&working_dir)?;
        set_command_environment(&mut builder, &display_program, size, &self.workspace_root);

        let child = pair.slave.spawn_command(builder).with_context(|| {
            format!("failed to spawn PTY session command '{}'", display_program)
        })?;
        drop(pair.slave);

        let master = pair.master;
        let mut reader = master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = master.take_writer().context("failed to take PTY writer")?;

        let vt = Arc::new(Mutex::new(Vt::new(
            usize::from(size.cols),
            usize::from(size.rows),
        )));
        let scrollback = Arc::new(Mutex::new(PtyScrollback::new(self.config.scrollback_lines)));
        let vt_clone = Arc::clone(&vt);
        let scrollback_clone = Arc::clone(&scrollback);
        let session_name = session_id.clone();
        let reader_thread = thread::Builder::new()
            .name(format!("vtcode-pty-reader-{session_name}"))
            .spawn(move || {
                let mut buffer = [0u8; 4096];
                let mut utf8_buffer: Vec<u8> = Vec::new();
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => {
                            debug!("PTY session '{}' reader reached EOF", session_name);
                            break;
                        }
                        Ok(bytes_read) => {
                            let chunk = &buffer[..bytes_read];
                            utf8_buffer.extend_from_slice(chunk);
                            if let Ok(mut terminal) = vt_clone.lock() {
                                loop {
                                    match std::str::from_utf8(&utf8_buffer) {
                                        Ok(valid) => {
                                            if !valid.is_empty() {
                                                let _ = terminal.feed_str(valid);
                                            }
                                            utf8_buffer.clear();
                                            break;
                                        }
                                        Err(error) => {
                                            let valid_up_to = error.valid_up_to();
                                            if valid_up_to > 0 {
                                                if let Ok(valid) =
                                                    std::str::from_utf8(&utf8_buffer[..valid_up_to])
                                                {
                                                    let _ = terminal.feed_str(valid);
                                                }
                                                utf8_buffer.drain(..valid_up_to);
                                                continue;
                                            }

                                            if let Some(error_len) = error.error_len() {
                                                let _ = terminal.feed_str("\u{FFFD}");
                                                utf8_buffer.drain(..error_len);
                                                continue;
                                            }

                                            break;
                                        }
                                    }
                                }
                            }
                            if let Ok(mut scrollback) = scrollback_clone.lock() {
                                scrollback.push(chunk);
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
            scrollback: None,
        };

        sessions.insert(
            session_id.clone(),
            Arc::new(PtySessionHandle {
                master: Mutex::new(master),
                child: Mutex::new(child),
                writer: Mutex::new(Some(writer)),
                terminal: vt,
                scrollback,
                reader_thread: Mutex::new(Some(reader_thread)),
                metadata: metadata.clone(),
            }),
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
            .map(|handle| handle.snapshot_metadata())
            .collect()
    }

    pub fn snapshot_session(&self, session_id: &str) -> Result<VTCodePtySession> {
        let handle = self.session_handle(session_id)?;
        Ok(handle.snapshot_metadata())
    }

    pub fn read_session_output(&self, session_id: &str, drain: bool) -> Result<Option<String>> {
        let handle = self.session_handle(session_id)?;
        Ok(handle.read_output(drain))
    }

    pub fn send_input_to_session(
        &self,
        session_id: &str,
        data: &[u8],
        append_newline: bool,
    ) -> Result<usize> {
        let handle = self.session_handle(session_id)?;
        let mut writer_guard = handle.writer.lock().expect("PTY writer mutex poisoned");
        let writer = writer_guard
            .as_mut()
            .ok_or_else(|| anyhow!("PTY session '{}' is no longer writable", session_id))?;

        writer
            .write_all(data)
            .context("failed to write input to PTY session")?;
        let mut written = data.len();
        if append_newline {
            writer
                .write_all(b"\n")
                .context("failed to write newline to PTY session")?;
            written += 1;
        }
        writer
            .flush()
            .context("failed to flush PTY session input")?;

        Ok(written)
    }

    pub fn resize_session(&self, session_id: &str, size: PtySize) -> Result<VTCodePtySession> {
        let handle = self.session_handle(session_id)?;
        {
            let master = handle.master.lock().expect("PTY master mutex poisoned");
            master
                .resize(size)
                .context("failed to resize PTY session")?;
        }
        Ok(handle.snapshot_metadata())
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

    fn session_handle(&self, session_id: &str) -> Result<Arc<PtySessionHandle>> {
        let sessions = self
            .inner
            .sessions
            .lock()
            .expect("PTY session mutex poisoned");
        sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow!("PTY session '{}' not found", session_id))
    }

    fn ensure_within_workspace(&self, candidate: &Path) -> Result<()> {
        let normalized = normalize_path(candidate);
        if !normalized.starts_with(&self.workspace_root) {
            return Err(anyhow!(
                "Path '{}' escapes workspace '{}'",
                candidate.display(),
                self.workspace_root.display()
            ));
        }
        Ok(())
    }
}

fn clamp_timeout(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

fn exit_status_code(status: portable_pty::ExitStatus) -> i32 {
    if status.signal().is_some() {
        -1
    } else {
        status.exit_code() as i32
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

fn set_command_environment(
    builder: &mut CommandBuilder,
    program: &str,
    size: PtySize,
    workspace_root: &Path,
) {
    builder.env("TERM", "xterm-256color");
    builder.env("PAGER", "cat");
    builder.env("GIT_PAGER", "cat");
    builder.env("LESS", "R");
    builder.env("COLUMNS", size.cols.to_string());
    builder.env("LINES", size.rows.to_string());
    builder.env("WORKSPACE_DIR", workspace_root.as_os_str());

    if is_shell_program(program) {
        builder.env("SHELL", program);
    }
}

fn is_shell_program(program: &str) -> bool {
    let name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "bash" | "sh" | "zsh" | "fish" | "dash" | "ash" | "busybox"
    )
}
