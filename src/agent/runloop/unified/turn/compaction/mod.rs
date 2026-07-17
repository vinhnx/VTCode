mod memory_envelope;
mod recovery_preview;

pub(crate) use self::memory_envelope::refresh_session_memory_envelope;
pub(crate) use self::recovery_preview::build_recovery_context_previews_with_workspace;

pub(crate) use vtcode_core::compaction::memory_envelope::{
    MemoryEnvelopePersistence, MemoryEnvelopePlacement, SessionMemoryEnvelope,
    SessionMemoryEnvelopeUpdate, build_session_memory_envelope,
    build_zero_cost_summarized_fork_history, configured_retained_user_messages,
    dedup_repeated_file_reads_for_local_compaction, default_memory_envelope_path_for_session,
    derive_continuity_summary, effective_compaction_threshold, has_latest_memory_envelope,
    insert_memory_envelope_message, latest_memory_envelope_path_for_session,
    load_latest_memory_envelope, local_compaction_config, persist_memory_envelope,
    read_task_tracker_snapshot, resolve_compaction_threshold, should_persist_memory_envelope,
    strip_existing_memory_envelope, write_memory_envelope_to_path,
};

// Test-only symbols referenced by the runloop compaction test suite.
#[cfg(test)]
pub(crate) use vtcode_core::compaction::memory_envelope::{
    DEDUPED_FILE_READ_NOTE, SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION, inject_latest_memory_envelope,
};
#[cfg(test)]
pub(crate) use vtcode_core::persistent_memory::{GroundedFactRecord, dedup_latest_facts};

use anyhow::Result;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use vtcode_commons::preview::{condense_text_bytes, tail_preview_text};
use vtcode_core::compaction::auto::{AutoCompactionInput, auto_compact_messages};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::hooks::LifecycleHookEngine;
use vtcode_core::llm::provider::{LLMProvider, Message, MessageRole};
use vtcode_core::persistent_memory::normalize_whitespace;

use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::inline_events::harness::{
    HarnessEventEmitter, compact_boundary_event,
};
use crate::agent::runloop::unified::state::SessionStats;

const RECOVERY_PREVIEW_MAX_CHARS: usize = 220;
const RECOVERY_PREVIEW_MAX_TOOL_OUTPUTS: usize = 3;
const RECOVERY_PREVIEW_USER_LABEL: &str = "Latest user request";
const RECOVERY_PREVIEW_TOOL_LABEL: &str = "Latest tool output";
const RECOVERY_PREVIEW_ASSISTANT_LABEL: &str = "Latest assistant text";
const RECOVERY_PREVIEW_SPOOL_READ_HEAD_BYTES: usize = 2_000;
const RECOVERY_PREVIEW_SPOOL_READ_TAIL_BYTES: usize = 1_500;
const RECOVERY_PREVIEW_SPOOL_EXEC_TAIL_BYTES: usize = 4_000;
const RECOVERY_PREVIEW_SPOOL_EXEC_MAX_LINES: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompactionEnvelopeMode {
    persistence: MemoryEnvelopePersistence,
    placement: MemoryEnvelopePlacement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompactionOutcome {
    pub original_len: usize,
    pub compacted_len: usize,
    pub mode: vtcode_core::exec::events::CompactionMode,
    pub history_artifact_path: Option<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct CompactionContext<'a> {
    provider: &'a dyn LLMProvider,
    model: &'a str,
    session_id: &'a str,
    thread_id: &'a str,
    workspace_root: &'a Path,
    vt_cfg: Option<&'a VTCodeConfig>,
    lifecycle_hooks: Option<&'a LifecycleHookEngine>,
    harness_emitter: Option<&'a HarnessEventEmitter>,
}

impl<'a> CompactionContext<'a> {
    pub(crate) fn new(
        provider: &'a dyn LLMProvider,
        model: &'a str,
        session_id: &'a str,
        thread_id: &'a str,
        workspace_root: &'a Path,
        vt_cfg: Option<&'a VTCodeConfig>,
        lifecycle_hooks: Option<&'a LifecycleHookEngine>,
        harness_emitter: Option<&'a HarnessEventEmitter>,
    ) -> Self {
        Self {
            provider,
            model,
            session_id,
            thread_id,
            workspace_root,
            vt_cfg,
            lifecycle_hooks,
            harness_emitter,
        }
    }
}

pub(crate) struct CompactionState<'a> {
    history: &'a mut Vec<Message>,
    session_stats: &'a mut SessionStats,
    context_manager: &'a mut ContextManager,
}

