use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};
use anyhow::Result;
use serde_json::Value;
use vtcode_core::utils::ansi::MessageStyle;

use std::sync::Arc;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::validation_cache::ValidationCache;

/// Validates that a textual tool call has required arguments before execution.
/// Returns `None` if valid, or `Some(missing_params)` if validation fails.
///
/// This prevents executing tools with empty args that will just fail,
/// allowing the Model to continue naturally instead of hitting loop detection.
/// Validates that a textual tool call has required arguments and passes security checks.
/// Returns `None` if valid, or `Some(failures)` if validation fails.
///
/// Optimization: Uses static slices for required params to avoid allocations
pub(crate) fn validate_tool_args_security(
    name: &str,
    args: &serde_json::Value,
    validation_cache: Option<&Arc<ValidationCache>>,
    tool_registry: Option<&vtcode_core::tools::ToolRegistry>,
) -> Option<Vec<String>> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use vtcode_core::tools::validation::{commands, paths};

    // Calculate hash for caching
    let args_hash = if validation_cache.is_some() {
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        // Optimization: For large arguments, hashing the string is still faster than
        // recursive Value hashing, but we can avoid to_string() if we use a
        // specialized hasher or if we use serde_json::to_writer.
        // For now, we keep it simple but acknowledge the allocation.
        args.to_string().hash(&mut hasher);
        Some(hasher.finish())
    } else {
        None
    };

    // Check cache
    if let Some(hash) = args_hash
        && let Some(cache) = validation_cache
    {
        // ValidationCache has interior mutability, use directly
        if let Some(is_valid) = cache.check(name, hash)
            && is_valid
        {
            return None; // Valid cached
        }
        // If invalid (false), we continue to re-validate to generate error messages
    }

    if let Some(registry) = tool_registry {
        let preflight = registry.preflight_validate_call(name, args);
        match preflight {
            Ok(_) => {
                if let Some(hash) = args_hash
                    && let Some(cache) = validation_cache
                {
                    cache.insert(name, hash, true);
                }
                return None;
            }
            Err(err) => {
                return Some(vec![err.to_string()]);
            }
        }
    }

    use vtcode_core::config::constants::tools as tool_names;

    // Optimization: Early return for tools with no requirements
    static EMPTY_REQUIRED: &[&str] = &[];

    // 1. Check required arguments using static slices
    let required: &[&str] = match name {
        n if n == tool_names::READ_FILE => &["path"],
        n if n == tool_names::WRITE_FILE => &["path", "content"],
        n if n == tool_names::EDIT_FILE => &["path", "old_string", "new_string"],
        n if n == tool_names::LIST_FILES => &["path"],
        n if n == tool_names::GREP_FILE => &["pattern", "path"],
        n if n == tool_names::CODE_INTELLIGENCE => &["operation"],
        n if n == tool_names::RUN_PTY_CMD => &["command"],
        n if n == tool_names::APPLY_PATCH => &["patch"],
        _ => EMPTY_REQUIRED,
    };

    // Optimization: Pre-allocate failures vec only when needed
    let mut failures: Option<Vec<String>> = None;

    if !required.is_empty() {
        for key in required {
            let is_missing = match args.get(*key) {
                Some(v) => {
                    v.is_null() || (v.is_string() && v.as_str().is_none_or(|s| s.is_empty()))
                }
                None => true,
            };

            if is_missing {
                failures
                    .get_or_insert_with(|| Vec::with_capacity(required.len()))
                    .push(format!("Missing required argument: {}", key));
            }
        }
    }

    // Early return if required args are missing
    if failures.is_some() {
        // Validation failed, no cache update (or cache as invalid if we wanted)
        return failures;
    }

    // 2. Perform security checks only if required args passed
    // Path safety checks
    if let Some(path) = args.get("path").and_then(|v| v.as_str())
        && let Err(e) = paths::validate_path_safety(path)
    {
        failures
            .get_or_insert_with(|| Vec::with_capacity(2))
            .push(format!("Path security check failed: {}", e));
    }

    // Command safety checks
    if name == tool_names::RUN_PTY_CMD
        && let Some(cmd) = args.get("command").and_then(|v| v.as_str())
        && let Err(e) = commands::validate_command_safety(cmd)
    {
        failures
            .get_or_insert_with(|| Vec::with_capacity(2))
            .push(format!("Command security check failed: {}", e));
    }

    // Update cache if valid
    if failures.is_none()
        && let Some(hash) = args_hash
        && let Some(cache) = validation_cache
    {
        // ValidationCache has interior mutability
        cache.insert(name, hash, true);
    }

    failures
}

// Deprecated: use validate_tool_args_security instead
#[allow(dead_code)]
pub(crate) fn validate_required_tool_args(
    name: &str,
    args: &serde_json::Value,
) -> Option<Vec<&'static str>> {
    // This function is kept for backward compatibility if needed,
    // but the logic is now superset in validate_tool_args_security.
    // We map the new result back to the old signature roughly.
    let result = validate_tool_args_security(name, args, None, None);
    result.map(|_failures| {
        // Convert dynamic strings to static error messages for compatibility
        // THIS IS A LOSS OF DETAIL but necessary to match signature.
        // Consumers should migrate to validate_tool_args_security.
        vec!["Validation failed (use validate_tool_args_security for details)"]
    })
}

