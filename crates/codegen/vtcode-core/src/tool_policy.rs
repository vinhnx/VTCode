//! Tool policy management system
//!
//! This module manages user preferences for tool usage, storing choices in
//! the canonical user config directory's `tool-policy.json` to minimize repeated prompts while maintaining
//! user control overwhich tools the agent can use.

use indexmap::IndexSet;
use regex::Regex;

use crate::config::constants::tools;

pub use crate::config::core::tools::ToolPolicy;

mod alternative;
mod manager;
mod mcp;
mod model;
mod shell_approval;

pub use self::alternative::{
    AlternativeArgsPolicy, AlternativeDefaultPolicy, AlternativeToolPolicy, AlternativeToolPolicyConfig,
};

pub use self::manager::ToolPolicyManager;
pub use self::mcp::{McpPolicyStore, McpProviderPolicy};
pub use self::model::{
    APPROVAL_CACHE_MAX_ENTRIES, ApprovalCacheConfig, PermissionPromptHandler, ToolConstraints, ToolExecutionDecision,
    ToolPolicyConfig,
};

/// Memoized compiled approval regexes, keyed by the exact pattern list, so we
/// don't recompile `Regex::new` on every approval-policy check.
static APPROVAL_REGEX_CACHE: std::sync::Mutex<Option<(IndexSet<String>, Vec<Regex>)>> = std::sync::Mutex::new(None);

const AUTO_ALLOW_TOOLS: &[&str] = &[
    tools::START_PLANNING,
    tools::TASK_TRACKER,
    tools::MATRIX,
    tools::RECORD_DECISION,
    tools::READ_FILE,
    // This tool dispatches to the interactive front-end wizard. Asking the
    // user is safe and must not be intercepted by a second permission prompt.
    tools::REQUEST_USER_INPUT,
    // Whitelist the core execution tool itself; individual shell commands remain
    // gated by command/sandbox approval policy.
    tools::UNIFIED_EXEC,
    // Internal PTY helpers used by tests and internal workflows.
    tools::RUN_PTY_CMD,
    tools::CREATE_PTY_SESSION,
    tools::LIST_PTY_SESSIONS,
    tools::SEND_PTY_INPUT,
    tools::CLOSE_PTY_SESSION,
    "cargo_check",
    "cargo_test",
    "git_status",
    "git_diff",
    "git_log",
];

const SHELL_APPROVAL_SCOPE_MARKER: &str = "|sandbox_permissions=";
const DEFAULT_APPROVAL_SCOPE_SIGNATURE: &str = "sandbox_permissions=\"use_default\"|additional_permissions=null";
const KNOWN_MUTATING_COMMANDS: &[&str] = &[
    "awk", "cargo", "chmod", "chown", "cp", "curl", "dd", "install", "ln", "mkdir", "mv", "perl", "python", "python3",
    "rm", "rmdir", "rsync", "ruby", "sh", "bash", "zsh", "tee", "touch", "truncate", "wget",
];
const MUTATING_OPTION_HINTS: &[&str] = &[
    "--delete",
    "--exec",
    "--in-place",
    "--output",
    "--remove",
    "--write",
    "-delete",
    "-exec",
    "-execdir",
    "-i",
    "-o",
];

#[cfg(test)]
mod tests;
