//! Audit logging for externally modified file conflicts.

use crate::utils::error_messages::ERR_CREATE_AUDIT_DIR;
use crate::utils::file_utils::ensure_dir_exists_sync;
use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

/// Audit event emitted when a tracked file changes outside VT Code control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConflictAuditEvent {
    pub timestamp: DateTime<Local>,
    pub path: PathBuf,
    pub reason: String,
    pub file_exists: bool,
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
}

/// JSONL audit logger for file-conflict events.
pub struct FileConflictAuditLog {
    writer: BufWriter<std::fs::File>,
    log_path: PathBuf,
}

impl FileConflictAuditLog {
    /// Create or open today's file-conflict audit log in the given directory.
    pub fn new(audit_dir: PathBuf) -> Result<Self> {
        ensure_dir_exists_sync(&audit_dir).context(ERR_CREATE_AUDIT_DIR)?;

        let date = Local::now().format("%Y-%m-%d");
        let log_path = audit_dir.join(format!("file-conflicts-{}.log", date));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("Failed to open file conflict audit log at {:?}", log_path))?;

        Ok(Self {
            writer: BufWriter::new(file),
            log_path,
        })
    }

    pub fn record(&mut self, event: &FileConflictAuditEvent) -> Result<()> {
        use std::io::Write;

        let json = serde_json::to_string(event)
            .context("Failed to serialize file conflict audit event")?;
        writeln!(self.writer, "{json}").context("Failed to write file conflict audit event")?;
        self.writer
            .flush()
            .context("Failed to flush file conflict audit log")?;
        Ok(())
    }

    pub fn log_path(&self) -> &Path {
        &self.log_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn creates_file_conflict_audit_log() -> Result<()> {
        let dir = TempDir::new()?;
        let log = FileConflictAuditLog::new(dir.path().to_path_buf())?;
        assert!(log.log_path().exists());
        Ok(())
    }
}
