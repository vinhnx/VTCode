//! Comprehensive dotfile protection system.
//!
//! This module implements protection for hidden configuration files (dotfiles)
//! to prevent automatic or implicit modifications by AI agents or automated tools.
//!
//! Features:
//! - Explicit user confirmation with clear disclosure
//! - Immutable audit logging of all access attempts
//! - Whitelist mechanism with secondary authentication
//! - Cascade prevention (blocking chain reactions)
//! - Backup and restore functionality
//! - Permission preservation

mod audit;
mod backup;
mod guardian;

pub use audit::{AccessType, AuditEntry, AuditLog, AuditOutcome};
pub use backup::{BackupManager, DotfileBackup};
pub use guardian::{
    get_global_guardian, init_global_guardian, is_protected_dotfile, AccessContext,
    ConfirmationRequest, DotfileGuardian, ProtectionDecision, ProtectionViolation,
};
