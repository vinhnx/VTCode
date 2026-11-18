//! Unified automatic compaction orchestration.
//!
//! This is the single entry point used by every runtime (the binary unified
//! runloop and the `vtcode-core` `AgentRunner`) for *automatic* context
//! compaction: when token pressure crosses the configured threshold, the
//! conversation trace is compressed (provider-native or local LLM summary),
//! a recoverable history artifact is written, and a session memory envelope is
//! built and injected so the model keeps its conversational continuity.

use std::path::Path;

use anyhow::Result;

use crate::compaction::memory_envelope::{
    MemoryEnvelopePersistence, MemoryEnvelopePlacement, SessionMemoryEnvelopeUpdate,
    dedup_repeated_file_reads_for_local_compaction, effective_compaction_threshold_with_reserve,
    effective_context_budget, persist_memory_envelope_async_with_update, strip_existing_memory_envelope,
};
use crate::compaction::two_pass::fingerprint_prefix;
use crate::compaction::{
    CompactionConfig, CompactionParentContext, CompactionStrategy, ManualCompactionOptions, SUPPRESS_NONE,
    build_local_compacted_history, build_summary_prompt, classify_suppress_reason,
    compact_history_manual_with_parent_context, compacted_history_shrinks, manual_compaction_strategy,
};
use crate::exec::events::CompactionMode;
use crate::llm::{
    collect_single_response,
    provider::{LLMProvider, LLMRequest, Message, ToolChoice},
};
use vtcode_config::loader::VTCodeConfig;

/// Result of a successful automatic compaction pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoCompactionOutcome {
    pub original_len: usize,
    pub compacted_len: usize,
    pub mode: CompactionMode,
    pub history_artifact_path: Option<String>,
    /// The session memory envelope produced during compaction.
    /// Used by the caller to write persistent checkpoints.
    pub envelope: Option<crate::compaction::memory_envelope::SessionMemoryEnvelope>,
}

/// Inputs for [`auto_compact_messages`].
pub struct AutoCompactionInput<'a> {
    pub provider: &'a dyn LLMProvider,
    pub model: &'a str,
    pub session_id: &'a str,
    pub workspace_root: &'a Path,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    /// Current estimated token usage of the live history (before compaction).
    pub current_token_usage: usize,
    /// Output limit of the next request, excluding prompt/history overhead.
    pub reserved_output_tokens: usize,
    /// Files touched so far this session (enrich the memory envelope).
    pub touched_files: &'a [String],
    /// Engine configuration (thresholds, retention, summary prompt overrides).
    pub engine_cfg: CompactionConfig,
    /// Manual compaction options (instructions, max output, reasoning, verbosity).
    pub manual_options: ManualCompactionOptions,
    /// Where to place the injected memory envelope in the compacted history.
    pub placement: MemoryEnvelopePlacement,
    /// Prefire two-pass state (background NOTE₁ cache).
    pub prefire: Option<&'a crate::compaction::PrefireState>,
    /// Mutable auto-compaction suppression state. `SUPPRESS_NONE` allows
    /// compaction; any other value gates automatic compaction until cleared
    /// by success, model switch, or explicit `/compact`.
    pub auto_compact_suppressed: &'a mut u8,
    /// Force a pending soft-threshold compaction at the next outer turn
    /// boundary. This is set only by the runloop boundary, never by a tool
    /// loop, so no hidden model call is introduced mid-turn.
    pub force_compaction: bool,
    /// Live steering state to snapshot with the compaction envelope.
    pub steering_update: Option<&'a SessionMemoryEnvelopeUpdate>,
    /// Parent segment prefix for cache-safe forking (system prompt + ordered
    /// tools from the live request envelope). When supplied, the local-summary
    /// fork reuses the parent's cached prefix instead of re-paying full input
    /// cost. Native strategies ignore it.
    pub parent_context: Option<CompactionParentContext>,
}

impl AutoCompactionInput<'_> {
    /// Check shared gates before a caller requests copy-on-write history.
    pub(crate) fn should_compact(&self) -> bool {
        if !self.vt_cfg.is_some_and(|cfg| cfg.agent.harness.auto_compaction_enabled)
            || *self.auto_compact_suppressed != SUPPRESS_NONE
        {
            return false;
        }
        let Some(threshold) = effective_compaction_threshold_with_reserve(
            self.vt_cfg,
            self.provider,
            self.model,
            self.reserved_output_tokens,
        ) else {
            return false;
        };
        let window = effective_context_budget(self.vt_cfg, self.provider, self.model);
        let threshold = crate::compaction::CompactionRoutePolicy::resolve(self.provider.name(), self.model)
            .apply_threshold_cap(threshold, window);
        self.force_compaction || self.current_token_usage >= threshold
    }
}

