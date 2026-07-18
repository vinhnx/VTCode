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
    MemoryEnvelopePersistence, MemoryEnvelopePlacement, dedup_repeated_file_reads_for_local_compaction,
    effective_compaction_threshold, persist_memory_envelope, strip_existing_memory_envelope,
};
use crate::compaction::two_pass::fingerprint_prefix;
use crate::compaction::{
    CompactionConfig, CompactionStrategy, ManualCompactionOptions, SUPPRESS_NONE, build_local_compacted_history,
    build_summary_prompt, classify_suppress_reason, compact_history_manual, manual_compaction_strategy,
};
use crate::exec::events::CompactionMode;
use crate::llm::provider::{LLMProvider, LLMRequest, Message};
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
    let AutoCompactionInput {
        provider,
        model,
        session_id,
        workspace_root,
        vt_cfg,
        current_token_usage,
        touched_files,
        engine_cfg,
        manual_options,
        placement,
        prefire,
        auto_compact_suppressed,
    } = input;

    if !vt_cfg.is_some_and(|cfg| cfg.agent.harness.auto_compaction_enabled) {
        return Ok(None);
    }

    let Some(threshold) = effective_compaction_threshold(vt_cfg, provider, model) else {
        return Ok(None);
    };
    if current_token_usage < threshold {
        return Ok(None);
    }

    if *auto_compact_suppressed != SUPPRESS_NONE {
        return Ok(None);
    }

    let mut compaction_input = history.clone();
    strip_existing_memory_envelope(&mut compaction_input);
    let original_history = compaction_input.clone();

    let compaction_result: Result<Option<AutoCompactionOutcome>> = async {
        // Try two-pass with prefire cache before falling back to single-pass.
        if let Some(prefire_state) = prefire {
            if let Some(mut compacted) =
                try_two_pass_with_prefire(prefire_state, provider, model, &original_history, &engine_cfg).await?
            {
                let original_len = original_history.len();
                let envelope = persist_memory_envelope(
                    workspace_root,
                    session_id,
                    vt_cfg,
                    &original_history,
                    touched_files,
                    &mut compacted,
                    MemoryEnvelopePersistence::PersistToDisk,
                    placement,
                    None,
                )?;
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
        // local LLM summary). This guarantees a *visible* structured summary plus
        // envelope for every provider, preserving conversational continuity rather
        // than relying on opaque server-side compaction.
        let strategy = manual_compaction_strategy(provider, model);
        let compaction_history = if matches!(strategy, CompactionStrategy::Local) {
            dedup_repeated_file_reads_for_local_compaction(&compaction_input)
        } else {
            compaction_input.clone()
        };

        // Preserve the legacy small-segment short-circuit: skip tiny histories.
        if !engine_cfg.always_summarize && compaction_history.len() <= engine_cfg.keep_last_messages {
            return Ok(None);
        }

        let (mut compacted, mode) =
            compact_history_manual(provider, model, &compaction_history, &engine_cfg, &manual_options).await?;

        if compacted == compaction_history {
            return Ok(None);
        }

        let original_len = original_history.len();
        let envelope = persist_memory_envelope(
            workspace_root,
            session_id,
            vt_cfg,
            &original_history,
            touched_files,
            &mut compacted,
            MemoryEnvelopePersistence::PersistToDisk,
            placement,
            None,
        )?;
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
async fn try_two_pass_with_prefire(
    prefire: &crate::compaction::PrefireState,
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
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
        ..Default::default()
    };

    let response = provider.generate(request).await?;
    let note2 = response.content.unwrap_or_default().trim().to_string();

    if note2.trim().is_empty() {
        return Ok(None);
    }

    let compacted = build_local_compacted_history(
        history,
        &note2,
        config.retained_user_message_tokens,
        config.retained_user_messages,
        true,
    );

    Ok(Some(compacted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compaction::SUPPRESS_STICKY;
    use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, Message};
    use async_trait::async_trait;

    struct FailingProvider;

    #[async_trait]
    impl LLMProvider for FailingProvider {
        fn name(&self) -> &str {
            "failing"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Err(LLMError::Provider {
                message: "context length exceeded".to_string(),
                metadata: None,
            })
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["failing-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        fn effective_context_size(&self, _model: &str) -> usize {
            1000
        }
    }

    #[tokio::test]
    async fn suppressed_state_skips_compaction() {
        let provider = FailingProvider;
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.harness.auto_compaction_enabled = true;
        let mut history = vec![Message::user("hello".to_string())];
        let mut suppressed = SUPPRESS_STICKY;

        let result = auto_compact_messages(
            AutoCompactionInput {
                provider: &provider,
                model: "failing-model",
                session_id: "s1",
                workspace_root: Path::new("."),
                vt_cfg: Some(&vt_cfg),
                current_token_usage: 900,
                touched_files: &[],
                engine_cfg: CompactionConfig::default(),
                manual_options: ManualCompactionOptions::default(),
                placement: MemoryEnvelopePlacement::BeforeLastUserOrSummary,
                prefire: None,
                auto_compact_suppressed: &mut suppressed,
            },
            &mut history,
        )
        .await;

        assert!(result.unwrap().is_none());
        assert_eq!(suppressed, SUPPRESS_STICKY);
    }

    #[tokio::test]
    async fn failure_sets_suppression_on_deterministic_error() {
        let provider = FailingProvider;
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.harness.auto_compaction_enabled = true;
        let mut history = vec![Message::user("hello".to_string())];
        let mut suppressed = SUPPRESS_NONE;

        let result = auto_compact_messages(
            AutoCompactionInput {
                provider: &provider,
                model: "failing-model",
                session_id: "s1",
                workspace_root: Path::new("."),
                vt_cfg: Some(&vt_cfg),
                current_token_usage: 900,
                touched_files: &[],
                engine_cfg: CompactionConfig {
                    keep_last_messages: 0,
                    ..CompactionConfig::default()
                },
                manual_options: ManualCompactionOptions::default(),
                placement: MemoryEnvelopePlacement::BeforeLastUserOrSummary,
                prefire: None,
                auto_compact_suppressed: &mut suppressed,
            },
            &mut history,
        )
        .await;

        assert!(result.is_err());
        assert_eq!(suppressed, SUPPRESS_STICKY);
    }
}
