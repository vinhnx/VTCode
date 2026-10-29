pub(super) use std::sync::Arc;
pub(super) use std::thread;
pub(super) use std::time::Duration;

pub(super) use super::{
    AutoPermissionDenial, CtrlCSignal, CtrlCState, FollowUpPromptAction, PREFLIGHT_CORRECTIVE_FOLLOW_UP_PREFIX,
    PromptCacheDiagnostics, SessionStats, ValidationCategory, is_follow_up_prompt_like,
    should_enforce_safe_mode_prompts,
};
pub(super) use vtcode_core::config::WorkspaceTrustLevel;
pub(super) use vtcode_core::config::constants::tools;
pub(super) use vtcode_core::core::agent::request_envelope::SegmentBoundaryReason;
pub(super) use vtcode_core::llm::provider::ToolDefinition;

fn function_tool(name: &str) -> ToolDefinition {
    ToolDefinition::function(name.to_string(), name.to_string(), serde_json::json!({"type": "object"}))
}

mod cache_lineage;
mod ctrl_c_basic;
mod ctrl_c_cancel;
mod envelope_cache;
mod followup_permission;
mod observability_stats;
mod segments;
mod verification_recovery;
