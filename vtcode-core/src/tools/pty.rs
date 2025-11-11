use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use shell_words::join;
use tokio::sync::Mutex as TokioMutex;
use tracing::{debug, warn};
use vt100::Parser;

use crate::audit::PermissionAuditLog;
use crate::config::PtyConfig;
use crate::sandbox::SandboxProfile;
use crate::tools::types::VTCodePtySession;

#[derive(Clone)]
pub struct PtyManager {
    workspace_root: PathBuf,
    config: PtyConfig,
    inner: Arc<PtyState>,
    sandbox_profile: Arc<Mutex<Option<SandboxProfile>>>,
    audit_log: Option<Arc<TokioMutex<PermissionAuditLog>>>,
}

#[derive(Default)]
struct PtyState {
    sessions: Mutex<HashMap<String, Arc<PtySessionHandle>>>,
}

struct CommandEchoState {
    command_bytes: Vec<u8>,
    failure: Vec<usize>,
    matched: usize,
    require_newline: bool,
    pending_newline: bool,
    consumed_once: bool,
}

impl CommandEchoState {
    fn new(command: &str, expect_newline: bool) -> Option<Self> {
        let trimmed = command.trim_matches(|ch| ch == '\n' || ch == '\r');
        if trimmed.is_empty() {
            return None;
        }

        let command_bytes = trimmed.as_bytes().to_vec();
        if command_bytes.is_empty() {
            return None;
        }

        let failure = build_failure(&command_bytes);

        Some(Self {
            command_bytes,
            failure,
            matched: 0,
            require_newline: expect_newline,
            pending_newline: expect_newline,
            consumed_once: false,
        })
    }

    fn reset(&mut self) {
        self.matched = 0;
        self.pending_newline = self.require_newline;
    }

    fn consume_chunk(&mut self, text: &str) -> (usize, bool) {
        let mut index = 0usize;
        let bytes = text.as_bytes();
        const ZERO_WIDTH_SPACE: &[u8] = "\u{200B}".as_bytes();

        while index < bytes.len() {
            let slice = &text[index..];

            if let Some(len) = parse_ansi_sequence(slice) {
                index += len;
                continue;
            }

            if slice.as_bytes().starts_with(ZERO_WIDTH_SPACE) {
                index += ZERO_WIDTH_SPACE.len();
                continue;
            }

            let byte = bytes[index];

            if byte == b'\r' {
                index += 1;
                self.reset();
                continue;
            }

            if self.pending_newline {
                if byte == b'\n' {
                    index += 1;
                    self.pending_newline = false;
                    continue;
                }
                self.pending_newline = false;
            }

            let mut matched_byte = false;
            loop {
                if let Some(&expected) = self.command_bytes.get(self.matched) {
                    if byte == expected {
                        self.matched += 1;
                        index += 1;
                        if self.matched == self.command_bytes.len() {
                            self.consumed_once = true;
                            self.pending_newline = self.require_newline;
                            self.matched = if self.command_bytes.len() > 1 {
                                self.failure[self.matched - 1]
                            } else {
                                0
                            };
                        }
                        matched_byte = true;
                        break;
                    }
                }

                if self.matched == 0 {
                    break;
                }

                self.matched = self.failure[self.matched - 1];
            }

            if matched_byte {
                continue;
            }

            break;
        }

        let done = self.consumed_once && !self.pending_newline && self.matched == 0;
        (index, done)
    }
}

fn build_failure(pattern: &[u8]) -> Vec<usize> {
    let mut failure = vec![0usize; pattern.len()];
    let mut length = 0usize;
    let mut index = 1usize;

    while index < pattern.len() {
        if pattern[index] == pattern[length] {
            length += 1;
            failure[index] = length;
            index += 1;
        } else if length != 0 {
            length = failure[length - 1];
        } else {
            failure[index] = 0;
            index += 1;
        }
    }

    failure
}

