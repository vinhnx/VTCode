use super::super::formatting::{format_terminal_file, sanitize_session_id};
use super::super::manager_utils::exit_status_code;
use super::super::session::{CommandEchoState, PtySessionHandle};
use super::PtyManager;
use crate::tools::types::VTCodePtySession;
use crate::utils::file_utils::{ensure_dir_exists, write_file_with_context};
use anyhow::{Context, Result, anyhow};
use portable_pty::PtySize;
use std::io::Write;
use std::sync::Arc;
use tracing::warn;

impl PtyManager {
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
        ensure_dir_exists(&terminals_dir).await.with_context(|| {
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

            if let Err(e) =
                write_file_with_context(&file_path, &content, "terminal session file").await
            {
                warn!(
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
        write_file_with_context(&index_path, &index_content, "terminals index")
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
                let cwd = session.working_dir.as_deref().unwrap_or("-");
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

        // 2. Terminate child process using graceful termination
        // This uses SIGTERM first, then SIGKILL after a grace period
        handle.graceful_terminate();

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
}
