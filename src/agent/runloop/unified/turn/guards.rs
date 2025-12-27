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
pub(crate) fn validate_required_tool_args(
    name: &str,
    args: &serde_json::Value,
) -> Option<Vec<&'static str>> {
    let required: &[&str] = match name {
        n if n == tool_names::READ_FILE => &["path"],
        n if n == tool_names::WRITE_FILE => &["path", "content"],
        n if n == tool_names::EDIT_FILE => &["path", "old_string", "new_string"],
        n if n == tool_names::LIST_FILES => &["path"],
        n if n == tool_names::GREP_FILE => &["pattern", "path"], // Require path for grep to avoid searching whole project root by default
        n if n == tool_names::CODE_INTELLIGENCE => &["operation"], // Operation is always required
        n if n == tool_names::RUN_PTY_CMD => &["command"],
        n if n == tool_names::APPLY_PATCH => &["patch"],
        _ => &[],
    };

    if required.is_empty() {
        return None;
    }

    let missing: Vec<&'static str> = required
        .iter()
        .filter(|key| {
            args.get(*key)
                .map(|v| v.is_null() || (v.is_string() && v.as_str().unwrap_or("").is_empty()))
                .unwrap_or(true)
        })
        .copied()
        .collect();

    if missing.is_empty() {
        None
    } else {
        Some(missing)
    }
}

pub(crate) async fn run_proactive_guards(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
) -> Result<()> {
    // Auto-prune decision ledgers to prevent unbounded memory growth
    {
        let mut decision_ledger = ctx.decision_ledger.write().await;
        decision_ledger.auto_prune();
    }
    
    // Acquire pruning ledger lock for the duration of the guards check
    // This allows us to record any proactive trims that occur
    let mut pruning_ledger = ctx.pruning_ledger.write().await;
    pruning_ledger.auto_prune();

    // Proactive token budget check - trim BEFORE consuming tokens
    // We implement a "check-trim-verify" loop here to ensure safety
    use crate::agent::runloop::unified::context_manager::PreRequestAction;

    let mut checks = 0;
    const MAX_CHECKS: usize = 3;

    loop {
        checks += 1;
        if checks > MAX_CHECKS {
            // Safety break
            tracing::warn!("Proactive guard loop limit reached");
            break;
        }

        let pre_check = ctx.context_manager.pre_request_check(ctx.working_history);
        match pre_check {
            PreRequestAction::Proceed => {
                break;
            }
            PreRequestAction::TrimLight => {
                tracing::debug!("Pre-request: light trim triggered at WARNING threshold");
                // Use adaptive_trim to handle recording to ledger automatically
                ctx.context_manager
                    .adaptive_trim(ctx.working_history, Some(&mut *pruning_ledger), step_count)
                    .await?;
            }
            PreRequestAction::TrimAggressive => {
                tracing::debug!("Pre-request: aggressive trim triggered at ALERT threshold");
                ctx.context_manager
                    .adaptive_trim(ctx.working_history, Some(&mut *pruning_ledger), step_count)
                    .await?;
            }
            PreRequestAction::Block => {
                ctx.renderer.line(
                    MessageStyle::Info,
                    "[!] Context usage critical - applying aggressive trim",
                )?;
                
                let outcome = ctx
                    .context_manager
                    .adaptive_trim(ctx.working_history, Some(&mut *pruning_ledger), step_count)
                    .await?;

                if !outcome.is_trimmed() {
                    // If blocked and cannot trim further, we must fail
                    if matches!(pre_check, PreRequestAction::Block) {
                        anyhow::bail!("Context budget exceeded and could not be resolved by trimming. Please summarize or reset the conversation.");
                    }
                    break;
                }
            }
        }
    }

    // Final safety check: ensure we didn't exit the loop (e.g. max checks) while still in a dangerous state
    let final_check = ctx.context_manager.pre_request_check(ctx.working_history);
    if matches!(final_check, PreRequestAction::Block) {
        anyhow::bail!("Context budget critical/overflow after trimming attempts. Conversation unsafe to proceed.");
    }

    Ok(())
}

pub(crate) async fn handle_turn_balancer(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    repeated_tool_attempts: &mut HashMap<String, usize>,
) -> TurnHandlerOutcome {
    use vtcode_core::llm::provider as uni;
    // --- Turn balancer: cap low-signal churn and request compaction if looping ---
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
                "[!] Turn balancer: pausing due to repeated low-signal calls; compacting context.",
            )
            .unwrap_or(()); // Best effort
        let _ = ctx
            .context_manager
            .adaptive_trim(ctx.working_history, None, step_count)
            .await;
        ctx.working_history.push(uni::Message::system(
            "Turn balancer paused turn after repeated low-signal calls.".to_string(),
        ));
        // Record in ledger
        {
            let mut ledger = ctx.decision_ledger.write().await;
            ledger.record_decision(
                "Turn balancer: Churn detected".to_string(),
                vtcode_core::core::decision_tracker::Action::Response {
                    content: "Turn balancer triggered; compacting context.".to_string(),
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
