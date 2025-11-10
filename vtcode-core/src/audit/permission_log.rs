//! Permission audit logging system
//! Tracks all permission decisions (allow/deny/prompt) with context
//! Writes to ~/.vtcode/audit/permissions-{date}.log in JSON format

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::path::PathBuf;
use tracing::info;

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
}

/// Type of permission event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionEventType {
    CommandExecution,
    ToolUsage,
    FileAccess { read: bool, write: bool },
    NetworkAccess { domain: String },
    SandboxOperation,
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
    writer: BufWriter<std::fs::File>,

    /// Count of events logged this session
    event_count: usize,
}

impl PermissionAuditLog {
    /// Create or open the audit log for today
    pub fn new(audit_dir: PathBuf) -> Result<Self> {
        // Create audit directory if needed
        std::fs::create_dir_all(&audit_dir).context("Failed to create audit directory")?;

        // Use today's date in filename
        let date = Local::now().format("%Y-%m-%d");
        let log_path = audit_dir.join(format!("permissions-{}.log", date));

        // Open file in append mode
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .context(format!("Failed to open audit log at {:?}", log_path))?;

        let writer = BufWriter::new(file);

        info!(?log_path, "Audit log initialized");

        Ok(Self {
            log_path,
            writer,
            event_count: 0,
        })
    }

    /// Record a permission event
    pub fn record(&mut self, event: PermissionEvent) -> Result<()> {
        use std::io::Write;

        let json = serde_json::to_string(&event).context("Failed to serialize permission event")?;

        writeln!(self.writer, "{}", json).context("Failed to write to audit log")?;

        self.writer.flush().context("Failed to flush audit log")?;

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
    ) -> Result<()> {
        let event = PermissionEvent {
            timestamp: Local::now(),
            subject: command.to_string(),
            event_type: PermissionEventType::CommandExecution,
            decision,
            reason: reason.to_string(),
            resolved_path,
            requested_by: "CommandPolicyEvaluator".to_string(),
        };

        self.record(event)
    }
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
        let _log = PermissionAuditLog::new(dir.path().to_path_buf())?;
        assert!(dir.path().exists());
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
        )?;

        assert_eq!(log.event_count(), 1);
        Ok(())
    }
}
