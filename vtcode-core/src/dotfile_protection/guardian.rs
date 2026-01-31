//! Dotfile Guardian - The core protection mechanism.
//!
//! Provides comprehensive protection decisions for dotfile access,
//! integrating audit logging, backup management, and cascade prevention.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::audit::{AccessType, AuditEntry, AuditLog, AuditOutcome};
use super::backup::BackupManager;
use vtcode_config::core::DotfileProtectionConfig;

/// Global dotfile guardian instance.
static GLOBAL_GUARDIAN: OnceCell<Arc<DotfileGuardian>> = OnceCell::new();

/// Initialize the global dotfile guardian.
///
/// Should be called once at application startup. Subsequent calls are ignored.
pub async fn init_global_guardian(config: DotfileProtectionConfig) -> Result<()> {
    if GLOBAL_GUARDIAN.get().is_some() {
        return Ok(());
    }

    let guardian = DotfileGuardian::new(config).await?;
    let _ = GLOBAL_GUARDIAN.set(Arc::new(guardian));
    Ok(())
}

/// Get the global dotfile guardian.
///
/// Returns None if the guardian hasn't been initialized.
pub fn get_global_guardian() -> Option<Arc<DotfileGuardian>> {
    GLOBAL_GUARDIAN.get().cloned()
}

/// Check if a path is a protected dotfile using the global guardian.
///
/// Returns false if the guardian hasn't been initialized.
pub fn is_protected_dotfile(path: &Path) -> bool {
    GLOBAL_GUARDIAN
        .get()
        .map(|g| g.is_protected(path))
        .unwrap_or(false)
}

/// Decision from the dotfile guardian.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectionDecision {
    /// Access allowed (file is not a dotfile or protection is disabled).
    Allowed,
    /// Access requires explicit user confirmation.
    RequiresConfirmation(ConfirmationRequest),
    /// Access requires secondary authentication (for whitelisted files).
    RequiresSecondaryAuth(ConfirmationRequest),
    /// Access is blocked (during automation or cascading modification).
    Blocked(ProtectionViolation),
    /// Access is denied (policy violation).
    Denied(ProtectionViolation),
}

impl ProtectionDecision {
    /// Check if access is allowed without any user interaction.
    pub fn is_allowed(&self) -> bool {
        matches!(self, ProtectionDecision::Allowed)
    }

    /// Check if any form of confirmation is required.
    pub fn requires_confirmation(&self) -> bool {
        matches!(
            self,
            ProtectionDecision::RequiresConfirmation(_)
                | ProtectionDecision::RequiresSecondaryAuth(_)
        )
    }

    /// Check if access is blocked or denied.
    pub fn is_blocked(&self) -> bool {
        matches!(
            self,
            ProtectionDecision::Blocked(_) | ProtectionDecision::Denied(_)
        )
    }
}

/// Request for user confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmationRequest {
    /// Path to the dotfile.
    pub file_path: String,
    /// Type of access being requested.
    pub access_type: String,
    /// Detailed description of proposed changes.
    pub proposed_changes: String,
    /// Tool or operation requesting access.
    pub initiator: String,
    /// Why this file is protected.
    pub protection_reason: String,
    /// Whether this is a whitelisted file (requires secondary auth).
    pub is_whitelisted: bool,
    /// Warning message for the user.
    pub warning: String,
}

/// A protection violation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectionViolation {
    /// Path to the dotfile.
    pub file_path: String,
    /// Type of access attempted.
    pub access_type: String,
    /// Reason for the violation.
    pub reason: String,
    /// Suggested action.
    pub suggestion: String,
}

impl std::fmt::Display for ProtectionViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Dotfile protection violation for '{}': {}. {}",
            self.file_path, self.reason, self.suggestion
        )
    }
}

impl std::error::Error for ProtectionViolation {}

