mod file_read_dedup;
mod memory_envelope;
mod recovery_preview;

use self::file_read_dedup::dedup_repeated_file_reads_for_local_compaction;
#[cfg(test)]
pub(crate) use self::memory_envelope::latest_memory_envelope_path_for_session;
use self::memory_envelope::{
    build_zero_cost_summarized_fork_history, configured_retained_user_messages,
    insert_memory_envelope_message, load_latest_memory_envelope, local_compaction_config,
    persist_memory_envelope, strip_existing_memory_envelope,
};
pub(crate) use self::memory_envelope::{
    has_latest_memory_envelope, inject_latest_memory_envelope, refresh_session_memory_envelope,
};
pub(crate) use self::recovery_preview::build_recovery_context_previews_with_workspace;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use vtcode_commons::preview::{condense_text_bytes, tail_preview_text};
use vtcode_config::constants::context::DEFAULT_COMPACTION_TRIGGER_RATIO;
use vtcode_core::compaction::CompactionConfig;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::context::history_files::{HistoryFileManager, messages_to_history_messages};
use vtcode_core::core::agent::harness_artifacts::{
    current_task_path, read_evaluation_summary, read_spec_summary,
};
use vtcode_core::hooks::LifecycleHookEngine;
use vtcode_core::llm::provider::{LLMProvider, Message, MessageRole, ResponsesCompactionOptions};
use vtcode_core::llm::utils::truncate_to_token_limit;
use vtcode_core::persistent_memory::{
    GroundedFactRecord, dedup_latest_facts, normalize_whitespace, truncate_for_fact,
};

use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::inline_events::harness::{
    HarnessEventEmitter, compact_boundary_event,
};
use crate::agent::runloop::unified::state::SessionStats;

