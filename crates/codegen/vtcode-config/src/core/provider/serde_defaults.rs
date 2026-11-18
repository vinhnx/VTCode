//! Serde `default = "..."` helper functions for provider configuration.
//!
//! Grouped here so `provider/mod.rs` stays focused on the configuration types
//! themselves. Each helper is re-imported into the parent with `use`, which
//! keeps the string-named serde defaults resolving exactly as before.

#[inline]
pub(super) fn default_interleaved_thinking_beta() -> String {
    "interleaved-thinking-2025-05-14".to_string()
}

#[inline]
pub(super) fn default_interleaved_thinking_budget_tokens() -> u32 {
    crate::constants::context::DEFAULT_THINKING_BUDGET
}

#[inline]
pub(super) fn default_interleaved_thinking_type() -> String {
    "enabled".to_string()
}

#[inline]
pub(super) fn default_task_budget_beta() -> String {
    "task-budgets-2026-03-13".to_string()
}
