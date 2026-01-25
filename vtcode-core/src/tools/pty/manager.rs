use std::collections::HashMap;
use std::io::Read;
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
use crate::utils::path::normalize_path;
use crate::utils::unicode_monitor::UNICODE_MONITOR;
use super::command_utils::is_shell_program;
use super::manager_utils::{clamp_timeout, exit_status_code, set_command_environment};
use super::scrollback::PtyScrollback;
use super::session::PtySessionHandle;
use super::types::{PtyCommandRequest, PtyCommandResult};

mod session_ops;

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

    fn format_working_dir(&self, path: &Path) -> String {
        match path.strip_prefix(&self.workspace_root) {
            Ok(relative) if relative.as_os_str().is_empty() => ".".into(),
            Ok(relative) => relative.to_string_lossy().replace("\\", "/"),
            Err(_) => path.to_string_lossy().into_owned(),
        }
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
