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
use crate::compaction::{
    CompactionConfig, CompactionStrategy, ManualCompactionOptions, compact_history_manual, manual_compaction_strategy,
};
use crate::exec::events::CompactionMode;
use crate::llm::provider::{LLMProvider, Message};
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

    let mut compaction_input = history.clone();
    strip_existing_memory_envelope(&mut compaction_input);
    let original_history = compaction_input.clone();

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
