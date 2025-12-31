//! Audit logging for command safety decisions.
//!
//! Records all command safety checks for compliance and debugging.
//! Can be used to generate security audit trails and identify patterns.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

/// A single command safety audit entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// The command that was evaluated
    pub command: Vec<String>,
    /// Whether the command was allowed
    pub allowed: bool,
    /// Reason for the decision
    pub reason: String,
    /// Decision type (Allow, Deny, Unknown)
    pub decision_type: String,
    /// Timestamp (ISO 8601 format)
    pub timestamp: String,
}

impl AuditEntry {
    /// Creates a new audit entry
    pub fn new(
        command: Vec<String>,
        allowed: bool,
        reason: String,
        decision_type: String,
    ) -> Self {
        // Use a simple timestamp (can be improved with chrono if available)
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        
        Self {
            command,
            allowed,
            reason,
            decision_type,
            timestamp,
        }
    }
}

/// Audit logger for command safety decisions
pub struct SafetyAuditLogger {
    entries: Arc<Mutex<Vec<AuditEntry>>>,
    enabled: bool,
}

impl SafetyAuditLogger {
    /// Creates a new audit logger
    pub fn new(enabled: bool) -> Self {
        Self {
            entries: Arc::new(Mutex::new(Vec::new())),
            enabled,
        }
    }

    /// Logs an audit entry
    pub async fn log(&self, entry: AuditEntry) {
        if self.enabled {
            let mut entries = self.entries.lock().await;
            entries.push(entry);
        }
    }

    /// Returns all logged entries
    pub async fn entries(&self) -> Vec<AuditEntry> {
        let entries = self.entries.lock().await;
        entries.clone()
    }

    /// Returns entries for a specific command
    pub async fn entries_for_command(&self, cmd: &str) -> Vec<AuditEntry> {
        let entries = self.entries.lock().await;
        entries
            .iter()
            .filter(|e| e.command.join(" ").contains(cmd))
            .cloned()
            .collect()
    }

    /// Returns denied entries only
    pub async fn denied_entries(&self) -> Vec<AuditEntry> {
        let entries = self.entries.lock().await;
        entries.iter().filter(|e| !e.allowed).cloned().collect()
    }

    /// Clears all entries
    pub async fn clear(&self) {
        let mut entries = self.entries.lock().await;
        entries.clear();
    }

    /// Returns count of entries
    pub async fn count(&self) -> usize {
        let entries = self.entries.lock().await;
        entries.len()
    }
}

impl Clone for SafetyAuditLogger {
    fn clone(&self) -> Self {
        Self {
            entries: Arc::clone(&self.entries),
            enabled: self.enabled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn creates_audit_entry() {
        let cmd = vec!["git".to_string(), "status".to_string()];
        let entry = AuditEntry::new(cmd, true, "git status allowed".to_string(), "Allow".to_string());
        assert!(entry.allowed);
        assert!(!entry.timestamp.is_empty());
    }

    #[tokio::test]
    async fn logs_entries() {
        let logger = SafetyAuditLogger::new(true);
        let cmd = vec!["git".to_string(), "status".to_string()];
        let entry = AuditEntry::new(cmd, true, "git status allowed".to_string(), "Allow".to_string());

        logger.log(entry).await;
        assert_eq!(logger.count().await, 1);
    }

    #[tokio::test]
    async fn filters_denied_entries() {
        let logger = SafetyAuditLogger::new(true);

        let cmd1 = vec!["git".to_string(), "status".to_string()];
        logger.log(AuditEntry::new(cmd1, true, "allowed".to_string(), "Allow".to_string())).await;

        let cmd2 = vec!["git".to_string(), "reset".to_string()];
        logger.log(AuditEntry::new(cmd2, false, "denied".to_string(), "Deny".to_string())).await;

        let denied = logger.denied_entries().await;
        assert_eq!(denied.len(), 1);
        assert!(!denied[0].allowed);
    }

    #[tokio::test]
    async fn disabled_logger_ignores_entries() {
        let logger = SafetyAuditLogger::new(false);
        let cmd = vec!["git".to_string(), "status".to_string()];
        let entry = AuditEntry::new(cmd, true, "allowed".to_string(), "Allow".to_string());

        logger.log(entry).await;
        assert_eq!(logger.count().await, 0);
    }

    #[tokio::test]
    async fn clones_share_same_entries() {
        let logger1 = SafetyAuditLogger::new(true);
        let logger2 = logger1.clone();

        let cmd = vec!["git".to_string(), "status".to_string()];
        let entry = AuditEntry::new(cmd, true, "allowed".to_string(), "Allow".to_string());

        logger1.log(entry).await;

        // Both loggers see the same entry
        assert_eq!(logger1.count().await, 1);
        assert_eq!(logger2.count().await, 1);
    }
}
