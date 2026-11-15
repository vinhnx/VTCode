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
use vtcode_commons::VtCodePaths;
use vtcode_commons::utils::calculate_sha256;

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
    /// Access is awaiting the user's decision (confirmation requested).
    PendingConfirmation,
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
            AuditOutcome::PendingConfirmation => write!(f, "PENDING_CONFIRMATION"),
        }
    }
}

/// Type of access being attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessType {
    /// Read access to a dotfile.
    Read,
    /// Write access to a dotfile.
    Write,
    /// Create a new dotfile.
    Create,
    /// Delete a dotfile.
    Delete,
    /// Modify an existing dotfile.
    Modify,
    /// Append to a dotfile.
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
        hasher.update([self.during_automation as u8]);
        calculate_sha256(&hasher.finalize())
    }

    /// Verify the entry hash is valid.
    pub fn verify(&self) -> bool {
        self.entry_hash.as_ref().is_some_and(|hash| *hash == self.compute_hash())
    }
}

/// Immutable audit log for dotfile access.
///
/// **Single-instance invariant**: exactly one `AuditLog` should exist per log
/// file path. The `write_lock` and `last_hash` are in-process `Arc<Mutex>`
/// fields — two separate instances for the same path would have independent
/// locks and chain-hash caches, allowing interleaved appends that corrupt the
/// tamper-evident chain. The sole production caller (`DotfileGuardian::new`)
/// respects this by constructing one instance and sharing it via `Arc`.
pub struct AuditLog {
    /// Path to the log file.
    log_path: PathBuf,
    /// Lock for serializing writes and reads. Uses `tokio::sync::Mutex` so an
    /// `OwnedMutexGuard` can be moved into `spawn_blocking` closures, keeping
    /// operations serialized even if the calling task is cancelled.
    write_lock: Arc<Mutex<()>>,
    /// Hash of the last entry (for chaining). Uses `tokio::sync::Mutex` so an
    /// `OwnedMutexGuard<String>` can be moved into `spawn_blocking` alongside
    /// the write-lock guard — both survive task cancellation inside the
    /// blocking closure, and no `std::sync::Mutex` poisoning can occur.
    ///
    /// Lock ordering: always acquire `write_lock` before `last_hash`. No
    /// method acquires `last_hash` without first holding `write_lock`.
    last_hash: Arc<Mutex<String>>,
}