pub(crate) async fn run_proactive_guards(
    ctx: &mut TurnProcessingContext<'_>,
    _step_count: usize,
) -> Result<()> {
    // Auto-prune decision ledgers to prevent unbounded memory growth
    {
        let mut decision_ledger = ctx.decision_ledger.write().await;
        decision_ledger.auto_prune();
    }

    // Context trim and compaction has been removed - no proactive guards needed
    // The function is kept for future extensibility but now does minimal work

    Ok(())
}

/// Check if a tool signature represents a read-only operation
/// Signature format: "tool_name:args_json" where args_json is serialized Value
fn is_readonly_signature(signature: &str) -> bool {
    // Split signature into tool name and args JSON
    let Some(colon_pos) = signature.find(':') else {
        return false;
    };
    let tool_name = &signature[..colon_pos];
    let args_json = &signature[colon_pos + 1..];

    // Always read-only tools (legacy and unified)
    if matches!(
        tool_name,
        tool_names::READ_FILE
            | tool_names::GREP_FILE
            | tool_names::LIST_FILES
            | tool_names::CODE_INTELLIGENCE
            | tool_names::SEARCH_TOOLS
            | tool_names::AGENT_INFO
            | tool_names::UNIFIED_SEARCH
    ) {
        return true;
    }

    // For unified tools, parse JSON args to check action parameter
    // Try parsing as JSON (Value Display may serialize with varying formats)
    if let Ok(args) = serde_json::from_str::<Value>(args_json) {
        // unified_file: read action is read-only
        if tool_name == tool_names::UNIFIED_FILE
            && let Some(action) = args.get("action").and_then(|v| v.as_str())
        {
            return action == "read";
        }
        // If no action but has mutating fields, it's not read-only
        // Default to mutating if unclear
        // unified_exec: poll and list actions are read-only
        if tool_name == tool_names::UNIFIED_EXEC
            && let Some(action) = args.get("action").and_then(|v| v.as_str())
        {
            return matches!(action, "poll" | "list");
        }
    } else {
        // If JSON parsing fails, fall back to string matching for robustness
        // This handles cases where Value Display might not produce valid JSON
        if tool_name == tool_names::UNIFIED_FILE {
            // Check for read action in various JSON formats
            let lower_json = args_json.to_lowercase();
            return lower_json.contains(r#""action":"read""#)
                || lower_json.contains(r#""action": "read""#)
                || lower_json.contains(r#"'action':'read'"#);
        }
        if tool_name == tool_names::UNIFIED_EXEC {
            let lower_json = args_json.to_lowercase();
            return lower_json.contains(r#""action":"poll""#)
                || lower_json.contains(r#""action":"list""#)
                || lower_json.contains(r#""action": "poll""#)
                || lower_json.contains(r#""action": "list""#);
        }
    }

    false
}

pub(crate) async fn handle_turn_balancer(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    repeated_tool_attempts: &mut crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker,
    max_tool_loops: usize,
    tool_repeat_limit: usize,
) -> TurnHandlerOutcome {
    use vtcode_core::llm::provider as uni;

    // Optimization: Skip check with exponential backoff to reduce iteration frequency
    // Check intervals: 1, 2, 4, 8, 16, 32... steps
    let check_interval = if step_count <= 4 {
        1 // Check every step in early turns
    } else {
        // Exponential backoff: 2^(floor(log2(step_count/4)))
        1_usize << ((step_count / 4).ilog2())
    };

    if !step_count.is_multiple_of(check_interval) {
        return TurnHandlerOutcome::Continue;
    }

    // --- Turn balancer: cap low-signal churn ---
    // Exclude read-only tools from repeated count (they're legitimate exploration)
    let max_repeated = repeated_tool_attempts.max_count_filtered(is_readonly_signature);

    if crate::agent::runloop::unified::turn::utils::should_trigger_turn_balancer(
        step_count,
        max_tool_loops,
        max_repeated,
        tool_repeat_limit,
    ) {
        ctx.renderer
            .line(
                MessageStyle::Info,
                "[!] Turn balancer: pausing due to repeated low-signal calls.",
            )
            .unwrap_or(()); // Best effort
        ctx.working_history.push(uni::Message::system(
            "Turn balancer paused turn after repeated low-signal calls.".to_string(),
        ));
        // Record in ledger
        {
            let mut ledger = ctx.decision_ledger.write().await;
            ledger.record_decision(
                "Turn balancer: Churn detected".to_string(),
                vtcode_core::core::decision_tracker::Action::Response {
                    content: "Turn balancer triggered.".to_string(),
                    response_type:
                        vtcode_core::core::decision_tracker::ResponseType::ContextSummary,
                },
                None,
            );
        }
        return TurnHandlerOutcome::Break(TurnLoopResult::Completed);
    }

    TurnHandlerOutcome::Continue
}
