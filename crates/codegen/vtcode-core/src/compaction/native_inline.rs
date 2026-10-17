//! Anthropic native inline compaction context edits.

use super::*;

/// Native inline compaction (Anthropic `compact_20260112`).
///
/// Forces a compaction pass by setting the minimum trigger threshold with
/// `pause_after_compaction: true`, so the response contains only the compaction
/// block. If compaction does not fire (history below the provider's minimum
/// trigger, currently 50k tokens for Anthropic), transparently falls back to
/// local summarization so the manual command always succeeds.
///
/// The inline request already carries the full parent history; the parent
/// system/tools prefix is attached as well so the fork hits the provider
/// prompt cache. `tool_choice` stays `none` so the pass cannot be spent on
/// tool calls, and both local fallbacks forward `parent` for the same reason.
/// Anthropic inline context-management triggers, matching the provider docs:
/// thinking blocks are cheapest to clear, tool results next, and full
/// compaction is the most aggressive last resort./// Native inline compaction (Anthropic `compact_20260112`).
///
/// Forces a compaction pass by setting the minimum trigger threshold with
/// `pause_after_compaction: true`, so the response contains only the compaction
/// block. If compaction does not fire (history below the provider's minimum
/// trigger, currently 50k tokens for Anthropic), transparently falls back to
/// local summarization so the manual command always succeeds.
///
/// The inline request already carries the full parent history; the parent
/// system/tools prefix is attached as well so the fork hits the provider
/// prompt cache. `tool_choice` stays `none` so the pass cannot be spent on
/// tool calls, and both local fallbacks forward `parent` for the same reason.
/// Anthropic inline context-management triggers, matching the provider docs:
/// thinking blocks are cheapest to clear, tool results next, and full
/// compaction is the most aggressive last resort.
const ANTHROPIC_COMPACT_TRIGGER_FLOOR: u64 = 50_000;
const ANTHROPIC_CLEAR_TOOL_USES_TRIGGER_TOKENS: u64 = 100_000;
const ANTHROPIC_CLEAR_TOOL_USES_KEEP: u64 = 3;
const ANTHROPIC_CLEAR_THINKING_KEEP_TURNS: u64 = 2;

/// Build the Anthropic inline `context_management.edits` ladder.
///
/// Order matters: the thinking-block edit runs first, then tool-result
/// clearing, then the `compact_20260112` summarization edit. The cheaper
/// clearing edits are only included when the provider reports
/// `supports_context_edits`; otherwise the request carries the compact edit
/// alone so non-Anthropic inline routes keep working.
pub(crate) fn anthropic_inline_compaction_edits(instructions: Option<&str>, include_context_edits: bool) -> Vec<Value> {
    let mut edits = Vec::with_capacity(3);
    if include_context_edits {
        edits.push(json!({
            "type": "clear_thinking_20251015",
            "keep": { "type": "thinking_turns", "value": ANTHROPIC_CLEAR_THINKING_KEEP_TURNS },
        }));
        edits.push(json!({
            "type": "clear_tool_uses_20250919",
            "trigger": { "type": "input_tokens", "value": ANTHROPIC_CLEAR_TOOL_USES_TRIGGER_TOKENS },
            "keep": { "type": "tool_uses", "value": ANTHROPIC_CLEAR_TOOL_USES_KEEP },
        }));
    }
    let mut compact_edit = serde_json::Map::new();
    compact_edit.insert("type".to_string(), json!("compact_20260112"));
    compact_edit
        .insert("trigger".to_string(), json!({ "type": "input_tokens", "value": ANTHROPIC_COMPACT_TRIGGER_FLOOR }));
    compact_edit.insert("pause_after_compaction".to_string(), json!(true));
    if let Some(instructions) = instructions.map(str::trim).filter(|instructions| !instructions.is_empty()) {
        compact_edit.insert("instructions".to_string(), json!(instructions));
    }
    edits.push(Value::Object(compact_edit));
    edits
}

pub(crate) async fn compact_history_native_inline(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
    options: &ManualCompactionOptions,
    context_budget: Option<usize>,
    parent: Option<&CompactionParentContext>,
) -> Result<(Vec<Message>, CompactionMode)> {
    let edits =
        anthropic_inline_compaction_edits(options.instructions.as_deref(), provider.supports_context_edits(model));

    // Bound the inline request input so a near-full history fits the
    // summarizer window, mirroring the local-summary fork bound.
    let inline_source = bound_history_for_native_compaction(
        history,
        options.instructions.as_deref().unwrap_or(""),
        compaction_history_budget(provider, model, context_budget),
    );
    let request = LLMRequest {
        messages: Arc::new(inline_source),
        model: model.to_string(),
        system_prompt: parent.and_then(|parent| parent.system_prompt.clone()),
        tools: parent.and_then(|parent| parent.tools.clone()).filter(|tools| !tools.is_empty()),
        tool_choice: Some(ToolChoice::none()),
        context_management: Some(json!({ "edits": edits })),
        max_tokens: options.max_output_tokens,
        reasoning_effort: options.reasoning_effort,
        verbosity: options.verbosity,
        ..Default::default()
    };

    // The inline compaction request is Anthropic-specific (`compact_20260112`).
    // If the provider does not actually support inline compaction (e.g. it was
    // selected because it exposes a different standalone Responses compact
    // endpoint) the request may be rejected. Per the manual `/compact` contract
    // ("always succeeds"), swallow the inline error and fall back to local
    // summarization rather than aborting the whole command.
    let response = match collect_single_response(provider, request).await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(
                error = ?error,
                "provider-native inline compaction request failed; \
                 falling back to local summarization"
            );
            let compacted =
                summarize_locally(provider, model, history, config, options, context_budget, parent).await?;
            return Ok((compacted, CompactionMode::Local));
        }
    };

    if response.finish_reason == FinishReason::Pause
        && let Some(summary) = response
            .compaction
            .as_ref()
            .map(|summary| summary.trim())
            .filter(|summary| !summary.is_empty())
    {
        let effective_config = context_bounded_compaction_config(provider, model, history, config, context_budget);
        let tail_target = route_tail_target_tokens(provider, model, context_budget);
        if let Some(detail) = response_compaction_detail(&response) {
            // The provider compaction block is opaque state. Preserve the exact
            // block and the selected protocol-safe tail for replay; applying the
            // generic local bound here could turn it into an ordinary text/system
            // message and invalidate the provider's continuation contract.
            return Ok((
                build_provider_compacted_history(history, detail, &effective_config, true, tail_target),
                CompactionMode::Provider,
            ));
        }

        // A provider may expose only the public summary field without the raw
        // continuation block. Treat that response as an ordinary local summary
        // so it receives the local context bound and is not mislabeled as a
        // provider-native replay window.
        let compacted = bound_compacted_history_to_context(
            build_summary_compacted_history(history, summary, &effective_config, true, tail_target),
            provider,
            model,
            context_budget,
        );
        return Ok((compacted, CompactionMode::Local));
    }

    // Compaction did not fire (e.g. history below the minimum trigger threshold);
    // fall back to local summarization so the manual command always succeeds.
    let compacted = summarize_locally(provider, model, history, config, options, context_budget, parent).await?;
    Ok((compacted, CompactionMode::Local))
}