fn parse_ansi_sequence(text: &str) -> Option<usize> {
    crate::utils::ansi_parser::parse_ansi_sequence(text)
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

    fn push_text(&mut self, text: &str) {
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

    fn push_utf8(&mut self, buffer: &mut Vec<u8>, eof: bool) {
        loop {
            match std::str::from_utf8(buffer) {
                Ok(valid) => {
                    if !valid.is_empty() {
                        self.push_text(valid);
                    }
                    buffer.clear();
                    break;
                }
                Err(error) => {
                    let valid_up_to = error.valid_up_to();
                    if valid_up_to > 0 {
                        if let Ok(valid) = std::str::from_utf8(&buffer[..valid_up_to]) {
                            if !valid.is_empty() {
                                self.push_text(valid);
                            }
                        }
                        buffer.drain(..valid_up_to);
                        continue;
                    }

                    if let Some(error_len) = error.error_len() {
                        self.push_text("\u{FFFD}");
                        buffer.drain(..error_len);
                        continue;
                    }

                    if eof && !buffer.is_empty() {
                        self.push_text("\u{FFFD}");
                        buffer.clear();
                    }

                    break;
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
    terminal: Arc<Mutex<Parser>>,
    scrollback: Arc<Mutex<PtyScrollback>>,
    reader_thread: Mutex<Option<JoinHandle<()>>>,
    metadata: VTCodePtySession,
    last_input: Mutex<Option<CommandEchoState>>,
}

impl PtySessionHandle {
    fn snapshot_metadata(&self) -> VTCodePtySession {
        let mut metadata = self.metadata.clone();
        {
            let master = self.master.lock();
            if let Ok(size) = master.get_size() {
                metadata.rows = size.rows;
                metadata.cols = size.cols;
            }
        }
        {
            let parser = self.terminal.lock();
            let contents = parser.screen().contents();
            metadata.screen_contents = Some(contents);
        }
        {
            let scrollback = self.scrollback.lock();
            let contents = scrollback.snapshot();
            if !contents.is_empty() {
                metadata.scrollback = Some(contents);
            }
        }
        metadata
    }

    fn read_output(&self, drain: bool) -> Option<String> {
        let mut scrollback = self.scrollback.lock();
        let text = if drain {
            scrollback.take_pending()
        } else {
            scrollback.pending()
        };
        if text.is_empty() {
            return None;
        }

        let filtered = self.strip_command_echo(text);
        if filtered.is_empty() {
            None
        } else {
            Some(filtered)
        }
    }

    fn strip_command_echo(&self, text: String) -> String {
        let mut guard = self.last_input.lock();
        let Some(state) = guard.as_mut() else {
            return text;
        };

        let (consumed, done) = state.consume_chunk(&text);
        if done {
            *guard = None;
        }

        text.get(consumed..)
            .map(|tail| tail.to_string())
            .unwrap_or_default()
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
            audit_log: None,
        }
    }

    pub fn with_audit_log(mut self, audit_log: Arc<TokioMutex<PermissionAuditLog>>) -> Self {
        self.audit_log = Some(audit_log);
        self
    }

    pub fn config(&self) -> &PtyConfig {
        &self.config
    }

    pub fn set_sandbox_profile(&self, profile: Option<SandboxProfile>) {
        let mut slot = self.sandbox_profile.lock();
        *slot = profile;
    }

    pub fn sandbox_profile(&self) -> Option<SandboxProfile> {
        self.current_sandbox_profile()
    }

    fn current_sandbox_profile(&self) -> Option<SandboxProfile> {
        self.sandbox_profile.lock().clone()
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

            // Try to resolve the program path. If not found, wrap in shell.
            let (exec_program, exec_args, display_program, env_profile, _use_shell_wrapper) =
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
                        Some(profile),
                        false,
                    )
                } else if let Some(_resolved_path) = resolve_program_path(&program) {
                    // Program found in PATH, use it directly
                    (program.clone(), args.clone(), program.clone(), None, false)
                } else {
                    // Program not found in PATH, wrap in shell to leverage user's PATH
                    let shell = "/bin/sh";
                    let full_command =
                        join(std::iter::once(program.clone()).chain(args.iter().cloned()));
                    (
                        shell.to_string(),
                        vec!["-c".to_string(), full_command.clone()],
                        program.clone(),
                        None,
                        true,
                    )
                };

            let mut builder = CommandBuilder::new(exec_program.clone());
            for arg in &exec_args {
                builder.arg(arg);
            }
            builder.cwd(&work_dir);
            set_command_environment(
                &mut builder,
                &display_program,
                size,
                &workspace_root,
                env_profile.as_ref(),
            );

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

    pub async fn resolve_working_dir(&self, requested: Option<&str>) -> Result<PathBuf> {
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
        let metadata = tokio::fs::metadata(&normalized).await.with_context(|| {
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

        let mut sessions = self.inner.sessions.lock();
        if sessions.contains_key(&session_id) {
            return Err(anyhow!("PTY session '{}' already exists", session_id));
        }

        let mut command_parts = command.clone();
        let program = command_parts.remove(0);
        let args = command_parts;
        let sandbox_profile = self.current_sandbox_profile();

        let (exec_program, exec_args, display_program, env_profile) = if let Some(profile) =
            sandbox_profile.clone()
        {
            let command_string = join(std::iter::once(program.clone()).chain(args.iter().cloned()));
            (
                profile.binary().display().to_string(),
                vec![
                    "--settings".to_string(),
                    profile.settings().display().to_string(),
                    command_string,
                ],
                program.clone(),
                Some(profile),
            )
        } else if let Some(_resolved_path) = resolve_program_path(&program) {
            // Program found in PATH, use it directly
            (program.clone(), args.clone(), program.clone(), None)
        } else {
            // Program not found in PATH, wrap in shell to leverage user's PATH
            let shell = "/bin/sh";
            let full_command = join(std::iter::once(program.clone()).chain(args.iter().cloned()));
            (
                shell.to_string(),
                vec!["-c".to_string(), full_command.clone()],
                program.clone(),
                None,
            )
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
        set_command_environment(
            &mut builder,
            &display_program,
            size,
            &self.workspace_root,
            env_profile.as_ref(),
        );

        let child = pair.slave.spawn_command(builder).with_context(|| {
            format!("failed to spawn PTY session command '{}'", display_program)
        })?;
        drop(pair.slave);

        let master = pair.master;
        let mut reader = master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = master.take_writer().context("failed to take PTY writer")?;

        let parser = Arc::new(Mutex::new(Parser::new(
            size.rows,
            size.cols,
            self.config.scrollback_lines,
        )));
        let scrollback = Arc::new(Mutex::new(PtyScrollback::new(self.config.scrollback_lines)));
        let parser_clone = Arc::clone(&parser);
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
                            if !utf8_buffer.is_empty() {
                                let mut scrollback = scrollback_clone.lock();
                                scrollback.push_utf8(&mut utf8_buffer, true);
                            }
                            debug!("PTY session '{}' reader reached EOF", session_name);
                            break;
                        }
                        Ok(bytes_read) => {
                            let chunk = &buffer[..bytes_read];
                            {
                                let mut parser = parser_clone.lock();
                                parser.process(chunk);
                            }

                            utf8_buffer.extend_from_slice(chunk);
                            {
                                let mut scrollback = scrollback_clone.lock();
                                scrollback.push_utf8(&mut utf8_buffer, false);
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
                terminal: parser,
                scrollback,
                reader_thread: Mutex::new(Some(reader_thread)),
                metadata: metadata.clone(),
                last_input: Mutex::new(None),
            }),
        );

        Ok(metadata)
    }

    pub fn list_sessions(&self) -> Vec<VTCodePtySession> {
        let sessions = self.inner.sessions.lock();
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

        if let Ok(input_text) = std::str::from_utf8(data) {
            let mut last_input = handle.last_input.lock();
            *last_input = CommandEchoState::new(input_text, append_newline);
        } else {
            let mut last_input = handle.last_input.lock();
            *last_input = None;
        }

        let mut writer_guard = handle.writer.lock();
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
            let master = handle.master.lock();
            master
                .resize(size)
                .context("failed to resize PTY session")?;
        }
        let mut parser = handle.terminal.lock();
        parser.set_size(size.rows, size.cols);
        Ok(handle.snapshot_metadata())
    }

    pub fn is_session_completed(&self, session_id: &str) -> Result<Option<i32>> {
        let handle = self.session_handle(session_id)?;
        let mut child = handle.child.lock();
        Ok(
            if let Some(status) = child
                .try_wait()
                .context("failed to poll PTY session status")?
            {
                Some(exit_status_code(status))
            } else {
                None
            },
        )
    }

    pub fn close_session(&self, session_id: &str) -> Result<VTCodePtySession> {
        let handle = {
            let mut sessions = self.inner.sessions.lock();
            sessions
                .remove(session_id)
                .ok_or_else(|| anyhow!("PTY session '{}' not found", session_id))?
        };

        {
            let mut writer_guard = handle.writer.lock();
            if let Some(mut writer) = writer_guard.take() {
                let _ = writer.write_all(b"exit\n");
                let _ = writer.flush();
            }
        }

        let mut child = handle.child.lock();
        if child
            .try_wait()
            .context("failed to poll PTY session status")?
            .is_none()
        {
            child.kill().context("failed to terminate PTY session")?;
            let _ = child.wait();
        }

        {
            let mut thread_guard = handle.reader_thread.lock();
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
        let sessions = self.inner.sessions.lock();
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
    sandbox_profile: Option<&SandboxProfile>,
) {
    // Inherit environment from parent process to preserve PATH and other important variables
    for (key, value) in std::env::vars() {
        builder.env(&key, &value);
    }

    // Override or set specific environment variables for TTY
    builder.env("TERM", "xterm-256color");
    builder.env("PAGER", "cat");
    builder.env("GIT_PAGER", "cat");
    builder.env("LESS", "R");
    builder.env("COLUMNS", size.cols.to_string());
    builder.env("LINES", size.rows.to_string());
    builder.env("WORKSPACE_DIR", workspace_root.as_os_str());

    // Disable automatic color output from ls and other commands
    builder.env("CLICOLOR", "0");
    builder.env("CLICOLOR_FORCE", "0");
    builder.env("LS_COLORS", "");
    builder.env("NO_COLOR", "1");

    if let Some(profile) = sandbox_profile {
        builder.env("VT_SANDBOX_RUNTIME", profile.runtime_kind().as_str());
        builder.env("VT_SANDBOX_SETTINGS", profile.settings().as_os_str());
        builder.env(
            "VT_SANDBOX_PERSISTENT_DIR",
            profile.persistent_storage().as_os_str(),
        );
        if profile.allowed_paths().is_empty() {
            builder.env("VT_SANDBOX_ALLOWED_PATHS", "");
        } else {
            match std::env::join_paths(profile.allowed_paths()) {
                Ok(joined) => builder.env("VT_SANDBOX_ALLOWED_PATHS", joined),
                Err(_) => builder.env("VT_SANDBOX_ALLOWED_PATHS", ""),
            };
        }
    }

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

/// Resolve program path - if program doesn't exist in PATH, return None to signal shell fallback.
/// This allows the shell to find programs installed in user-specific directories.
fn resolve_program_path(program: &str) -> Option<String> {
    // Check if program is an absolute or relative path
    if program.contains(std::path::MAIN_SEPARATOR) || program.contains('/') {
        return Some(program.to_string());
    }

    // Try to find the program in PATH
    if let Ok(path_env) = std::env::var("PATH") {
        for path_dir in path_env.split(std::path::MAIN_SEPARATOR) {
            let full_path = Path::new(path_dir).join(program);
            if full_path.exists() && full_path.is_file() {
                return Some(full_path.to_string_lossy().to_string());
            }
        }
    }

    // If not found in PATH, return None to signal shell wrapping
    None
}

pub fn is_development_toolchain_command(program: &str) -> bool {
    let name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "cargo"
            | "rustc"
            | "rustup"
            | "rustfmt"
            | "clippy"
            | "npm"
            | "node"
            | "yarn"
            | "pnpm"
            | "bun"
            | "go"
            | "python"
            | "python3"
            | "pip"
            | "pip3"
            | "java"
            | "javac"
            | "mvn"
            | "gradle"
            | "make"
            | "cmake"
            | "gcc"
            | "g++"
            | "clang"
            | "clang++"
    )
}