/// Context for a dotfile access request.
#[derive(Debug, Clone)]
pub struct AccessContext {
    /// Path to the dotfile being accessed.
    pub file_path: PathBuf,
    /// Type of access being requested.
    pub access_type: AccessType,
    /// Tool or operation requesting access.
    pub initiator: String,
    /// Session identifier.
    pub session_id: String,
    /// Description of proposed changes.
    pub proposed_changes: Option<String>,
    /// Whether this is during an automated operation.
    pub is_automated: bool,
    /// Whether this is a cascading modification.
    pub is_cascading: bool,
    /// Parent file that triggered this modification (if cascading).
    pub triggered_by: Option<PathBuf>,
}

impl AccessContext {
    /// Create a new access context.
    pub fn new(
        file_path: impl Into<PathBuf>,
        access_type: AccessType,
        initiator: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            file_path: file_path.into(),
            access_type,
            initiator: initiator.into(),
            session_id: session_id.into(),
            proposed_changes: None,
            is_automated: false,
            is_cascading: false,
            triggered_by: None,
        }
    }

    /// Set proposed changes.
    pub fn with_proposed_changes(mut self, changes: impl Into<String>) -> Self {
        self.proposed_changes = Some(changes.into());
        self
    }

    /// Mark as automated operation.
    pub fn as_automated(mut self) -> Self {
        self.is_automated = true;
        self
    }

    /// Mark as cascading modification.
    pub fn as_cascading(mut self, triggered_by: impl Into<PathBuf>) -> Self {
        self.is_cascading = true;
        self.triggered_by = Some(triggered_by.into());
        self
    }
}

/// The Dotfile Guardian.
///
/// Central protection mechanism that:
/// - Detects protected dotfiles
/// - Enforces confirmation requirements
/// - Logs all access attempts
/// - Manages backups
/// - Prevents cascading modifications
pub struct DotfileGuardian {
    /// Configuration.
    config: DotfileProtectionConfig,
    /// Audit log.
    audit_log: Option<Arc<AuditLog>>,
    /// Backup manager.
    backup_manager: Option<Arc<BackupManager>>,
    /// Files modified in current session (for cascade detection).
    modified_files: Arc<Mutex<HashSet<PathBuf>>>,
    /// Pending modifications (waiting for confirmation).
    pending_modifications: Arc<Mutex<HashSet<PathBuf>>>,
}

impl DotfileGuardian {
    /// Expand tilde (~) in paths to home directory.
    fn expand_path(path: &str) -> String {
        if path.starts_with("~/") {
            if let Some(home) = dirs::home_dir() {
                return home.join(&path[2..]).to_string_lossy().into_owned();
            }
        }
        path.to_string()
    }

