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
pub(crate) fn validate_tool_args_security(
    name: &str,
    args: &serde_json::Value,
) -> Option<Vec<String>> {
    use vtcode_core::tools::validation::{commands, paths};

    let mut failures = Vec::new();

    // 1. Check required arguments
    // -------------------------
    let required: &[&str] = match name {
        n if n == tool_names::READ_FILE => &["path"],
        n if n == tool_names::WRITE_FILE => &["path", "content"],
        n if n == tool_names::EDIT_FILE => &["path", "old_string", "new_string"],
        n if n == tool_names::LIST_FILES => &["path"],
        n if n == tool_names::GREP_FILE => &["pattern", "path"],
        n if n == tool_names::CODE_INTELLIGENCE => &["operation"],
        n if n == tool_names::RUN_PTY_CMD => &["command"],
        n if n == tool_names::APPLY_PATCH => &["patch"],
        _ => &[],
    };

    if !required.is_empty() {
        let missing: Vec<&str> = required
            .iter()
            .filter(|key| {
                args.get(*key)
                    .map(|v| v.is_null() || (v.is_string() && v.as_str().unwrap_or("").is_empty()))
                    .unwrap_or(true)
            })
            .copied()
            .collect();

        for m in missing {
            failures.push(format!("Missing required argument: {}", m));
        }
    }

    if !failures.is_empty() {
        return Some(failures);
    }

    // 2. Perform security checks
    // --------------------------

    // Path safety checks
    if let Some(path) = args.get("path").and_then(|v| v.as_str())
        && let Err(e) = paths::validate_path_safety(path)
    {
        failures.push(format!("Path security check failed: {}", e));
    }

    // Command safety checks
    // Check both 'command' argument and specific tool usage
    if name == tool_names::RUN_PTY_CMD
        && let Some(cmd) = args.get("command").and_then(|v| v.as_str())
        && let Err(e) = commands::validate_command_safety(cmd)
    {
        failures.push(format!("Command security check failed: {}", e));
    }

    if failures.is_empty() {
        None
    } else {
        Some(failures)
    }
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
) -> TurnHandlerOutcome {
    use vtcode_core::llm::provider as uni;
    // --- Turn balancer: cap low-signal churn ---
    let max_tool_loops = ctx
        .vt_cfg
        .map(|cfg| cfg.tools.max_tool_loops)
        .filter(|&value| value > 0)
        .unwrap_or(vtcode_core::config::constants::defaults::DEFAULT_MAX_TOOL_LOOPS);

    let tool_repeat_limit = ctx
        .vt_cfg
        .map(|cfg| cfg.tools.max_repeated_tool_calls)
        .filter(|&value| value > 0)
        .unwrap_or(vtcode_core::config::constants::defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS);

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
