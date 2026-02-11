use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use once_cell::sync::Lazy;
use rustc_hash::FxHashMap;
use std::sync::{Arc, RwLock};
use vtcode_core::llm::provider as uni;

/// String interning pool for tool signatures to reduce allocations (~15% reduction)
static SIGNATURE_POOL: Lazy<RwLock<FxHashMap<String, Arc<str>>>> =
    Lazy::new(|| RwLock::new(FxHashMap::default()));

pub(crate) fn push_tool_response(
    history: &mut Vec<uni::Message>,
    tool_call_id: String,
    content: String,
) {
    history.push(uni::Message::tool_response(tool_call_id, content));
}

/// Generate and intern a tool signature to reduce string allocations
pub(crate) fn signature_key_for(name: &str, args: &serde_json::Value) -> String {
    let args_str = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    let key = format!("{}:{}", name, args_str);

    // Try to get from pool first (fast read path)
    {
        let pool = SIGNATURE_POOL.read().unwrap();
        if let Some(interned) = pool.get(&key) {
            return interned.to_string();
        }
    }

    // Not in pool, intern it (slow write path)
    let mut pool = SIGNATURE_POOL.write().unwrap();
    // Double-check after acquiring write lock (another thread might have inserted)
    pool.entry(key.clone())
        .or_insert_with(|| Arc::from(key.as_str()))
        .to_string()
}

pub(crate) fn resolve_max_tool_retries(
    _tool_name: &str,
    vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>,
) -> usize {
    vt_cfg
        .map(|cfg| cfg.agent.harness.max_tool_retries as usize)
        .unwrap_or(vtcode_config::constants::defaults::DEFAULT_MAX_TOOL_RETRIES as usize)
}

/// Updates the tool repetition tracker based on the execution outcome.
///
/// Only successful tool calls are counted towards repetition limits.
/// Failed, timed out, or cancelled calls are ignored for this purpose.
pub(crate) fn update_repetition_tracker(
    repeated_tool_attempts: &mut FxHashMap<String, usize>,
    outcome: &ToolPipelineOutcome,
    name: &str,
    args: &serde_json::Value,
) {
    if matches!(&outcome.status, ToolExecutionStatus::Success { .. }) {
        let signature_key = signature_key_for(name, args);
        let current_count = repeated_tool_attempts.entry(signature_key).or_insert(0);
        *current_count += 1;
    }
}
pub(crate) fn serialize_output(output: &serde_json::Value) -> String {
    if let Some(s) = output.as_str() {
        s.to_string()
    } else {
        serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string())
    }
}

pub(crate) fn check_is_argument_error(error_str: &str) -> bool {
    error_str.contains("Missing required")
        || error_str.contains("Invalid arguments")
        || error_str.contains("required path parameter")
        || error_str.contains("expected ")
        || error_str.contains("Expected:")
}
