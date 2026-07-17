//! Permission audit logging system
//! Tracks all permission decisions (allow/deny/prompt) with context
//! Writes to ~/.vtcode/audit/permissions-{date}.log in JSON format
//! Old audit logs are pruned after DEFAULT_AUDIT_LOG_MAX_AGE_DAYS days.

use crate::utils::error_messages::ERR_CREATE_AUDIT_DIR;
use crate::utils::file_utils::ensure_dir_exists_sync;
use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::info;

/// Default maximum age in days for audit log files before they are pruned.
pub const DEFAULT_AUDIT_LOG_MAX_AGE_DAYS: u64 = 90;

/// Prefix for permission audit log files.
const PERMISSION_LOG_PREFIX: &str = "permissions-";

/// Seconds per day constant (avoiding dependency on core crate).
const SECONDS_PER_DAY: u64 = 86400;

/// Record of a single permission decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionEvent {
    /// When the decision was made
    pub timestamp: DateTime<Local>,

    /// What was being requested (command, tool, path, etc.)
    pub subject: String,

    /// Type of permission check
    pub event_type: PermissionEventType,

    /// The decision reached
    pub decision: PermissionDecision,

    /// Why the decision was made
    pub reason: String,

    /// Optional resolved path (if applicable)
    pub resolved_path: Option<PathBuf>,

    /// Tool or component that made the request
    pub requested_by: String,

    /// Name of the agent that triggered this permission check, if known
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,

    /// Whether the requesting agent is a subagent (false for primary agent)
    pub is_subagent: bool,
}

/// Type of file access permission
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FileAccessPermission {
    Read,
    Write,
    ReadWrite,
}

/// Type of permission event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionEventType {
    CommandExecution,
    ToolUsage,
    FileAccess(FileAccessPermission),
    NetworkAccess { domain: String },
    HookExecution,
}

/// The decision reached
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PermissionDecision {
    Allowed,
    Denied,
    Prompted,
    Cached,
}

/// Audit log for permission decisions
pub struct PermissionAuditLog {
    /// Path to the audit log file
    log_path: PathBuf,

    /// Writer for the log file
    writer: Option<BufWriter<fs::File>>,

    /// Count of events logged this session
    event_count: usize,
}

impl PermissionAuditLog {
    /// Create or open the audit log for today
    pub fn new(audit_dir: PathBuf) -> Result<Self> {
        // Create audit directory if needed
        ensure_dir_exists_sync(&audit_dir).context(ERR_CREATE_AUDIT_DIR)?;

        // Prune old audit logs
        if let Err(err) = cleanup_old_audit_logs(&audit_dir, DEFAULT_AUDIT_LOG_MAX_AGE_DAYS) {
            tracing::warn!(error = %err, "Failed to prune old audit logs");
        }

        // Use today's date in filename
        let date = Local::now().format("%Y-%m-%d");
        let log_path = audit_dir.join(format!("permissions-{date}.log"));

        info!(?log_path, "Audit log initialized");

        Ok(Self { log_path, writer: None, event_count: 0 })
    }

    /// Record a permission event
    pub fn record(&mut self, event: PermissionEvent) -> Result<()> {
        use std::io::Write;

        let json = serde_json::to_string(&event).context("Failed to serialize permission event")?;

        let writer = self.writer_mut()?;
        writeln!(writer, "{json}").context("Failed to write to audit log")?;

        writer.flush().context("Failed to flush audit log")?;

        self.event_count += 1;

        info!(
            subject = &event.subject,
            decision = ?event.decision,
            "Permission event logged"
        );

        Ok(())
    }

    /// Get the number of events logged
    pub fn event_count(&self) -> usize {
        self.event_count
    }

    /// Get path to the log file
    pub fn log_path(&self) -> &PathBuf {
        &self.log_path
    }

    /// Helper to create and log a permission event
    pub fn log_command_decision(
        &mut self,
        command: &str,
        decision: PermissionDecision,
        reason: &str,
        resolved_path: Option<PathBuf>,
        agent_name: Option<&str>,
        is_subagent: bool,
    ) -> Result<()> {
        let event = PermissionEvent {
            timestamp: Local::now(),
            subject: command.to_owned(),
            event_type: PermissionEventType::CommandExecution,
            decision,
            reason: reason.to_owned(),
            resolved_path,
            requested_by: "CommandPolicyEvaluator".into(),
            agent_name: agent_name.map(String::from),
            is_subagent,
        };

        self.record(event)
    }

    fn writer_mut(&mut self) -> Result<&mut BufWriter<fs::File>> {
        if self.writer.is_none() {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.log_path)
                .with_context(|| format!("Failed to open audit log at {:?}", self.log_path))?;
            self.writer = Some(BufWriter::new(file));
        }

        self.writer.as_mut().context("audit log writer was not initialized")
    }
}

/// Prune audit log files older than `max_age_days` from the given directory.
/// Only removes files matching the `permissions-YYYY-MM-DD.log` pattern.
fn cleanup_old_audit_logs(audit_dir: &Path, max_age_days: u64) -> Result<()> {
    if max_age_days == 0 {
        return Ok(());
    }

    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(max_age_days.saturating_mul(SECONDS_PER_DAY)))
        .unwrap_or(UNIX_EPOCH);

    let entries = match fs::read_dir(audit_dir) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("Failed to read audit log directory {}", audit_dir.display())
            });
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(PERMISSION_LOG_PREFIX) || !name.ends_with(".log") {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        if modified <= cutoff {
            if let Err(err) = fs::remove_file(&path) {
                tracing::warn!(
                    path = %path.display(),
                    error = %err,
                    "Failed to remove expired audit log"
                );
            }
        }
    }

    Ok(())
}

/// Generate a human-readable summary of permission decisions
pub struct PermissionSummary {
    pub total_events: usize,
    pub allowed: usize,
    pub denied: usize,
    pub prompted: usize,
    pub cached: usize,
}

impl PermissionSummary {
    pub fn format(&self) -> String {
        format!(
            "Permission Summary: {} total | {} allowed | {} denied | {} prompted | {} cached",
            self.total_events, self.allowed, self.denied, self.prompted, self.cached
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_audit_log_creation() -> Result<()> {
        let dir = TempDir::new()?;
        let log = PermissionAuditLog::new(dir.path().to_path_buf())?;
        assert!(dir.path().exists());
        assert!(!log.log_path().exists());
        Ok(())
    }

    #[test]
    fn test_log_permission_event() -> Result<()> {
        let dir = TempDir::new()?;
        let mut log = PermissionAuditLog::new(dir.path().to_path_buf())?;

        log.log_command_decision(
            "cargo fmt",
            PermissionDecision::Allowed,
            "Allow list match",
            Some(PathBuf::from("/usr/local/cargo")),
            Some("coder"),
            false,
        )?;

        assert_eq!(log.event_count(), 1);
        assert!(log.log_path().exists());
        Ok(())
    }

    #[test]
    fn test_cleanup_ignores_non_audit_files() -> Result<()> {
        let dir = TempDir::new()?;

        fs::write(dir.path().join("not-audit.log"), "noise")?;
        fs::write(dir.path().join("other.txt"), "noise")?;

        cleanup_old_audit_logs(dir.path(), 1)?;

        assert!(dir.path().join("not-audit.log").exists());
        assert!(dir.path().join("other.txt").exists());
        Ok(())
    }

    #[test]
    fn test_cleanup_noop_on_missing_dir() {
        let result = cleanup_old_audit_logs(Path::new("/nonexistent/audit"), 90);
        assert!(result.is_ok());
    }
}
