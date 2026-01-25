use std::collections::HashMap;

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use shell_words::join;
use tokio::sync::Mutex as TokioMutex;
use tracing::{debug, info, warn};
use vt100::Parser;

use crate::audit::PermissionAuditLog;
use crate::config::{CommandsConfig, PtyConfig};
use crate::tools::path_env;
use crate::tools::shell::resolve_fallback_shell;
use crate::tools::types::VTCodePtySession;
use crate::utils::unicode_monitor::UNICODE_MONITOR;
use super::command_utils::is_shell_program;
use super::formatting::{format_terminal_file, sanitize_session_id};
use super::scrollback::PtyScrollback;
use super::session::{CommandEchoState, PtySessionHandle};
use super::types::{PtyCommandRequest, PtyCommandResult};

#[derive(Clone)]
pub struct PtyManager {
    workspace_root: PathBuf,
    config: PtyConfig,
    inner: Arc<PtyState>,
    audit_log: Option<Arc<TokioMutex<PermissionAuditLog>>>,
    extra_paths: Arc<Mutex<Vec<PathBuf>>>,
}

#[derive(Default)]
struct PtyState {
    sessions: Mutex<HashMap<String, Arc<PtySessionHandle>>>,
}

impl PtyManager {
    pub fn new(workspace_root: PathBuf, config: PtyConfig) -> Self {
        let resolved_root = workspace_root
            .canonicalize()
            .unwrap_or(workspace_root.clone());

        let default_paths = path_env::compute_extra_search_paths(
            &CommandsConfig::default().extra_path_entries,
            &resolved_root,
        );

        Self {
            workspace_root: resolved_root,
            config,
            inner: Arc::new(PtyState::default()),
            audit_log: None,
            extra_paths: Arc::new(Mutex::new(default_paths)),
        }
    }

    pub fn with_audit_log(mut self, audit_log: Arc<TokioMutex<PermissionAuditLog>>) -> Self {
        self.audit_log = Some(audit_log);
        self
    }

    pub fn config(&self) -> &PtyConfig {
        &self.config
    }

    pub fn apply_commands_config(&self, commands_config: &CommandsConfig) {
        let mut extra = self.extra_paths.lock();
        *extra = path_env::compute_extra_search_paths(
            &commands_config.extra_path_entries,
            &self.workspace_root,
        );
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
        let extra_paths = self.extra_paths.lock().clone();
        let max_tokens = request.max_tokens; // Get max_tokens from request

        let result =
            tokio::task::spawn_blocking(move || -> Result<PtyCommandResult> {
                let timeout_duration = Duration::from_millis(timeout);

                // Use login shell for command execution to ensure user's PATH and environment
                // is properly initialized from their shell configuration files (~/.bashrc, ~/.zshrc, etc).
                // However, we avoid double-wrapping if the command is already a shell invocation.
                let (exec_program, exec_args, display_program, _use_shell_wrapper) =
                    if is_shell_program(&program)
                        && args.iter().any(|arg| arg == "-c" || arg == "/C")
                    {
                        // Already a shell command, don't wrap again
                        (program.clone(), args.clone(), program.clone(), false)
                    } else {
                        let shell = resolve_fallback_shell();
                        let full_command =
                            join(std::iter::once(program.clone()).chain(args.iter().cloned()));
                        (
                            shell.clone(),
                            vec!["-lc".to_owned(), full_command.clone()],
                            program.clone(),
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
                    &extra_paths,
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
                            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                                continue;
                            }
                            Err(error) => {
                                return Err(error).context("failed to read PTY command output");
                            }
                        }
                    }

                    Ok(collected)
                });

