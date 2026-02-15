//! Execution Policy Module
//!
//! Provides types and managers for controlling command execution authorization,
//! inspired by OpenAI Codex's execution policy patterns.
//!
//! # Architecture
//!
//! - `approval`: Approval requirement types (ExecApprovalRequirement, AskForApproval)
//! - `policy`: Policy definitions and rule matching (Policy, PrefixRule, Decision)
//! - `parser`: Policy file parsing (TOML, JSON, simple formats)
//! - `manager`: The central ExecPolicyManager coordinating all components
//!
//! # Example
//!
//! ```rust,ignore
//! use vtcode_core::exec_policy::{ExecPolicyManager, Decision};
//!
//! let manager = ExecPolicyManager::with_defaults(workspace_root);
//! manager.add_prefix_rule(&["cargo".to_string()], Decision::Allow).await?;
//!
//! let result = manager.check_approval(&["cargo", "build"]).await;
//! ```

mod approval;
pub mod command_validation;
mod manager;
mod parser;
mod policy;

pub use approval::{AskForApproval, ExecApprovalRequirement, ExecPolicyAmendment};
pub use manager::{ExecPolicyConfig, ExecPolicyManager, SharedExecPolicyManager};
pub use parser::{PolicyFile, PolicyParser, PolicyRule};
pub use policy::{Decision, Policy, PolicyEvaluation, PrefixRule, RuleMatch};
