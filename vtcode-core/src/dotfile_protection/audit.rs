//! Immutable audit logging for dotfile access attempts.
//!
//! Provides comprehensive, tamper-evident logging of all dotfile
//! access attempts with timestamps, outcomes, and contextual information.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::utils::file_utils::ensure_dir_exists;

/// Outcome of a dotfile access attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    /// Access was allowed after user confirmation.
    AllowedWithConfirmation,
    /// Access was allowed via whitelist (with secondary auth).
    AllowedViaWhitelist,
    /// Access was blocked (no confirmation given).
    Blocked,
    /// Access was denied (policy violation).
    Denied,
    /// User explicitly rejected the modification.
    UserRejected,
    /// Access was allowed without confirmation (protection disabled).
    AllowedUnprotected,
}

impl std::fmt::Display for AuditOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditOutcome::AllowedWithConfirmation => write!(f, "ALLOWED_WITH_CONFIRMATION"),
            AuditOutcome::AllowedViaWhitelist => write!(f, "ALLOWED_VIA_WHITELIST"),
            AuditOutcome::Blocked => write!(f, "BLOCKED"),
            AuditOutcome::Denied => write!(f, "DENIED"),
            AuditOutcome::UserRejected => write!(f, "USER_REJECTED"),
            AuditOutcome::AllowedUnprotected => write!(f, "ALLOWED_UNPROTECTED"),
        }
    }
}

/// Type of access being attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessType {
    Read,
    Write,
    Create,
    Delete,
    Modify,
    Append,
}

impl std::fmt::Display for AccessType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccessType::Read => write!(f, "READ"),
            AccessType::Write => write!(f, "WRITE"),
            AccessType::Create => write!(f, "CREATE"),
            AccessType::Delete => write!(f, "DELETE"),
            AccessType::Modify => write!(f, "MODIFY"),
            AccessType::Append => write!(f, "APPEND"),
        }
    }
}

/// A single audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Unique identifier for this entry.
    pub id: String,
    /// Timestamp of the access attempt (UTC).
    pub timestamp: DateTime<Utc>,
    /// Path to the dotfile being accessed.
    pub file_path: String,
    /// Type of access attempted.
    pub access_type: AccessType,
    /// Outcome of the access attempt.
    pub outcome: AuditOutcome,
    /// Tool or operation that initiated the access.
    pub initiator: String,
    /// Session identifier.
    pub session_id: String,
    /// Description of proposed changes (if applicable).
    pub proposed_changes: Option<String>,
    /// Hash of the previous entry (for tamper detection).
    pub previous_hash: String,
    /// Hash of this entry (computed after creation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_hash: Option<String>,
    /// Additional context or reason.
    pub context: Option<String>,
    /// Whether this was during an automated operation.
    pub during_automation: bool,
}

impl AuditEntry {
    /// Create a new audit entry.
    pub fn new(
        file_path: impl Into<String>,
        access_type: AccessType,
        outcome: AuditOutcome,
        initiator: impl Into<String>,
        session_id: impl Into<String>,
        previous_hash: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            file_path: file_path.into(),
            access_type,
            outcome,
            initiator: initiator.into(),
            session_id: session_id.into(),
            proposed_changes: None,
            previous_hash: previous_hash.into(),
            entry_hash: None,
            context: None,
            during_automation: false,
        }
    }

    /// Set proposed changes description.
    pub fn with_proposed_changes(mut self, changes: impl Into<String>) -> Self {
        self.proposed_changes = Some(changes.into());
        self
    }

    /// Set context/reason.
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Mark as during automation.
    pub fn during_automation(mut self) -> Self {
        self.during_automation = true;
        self
    }

    /// Compute and set the entry hash.
    pub fn finalize(mut self) -> Self {
        self.entry_hash = Some(self.compute_hash());
        self
    }

    /// Compute SHA-256 hash of the entry (excluding entry_hash field).
    fn compute_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.id.as_bytes());
        hasher.update(self.timestamp.to_rfc3339().as_bytes());
        hasher.update(self.file_path.as_bytes());
        hasher.update(format!("{:?}", self.access_type).as_bytes());
        hasher.update(format!("{:?}", self.outcome).as_bytes());
        hasher.update(self.initiator.as_bytes());
        hasher.update(self.session_id.as_bytes());
        hasher.update(self.previous_hash.as_bytes());
        if let Some(ref changes) = self.proposed_changes {
            hasher.update(changes.as_bytes());
        }
        if let Some(ref ctx) = self.context {
            hasher.update(ctx.as_bytes());
        }
        hasher.update(&[self.during_automation as u8]);
        format!("{:x}", hasher.finalize())
    }

    /// Verify the entry hash is valid.
    pub fn verify(&self) -> bool {
        self.entry_hash
            .as_ref()
            .is_some_and(|hash| *hash == self.compute_hash())
    }
}

/// Immutable audit log for dotfile access.
pub struct AuditLog {
    /// Path to the log file.
    log_path: PathBuf,
    /// Lock for thread-safe writes.
    write_lock: Arc<Mutex<()>>,
    /// Hash of the last entry (for chaining).
    last_hash: Arc<Mutex<String>>,
}

