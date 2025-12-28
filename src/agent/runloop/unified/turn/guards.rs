use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};
use anyhow::Result;
use std::collections::HashMap;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::context::TrimPhase;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::llm::provider as uni;

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

    // Fix #5 Phase 4: Monitor memory pressure and apply cache eviction if needed
    let memory_pressure = ctx.context_manager.check_memory_pressure();
    if !matches!(memory_pressure, vtcode_core::memory::MemoryPressure::Normal) {
        tracing::debug!("Memory pressure detected: {:?}", memory_pressure);
        ctx.context_manager.record_memory_checkpoint(&format!("guard_check_{:?}", memory_pressure));
        
        // Log warning if critical
        if matches!(memory_pressure, vtcode_core::memory::MemoryPressure::Critical) {
            ctx.renderer.line(
                MessageStyle::Error,
                "[MEMORY] Critical memory pressure - applying aggressive cleanup",
            )?;
        }
    }

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
                let outcome = ctx
                    .context_manager
                    .adaptive_trim(ctx.working_history, Some(&mut *pruning_ledger), step_count)
                    .await?;

                if matches!(outcome.phase, TrimPhase::SummarizationRecommended) {
                    drop(pruning_ledger); // Release lock before summarization
                    attempt_context_summarization(ctx).await?;
                    pruning_ledger = ctx.pruning_ledger.write().await; // Re-acquire
                }
            }
            PreRequestAction::TrimAggressive => {
                tracing::debug!("Pre-request: aggressive trim triggered at ALERT threshold");
                let outcome = ctx
                    .context_manager
                    .adaptive_trim(ctx.working_history, Some(&mut *pruning_ledger), step_count)
                    .await?;

                if matches!(outcome.phase, TrimPhase::SummarizationRecommended) {
                    drop(pruning_ledger); // Release lock before summarization
                    attempt_context_summarization(ctx).await?;
                    pruning_ledger = ctx.pruning_ledger.write().await; // Re-acquire
                }
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

                if matches!(outcome.phase, TrimPhase::SummarizationRecommended) {
                    drop(pruning_ledger); // Release lock before summarization
                    attempt_context_summarization(ctx).await?;
                    pruning_ledger = ctx.pruning_ledger.write().await; // Re-acquire
                    // Continue loop to re-check budget
                    continue;
                }

                if !outcome.is_trimmed() {
                    // If blocked and cannot trim further, we must fail
                    if matches!(pre_check, PreRequestAction::Block) {
                        anyhow::bail!(
                            "Context budget exceeded and could not be resolved by trimming. Please summarize or reset the conversation."
                        );
                    }
                    break;
                }
            }
        }
    }

    // Final safety check: ensure we didn't exit the loop (e.g. max checks) while still in a dangerous state
    let final_check = ctx.context_manager.pre_request_check(ctx.working_history);
    if matches!(final_check, PreRequestAction::Block) {
        anyhow::bail!(
            "Context budget critical/overflow after trimming attempts. Conversation unsafe to proceed."
        );
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

async fn attempt_context_summarization(ctx: &mut TurnProcessingContext<'_>) -> Result<()> {
    // 1. Get indices of summarizable messages
    let indices = ctx
        .context_manager
        .get_summarizable_indices(ctx.working_history);
    if indices.is_empty() {
        return Ok(());
    }

    // 2. Identify contiguous range (simple greedy: take the longest sequence)
    // For now, simplistically take the first chunk of at least 3 messages
    let mut range_start = indices[0];
    let mut range_end = indices[0];
    let mut count = 1;
    let mut best_range = range_start..=range_end;
    let mut best_count = 1;

    for &idx in indices.iter().skip(1) {
        if idx == range_end + 1 {
            range_end = idx;
            count += 1;
        } else {
            if count > best_count {
                best_count = count;
                best_range = range_start..=range_end;
            }
            range_start = idx;
            range_end = idx;
            count = 1;
        }
    }
    if count > best_count {
        best_count = count;
        best_range = range_start..=range_end;
    }

    if best_count < 3 {
        // Not enough contiguous messages to justify summarization overhead
        return Ok(());
    }

    let start_idx = *best_range.start();
    let end_idx = *best_range.end(); // inclusive

    // Safety check
    if end_idx >= ctx.working_history.len() {
        return Ok(());
    }

    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "[Context] Summarizing {} messages (indices {}-{}) to save tokens...",
            best_count, start_idx, end_idx
        ),
    )?;

    // 3. Create summarization request
    let messages_to_summarize = &ctx.working_history[start_idx..=end_idx];

    let mut summary_request_messages = Vec::new();
    summary_request_messages.push(uni::Message::system("You are a helpful assistant. Please summarize the following conversation segment concisely, preserving key technical details, decisions, and tool outputs. Output ONLY the summary.".to_string()));
    summary_request_messages.extend_from_slice(messages_to_summarize);

    let request = uni::LLMRequest {
        messages: summary_request_messages,
        model: ctx
            .vt_cfg
            .as_ref()
            .map(|c| c.agent.small_model.model.clone())
            .unwrap_or_else(|| "claude-3-5-sonnet-latest".to_string()),
        max_tokens: Some(1000),
        temperature: Some(0.3),
        ..Default::default()
    };

    // 4. Execute LLM call
    let response = ctx.provider_client.generate(request).await;

    match response {
        Ok(resp) => {
            if let Some(text) = resp.content {
                let summary_msg =
                    uni::Message::system(format!("(Summary of previous interaction: {})", text));

                // 5. Replace messages
                // Remove the range
                // Determine how many to remove
                let remove_count = end_idx - start_idx + 1;
                ctx.working_history.drain(start_idx..=end_idx);
                // Insert summary
                ctx.working_history.insert(start_idx, summary_msg);

                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "[Context] Successfully summarized {} messages.",
                        remove_count
                    ),
                )?;
            }
        }
        Err(e) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("[Context] Summarization failed: {}", e),
            )?;
        }
    }

    Ok(())
}