impl<'a> CompactionState<'a> {
    pub(crate) fn new(
        history: &'a mut Vec<Message>,
        session_stats: &'a mut SessionStats,
        context_manager: &'a mut ContextManager,
    ) -> Self {
        Self { history, session_stats, context_manager }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompactionPlan {
    trigger: vtcode_core::exec::events::CompactionTrigger,
    envelope_mode: CompactionEnvelopeMode,
}

#[allow(clippy::cast_sign_loss)] // context_size is usize (non-negative), ratio is positive
pub(crate) fn build_server_compaction_context_management(
    configured_threshold: Option<u64>,
    context_size: usize,
) -> Option<Value> {
    resolve_compaction_threshold(configured_threshold, context_size).map(|compact_threshold| {
        json!([{
            "type": "compaction",
            "compact_threshold": compact_threshold,
        }])
    })
}

pub(crate) async fn build_summarized_fork_history(
    provider: &dyn LLMProvider,
    model: &str,
    source_session_id: &str,
    target_session_id: &str,
    workspace_root: &Path,
    vt_cfg: Option<&VTCodeConfig>,
    source_history: &[Message],
    prefer_saved_summary: bool,
) -> Result<Vec<Message>> {
    if source_history.is_empty() {
        return Ok(Vec::new());
    }

    let mut source_history = source_history.to_vec();
    let source_envelope = load_latest_memory_envelope(workspace_root, source_session_id);
    if let Some(envelope) = source_envelope.as_ref() {
        strip_existing_memory_envelope(&mut source_history);
        insert_memory_envelope_message(
            &mut source_history,
            envelope,
            MemoryEnvelopePlacement::Start,
        );
    }

    let mut compacted = if prefer_saved_summary && source_envelope.is_some() {
        build_zero_cost_summarized_fork_history(
            &source_history,
            source_envelope.as_ref(),
            configured_retained_user_messages(vt_cfg),
        )
    } else {
        let compaction_input = dedup_repeated_file_reads_for_local_compaction(&source_history);
        vtcode_core::compaction::compact_history(
            provider,
            model,
            &compaction_input,
            &local_compaction_config(vt_cfg, true),
        )
        .await?
    };

    let _ = persist_memory_envelope(
        workspace_root,
        target_session_id,
        vt_cfg,
        &source_history,
        &[],
        &mut compacted,
        MemoryEnvelopePersistence::InMemoryOnly,
        MemoryEnvelopePlacement::Start,
        source_envelope.as_ref(),
    )?;

    Ok(compacted)
}

#[cfg_attr(not(test), expect(dead_code))]
pub(crate) async fn compact_history_in_place(
    provider: &dyn LLMProvider,
    model: &str,
    session_id: &str,
    workspace_root: &Path,
    vt_cfg: Option<&VTCodeConfig>,
    history: &mut Vec<Message>,
    session_stats: &mut SessionStats,
    context_manager: &mut ContextManager,
) -> Result<Option<CompactionOutcome>> {
    compact_history_in_place_with_events(
        CompactionContext::new(
            provider,
            model,
            session_id,
            session_id,
            workspace_root,
            vt_cfg,
            None,
            None,
        ),
        CompactionState::new(history, session_stats, context_manager),
        vtcode_core::exec::events::CompactionTrigger::Manual,
    )
    .await
}

pub(crate) async fn compact_history_in_place_with_events(
    context: CompactionContext<'_>,
    state: CompactionState<'_>,
    trigger: vtcode_core::exec::events::CompactionTrigger,
) -> Result<Option<CompactionOutcome>> {
    compact_history_segment_in_place(
        context,
        state,
        CompactionPlan {
            trigger,
            envelope_mode: CompactionEnvelopeMode {
                persistence: MemoryEnvelopePersistence::PersistToDisk,
                placement: MemoryEnvelopePlacement::Start,
            },
        },
    )
    .await
}

pub(crate) async fn manual_compact_history_in_place(
    context: CompactionContext<'_>,
    state: CompactionState<'_>,
    options: &vtcode_core::compaction::ManualCompactionOptions,
    native_only: bool,
) -> Result<Option<CompactionOutcome>> {
    run_manual_compaction(
        context,
        state,
        options,
        native_only,
        vtcode_core::exec::events::CompactionTrigger::Manual,
    )
    .await
}

/// Compact the conversation when the main session model or provider is switched
/// mid-session, so the newly selected model starts from a summary rather than
/// the outgoing model's raw trace. Mirrors `/compact` (forces `always_summarize`
/// via `local_compaction_config(vt_cfg, true)` and routes through the same
/// strategy dispatch) but is tagged with `CompactionTrigger::ModelSwitch`.
pub(crate) async fn compact_history_on_model_switch_in_place(
    context: CompactionContext<'_>,
    state: CompactionState<'_>,
) -> Result<Option<CompactionOutcome>> {
    run_manual_compaction(
        context,
        state,
        &vtcode_core::compaction::ManualCompactionOptions::default(),
        false,
        vtcode_core::exec::events::CompactionTrigger::ModelSwitch,
    )
    .await
}

async fn run_manual_compaction(
    context: CompactionContext<'_>,
    state: CompactionState<'_>,
    options: &vtcode_core::compaction::ManualCompactionOptions,
    native_only: bool,
    trigger: vtcode_core::exec::events::CompactionTrigger,
) -> Result<Option<CompactionOutcome>> {
    let CompactionContext {
        provider,
        model,
        session_id,
        thread_id,
        workspace_root,
        vt_cfg,
        lifecycle_hooks,
        harness_emitter,
    } = context;
    let CompactionState { history, session_stats, context_manager } = state;

    // `--native-only` preserves the legacy strict behavior: refuse unless the
    // provider exposes a real standalone compaction endpoint (OpenAI
    // `/responses/compact`). Without the flag, every provider proceeds via the
    // strategy dispatch (native standalone, native inline, or local summary).
    if native_only && !provider.supports_manual_openai_compaction(model) {
        anyhow::bail!(provider.manual_openai_compaction_unavailable_message(model));
    }

    let previous_response_chain_present =
        session_stats.previous_response_id_for(provider.name(), model).is_some();
    let mut compaction_input = history.clone();
    strip_existing_memory_envelope(&mut compaction_input);
    let original_history = compaction_input.clone();
    let (compacted, compaction_mode) = vtcode_core::compaction::compact_history_manual(
        provider,
        model,
        &compaction_input,
        &local_compaction_config(vt_cfg, true),
        options,
    )
    .await?;
    if compacted == compaction_input {
        return Ok(None);
    }

    apply_compacted_history(
        CompactionContext {
            provider,
            model,
            session_id,
            thread_id,
            workspace_root,
            vt_cfg,
            lifecycle_hooks,
            harness_emitter,
        },
        CompactionState::new(history, session_stats, context_manager),
        CompactionPlan {
            trigger,
            envelope_mode: CompactionEnvelopeMode {
                persistence: MemoryEnvelopePersistence::PersistToDisk,
                placement: MemoryEnvelopePlacement::Start,
            },
        },
        original_history,
        previous_response_chain_present,
        compacted,
        compaction_mode,
    )
    .await
    .map(Some)
}

#[cfg(test)]
pub(crate) async fn compact_history_for_recovery_in_place(
    context: CompactionContext<'_>,
    state: CompactionState<'_>,
    preserve_from_index: usize,
) -> Result<Option<CompactionOutcome>> {
    compact_history_before_index_in_place(
        context,
        state,
        preserve_from_index,
        CompactionPlan {
            trigger: vtcode_core::exec::events::CompactionTrigger::Recovery,
            envelope_mode: CompactionEnvelopeMode {
                persistence: MemoryEnvelopePersistence::PersistToDisk,
                placement: MemoryEnvelopePlacement::Start,
            },
        },
    )
    .await
}

async fn compact_history_segment_in_place(
    context: CompactionContext<'_>,
    state: CompactionState<'_>,
    plan: CompactionPlan,
) -> Result<Option<CompactionOutcome>> {
    let CompactionContext {
        provider,
        model,
        session_id,
        thread_id,
        workspace_root,
        vt_cfg,
        lifecycle_hooks,
        harness_emitter,
    } = context;
    let CompactionState { history, session_stats, context_manager } = state;

    let previous_response_chain_present =
        session_stats.previous_response_id_for(provider.name(), model).is_some();
    let mut compaction_input = history.clone();
    strip_existing_memory_envelope(&mut compaction_input);
    let original_history = compaction_input.clone();

    // Route through the same strategy dispatch as the manual `/compact` command
    // (`compact_history_manual`), so auto/recovery/targeted compaction uses the
    // correct native strategy per provider instead of the legacy binary
    // `supports_responses_compaction` path. NativeInline (Anthropic) replaces a
    // hard `Err` (Anthropic does not override `compact_history`) with a graceful
    // Local fallback, so recovery never aborts.
    let config = local_compaction_config(vt_cfg, false);
    let strategy = vtcode_core::compaction::manual_compaction_strategy(provider, model);
    let compaction_history =
        if matches!(strategy, vtcode_core::compaction::CompactionStrategy::Local) {
            dedup_repeated_file_reads_for_local_compaction(&compaction_input)
        } else {
            compaction_input.clone()
        };
    // Preserve the legacy small-segment short-circuit: auto/recovery/targeted
    // pass `always_summarize=false`, so skip compaction for tiny segments.
    if !config.always_summarize && compaction_history.len() <= config.keep_last_messages {
        return Ok(None);
    }
    let (compacted, compaction_mode) = vtcode_core::compaction::compact_history_manual(
        provider,
        model,
        &compaction_history,
        &config,
        &vtcode_core::compaction::ManualCompactionOptions::default(),
    )
    .await?;

    if compacted == compaction_history {
        return Ok(None);
    }

    apply_compacted_history(
        CompactionContext {
            provider,
            model,
            session_id,
            thread_id,
            workspace_root,
            vt_cfg,
            lifecycle_hooks,
            harness_emitter,
        },
        CompactionState::new(history, session_stats, context_manager),
        plan,
        original_history,
        previous_response_chain_present,
        compacted,
        compaction_mode,
    )
    .await
    .map(Some)
}

async fn apply_compacted_history(
    context: CompactionContext<'_>,
    state: CompactionState<'_>,
    plan: CompactionPlan,
    original_history: Vec<Message>,
    previous_response_chain_present: bool,
    compacted: Vec<Message>,
    compaction_mode: vtcode_core::exec::events::CompactionMode,
) -> Result<CompactionOutcome> {
    let CompactionContext {
        provider,
        model,
        session_id,
        thread_id,
        workspace_root,
        vt_cfg,
        lifecycle_hooks,
        harness_emitter,
    } = context;
    let CompactionState { history, session_stats, context_manager } = state;

    let original_len = original_history.len();
    if let Some(lifecycle_hooks) = lifecycle_hooks {
        let outcome = lifecycle_hooks
            .run_pre_compact(plan.trigger, compaction_mode, original_len, compacted.len(), None)
            .await?;
        for message in outcome.messages {
            tracing::debug!(message = %message.text, "pre-compact hook message");
        }
    }

    let mut compacted = compacted;
    let compacted_len = compacted.len();
    let touched_files = session_stats.recent_touched_files();
    let envelope = persist_memory_envelope(
        workspace_root,
        session_id,
        vt_cfg,
        &original_history,
        &touched_files,
        &mut compacted,
        plan.envelope_mode.persistence,
        plan.envelope_mode.placement,
        None,
    )?;
    let history_artifact_path =
        envelope.as_ref().and_then(|item| item.history_artifact_path.clone());
    *history = compacted;
    session_stats.clear_previous_response_chain_for(provider.name(), model);
    context_manager
        .cap_token_usage_after_compaction(effective_compaction_threshold(vt_cfg, provider, model));
    if let Some(ref envelope) = envelope {
        tracing::info!(
            provider = %provider.name(),
            model = %model,
            turn = compacted_len,
            tool_count = 0usize,
            parallelized = false,
            compaction_mode = %compaction_mode.as_str(),
            grounded_fact_count = envelope.grounded_facts.len(),
            previous_response_chain_present,
            "Injected session memory envelope"
        );
    }
    tracing::info!(
        provider = %provider.name(),
        model = %model,
        turn = original_len,
        tool_count = 0usize,
        parallelized = false,
        compaction_mode = %compaction_mode.as_str(),
        grounded_fact_count = envelope.as_ref().map_or(0, |item| item.grounded_facts.len()),
        previous_response_chain_present,
        "Applied conversation compaction"
    );
    if let Some(harness_emitter) = harness_emitter {
        let event = compact_boundary_event(
            thread_id.to_string(),
            plan.trigger,
            compaction_mode,
            original_len,
            compacted_len,
            history_artifact_path.clone(),
        );
        if let Err(err) = harness_emitter.emit(event) {
            tracing::debug!(error = %err, "harness compact boundary event emission failed");
        }
    }

    Ok(CompactionOutcome {
        original_len,
        compacted_len,
        mode: compaction_mode,
        history_artifact_path,
    })
}

#[cfg(test)]
async fn compact_history_before_index_in_place(
    context: CompactionContext<'_>,
    state: CompactionState<'_>,
    preserve_from_index: usize,
    plan: CompactionPlan,
) -> Result<Option<CompactionOutcome>> {
    let CompactionContext {
        provider,
        model,
        session_id,
        thread_id,
        workspace_root,
        vt_cfg,
        lifecycle_hooks,
        harness_emitter,
    } = context;
    let CompactionState { history, session_stats, context_manager } = state;

    if preserve_from_index == 0 {
        return Ok(None);
    }

    if preserve_from_index >= history.len() {
        return compact_history_segment_in_place(
            CompactionContext {
                provider,
                model,
                session_id,
                thread_id,
                workspace_root,
                vt_cfg,
                lifecycle_hooks,
                harness_emitter,
            },
            CompactionState::new(history, session_stats, context_manager),
            plan,
        )
        .await;
    }

    let original_len = history.len();
    let mut prefix = history[..preserve_from_index].to_vec();
    let suffix = history[preserve_from_index..].to_vec();
    let Some(prefix_outcome) = compact_history_segment_in_place(
        CompactionContext {
            provider,
            model,
            session_id,
            thread_id,
            workspace_root,
            vt_cfg,
            lifecycle_hooks,
            harness_emitter: None,
        },
        CompactionState::new(&mut prefix, session_stats, context_manager),
        plan,
    )
    .await?
    else {
        return Ok(None);
    };

    history.clear();
    history.extend(prefix);
    history.extend(suffix);

    let compacted_len = history.len();
    let history_artifact_path = prefix_outcome.history_artifact_path.clone();
    if let Some(harness_emitter) = harness_emitter {
        let event = compact_boundary_event(
            thread_id.to_string(),
            plan.trigger,
            prefix_outcome.mode,
            original_len,
            compacted_len,
            history_artifact_path.clone(),
        );
        if let Err(err) = harness_emitter.emit(event) {
            tracing::debug!(error = %err, "harness compact boundary event emission failed");
        }
    }

    Ok(Some(CompactionOutcome {
        original_len,
        compacted_len,
        mode: prefix_outcome.mode,
        history_artifact_path,
    }))
}

#[cfg(test)]
pub(crate) async fn compact_history_from_index_in_place(
    provider: &dyn LLMProvider,
    model: &str,
    session_id: &str,
    workspace_root: &Path,
    vt_cfg: Option<&VTCodeConfig>,
    history: &mut Vec<Message>,
    start_index: usize,
    session_stats: &mut SessionStats,
    context_manager: &mut ContextManager,
) -> Result<Option<CompactionOutcome>> {
    if start_index >= history.len() {
        return Ok(None);
    }
    let context = CompactionContext::new(
        provider,
        model,
        session_id,
        session_id,
        workspace_root,
        vt_cfg,
        None,
        None,
    );

    if start_index == 0 {
        return compact_history_segment_in_place(
            context,
            CompactionState::new(history, session_stats, context_manager),
            CompactionPlan {
                trigger: vtcode_core::exec::events::CompactionTrigger::Manual,
                envelope_mode: CompactionEnvelopeMode {
                    persistence: MemoryEnvelopePersistence::InMemoryOnly,
                    placement: MemoryEnvelopePlacement::Start,
                },
            },
        )
        .await;
    }

    let prefix = history[..start_index].to_vec();
    let mut suffix = history[start_index..].to_vec();
    let Some(suffix_outcome) = compact_history_segment_in_place(
        context,
        CompactionState::new(&mut suffix, session_stats, context_manager),
        CompactionPlan {
            trigger: vtcode_core::exec::events::CompactionTrigger::Manual,
            envelope_mode: CompactionEnvelopeMode {
                persistence: MemoryEnvelopePersistence::InMemoryOnly,
                placement: MemoryEnvelopePlacement::Start,
            },
        },
    )
    .await?
    else {
        return Ok(None);
    };

    history.clear();
    history.extend(prefix);
    history.extend(suffix);

    Ok(Some(CompactionOutcome {
        original_len: start_index + suffix_outcome.original_len,
        compacted_len: start_index + suffix_outcome.compacted_len,
        mode: suffix_outcome.mode,
        history_artifact_path: suffix_outcome.history_artifact_path,
    }))
}

pub(crate) async fn maybe_auto_compact_history(
    context: CompactionContext<'_>,
    state: CompactionState<'_>,
) -> Result<Option<CompactionOutcome>> {
    let CompactionContext {
        provider,
        model,
        session_id,
        thread_id,
        workspace_root,
        vt_cfg,
        harness_emitter,
        ..
    } = context;
    let CompactionState { history, session_stats, context_manager } = state;

    let current_prompt_pressure_tokens = context_manager.current_token_usage();

    // Delegate to the shared compaction orchestrator (used by both runloops).
    // It enforces the `auto_compaction_enabled` gate, the token threshold, and
    // the engine + memory-envelope + artifact compression in one place.
    let Some(outcome) = auto_compact_messages(
        AutoCompactionInput {
            provider,
            model,
            session_id,
            workspace_root,
            vt_cfg,
            current_token_usage: current_prompt_pressure_tokens,
            touched_files: &session_stats.recent_touched_files(),
            engine_cfg: local_compaction_config(vt_cfg, false),
            manual_options: vtcode_core::compaction::ManualCompactionOptions::default(),
            placement: MemoryEnvelopePlacement::BeforeLastUserOrSummary,
        },
        history,
    )
    .await?
    else {
        return Ok(None);
    };

    // Binary-specific post-step: reset response-chain and token tracking, then
    // emit the canonical `thread.compact_boundary` event.
    session_stats.clear_previous_response_chain_for(provider.name(), model);
    context_manager
        .cap_token_usage_after_compaction(effective_compaction_threshold(vt_cfg, provider, model));
    if let Some(harness_emitter) = harness_emitter {
        let event = compact_boundary_event(
            thread_id.to_string(),
            vtcode_core::exec::events::CompactionTrigger::Auto,
            outcome.mode,
            outcome.original_len,
            outcome.compacted_len,
            outcome.history_artifact_path.clone(),
        );
        let _ = harness_emitter.emit(event);
    }
    tracing::info!(
        provider = %provider.name(),
        model = %model,
        original_len = outcome.original_len,
        compacted_len = outcome.compacted_len,
        compaction_mode = %outcome.mode.as_str(),
        "Applied automatic conversation compaction"
    );
    Ok(Some(CompactionOutcome {
        original_len: outcome.original_len,
        compacted_len: outcome.compacted_len,
        mode: outcome.mode,
        history_artifact_path: outcome.history_artifact_path,
    }))
}

#[cfg(test)]
mod tests;
