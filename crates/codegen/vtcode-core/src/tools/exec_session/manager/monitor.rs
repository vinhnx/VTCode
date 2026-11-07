use super::*;

impl ExecSessionManager {
    /// Attach or replace the pipe-session line filter; pipe-only in v1.
    pub(crate) async fn set_monitor_filter(&self, session_id: &str, spec: MonitorSpec) -> Result<()> {
        let record = self.session_record(session_id).await?;
        match record.backend {
            ExecSessionBackend::Pipe => self.pipe_sessions.set_monitor_filter(session_id, spec).await,
            ExecSessionBackend::Pty => {
                bail!("monitor subscriptions are pipe-only in v1; session '{session_id}' uses the pty backend")
            }
        }
    }

    /// Peek or drain the bounded match digest; pty sessions yield `None`.
    pub(crate) async fn read_monitor_digest(&self, session_id: &str, drain: bool) -> Result<Option<MonitorDigest>> {
        let record = self.session_record(session_id).await?;
        match record.backend {
            ExecSessionBackend::Pipe => self.pipe_sessions.read_monitor_digest(session_id, drain).await,
            ExecSessionBackend::Pty => Ok(None),
        }
    }

    /// Drain unconsumed monitor windows for idle wake, round-robin by id.
    ///
    /// At most `MONITOR_WAKE_MAX_SESSIONS` non-empty windows per call; the
    /// cursor resumes after the last delivered session so a chatty prefix
    /// cannot starve later sessions. Sessions drained dry by a racing tool
    /// response simply yield nothing.
    pub async fn drain_ready_monitor_digests(&self) -> Vec<MonitorWakeNote> {
        let sessions = self.list_sessions().await;
        let start = self
            .monitor_drain_cursor
            .lock()
            .as_ref()
            .and_then(|last| sessions.iter().position(|metadata| metadata.id.as_str() > last.as_str()));
        // Rotation without indexing: cycle past the cursor, visit each once.
        let ordered: Vec<_> = sessions.iter().cycle().skip(start.unwrap_or(0)).take(sessions.len()).collect();
        let mut notes = Vec::new();
        for metadata in ordered {
            if notes.len() >= MONITOR_WAKE_MAX_SESSIONS {
                break;
            }
            let session_id = metadata.id.as_str();
            // Closed between the list and the read: nothing left to wake.
            let digest: Option<MonitorDigest> = self.read_monitor_digest(session_id, true).await.unwrap_or_default();
            if let Some(digest) = digest {
                notes.push(MonitorWakeNote {
                    session_id: session_id.to_owned(),
                    command: metadata.command.clone(),
                    text: digest.render(),
                    dropped_lines: digest.dropped_lines,
                    dropped_bytes: digest.dropped_bytes,
                    total_seen: digest.total_seen,
                });
            }
        }
        if let Some(last) = notes.last() {
            *self.monitor_drain_cursor.lock() = Some(last.session_id.clone().into());
        }
        notes
    }
}
