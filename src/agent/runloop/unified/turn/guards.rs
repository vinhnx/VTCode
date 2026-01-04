use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};
use anyhow::Result;
use std::collections::HashMap;
use vtcode_core::utils::ansi::MessageStyle;

use vtcode_core::config::constants::tools as tool_names;

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
) -> Option<Vec<String>> {
    use vtcode_core::tools::validation::{commands, paths};

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
            let is_missing = args
                .get(*key)
                .map(|v| v.is_null() || (v.is_string() && v.as_str().unwrap_or("").is_empty()))
                .unwrap_or(true);

            if is_missing {
                failures
                    .get_or_insert_with(|| Vec::with_capacity(4))
                    .push(format!("Missing required argument: {}", key));
            }
        }
    }

    // Early return if required args are missing
    if failures.is_some() {
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
    let result = validate_tool_args_security(name, args);
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

pub(crate) async fn handle_turn_balancer(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    repeated_tool_attempts: &mut HashMap<String, usize>,
    max_tool_loops: usize,
    tool_repeat_limit: usize,
) -> TurnHandlerOutcome {
    use vtcode_core::llm::provider as uni;
    // --- Turn balancer: cap low-signal churn ---

    if crate::agent::runloop::unified::turn::utils::should_trigger_turn_balancer(
        step_count,
        max_tool_loops,
        repeated_tool_attempts.values().copied().max().unwrap_or(0),
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