                let wait_result = match wait_rx.recv_timeout(timeout_duration) {
                    Ok(()) => wait_thread.join().map_err(|panic| {
                        anyhow!("PTY command wait thread panicked: {:?}", panic)
                    })?,
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
                let mut output = String::from_utf8_lossy(&output_bytes).into_owned();
                let exit_code = exit_status_code(status);

                // Apply max_tokens truncation if specified
                if let Some(max_tokens) = max_tokens {
                    if max_tokens > 0 {
                        // Simple byte-based truncation
                        if output.len() > max_tokens * 4 {
                            let truncate_point = (max_tokens * 4).min(output.len());
                            output.truncate(truncate_point);
                            output.push_str("\n[... truncated by max_tokens ...]");
                        }
                    } else {
                        // Keep original if max_tokens is not valid
                    }
                }
                // Keep original if max_tokens is None

                Ok(PtyCommandResult {
                    exit_code,
                    output,
                    duration: start.elapsed(),
                    size,
                    applied_max_tokens: max_tokens,
                })
            })
            .await
            .context("failed to join PTY command task")??;

        Ok(result)
    }

    pub async fn resolve_working_dir(&self, requested: Option<&str>) -> Result<PathBuf> {
        let requested = match requested {
            Some(dir) if !dir.trim().is_empty() => dir.trim(),
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
            return Err(anyhow!(
                "PTY session command cannot be empty.\n\
                 This is an internal error - command validation should have caught this earlier.\n\
                 Please report this with the run_pty_cmd parameters used."
            ));
        }

        // Use entry API to avoid double lookup
        let mut sessions = self.inner.sessions.lock();
        use std::collections::hash_map::Entry;
        let entry = match sessions.entry(session_id.clone()) {
            Entry::Occupied(_) => {
                return Err(anyhow!("PTY session '{}' already exists", session_id));
            }
            Entry::Vacant(e) => e,
        };

        let mut command_parts = command.clone();
        let program = command_parts.remove(0);
        let args = command_parts;
        let extra_paths = self.extra_paths.lock().clone();

        // Use login shell for command execution to ensure user's PATH and environment
        // is properly initialized from their shell configuration files (~/.bashrc, ~/.zshrc, etc).
        // However, we avoid double-wrapping if the command is already a shell invocation.
        let (exec_program, exec_args, display_program) = if is_shell_program(&program)
            && args.iter().any(|arg| arg == "-c" || arg == "/C")
        {
            // Already a shell command, don't wrap again
            (program.clone(), args.clone(), program.clone())
        } else {
            let shell = resolve_fallback_shell();
            let full_command = join(std::iter::once(program.clone()).chain(args.iter().cloned()));

            // Verify we have a valid command string
            if full_command.is_empty() {
                return Err(anyhow!(
                    "Failed to construct command string from program '{}' and args {:?}",
                    program,
                    args
                ));
            }

            (
                shell.clone(),
                vec!["-lc".to_owned(), full_command.clone()],
                program.clone(),
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
            &extra_paths,
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
        let scrollback = Arc::new(Mutex::new(PtyScrollback::new(
            self.config.scrollback_lines,
            self.config.max_scrollback_bytes,
        )));
        let parser_clone = Arc::clone(&parser);
        let scrollback_clone = Arc::clone(&scrollback);
        let session_name = session_id.clone();
        // Start unicode monitoring for this session
        UNICODE_MONITOR.start_session();

        let reader_thread = thread::Builder::new()
            .name(format!("vtcode-pty-reader-{session_name}"))
            .spawn(move || {
                let mut buffer = [0u8; 8192]; // Increased buffer size for better performance
                let mut utf8_buffer: Vec<u8> = Vec::with_capacity(8192); // Pre-allocate buffer
                let mut total_bytes = 0usize;
                let mut unicode_detection_hits = 0usize;

                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => {
                            if !utf8_buffer.is_empty() {
                                let mut scrollback = scrollback_clone.lock();
                                scrollback.push_utf8(&mut utf8_buffer, true);
                            }
                            debug!("PTY session '{}' reader reached EOF (processed {} bytes, {} unicode detections)",
                                   session_name, total_bytes, unicode_detection_hits);
                            break;
                        }
                        Ok(bytes_read) => {
                            let chunk = &buffer[..bytes_read];
                            total_bytes += bytes_read;

                            // Quick unicode detection heuristic
                            let likely_unicode = chunk.iter().any(|&b| b >= 0x80);
                            if likely_unicode {
                                unicode_detection_hits += 1;
                            }

                            // Process chunk through VT100 parser for screen updates
                            {
                                let mut parser = parser_clone.lock();
                                parser.process(chunk);
                            }

                            utf8_buffer.extend_from_slice(chunk);
                            {
                                let mut scrollback = scrollback_clone.lock();
                                scrollback.push_utf8(&mut utf8_buffer, false);
                            }

                            // Periodic buffer cleanup to prevent excessive memory usage
                            if utf8_buffer.capacity() > 32768 && utf8_buffer.len() < 1024 {
                                utf8_buffer.shrink_to_fit();
                            }
                        }
                        Err(error) => {
                            warn!("PTY session '{}' reader error: {} (processed {} bytes)",
                                  session_name, error, total_bytes);
                            break;
                        }
                    }
                }
                debug!("PTY session '{}' reader thread finished (total: {} bytes, unicode detections: {})",
                       session_name, total_bytes, unicode_detection_hits);

                // End unicode monitoring for this session
                UNICODE_MONITOR.end_session();

                // Log unicode statistics if any unicode was detected
                if unicode_detection_hits > 0 {
                    let scrollback = scrollback_clone.lock();
                    let metrics = scrollback.metrics();
                    if metrics.unicode_errors > 0 {
                        warn!("PTY session '{}' had {} unicode errors during processing",
                              session_name, metrics.unicode_errors);
                    }
                    if metrics.total_unicode_chars > 0 {
                        info!("PTY session '{}' processed {} unicode characters across {} sessions with {} buffer remainder",
                              session_name, metrics.total_unicode_chars, metrics.unicode_sessions, metrics.utf8_buffer_size);
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

        // Use the entry we obtained earlier to insert without additional lookup
        entry.insert(Arc::new(PtySessionHandle {
            master: Mutex::new(master),
            child: Mutex::new(child),
            writer: Mutex::new(Some(writer)),
            terminal: parser,
            scrollback,
            reader_thread: Mutex::new(Some(reader_thread)),
            metadata: metadata.clone(),
            last_input: Mutex::new(None),
        }));

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

        // Acquire last_input lock once and update conditionally
        {
            let mut last_input = handle.last_input.lock();
            *last_input = if let Ok(input_text) = std::str::from_utf8(data) {
                CommandEchoState::new(input_text, append_newline)
            } else {
                None
            };
        }

        // Acquire writer lock once for all write operations
        {
            let mut writer_guard = handle.writer.lock();
            let writer = writer_guard
                .as_mut()
                .ok_or_else(|| anyhow!("PTY session '{}' is no longer writable", session_id))?;

            writer
                .write_all(data)
                .context("failed to write input to PTY session")?;

            if append_newline {
                writer
                    .write_all(b"\n")
                    .context("failed to write newline to PTY session")?;
            }

            writer
                .flush()
                .context("failed to flush PTY session input")?;
        }

        let written = data.len() + if append_newline { 1 } else { 0 };
        Ok(written)
    }

    pub fn resize_session(&self, session_id: &str, size: PtySize) -> Result<VTCodePtySession> {
        let handle = self.session_handle(session_id)?;

        // Lock order: master -> terminal (Arc-wrapped, separate scope)
        {
            let master = handle.master.lock();
            master
                .resize(size)
                .context("failed to resize PTY session")?;
        }

        // Terminal lock acquired separately (Arc, safe to interleave)
        {
            let mut parser = handle.terminal.lock();
            parser.set_size(size.rows, size.cols);
        }

        Ok(handle.snapshot_metadata())
    }

    pub fn is_session_completed(&self, session_id: &str) -> Result<Option<i32>> {
        let handle = self.session_handle(session_id)?;
        let mut child = handle.child.lock();
        child
            .try_wait()
            .context("failed to poll PTY session status")
            .map(|opt| opt.map(exit_status_code))
    }

    /// Sync all terminal sessions to files for dynamic context discovery
    ///
    /// This implements Cursor-style dynamic context discovery:
    /// - Each terminal session is written to `.vtcode/terminals/{session_id}.txt`
    /// - Includes metadata header (cwd, last command, exit code)
    /// - Agent can reference terminal output via grep/read_file
    pub async fn sync_sessions_to_files(&self) -> Result<Vec<std::path::PathBuf>> {
        let terminals_dir = self.workspace_root.join(".vtcode").join("terminals");
        tokio::fs::create_dir_all(&terminals_dir)
            .await
            .with_context(|| {
                format!(
                    "Failed to create terminals directory: {}",
                    terminals_dir.display()
                )
            })?;

        let sessions = self.list_sessions();
        let mut written_files = Vec::with_capacity(sessions.len());

        for session in &sessions {
            let output = match self.read_session_output(&session.id, false) {
                Ok(Some(output)) => output,
                Ok(None) => String::new(),
                Err(_) => continue,
            };

            let content = format_terminal_file(session, &output);
            let file_path = terminals_dir.join(format!("{}.txt", sanitize_session_id(&session.id)));

            if let Err(e) = tokio::fs::write(&file_path, &content).await {
                tracing::warn!(
                    session_id = %session.id,
                    error = %e,
                    "Failed to sync terminal session to file"
                );
                continue;
            }

            written_files.push(file_path);
        }

        // Write index file
        let index_content = self.generate_terminals_index(&sessions);
        let index_path = terminals_dir.join("INDEX.md");
        tokio::fs::write(&index_path, &index_content)
            .await
            .with_context(|| {
                format!("Failed to write terminals index: {}", index_path.display())
            })?;

        tracing::info!(
            sessions = sessions.len(),
            files = written_files.len(),
            "Synced terminal sessions to files"
        );

        Ok(written_files)
    }

    /// Generate INDEX.md content for terminal sessions
    fn generate_terminals_index(&self, sessions: &[VTCodePtySession]) -> String {
        let mut content = String::new();
        content.push_str("# Terminal Sessions Index\n\n");
        content.push_str("This file lists all active terminal sessions for dynamic discovery.\n");
        content.push_str("Use `read_file` on individual session files for full output.\n\n");

        if sessions.is_empty() {
            content.push_str("*No active terminal sessions.*\n");
        } else {
            content.push_str(&format!("**Active Sessions**: {}\n\n", sessions.len()));
            content.push_str("| Session ID | Command | Working Dir | Size |\n");
            content.push_str("|------------|---------|-------------|------|\n");

            for session in sessions {
                let cwd = session
                    .working_dir
                    .as_ref()
                    .map(|s| s.as_str())
                    .unwrap_or("-");
                let cmd_truncated = if session.command.len() > 25 {
                    format!("{}...", &session.command[..22])
                } else {
                    session.command.clone()
                };

                content.push_str(&format!(
                    "| `{}` | {} | {} | {}x{} |\n",
                    session.id,
                    cmd_truncated.replace('|', "\\|"),
                    cwd.replace('|', "\\|"),
                    session.cols,
                    session.rows
                ));
            }

            content.push_str("\n## Session Details\n\n");
            for session in sessions {
                content.push_str(&format!("### {}\n\n", session.id));
                content.push_str(&format!("- **Command**: `{}`\n", session.command));
                if !session.args.is_empty() {
                    content.push_str(&format!("- **Args**: {}\n", session.args.join(" ")));
                }
                if let Some(cwd) = &session.working_dir {
                    content.push_str(&format!("- **Working Dir**: {}\n", cwd));
                }
                content.push_str(&format!(
                    "- **Terminal Size**: {}x{}\n",
                    session.cols, session.rows
                ));
                content.push_str(&format!(
                    "- **File**: `.vtcode/terminals/{}.txt`\n\n",
                    sanitize_session_id(&session.id)
                ));
            }
        }

        content.push_str("---\n");
        content.push_str("*Generated automatically. Do not edit manually.*\n");

        content
    }

    /// Get the terminals directory path
    pub fn terminals_dir(&self) -> std::path::PathBuf {
        self.workspace_root.join(".vtcode").join("terminals")
    }

    pub fn close_session(&self, session_id: &str) -> Result<VTCodePtySession> {
        // Remove session from global map first
        let handle = {
            let mut sessions = self.inner.sessions.lock();
            sessions
                .remove(session_id)
                .ok_or_else(|| anyhow!("PTY session '{}' not found", session_id))?
        };

        // Lock order: writer -> child -> reader_thread (follow documented order)

        // 1. Close writer
        {
            let mut writer_guard = handle.writer.lock();
            if let Some(mut writer) = writer_guard.take() {
                let _ = writer.write_all(b"exit\n");
                let _ = writer.flush();
            }
        }

        // 2. Terminate child process
        {
            let mut child = handle.child.lock();
            if child
                .try_wait()
                .context("failed to poll PTY session status")?
                .is_none()
            {
                let kill_started = Instant::now();
                child.kill().context("failed to terminate PTY session")?;
                let _ = child.wait();
                let elapsed = kill_started.elapsed();
                if elapsed > Duration::from_secs(2) {
                    warn!(
                        session = %session_id,
                        elapsed_ms = %elapsed.as_millis(),
                        "PTY session termination exceeded budget"
                    );
                }
            }
        }

        // 3. Join reader thread
        {
            let mut thread_guard = handle.reader_thread.lock();
            if let Some(reader_thread) = thread_guard.take()
                && let Err(panic) = reader_thread.join()
            {
                warn!(
                    "PTY session '{}' reader thread panicked: {:?}",
                    session_id, panic
                );
            }
        }

        // Snapshot metadata calls snapshot_metadata() which acquires master, terminal, scrollback locks
        Ok(handle.snapshot_metadata())
    }

    fn format_working_dir(&self, path: &Path) -> String {
        match path.strip_prefix(&self.workspace_root) {
            Ok(relative) if relative.as_os_str().is_empty() => ".".into(),
            Ok(relative) => relative.to_string_lossy().replace("\\", "/"),
            Err(_) => path.to_string_lossy().into_owned(),
        }
    }

    pub fn terminate_all_sessions(&self) {
        let session_ids: Vec<String> = {
            let sessions = self.inner.sessions.lock();
            sessions.keys().cloned().collect()
        };
        for id in session_ids {
            if let Err(e) = self.close_session(&id) {
                warn!("Failed to close PTY session {}: {}", id, e);
            }
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

use crate::utils::path::normalize_path;

fn set_command_environment(
    builder: &mut CommandBuilder,
    program: &str,
    size: PtySize,
    workspace_root: &Path,
    extra_paths: &[PathBuf],
) {
    // Inherit environment from parent process to preserve PATH and other important variables
    let mut env_map: HashMap<OsString, OsString> = std::env::vars_os().collect();

    // Ensure HOME is set - this is crucial for proper path expansion in cargo and other tools
    let home_key = OsString::from("HOME");
    if !env_map.contains_key(&home_key)
        && let Some(home_dir) = dirs::home_dir()
    {
        env_map.insert(home_key.clone(), OsString::from(home_dir.as_os_str()));
    }

    let path_key = OsString::from("PATH");
    let current_path = env_map.get(&path_key).map(|value| value.as_os_str());
    if let Some(merged) = path_env::merge_path_env(current_path, extra_paths) {
        env_map.insert(path_key, merged);
    }

    for (key, value) in env_map {
        builder.env(key, value);
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

    // For Rust/Cargo, disable colors at the source
    builder.env("CARGO_TERM_COLOR", "never");

    // Suppress macOS malloc debugging junk that can pollute PTY output
    // This is especially common when running in login shells (-l)
    builder.env_remove("MallocStackLogging");
    builder.env_remove("MallocStackLoggingNoCompact");
    builder.env_remove("MallocStackLoggingDirectory");
    builder.env_remove("MallocErrorAbort");
    builder.env_remove("MallocCheckHeapStart");
    builder.env_remove("MallocCheckHeapEach");
    builder.env_remove("MallocCheckHeapSleep");
    builder.env_remove("MallocCheckHeapAbort");
    builder.env_remove("MallocGuardEdges");
    builder.env_remove("MallocScribble");
    builder.env_remove("MallocDoNotProtectSentinel");
    builder.env_remove("MallocQuiet");

    if is_shell_program(program) {
        builder.env("SHELL", program);
    }
}