impl AuditLog {
    /// Create or open an audit log at the specified path.
    pub async fn new(log_path: impl AsRef<Path>) -> Result<Self> {
        let log_path = log_path.as_ref().to_path_buf();

        // Create parent directories if needed
        if let Some(parent) = log_path.parent() {
            VtCodePaths::ensure_user_dir(parent)
                .with_context(|| format!("Failed to create audit log directory: {parent:?}"))?;
        }

        // `read_last_hash` does a blocking file seek+read; run it off the async
        // executor. See `# Blocking` docs in `src/agent/runloop/git.rs`.
        let last_hash = if log_path.exists() {
            let path = log_path.clone();
            tokio::task::spawn_blocking(move || Self::read_last_hash(&path))
                .await
                .context("audit log hash read task panicked")??
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
    ///
    /// Only the tail of the file is scanned (bounded window) so startup cost is
    /// `O(window)` rather than `O(file size)` — the audit log grows unbounded
    /// over the tool's lifetime, and the prior full-file scan made launch time
    /// scale with historical audit volume.
    fn read_last_hash(log_path: &Path) -> Result<String> {
        use std::io::{Read, Seek, SeekFrom};

        const DEFAULT_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

        let mut file = File::open(log_path).with_context(|| "Failed to open audit log")?;
        let len = file.metadata().with_context(|| "Failed to read audit log metadata")?.len();
        if len == 0 {
            return Ok(DEFAULT_HASH.to_string());
        }

        // Window large enough to contain the final (and longest plausible)
        // entry; capped so the scan cost is bounded regardless of log size.
        let window: u64 = (1 << 18).min(len); // 256 KiB cap
        file.seek(SeekFrom::End(-(window as i64)))
            .with_context(|| "Failed to seek audit log")?;
        let mut buf = Vec::with_capacity(window as usize);
        file.read_to_end(&mut buf).with_context(|| "Failed to read audit log tail")?;

        let text = String::from_utf8_lossy(&buf);
        let mut last_hash = DEFAULT_HASH.to_string();
        for raw in text.lines().rev() {
            let raw = raw.trim_end_matches(['\n', '\r']);
            if raw.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<AuditEntry>(raw)
                && let Some(hash) = entry.entry_hash
            {
                last_hash = hash;
                break;
            }
        }
        Ok(last_hash)
    }

    /// Log an access attempt.
    ///
    /// Both `write_lock` and `last_hash` are acquired as `OwnedMutexGuard`s
    /// and moved into the `spawn_blocking` closure so that:
    ///
    /// 1. Appends stay serialized even if the calling task is cancelled —
    ///    both guards survive in the blocking closure until the IO finishes.
    /// 2. `last_hash` is updated only after the full append succeeds (the
    ///    line is in the OS page cache, visible to subsequent reads) but
    ///    before `sync_all()`. If `sync_all()` fails, the entry is still
    ///    readable and the chain is consistent. If `open` or `write_all`
    ///    fails, `last_hash` is not updated **and** any partial bytes are
    ///    truncated back to the pre-append length, so the file never has a
    ///    malformed tail that would break future reads.
    pub async fn log(&self, mut entry: AuditEntry) -> Result<()> {
        let write_guard = self.write_lock.clone().lock_owned().await;
        let mut hash_guard = self.last_hash.clone().lock_owned().await;

        // Build the finalized entry under serialization.
        entry.previous_hash = hash_guard.clone();
        let entry = entry.finalize();
        let new_hash = entry.entry_hash.clone();
        let json = serde_json::to_string(&entry).with_context(|| "Failed to serialize audit entry")?;

        // File open + write + fsync are blocking; run them off the async
        // executor. Both owned guards are moved into the closure so operations
        // stay serialized even if the caller is cancelled. See `# Blocking`
        // docs in `src/agent/runloop/git.rs`.
        let log_path = self.log_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let _write_guard = write_guard; // held until closure returns

            Self::append_entry_blocking(&log_path, &json)?;

            // The append succeeded — the full line is in the OS page cache and
            // visible to subsequent reads. Update the in-memory chain hash now,
            // before fsync. An fsync failure does NOT undo the page-cache
            // write, so the chain stays consistent with the readable file.
            if let Some(hash) = new_hash {
                *hash_guard = hash;
            }

            // Best-effort durability. A failure here does not roll back the
            // append; the entry is readable and the chain is consistent.
            // Reopen for sync_all to avoid keeping the append handle open
            // longer than necessary.
            let sync_result = File::open(&log_path).and_then(|f| f.sync_all());

            if let Err(e) = sync_result {
                // Chain hash already matches the readable entry. Report the
                // durability error but keep in-memory state consistent.
                drop(hash_guard);
                return Err(e).with_context(|| "Failed to sync audit log");
            }

            drop(hash_guard);
            Ok(())
        })
        .await
        .context("audit log write task panicked")?
    }

    /// Append one finalized JSON line to the audit log file.
    ///
    /// Records the pre-append file length and truncates back to it on any
    /// `write_all` failure, ensuring the file never has a partial/malformed
    /// tail that would break `get_entries()`. The append uses a single
    /// `write_all` of the pre-built line (including `\n`) to minimize the
    /// chance of a partial write.
    ///
    /// # Blocking
    /// Performs synchronous file open/write. Must not be called on a Tokio
    /// worker thread.
    fn append_entry_blocking(log_path: &Path, json: &str) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .with_context(|| format!("Failed to open audit log: {log_path:?}"))?;

        // Record the pre-append length so we can roll back on write failure.
        let pre_len = file.metadata().with_context(|| "Failed to read audit log metadata")?.len();

        // Build the full line in memory so the append is a single write_all,
        // minimizing the chance of a partial line on failure.
        let mut line = json.to_string();
        line.push('\n');