/// Compress `history` in place when automatic compaction should fire.
///
/// Returns `None` when compaction is disabled, the history is below the trigger
/// threshold, or the engine produced no change. On a successful pass the
/// compacted history (with envelope injected) replaces `history` and an
/// [`AutoCompactionOutcome`] is returned; the caller is responsible for
/// emitting the `thread.compact_boundary` event and resetting token accounting.
pub async fn auto_compact_messages(
    input: AutoCompactionInput<'_>,
    history: &mut Vec<Message>,
) -> Result<Option<AutoCompactionOutcome>> {
    if !input.should_compact() {
        return Ok(None);
    }
    let AutoCompactionInput {
        provider,
        model,
        session_id,
        workspace_root,
        vt_cfg,
        touched_files,
        engine_cfg,
        manual_options,
        placement,
        prefire,
        auto_compact_suppressed,
        force_compaction,
        steering_update,
        parent_context,
        ..
    } = input;

    // Use the same provider/session intersection as the trigger and preflight
    // paths. This bounds local input and output, while native output remains
    // provider-canonical and is not rewritten by the session budget.
    let context_budget = match effective_context_budget(vt_cfg, provider, model) {
        0 => None,
        budget => Some(budget),
    };
    let mut compaction_input = history.clone();
    strip_existing_memory_envelope(&mut compaction_input);
    let original_history = compaction_input.clone();

    let compaction_result: Result<Option<AutoCompactionOutcome>> = async {
        // Try two-pass with prefire cache before falling back to single-pass.
        if let Some(prefire_state) = prefire {
            if let Some(mut compacted) = try_two_pass_with_prefire(
                prefire_state,
                provider,
                model,
                &original_history,
                &engine_cfg,
                context_budget,
                parent_context.as_ref(),
            )
            .await?
            {
                let original_len = original_history.len();
                let envelope = persist_memory_envelope_async_with_update(
                    workspace_root,
                    session_id,
                    vt_cfg,
                    &original_history,
                    touched_files,
                    &mut compacted,
                    MemoryEnvelopePersistence::PersistToDisk,
                    placement,
                    None,
                    steering_update,
                )
                .await?;
                compacted =
                    crate::compaction::bound_compacted_history_to_context(compacted, provider, model, context_budget);
                let history_artifact_path = envelope.as_ref().and_then(|item| item.history_artifact_path.clone());
                let compacted_len = compacted.len();
                *history = compacted;
                return Ok(Some(AutoCompactionOutcome {
                    original_len,
                    compacted_len,
                    mode: CompactionMode::Local,
                    history_artifact_path,
                    envelope,
                }));
            }
        }

        // Route through the same strategy dispatch as the manual `/compact` command
        // so every provider uses its correct native strategy (or falls back to a
        // local LLM summary). Native outputs remain canonical replay windows, while
        // local strategies provide the visible summary and session envelope.
        let strategy = manual_compaction_strategy(provider, model);
        let compaction_history = if matches!(strategy, CompactionStrategy::Local) {
            dedup_repeated_file_reads_for_local_compaction(&compaction_input)
        } else {
            compaction_input.clone()
        };

        // Preserve the legacy small-segment short-circuit: skip tiny histories.
        if !force_compaction
            && !engine_cfg.always_summarize
            && compaction_history.len() <= engine_cfg.keep_last_messages
        {
            return Ok(None);
        }

        let (mut compacted, mode) = compact_history_manual_with_parent_context(
            provider,
            model,
            &compaction_history,
            &engine_cfg,
            &manual_options,
            context_budget,
            parent_context.as_ref(),
        )
        .await?;

        if compacted == compaction_history {
            return Ok(None);
        }

        // Never persist growth as a compaction: when the continuity tail
        // already covers the history, the rebuild keeps every message and the
        // envelope framing only adds more. Report already-compact instead.
        if !compacted_history_shrinks(original_history.len(), compacted.len(), mode) {
            return Ok(None);
        }

        let original_len = original_history.len();
        // Memory-envelope persistence also injects a local system message. A
        // native provider window is an opaque replay contract (for example an
        // Anthropic signed compaction block must stay first), so persist the
        // envelope against a private copy and leave the provider result intact.
        let mut envelope_history = if mode == CompactionMode::Provider {
            compacted.clone()
        } else {
            std::mem::take(&mut compacted)
        };
        let envelope = persist_memory_envelope_async_with_update(
            workspace_root,
            session_id,
            vt_cfg,
            &original_history,
            touched_files,
            &mut envelope_history,
            MemoryEnvelopePersistence::PersistToDisk,
            placement,
            None,
            steering_update,
        )
        .await?;
        if mode == CompactionMode::Local {
            compacted = envelope_history;
            compacted =
                crate::compaction::bound_compacted_history_to_context(compacted, provider, model, context_budget);
        }
        let history_artifact_path = envelope.as_ref().and_then(|item| item.history_artifact_path.clone());
        let compacted_len = compacted.len();
        *history = compacted;
        Ok(Some(AutoCompactionOutcome {
            original_len,
            compacted_len,
            mode,
            history_artifact_path,
            envelope,
        }))
    }
    .await;

    match &compaction_result {
        Ok(Some(_)) => {
            *auto_compact_suppressed = SUPPRESS_NONE;
        }
        Ok(None) => {}
        Err(error) => {
            let reason = classify_suppress_reason(&error.root_cause().to_string());
            let new_state = reason.suppress_state();
            // Only transition from SUPPRESS_NONE to a new state; don't downgrade
            // a sticky/until-success suppression.
            if *auto_compact_suppressed == SUPPRESS_NONE {
                *auto_compact_suppressed = new_state;
            }
        }
    }

    compaction_result
}

