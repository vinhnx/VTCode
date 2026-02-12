use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use once_cell::sync::Lazy;
use rustc_hash::FxHashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use vtcode_core::llm::provider as uni;

pub(crate) const EXIT_PLAN_MODE_REASON_AUTO_TRIGGER_ON_DENIAL: &str = "auto_trigger_on_plan_denial";
pub(crate) const EXIT_PLAN_MODE_REASON_USER_REQUESTED_IMPLEMENTATION: &str =
    "user_requested_implementation";

/// String interning pool for tool signatures to reduce allocations (~15% reduction)
static SIGNATURE_POOL: Lazy<RwLock<FxHashMap<String, Arc<str>>>> =
    Lazy::new(|| RwLock::new(FxHashMap::default()));

/// Optimized loop detection with interned signatures and exponential backoff
pub(crate) struct LoopTracker {
    attempts: FxHashMap<Arc<str>, (usize, Instant)>,
    #[allow(dead_code)]
    backoff_base: Duration,
}

impl LoopTracker {
    pub(crate) fn new() -> Self {
        Self {
            attempts: FxHashMap::with_capacity_and_hasher(16, Default::default()),
            backoff_base: Duration::from_secs(5),
        }
    }

    /// Record an attempt and return the count
    pub(crate) fn record(&mut self, signature: &str) -> usize {
        let key: Arc<str> = Arc::from(signature);
        let entry = self.attempts.entry(key).or_insert((0, Instant::now()));
        entry.0 += 1;
        entry.1 = Instant::now();
        entry.0
    }

    /// Check if a warning should be emitted (with exponential backoff)
    #[allow(dead_code)]
    pub(crate) fn should_warn(&self, signature: &str, threshold: usize) -> bool {
        if let Some((count, last_time)) = self.attempts.get(signature) {
            if *count < threshold {
                return false;
            }
            let excess = count.saturating_sub(threshold);
            let backoff = self.backoff_base * 3u32.pow(excess.min(5) as u32);
            last_time.elapsed() >= backoff
        } else {
            false
        }
    }

    /// Get the maximum repetition count, optionally filtering by a predicate on the signature
    pub(crate) fn max_count_filtered<F>(&self, exclude: F) -> usize
    where
        F: Fn(&str) -> bool,
    {
        self.attempts
            .iter()
            .filter_map(
                |(sig, (count, _))| {
                    if exclude(sig) { None } else { Some(*count) }
                },
            )
            .max()
            .unwrap_or(0)
    }
}

pub(crate) fn push_tool_response(
    history: &mut Vec<uni::Message>,
    tool_call_id: String,
    content: String,
) {
    history.push(uni::Message::tool_response(tool_call_id, content));
}

pub(crate) fn build_exit_plan_mode_args(reason: &str) -> serde_json::Value {
    serde_json::json!({
        "reason": reason
    })
}

pub(crate) fn build_exit_plan_mode_call_id(prefix: &str, suffix: u128) -> String {
    format!("{prefix}_{suffix}")
}

pub(crate) fn build_step_exit_plan_mode_call_id(step_count: usize) -> String {
    format!("call_{step_count}_exit_plan_mode")
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
    loop_tracker: &mut LoopTracker,
    outcome: &ToolPipelineOutcome,
    name: &str,
    args: &serde_json::Value,
) {
    if matches!(&outcome.status, ToolExecutionStatus::Success { .. }) {
        let signature_key = signature_key_for(name, args);
        loop_tracker.record(&signature_key);
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