        if let Err(e) = file.write_all(line.as_bytes()) {
            // Truncate any partial bytes back to the pre-append length so
            // the file never has a malformed tail.
            let _ = file.set_len(pre_len);
            return Err(e).with_context(|| "Failed to write audit entry");
        }

        Ok(())
    }

    /// Get all entries from the log.
    ///
    /// Uses `lock_owned()` so the `write_lock` guard is moved into the
    /// `spawn_blocking` closure — reads stay consistent with in-flight writes
    /// even if the calling task is cancelled.
    pub async fn get_entries(&self) -> Result<Vec<AuditEntry>> {
        let guard = self.write_lock.clone().lock_owned().await;

        // File open + read_line loop are blocking; run them off the async
        // executor. The owned `write_lock` guard is moved into the closure so
        // reads stay serialized even if the caller is cancelled. See `# Blocking`
        // docs in `src/agent/runloop/git.rs`.
        let log_path = self.log_path.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<AuditEntry>> {
            let _guard = guard; // held until closure returns

            if !log_path.exists() {
                return Ok(Vec::new());
            }

            let file = File::open(&log_path).with_context(|| "Failed to open audit log")?;
            let mut reader = BufReader::new(file);
            let mut entries = Vec::new();
            let mut line = String::new();

            loop {
                line.clear();
                if reader.read_line(&mut line).with_context(|| "Failed to read audit log line")? == 0 {
                    break;
                }
                let raw = line.trim_end_matches(['\n', '\r']);
                if raw.trim().is_empty() {
                    continue;
                }
                let entry: AuditEntry = serde_json::from_str(raw).with_context(|| "Failed to parse audit entry")?;
                entries.push(entry);
            }

            Ok(entries)
        })
        .await
        .context("audit log read task panicked")?
    }

    /// Verify the integrity of the entire audit log.
    pub async fn verify_integrity(&self) -> Result<bool> {
        let entries = self.get_entries().await?;

        if entries.is_empty() {
            return Ok(true);
        }

        let mut expected_prev_hash = "0000000000000000000000000000000000000000000000000000000000000000".to_string();

        for entry in entries {
            // Verify entry hash
            if !entry.verify() {
                tracing::warn!("Audit log integrity violation: entry {} has invalid hash", entry.id);
                return Ok(false);
            }

            // Verify chain
            if entry.previous_hash != expected_prev_hash {
                tracing::warn!("Audit log integrity violation: entry {} has broken chain", entry.id);
                return Ok(false);
            }

            expected_prev_hash = entry.entry_hash.unwrap_or_default();
        }

        Ok(true)
    }

    /// Get entries for a specific file.
    pub async fn get_entries_for_file(&self, file_path: &str) -> Result<Vec<AuditEntry>> {
        let entries = self.get_entries().await?;
        Ok(entries.into_iter().filter(|e| e.file_path == file_path).collect())
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

        let entry =
            AuditEntry::new(".gitignore", AccessType::Write, AuditOutcome::Blocked, "write_file", "test-session", "");

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
                format!(".env.{i}"),
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
        let entry =
            AuditEntry::new(".bashrc", AccessType::Write, AuditOutcome::UserRejected, "shell", "sess-123", "prev-hash")
                .finalize();

        assert!(entry.verify());
    }

    /// After a failed write (file is read-only), `last_hash` must remain at
    /// the old value so the next successful entry chains correctly. This
    /// verifies the fix for the chain-corruption bug where `last_hash` was
    /// updated before the durable write.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_failed_write_preserves_chain() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let log_path = dir.path().join("audit.log");

        let log = AuditLog::new(&log_path).await.unwrap();

        // Write one entry successfully.
        let entry1 =
            AuditEntry::new(".gitignore", AccessType::Write, AuditOutcome::Blocked, "write_file", "test-session", "");
        log.log(entry1).await.unwrap();

        let entries = log.get_entries().await.unwrap();
        assert_eq!(entries.len(), 1);
        let good_hash = entries[0].entry_hash.clone().unwrap();

        // Make the log file read-only so append-mode open fails.
        std::fs::set_permissions(&log_path, std::fs::Permissions::from_mode(0o444)).unwrap();

        // Attempt a write — it must fail.
        let entry2 =
            AuditEntry::new(".env", AccessType::Modify, AuditOutcome::Blocked, "write_file", "test-session", "");
        let result = log.log(entry2).await;
        assert!(result.is_err(), "write to a read-only log should fail");

        // Restore write permission.
        std::fs::set_permissions(&log_path, std::fs::Permissions::from_mode(0o644)).unwrap();

        // The next successful entry must chain against the first entry's hash,
        // not the failed entry's hash.
        let entry3 =
            AuditEntry::new(".bashrc", AccessType::Read, AuditOutcome::AllowedWithConfirmation, "read_file", "s2", "");
        log.log(entry3).await.unwrap();

        let entries = log.get_entries().await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].previous_hash, good_hash, "chain must link to the last successful entry");
        assert!(log.verify_integrity().await.unwrap(), "integrity must be intact");
    }

    /// Concurrent log calls must be serialized — entries must form a valid
    /// chain with no duplicate or broken `previous_hash` links.
    #[tokio::test]
    async fn test_concurrent_writes_form_valid_chain() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("audit.log");
        let log = Arc::new(AuditLog::new(&log_path).await.unwrap());

        let mut handles = Vec::new();
        for i in 0..8 {
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                let entry = AuditEntry::new(
                    format!(".env.{i}"),
                    AccessType::Modify,
                    AuditOutcome::Blocked,
                    "test_tool",
                    "test-session",
                    "",
                );
                log.log(entry).await
            }));
        }
        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        let entries = log.get_entries().await.unwrap();
        assert_eq!(entries.len(), 8);
        assert!(log.verify_integrity().await.unwrap(), "all 8 concurrent entries must form a valid chain");
    }

    /// `append_entry_blocking` must write a valid single JSON line followed
    /// by `\n`. This verifies the pre-built-line + `write_all` approach.
    #[test]
    fn test_append_entry_blocking_writes_valid_line() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("audit.log");

        let entry = AuditEntry::new(".env", AccessType::Write, AuditOutcome::Blocked, "test", "s1", "").finalize();
        let json = serde_json::to_string(&entry).unwrap();

        AuditLog::append_entry_blocking(&log_path, &json).unwrap();

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.ends_with('\n'), "line must be newline-terminated");
        let line = content.trim_end();
        let parsed: AuditEntry = serde_json::from_str(line).expect("written line must be valid JSON");
        assert_eq!(parsed.file_path, ".env");
    }

    /// `append_entry_blocking` must append to an existing file without
    /// corrupting prior entries.
    #[test]
    fn test_append_entry_blocking_appends_correctly() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("audit.log");

        for i in 0..3 {
            let entry =
                AuditEntry::new(format!(".env.{i}"), AccessType::Modify, AuditOutcome::Blocked, "test", "s1", "")
                    .finalize();
            let json = serde_json::to_string(&entry).unwrap();
            AuditLog::append_entry_blocking(&log_path, &json).unwrap();
        }

        let content = std::fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 3, "must have exactly 3 lines");
        for (i, line) in lines.iter().enumerate() {
            let entry: AuditEntry = serde_json::from_str(line).unwrap();
            assert_eq!(entry.file_path, format!(".env.{i}"));
        }
    }

    /// `append_entry_blocking` must not leave a malformed tail when writing
    /// to a directory that doesn't exist (open fails). The file simply won't
    /// be created — no partial state.
    #[test]
    fn test_append_entry_blocking_missing_dir_is_clean_failure() {
        let dir = tempdir().unwrap();
        let nonexistent = dir.path().join("nonexistent_dir").join("audit.log");

        let entry = AuditEntry::new(".env", AccessType::Write, AuditOutcome::Blocked, "test", "s1", "").finalize();
        let json = serde_json::to_string(&entry).unwrap();

        let result = AuditLog::append_entry_blocking(&nonexistent, &json);
        assert!(result.is_err(), "writing to a missing directory must fail");
        assert!(!nonexistent.exists(), "no file should be created on failure");
    }
}