/// Attempt two-pass compaction using a prefire-cached NOTE₁.
///
/// Returns `Some(compacted_history)` if the cache is valid and pass-2 produces
/// a non-degenerate summary. `None` means the caller should fall back to
/// single-pass compaction.
///
/// The pass-2 history already reuses the conversation prefix verbatim; the
/// parent system/tools prefix is attached as well so the fork hits the
/// provider prompt cache.
async fn try_two_pass_with_prefire(
    prefire: &crate::compaction::PrefireState,
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
    context_budget: Option<usize>,
    parent: Option<&CompactionParentContext>,
) -> Result<Option<Vec<Message>>> {
    let cache = match prefire.take() {
        Some(cache) => cache,
        None => return Ok(None),
    };

    if cache.prefix_len == 0 || cache.prefix_len > history.len() {
        return Ok(None);
    }

    if cache.model_slug != model {
        return Ok(None);
    }

    let prefix = &history[..cache.prefix_len];
    if fingerprint_prefix(prefix) != cache.fingerprint {
        return Ok(None);
    }

    let tail = &history[cache.prefix_len..];
    let prompt = build_summary_prompt(tail, &config.summary_prompt);
    let pass2_history = crate::compaction::two_pass::build_two_pass_pass2_history(prefix, tail, &cache.note1, &prompt);

    let request = LLMRequest {
        messages: std::sync::Arc::new(pass2_history),
        model: model.to_string(),
        system_prompt: parent.and_then(|parent| parent.system_prompt.clone()),
        tools: parent.and_then(|parent| parent.tools.clone()).filter(|tools| !tools.is_empty()),
        tool_choice: Some(ToolChoice::none()),
        ..Default::default()
    };

    let response = collect_single_response(provider, request).await?;
    let note2 = response.content.unwrap_or_default().trim().to_string();

    if note2.trim().is_empty() {
        return Ok(None);
    }

    let effective_config =
        crate::compaction::context_bounded_compaction_config(provider, model, history, config, context_budget);
    let tail_target = crate::compaction::route_tail_target_tokens(provider, model, context_budget);
    let compacted = build_local_compacted_history(
        history,
        &note2,
        effective_config.retained_user_message_tokens,
        effective_config.retained_user_messages,
        true,
        tail_target,
    );

    let compacted = crate::compaction::bound_compacted_history_to_context(compacted, provider, model, context_budget);
    // A degenerate pass-2 (tail already covers the history) keeps every
    // message and only adds framing; persisting it would grow the
    // conversation, so fall back to single-pass instead.
    if !compacted_history_shrinks(history.len(), compacted.len(), CompactionMode::Local) {
        return Ok(None);
    }

    Ok(Some(compacted))
}

#[cfg(test)]
mod tests;