impl AuditLog {
    /// Create or open an audit log at the specified path.
    pub async fn new(log_path: impl AsRef<Path>) -> Result<Self> {
        let log_path = log_path.as_ref().to_path_buf();

        // Create parent directories if needed
        if let Some(parent) = log_path.parent() {
            ensure_dir_exists(parent)
                .await
                .with_context(|| format!("Failed to create audit log directory: {:?}", parent))?;
        }

        // Read the last hash from the log if it exists
        let last_hash = if log_path.exists() {
            Self::read_last_hash(&log_path)?
        } else {
            // Genesis hash
            "0000000000000000000000000000000000000000000000000000000000000000".to_string()
        };

        Ok(Self {
            log_path,
            write_lock: Arc::new(Mutex::new(())),
            last_hash: Arc::new(Mutex::new(last_hash)),
        })
    }

    /// Read the last entry's hash from the log file.
    fn read_last_hash(log_path: &Path) -> Result<String> {
        let file = File::open(log_path).with_context(|| "Failed to open audit log")?;
        let reader = BufReader::new(file);

        let mut last_hash =
            "0000000000000000000000000000000000000000000000000000000000000000".to_string();

        for line in reader.lines() {
            let line = line.with_context(|| "Failed to read audit log line")?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<AuditEntry>(&line) {
                if let Some(hash) = entry.entry_hash {
                    last_hash = hash;
                }
            }
        }

        Ok(last_hash)
    }

    /// Log an access attempt.
    pub async fn log(&self, mut entry: AuditEntry) -> Result<()> {
        let _guard = self.write_lock.lock().await;

        // Set the previous hash
        let mut last_hash = self.last_hash.lock().await;
        entry.previous_hash = last_hash.clone();

        // Finalize the entry with its hash
        let entry = entry.finalize();

        // Update the last hash
        if let Some(ref hash) = entry.entry_hash {
            *last_hash = hash.clone();
        }

        // Serialize and append to log
        let json =
            serde_json::to_string(&entry).with_context(|| "Failed to serialize audit entry")?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .with_context(|| format!("Failed to open audit log: {:?}", self.log_path))?;

        writeln!(file, "{}", json).with_context(|| "Failed to write audit entry")?;

        // Ensure data is flushed to disk
        file.sync_all()
            .with_context(|| "Failed to sync audit log")?;

        Ok(())
    }

    /// Get all entries from the log.
    pub async fn get_entries(&self) -> Result<Vec<AuditEntry>> {
        let _guard = self.write_lock.lock().await;

        if !self.log_path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.log_path).with_context(|| "Failed to open audit log")?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();

        for line in reader.lines() {
            let line = line.with_context(|| "Failed to read audit log line")?;
            if line.trim().is_empty() {
                continue;
            }
            let entry: AuditEntry =
                serde_json::from_str(&line).with_context(|| "Failed to parse audit entry")?;
            entries.push(entry);
        }

        Ok(entries)
    }

    /// Verify the integrity of the entire audit log.
    pub async fn verify_integrity(&self) -> Result<bool> {
        let entries = self.get_entries().await?;

        if entries.is_empty() {
            return Ok(true);
        }

        let mut expected_prev_hash =
            "0000000000000000000000000000000000000000000000000000000000000000".to_string();

        for entry in entries {
            // Verify entry hash
            if !entry.verify() {
                tracing::warn!(
                    "Audit log integrity violation: entry {} has invalid hash",
                    entry.id
                );
                return Ok(false);
            }

            // Verify chain
            if entry.previous_hash != expected_prev_hash {
                tracing::warn!(
                    "Audit log integrity violation: entry {} has broken chain",
                    entry.id
                );
                return Ok(false);
            }

            expected_prev_hash = entry.entry_hash.unwrap_or_default();
        }

        Ok(true)
    }

    /// Get entries for a specific file.
    pub async fn get_entries_for_file(&self, file_path: &str) -> Result<Vec<AuditEntry>> {
        let entries = self.get_entries().await?;
        Ok(entries
            .into_iter()
            .filter(|e| e.file_path == file_path)
            .collect())
    }

    /// Get recent entries (last N).
    pub async fn get_recent_entries(&self, count: usize) -> Result<Vec<AuditEntry>> {
        let entries = self.get_entries().await?;
        let len = entries.len();
        if len <= count {
            Ok(entries)
        } else {
            Ok(entries.into_iter().skip(len - count).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_audit_log_creation() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("audit.log");

        let log = AuditLog::new(&log_path).await.unwrap();

        let entry = AuditEntry::new(
            ".gitignore",
            AccessType::Write,
            AuditOutcome::Blocked,
            "write_file",
            "test-session",
            "",
        );

        log.log(entry).await.unwrap();

        let entries = log.get_entries().await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_path, ".gitignore");
    }

    #[tokio::test]
    async fn test_audit_log_integrity() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("audit.log");

        let log = AuditLog::new(&log_path).await.unwrap();

        // Add multiple entries
        for i in 0..5 {
            let entry = AuditEntry::new(
                format!(".env.{}", i),
                AccessType::Modify,
                AuditOutcome::Blocked,
                "test_tool",
                "test-session",
                "",
            );
            log.log(entry).await.unwrap();
        }

        // Verify integrity
        assert!(log.verify_integrity().await.unwrap());

        // Entries should be chainable
        let entries = log.get_entries().await.unwrap();
        assert_eq!(entries.len(), 5);

        for entry in &entries {
            assert!(entry.verify());
        }
    }

    #[test]
    fn test_entry_hash() {
        let entry = AuditEntry::new(
            ".bashrc",
            AccessType::Write,
            AuditOutcome::UserRejected,
            "shell",
            "sess-123",
            "prev-hash",
        )
        .finalize();

        assert!(entry.verify());
    }
}