const MEMORY_ENVELOPE_HEADER: &str = "[Session Memory Envelope]";
const MEMORY_ENVELOPE_SUFFIX: &str = ".memory.json";
const SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION: u32 = 2;
const MEMORY_LIST_LIMIT: usize = 5;
const DEDUPED_FILE_READ_NOTE: &str = "Older duplicate file read omitted during local compaction; a newer read of the same target slice is retained later in history.";
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
enum MemoryEnvelopePersistence {
    PersistToDisk,
    InMemoryOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemoryEnvelopePlacement {
    Start,
    BeforeLastUserOrSummary,
}

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
        Self {
            history,
            session_stats,
            context_manager,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompactionPlan {
    trigger: vtcode_core::exec::events::CompactionTrigger,
    envelope_mode: CompactionEnvelopeMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SessionMemoryEnvelope {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub schema_version: Option<u32>,
    pub summary: String,
    #[serde(default)]
    pub objective: Option<String>,
    pub task_summary: Option<String>,
    pub spec_summary: Option<String>,
    pub evaluation_summary: Option<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    pub grounded_facts: Vec<GroundedFactRecord>,
    pub touched_files: Vec<String>,
    #[serde(default)]
    pub open_questions: Vec<String>,
    #[serde(default)]
    pub verification_todo: Vec<String>,
    #[serde(default)]
    pub delegation_notes: Vec<String>,
    pub history_artifact_path: Option<String>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionMemoryEnvelopeUpdate {
    pub objective: Option<String>,
    pub constraints: Vec<String>,
    pub grounded_facts: Vec<GroundedFactRecord>,
    pub touched_files: Vec<String>,
    pub open_questions: Vec<String>,
    pub verification_todo: Vec<String>,
    pub delegation_notes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TaskTrackerSnapshot {
    summary: Option<String>,
    objective: Option<String>,
    verification_todo: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FileReadDedupKey {
    target: String,
    start_line: Option<u64>,
    end_line: Option<u64>,
    spool_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileReadDedupCandidate {
    key: FileReadDedupKey,
    placeholder_content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileReadToolKind {
    ReadFile,
    UnifiedFileRead,
}

pub(crate) fn resolve_compaction_threshold(
    configured_threshold: Option<u64>,
    context_size: usize,
) -> Option<u64> {
    let configured_threshold = configured_threshold.filter(|threshold| *threshold > 0);
    let derived_threshold = if context_size > 0 {
        Some(((context_size as f64) * DEFAULT_COMPACTION_TRIGGER_RATIO).round() as u64)
    } else {
        None
    };

    configured_threshold.or(derived_threshold).map(|threshold| {
        let mut threshold = threshold.max(1);
        if context_size > 0 {
            threshold = threshold.min(context_size as u64);
        }
        threshold
    })
}

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

fn effective_compaction_threshold(
    vt_cfg: Option<&VTCodeConfig>,
    provider: &dyn LLMProvider,
    model: &str,
) -> Option<usize> {
    let context_size = provider.effective_context_size(model);
    let configured_threshold =
        vt_cfg.and_then(|cfg| cfg.agent.harness.auto_compaction_threshold_tokens);

    resolve_compaction_threshold(configured_threshold, context_size)
        .and_then(|threshold| usize::try_from(threshold).ok())
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

#[cfg_attr(not(test), allow(dead_code))]
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

pub(crate) async fn manual_openai_compact_history_in_place(
    context: CompactionContext<'_>,
    state: CompactionState<'_>,
    options: &ResponsesCompactionOptions,
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
    let CompactionState {
        history,
        session_stats,
        context_manager,
    } = state;

    if !provider.supports_manual_openai_compaction(model) {
        anyhow::bail!(provider.manual_openai_compaction_unavailable_message(model));
    }

    let previous_response_chain_present = session_stats
        .previous_response_id_for(provider.name(), model)
        .is_some();
    let mut compaction_input = history.clone();
    strip_existing_memory_envelope(&mut compaction_input);
    let original_history = compaction_input.clone();
    let compacted = provider
        .compact_history_with_options(model, &compaction_input, options)
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
            trigger: vtcode_core::exec::events::CompactionTrigger::Manual,
            envelope_mode: CompactionEnvelopeMode {
                persistence: MemoryEnvelopePersistence::PersistToDisk,
                placement: MemoryEnvelopePlacement::Start,
            },
        },
        original_history,
        previous_response_chain_present,
        compacted,
        vtcode_core::exec::events::CompactionMode::Provider,
    )
    .await
    .map(Some)
}

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
    let CompactionState {
        history,
        session_stats,
        context_manager,
    } = state;

    let previous_response_chain_present = session_stats
        .previous_response_id_for(provider.name(), model)
        .is_some();
    let mut compaction_input = history.clone();
    strip_existing_memory_envelope(&mut compaction_input);
    let original_history = compaction_input.clone();
    let compaction_history = if provider.supports_responses_compaction(model) {
        compaction_input
    } else {
        dedup_repeated_file_reads_for_local_compaction(&compaction_input)
    };
    let compacted = vtcode_core::compaction::compact_history(
        provider,
        model,
        &compaction_history,
        &local_compaction_config(vt_cfg, false),
    )
    .await?;

    if compacted == compaction_history {
        return Ok(None);
    }

    let compaction_mode = if provider.supports_responses_compaction(model) {
        vtcode_core::exec::events::CompactionMode::Provider
    } else {
        vtcode_core::exec::events::CompactionMode::Local
    };
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
    let CompactionState {
        history,
        session_stats,
        context_manager,
    } = state;

    let original_len = original_history.len();
    if let Some(lifecycle_hooks) = lifecycle_hooks {
        let outcome = lifecycle_hooks
            .run_pre_compact(
                plan.trigger,
                compaction_mode,
                original_len,
                compacted.len(),
                None,
            )
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
    let history_artifact_path = envelope
        .as_ref()
        .and_then(|item| item.history_artifact_path.clone());
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
    let CompactionState {
        history,
        session_stats,
        context_manager,
    } = state;

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
    let current_prompt_pressure_tokens = state.context_manager.current_token_usage();
    let CompactionContext {
        provider,
        model,
        vt_cfg,
        ..
    } = context;
    let Some(vt_cfg) = vt_cfg else {
        return Ok(None);
    };

    if !vt_cfg.agent.harness.auto_compaction_enabled
        || provider.supports_responses_compaction(model)
    {
        return Ok(None);
    }

    let Some(compact_threshold) = effective_compaction_threshold(Some(vt_cfg), provider, model)
    else {
        return Ok(None);
    };

    if current_prompt_pressure_tokens < compact_threshold {
        return Ok(None);
    }

    compact_history_segment_in_place(
        CompactionContext {
            vt_cfg: Some(vt_cfg),
            ..context
        },
        state,
        CompactionPlan {
            trigger: vtcode_core::exec::events::CompactionTrigger::Auto,
            envelope_mode: CompactionEnvelopeMode {
                persistence: MemoryEnvelopePersistence::PersistToDisk,
                placement: MemoryEnvelopePlacement::BeforeLastUserOrSummary,
            },
        },
    )
    .await
}

#[cfg(test)]
mod tests;