    /// Create a new dotfile guardian with the given configuration.
    pub async fn new(config: DotfileProtectionConfig) -> Result<Self> {
        let audit_log = if config.audit_logging_enabled {
            let log_path = Self::expand_path(&config.audit_log_path);
            Some(Arc::new(
                AuditLog::new(&log_path)
                    .await
                    .with_context(|| "Failed to initialize dotfile audit log")?,
            ))
        } else {
            None
        };

        let backup_manager = if config.create_backups {
            let backup_dir = Self::expand_path(&config.backup_directory);
            Some(Arc::new(
                BackupManager::new(&backup_dir, config.max_backups_per_file)
                    .await
                    .with_context(|| "Failed to initialize dotfile backup manager")?,
            ))
        } else {
            None
        };

        Ok(Self {
            config,
            audit_log,
            backup_manager,
            modified_files: Arc::new(Mutex::new(HashSet::new())),
            pending_modifications: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    /// Create a guardian with default configuration.
    pub async fn with_defaults() -> Result<Self> {
        Self::new(DotfileProtectionConfig::default()).await
    }

    /// Check if a file path is a protected dotfile.
    pub fn is_protected(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        self.config.is_protected(&path_str)
    }

    /// Check if a file is whitelisted.
    pub fn is_whitelisted(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        self.config.is_whitelisted(&path_str)
    }

    /// Request access to a dotfile.
    ///
    /// Returns a protection decision that must be handled by the caller.
    pub async fn request_access(&self, context: &AccessContext) -> Result<ProtectionDecision> {
        // Check if protection is enabled
        if !self.config.enabled {
            self.log_access(context, AuditOutcome::AllowedUnprotected)
                .await?;
            return Ok(ProtectionDecision::Allowed);
        }

        // Check if this is a protected file
        if !self.is_protected(&context.file_path) {
            return Ok(ProtectionDecision::Allowed);
        }

        // Check for cascading modification
        if self.config.prevent_cascading_modifications && context.is_cascading {
            let violation = ProtectionViolation {
                file_path: context.file_path.to_string_lossy().into_owned(),
                access_type: format!("{}", context.access_type),
                reason: format!(
                    "Cascading modification blocked. This change was triggered by modifying '{}'",
                    context
                        .triggered_by
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "unknown".to_string())
                ),
                suggestion: "Modify each dotfile independently with explicit confirmation."
                    .to_string(),
            };
            self.log_access(context, AuditOutcome::Blocked).await?;
            return Ok(ProtectionDecision::Blocked(violation));
        }

        // Check if blocked during automation
        if self.config.block_during_automation && context.is_automated {
            let violation = ProtectionViolation {
                file_path: context.file_path.to_string_lossy().into_owned(),
                access_type: format!("{}", context.access_type),
                reason: format!(
                    "Dotfile modification blocked during automated operation ({})",
                    context.initiator
                ),
                suggestion: "Modify dotfiles manually or use explicit commands.".to_string(),
            };
            self.log_access(context, AuditOutcome::Blocked).await?;
            return Ok(ProtectionDecision::Blocked(violation));
        }

        // Build confirmation request
        let request = ConfirmationRequest {
            file_path: context.file_path.to_string_lossy().into_owned(),
            access_type: format!("{}", context.access_type),
            proposed_changes: context
                .proposed_changes
                .clone()
                .unwrap_or_else(|| "No details provided".to_string()),
            initiator: context.initiator.clone(),
            protection_reason: self.get_protection_reason(&context.file_path),
            is_whitelisted: self.is_whitelisted(&context.file_path),
            warning: self.build_warning_message(context),
        };

        // Track pending modification
        {
            let mut pending = self.pending_modifications.lock().await;
            pending.insert(context.file_path.clone());
        }

        if self.is_whitelisted(&context.file_path)
            && self.config.require_secondary_auth_for_whitelist
        {
            Ok(ProtectionDecision::RequiresSecondaryAuth(request))
        } else if self.config.require_explicit_confirmation {
            Ok(ProtectionDecision::RequiresConfirmation(request))
        } else {
            // Protection enabled but no confirmation required (unusual config)
            self.log_access(context, AuditOutcome::AllowedUnprotected)
                .await?;
            Ok(ProtectionDecision::Allowed)
        }
    }

    /// Record that user confirmed the modification.
    pub async fn confirm_modification(
        &self,
        context: &AccessContext,
        is_whitelisted: bool,
    ) -> Result<()> {
        // Create backup before modification
        if let Some(ref backup_manager) = self.backup_manager {
            if context.file_path.exists() {
                backup_manager
                    .create_backup(
                        &context.file_path,
                        format!("Before {} by {}", context.access_type, context.initiator),
                        &context.session_id,
                    )
                    .await?;
            }
        }

        // Log the confirmed access
        let outcome = if is_whitelisted {
            AuditOutcome::AllowedViaWhitelist
        } else {
            AuditOutcome::AllowedWithConfirmation
        };
        self.log_access(context, outcome).await?;

        // Track modified file (for cascade detection)
        {
            let mut modified = self.modified_files.lock().await;
            modified.insert(context.file_path.clone());
        }

        // Remove from pending
        {
            let mut pending = self.pending_modifications.lock().await;
            pending.remove(&context.file_path);
        }

        Ok(())
    }

    /// Record that user rejected the modification.
    pub async fn reject_modification(&self, context: &AccessContext) -> Result<()> {
        self.log_access(context, AuditOutcome::UserRejected).await?;

        // Remove from pending
        {
            let mut pending = self.pending_modifications.lock().await;
            pending.remove(&context.file_path);
        }

        Ok(())
    }

    /// Check if modifying a file would trigger a cascade.
    pub async fn would_cascade(&self, file_path: &Path) -> bool {
        if !self.config.prevent_cascading_modifications {
            return false;
        }

        let modified = self.modified_files.lock().await;
        !modified.is_empty() && self.is_protected(file_path)
    }

    /// Get the most recent backup for a file.
    pub async fn get_latest_backup(
        &self,
        file_path: &Path,
    ) -> Result<Option<super::backup::DotfileBackup>> {
        match &self.backup_manager {
            Some(manager) => manager.get_latest_backup(file_path).await,
            None => Ok(None),
        }
    }

    /// Restore a file from its most recent backup.
    pub async fn restore_from_backup(&self, file_path: &Path) -> Result<()> {
        let manager = self
            .backup_manager
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Backup manager not enabled"))?;

        manager.restore_latest(file_path).await
    }

    /// Get audit entries for a file.
    pub async fn get_audit_history(&self, file_path: &str) -> Result<Vec<AuditEntry>> {
        match &self.audit_log {
            Some(log) => log.get_entries_for_file(file_path).await,
            None => Ok(Vec::new()),
        }
    }

    /// Verify audit log integrity.
    pub async fn verify_audit_integrity(&self) -> Result<bool> {
        match &self.audit_log {
            Some(log) => log.verify_integrity().await,
            None => Ok(true),
        }
    }

    /// Reset session state (for new conversation).
    pub async fn reset_session(&self) {
        let mut modified = self.modified_files.lock().await;
        modified.clear();

        let mut pending = self.pending_modifications.lock().await;
        pending.clear();
    }

    /// Get list of files modified in this session.
    pub async fn get_modified_files(&self) -> Vec<PathBuf> {
        let modified = self.modified_files.lock().await;
        modified.iter().cloned().collect()
    }

    /// Log an access attempt.
    async fn log_access(&self, context: &AccessContext, outcome: AuditOutcome) -> Result<()> {
        if let Some(ref log) = self.audit_log {
            let mut entry = AuditEntry::new(
                context.file_path.to_string_lossy().as_ref(),
                context.access_type,
                outcome,
                &context.initiator,
                &context.session_id,
                "",
            );

            if let Some(ref changes) = context.proposed_changes {
                entry = entry.with_proposed_changes(changes);
            }

            if context.is_automated {
                entry = entry.during_automation();
            }

            if context.is_cascading {
                if let Some(ref triggered_by) = context.triggered_by {
                    entry = entry.with_context(format!(
                        "Cascading from: {}",
                        triggered_by.to_string_lossy()
                    ));
                }
            }

            log.log(entry).await?;
        }

        Ok(())
    }

    /// Get a human-readable reason why a file is protected.
    fn get_protection_reason(&self, path: &Path) -> String {
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        if filename.starts_with(".git") {
            "Git configuration file - changes may affect repository behavior".to_string()
        } else if filename.starts_with(".env") {
            "Environment configuration - may contain secrets or critical settings".to_string()
        } else if filename.contains("ssh") || filename.contains("gpg") {
            "Security-sensitive file - may contain credentials or keys".to_string()
        } else if filename.contains("rc") || filename.contains("profile") {
            "Shell configuration - changes may affect system behavior".to_string()
        } else if filename.contains("config") {
            "Configuration file - changes may affect tool behavior".to_string()
        } else {
            "Hidden configuration file - modifications require explicit approval".to_string()
        }
    }

    /// Build a warning message for the user.
    fn build_warning_message(&self, context: &AccessContext) -> String {
        let filename = context
            .file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        format!(
            "DOTFILE PROTECTION WARNING\n\n\
             The AI agent '{}' is requesting to {} the protected file '{}'.\n\n\
             This is a hidden configuration file that could affect your system, \
             development environment, or contain sensitive information.\n\n\
             Proposed changes:\n{}\n\n\
             Please review carefully before approving.",
            context.initiator,
            context.access_type.to_string().to_lowercase(),
            filename,
            context
                .proposed_changes
                .as_deref()
                .unwrap_or("No details provided")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn create_test_guardian() -> DotfileGuardian {
        let dir = tempdir().unwrap();
        let mut config = DotfileProtectionConfig::default();
        config.audit_log_path = dir.path().join("audit.log").to_string_lossy().into_owned();
        config.backup_directory = dir.path().join("backups").to_string_lossy().into_owned();

        DotfileGuardian::new(config).await.unwrap()
    }

    #[tokio::test]
    async fn test_protection_detection() {
        let guardian = create_test_guardian().await;

        assert!(guardian.is_protected(Path::new(".gitignore")));
        assert!(guardian.is_protected(Path::new(".env")));
        assert!(guardian.is_protected(Path::new(".bashrc")));
        assert!(guardian.is_protected(Path::new("/home/user/.ssh/config")));
        assert!(!guardian.is_protected(Path::new("README.md")));
    }

    #[tokio::test]
    async fn test_requires_confirmation() {
        let guardian = create_test_guardian().await;

        let context = AccessContext::new(
            ".gitignore",
            AccessType::Write,
            "write_file",
            "test-session",
        )
        .with_proposed_changes("Adding node_modules to ignore list");

        let decision = guardian.request_access(&context).await.unwrap();

        assert!(decision.requires_confirmation());
        if let ProtectionDecision::RequiresConfirmation(req) = decision {
            assert_eq!(req.file_path, ".gitignore");
            assert!(req.warning.contains("DOTFILE PROTECTION WARNING"));
        } else {
            panic!("Expected RequiresConfirmation");
        }
    }

    #[tokio::test]
    async fn test_blocks_during_automation() {
        let guardian = create_test_guardian().await;

        let context =
            AccessContext::new(".npmrc", AccessType::Write, "npm_install", "test-session")
                .as_automated();

        let decision = guardian.request_access(&context).await.unwrap();

        assert!(decision.is_blocked());
    }

    #[tokio::test]
    async fn test_blocks_cascading() {
        let guardian = create_test_guardian().await;

        // First modification
        let context1 = AccessContext::new(".gitignore", AccessType::Write, "test", "test-session");
        let _ = guardian.request_access(&context1).await.unwrap();
        guardian
            .confirm_modification(&context1, false)
            .await
            .unwrap();

        // Cascading modification
        let context2 =
            AccessContext::new(".gitattributes", AccessType::Write, "test", "test-session")
                .as_cascading(".gitignore");

        let decision = guardian.request_access(&context2).await.unwrap();
        assert!(decision.is_blocked());
    }

    #[tokio::test]
    async fn test_non_dotfile_allowed() {
        let guardian = create_test_guardian().await;

        let context =
            AccessContext::new("README.md", AccessType::Write, "write_file", "test-session");

        let decision = guardian.request_access(&context).await.unwrap();
        assert!(decision.is_allowed());
    }

    #[tokio::test]
    async fn test_disabled_protection() {
        let dir = tempdir().unwrap();
        let mut config = DotfileProtectionConfig::default();
        config.enabled = false;
        config.audit_log_path = dir.path().join("audit.log").to_string_lossy().into_owned();
        config.backup_directory = dir.path().join("backups").to_string_lossy().into_owned();

        let guardian = DotfileGuardian::new(config).await.unwrap();

        let context = AccessContext::new(
            ".gitignore",
            AccessType::Write,
            "write_file",
            "test-session",
        );

        let decision = guardian.request_access(&context).await.unwrap();
        assert!(decision.is_allowed());
    }
}
