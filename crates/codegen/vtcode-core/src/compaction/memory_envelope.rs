//! Session memory envelope + local compaction helpers, shared by every
//! compaction path (auto, manual `/compact`, model-switch, recovery, fork).
//!
//! This module was extracted from the binary unified runloop so that both the
//! binary runloop and the `vtcode-core` `AgentRunner` loop use a single
//! compaction path with identical continuity behavior.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::fs as async_fs;

use vtcode_config::context::default_max_context_tokens;
use vtcode_config::loader::VTCodeConfig;

use crate::compaction::CompactionConfig;
use crate::config::constants::tools as tool_names;
use crate::context::history_files::{HistoryFileManager, messages_to_history_messages};
use crate::core::agent::harness_artifacts::{
    artifact_is_stale, current_evaluation_path, current_spec_path, current_task_path, read_evaluation_summary_fresh,
    read_spec_summary_fresh, session_artifact_cutoff,
};
use crate::core::agent::steering::{
    MAX_APPLIED_FOLLOW_UP_INTENT_IDS, MAX_QUEUED_FOLLOW_UP_INTENTS, QueuedFollowUpIntent,
};
use crate::llm::provider::{LLMProvider, Message, MessageRole};
use crate::llm::utils::truncate_to_token_limit;
use crate::persistent_memory::{GroundedFactRecord, dedup_latest_facts, normalize_whitespace, truncate_for_fact};

pub const MEMORY_ENVELOPE_HEADER: &str = "[Session Memory Envelope]";
pub const MEMORY_ENVELOPE_SUFFIX: &str = ".memory.json";
pub const SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION: u32 = 3;
pub const MEMORY_LIST_LIMIT: usize = 5;
pub const APPLIED_INTENT_WINDOW: usize = MAX_APPLIED_FOLLOW_UP_INTENT_IDS;
pub const DEDUPED_FILE_READ_NOTE: &str = "Older duplicate file read omitted during local compaction; a newer read of the same target slice is retained later in history.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryEnvelopePersistence {
    PersistToDisk,
    InMemoryOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryEnvelopePlacement {
    Start,
    BeforeLastUserOrSummary,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionMemoryEnvelope {
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
    pub verification_summary: Option<String>,
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
    /// Follow-up steering intents accepted but not yet represented by a
    /// tagged user message. The list is bounded to preserve FIFO recovery.
    #[serde(default)]
    pub pending_intents: Vec<QueuedFollowUpIntent>,
    /// Recently applied steering IDs used to avoid replaying an already
    /// durable instruction after restart or compaction.
    #[serde(default)]
    pub applied_intent_ids: Vec<String>,
    pub history_artifact_path: Option<String>,
    pub generated_at: String,
}

impl SessionMemoryEnvelope {
    /// Returns true if this envelope carries the same meaningful content as
    /// `other`. Generated timestamps and history artifact paths are ignored
    /// because they change even when the underlying session state does not.
    pub fn is_content_equivalent_to(&self, other: &SessionMemoryEnvelope) -> bool {
        self.session_id == other.session_id
            && self.schema_version == other.schema_version
            && self.summary == other.summary
            && self.objective == other.objective
            && self.task_summary == other.task_summary
            && self.spec_summary == other.spec_summary
            && self.evaluation_summary == other.evaluation_summary
            && self.verification_summary == other.verification_summary
            && self.constraints == other.constraints
            && self.grounded_facts == other.grounded_facts
            && self.touched_files == other.touched_files
            && self.open_questions == other.open_questions
            && self.verification_todo == other.verification_todo
            && self.delegation_notes == other.delegation_notes
            && self.pending_intents == other.pending_intents
            && self.applied_intent_ids == other.applied_intent_ids
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionMemoryEnvelopeUpdate {
    pub objective: Option<String>,
    pub constraints: Vec<String>,
    pub grounded_facts: Vec<GroundedFactRecord>,
    pub touched_files: Vec<String>,
    pub open_questions: Vec<String>,
    pub verification_todo: Vec<String>,
    pub delegation_notes: Vec<String>,
    /// Replace the durable pending-intent snapshot when supplied.
    pub pending_intents: Option<Vec<QueuedFollowUpIntent>>,
    /// IDs acknowledged since the previous envelope.
    pub applied_intent_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TaskTrackerSnapshot {
    summary: Option<String>,
    objective: Option<String>,
    verification_summary: Option<String>,
    verification_todo: Vec<String>,
}

fn merge_dedup_push<T, K, F>(prior: &[T], updates: impl IntoIterator<Item = T>, limit: usize, key_fn: F) -> Vec<T>
where
    K: PartialEq,
    F: Fn(&T) -> K,
    T: Clone,
{
    let mut merged = prior.to_vec();
    for item in updates {
        if let Some(idx) = merged.iter().position(|e| key_fn(e) == key_fn(&item)) {
            merged.remove(idx);
        }
        merged.push(item);
    }
    let keep_from = merged.len().saturating_sub(limit);
    merged.into_iter().skip(keep_from).collect()
}

fn merge_touched_files(prior_envelope: Option<&SessionMemoryEnvelope>, touched_files: &[String]) -> Vec<String> {
    let prior = prior_envelope.map(|e| e.touched_files.as_slice()).unwrap_or(&[]);
    merge_dedup_push(prior, touched_files.iter().cloned(), usize::MAX, |s| s.clone())
}

fn merge_recent_strings(prior: &[String], updates: &[String], limit: usize) -> Vec<String> {
    let prior_normalized: Vec<_> = prior
        .iter()
        .map(|v| normalize_whitespace(v))
        .filter(|v| !v.is_empty())
        .collect();
    let updates_normalized: Vec<_> = updates
        .iter()
        .map(|v| normalize_whitespace(v))
        .filter(|v| !v.is_empty())
        .collect();
    merge_dedup_push(&prior_normalized, updates_normalized, limit, |s| s.to_ascii_lowercase())
}

fn merge_applied_intent_ids(prior: &[String], updates: &[String]) -> Vec<String> {
    merge_dedup_push(prior, updates.iter().cloned(), APPLIED_INTENT_WINDOW, |id| id.clone())
}

fn extract_constraints_from_summary(text: Option<&str>) -> Vec<String> {
    text.into_iter()
        .flat_map(|value| value.split(" | "))
        .map(normalize_whitespace)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let line = line.as_str();
            // Artifact summaries join their lines as "Spec: - a | - b"; strip
            // the label only when the payload keeps the bullet so ordinary
            // "key: value" lines are never misread as constraint bullets.
            let line = match line.split_once(": ") {
                Some((_, rest)) if rest.starts_with("- ") || rest.starts_with("* ") => rest,
                _ => line,
            };
            if let Some(rest) = line.strip_prefix("- ") {
                return Some(rest.trim().to_string());
            }
            line.strip_prefix("* ").map(|rest| rest.trim().to_string())
        })
        .take(MEMORY_LIST_LIMIT)
        .collect()
}

pub fn derive_continuity_summary(
    history: &[Message],
    prior_envelope: Option<&SessionMemoryEnvelope>,
    task_snapshot: &TaskTrackerSnapshot,
) -> String {
    let objective = task_snapshot
        .objective
        .as_deref()
        .or_else(|| prior_envelope.and_then(|e| e.objective.as_deref()))
        .filter(|s| !s.is_empty());

    let latest = history
        .iter()
        .rev()
        .filter(|message| message.role == MessageRole::User || message.role == MessageRole::Assistant)
        .find_map(|message| {
            let trimmed = normalize_whitespace(message.content.as_text().as_ref());
            (!trimmed.is_empty()).then_some((message.role.as_generic_str(), truncate_for_fact(&trimmed, 140)))
        });

    match (objective, latest) {
        (Some(obj), Some((role, text))) => {
            format!("Working on: {obj}. Latest {role} action: {text}.")
        }
        (Some(obj), None) => format!("Working on: {obj}. Session continuity preserved."),
        (None, Some((role, text))) => {
            format!("Latest {role} action: {text}.")
        }
        (None, None) => prior_envelope
            .map(|envelope| envelope.summary.clone())
            .unwrap_or_else(|| "Session continuity facts preserved.".to_string()),
    }
}

fn merge_grounded_facts(
    prior_envelope: Option<&SessionMemoryEnvelope>,
    original_history: &[Message],
    updates: &[GroundedFactRecord],
) -> Vec<GroundedFactRecord> {
    let mut merged = prior_envelope
        .map(|envelope| envelope.grounded_facts.clone())
        .unwrap_or_default();

    for fact in dedup_latest_facts(original_history, 5) {
        let normalized = normalize_whitespace(&fact.fact).to_ascii_lowercase();
        if let Some(existing_idx) = merged
            .iter()
            .position(|entry| normalize_whitespace(&entry.fact).to_ascii_lowercase() == normalized)
        {
            merged.remove(existing_idx);
        }
        merged.push(fact.clone());
    }

    for fact in updates {
        let normalized = normalize_whitespace(&fact.fact).to_ascii_lowercase();
        if let Some(existing_idx) = merged
            .iter()
            .position(|entry| normalize_whitespace(&entry.fact).to_ascii_lowercase() == normalized)
        {
            merged.remove(existing_idx);
        }
        merged.push(fact.clone());
    }

    let keep_from = merged.len().saturating_sub(5);
    merged.into_iter().skip(keep_from).collect()
}

#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
/// Builds an owned envelope; the optional history path is borrowed for display
/// and is not opened by this function.
#[must_use]
pub fn build_session_memory_envelope(
    session_id: &str,
    workspace_root: &Path,
    original_history: &[Message],
    touched_files: &[String],
    summary: String,
    history_artifact_path: Option<&Path>,
    prior_envelope: Option<&SessionMemoryEnvelope>,
    task_snapshot: &TaskTrackerSnapshot,
    envelope_update: Option<&SessionMemoryEnvelopeUpdate>,
) -> SessionMemoryEnvelope {
    let pe = prior_envelope;
    let artifact_cutoff = session_artifact_cutoff(workspace_root, session_id);
    // A present-but-stale artifact must not fall back to a prior envelope's
    // copy (that re-adopts the same leftover). Only inherit prior when the
    // file is absent.
    let spec_present = current_spec_path(workspace_root).exists();
    let evaluation_present = current_evaluation_path(workspace_root).exists();
    let spec_summary = if spec_present {
        read_spec_summary_fresh(workspace_root, artifact_cutoff)
    } else {
        pe.and_then(|e| e.spec_summary.clone())
    };
    let evaluation_summary = if evaluation_present {
        read_evaluation_summary_fresh(workspace_root, artifact_cutoff)
    } else {
        pe.and_then(|e| e.evaluation_summary.clone())
    };
    let merge = |prior: &[String], updates: &[String]| merge_recent_strings(prior, updates, MEMORY_LIST_LIMIT);
    // Constraints extracted from a dropped stale artifact must not re-enter via
    // the prior envelope: a present artifact that failed the freshness check
    // resets the channel. Fresh artifacts (or no artifact files) keep the
    // continuity merge of inherited constraints.
    let artifact_channel_reset = (spec_present
        && artifact_is_stale(&current_spec_path(workspace_root), artifact_cutoff))
        || (evaluation_present && artifact_is_stale(&current_evaluation_path(workspace_root), artifact_cutoff));
    let prior_constraints: &[String] = if artifact_channel_reset {
        &[]
    } else {
        pe.map(|e| e.constraints.as_slice()).unwrap_or(&[])
    };
    let constraints = merge(prior_constraints, &extract_constraints_from_summary(spec_summary.as_deref()));
    let constraints = merge(&constraints, &extract_constraints_from_summary(evaluation_summary.as_deref()));
    let update = envelope_update.cloned().unwrap_or_default();
    let pending_intents = update
        .pending_intents
        .map(|intents| intents.into_iter().take(MAX_QUEUED_FOLLOW_UP_INTENTS).collect())
        .or_else(|| pe.map(|envelope| envelope.pending_intents.clone()))
        .unwrap_or_default();
    let applied_intent_ids = merge_applied_intent_ids(
        pe.map(|envelope| envelope.applied_intent_ids.as_slice()).unwrap_or(&[]),
        &update.applied_intent_ids,
    );

    // Live task-tracker identity wins over a prior envelope from another
    // session/task. Workspace-global `current_task.md` is a live snapshot
    // source; inheriting a stale prior objective re-labels later sessions.
    let live_objective = task_snapshot.objective.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let prior_objective = pe.and_then(|e| e.objective.as_deref()).map(str::trim).filter(|s| !s.is_empty());
    let objective_changed = match (live_objective, prior_objective) {
        (Some(live), Some(prior)) => live != prior,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => false,
    };

    let objective = update
        .objective
        .or_else(|| live_objective.map(ToOwned::to_owned))
        .or_else(|| pe.and_then(|e| e.objective.clone()));

    let task_summary = if objective_changed {
        // Spec S2: never inherit another task's checklist narrative.
        task_snapshot.summary.clone().filter(|s| !s.trim().is_empty())
    } else {
        pe.and_then(|e| e.task_summary.clone())
            .or_else(|| task_snapshot.summary.clone())
    };

    let verification_todo = if objective_changed {
        // Replace, do not union: prior-task open items are not this session's todos.
        task_snapshot
            .verification_todo
            .iter()
            .cloned()
            .chain(update.verification_todo.iter().cloned())
            .take(MEMORY_LIST_LIMIT)
            .collect()
    } else {
        merge(
            pe.map(|e| e.verification_todo.as_slice()).unwrap_or(&[]),
            &task_snapshot
                .verification_todo
                .iter()
                .cloned()
                .chain(update.verification_todo)
                .collect::<Vec<_>>(),
        )
    };

    SessionMemoryEnvelope {
        session_id: session_id.to_string(),
        schema_version: Some(SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
        summary,
        objective,
        task_summary,
        spec_summary,
        evaluation_summary,
        verification_summary: task_snapshot
            .verification_summary
            .clone()
            .or_else(|| pe.and_then(|e| e.verification_summary.clone())),
        constraints: merge(&constraints, &update.constraints),
        grounded_facts: merge_grounded_facts(pe, original_history, &update.grounded_facts),
        touched_files: merge_touched_files(
            pe,
            &touched_files.iter().cloned().chain(update.touched_files).collect::<Vec<_>>(),
        ),
        open_questions: merge(pe.map(|e| e.open_questions.as_slice()).unwrap_or(&[]), &update.open_questions),
        verification_todo,
        delegation_notes: merge(pe.map(|e| e.delegation_notes.as_slice()).unwrap_or(&[]), &update.delegation_notes),
        pending_intents,
        applied_intent_ids,
        history_artifact_path: history_artifact_path
            .map(|p| p.display().to_string())
            .or_else(|| pe.and_then(|e| e.history_artifact_path.clone())),
        generated_at: Utc::now().to_rfc3339(),
    }
}

/// Persist the recoverable full-history artifact and build + inject a session
/// memory envelope into the compacted history. Returns the envelope (with the
/// artifact path) when one was produced.
pub fn persist_memory_envelope(
    workspace_root: &Path,
    session_id: &str,
    vt_cfg: Option<&VTCodeConfig>,
    original_history: &[Message],
    touched_files: &[String],
    compacted: &mut Vec<Message>,
    persistence: MemoryEnvelopePersistence,
    placement: MemoryEnvelopePlacement,
    seed_envelope: Option<&SessionMemoryEnvelope>,
) -> anyhow::Result<Option<SessionMemoryEnvelope>> {
    persist_memory_envelope_with_update(
        workspace_root,
        session_id,
        vt_cfg,
        original_history,
        touched_files,
        compacted,
        persistence,
        placement,
        seed_envelope,
        None,
    )
}

/// Variant of [`persist_memory_envelope`] that applies a live session update
/// while constructing the envelope. Keeping the compatibility wrapper above
/// avoids changing synchronous callers that do not track steering state.
#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub fn persist_memory_envelope_with_update(
    workspace_root: &Path,
    session_id: &str,
    vt_cfg: Option<&VTCodeConfig>,
    original_history: &[Message],
    touched_files: &[String],
    compacted: &mut Vec<Message>,
    persistence: MemoryEnvelopePersistence,
    placement: MemoryEnvelopePlacement,
    seed_envelope: Option<&SessionMemoryEnvelope>,
    envelope_update: Option<&SessionMemoryEnvelopeUpdate>,
) -> anyhow::Result<Option<SessionMemoryEnvelope>> {
    let should_persist = should_persist_memory_envelope(vt_cfg);
    if original_history.is_empty() || (!should_persist && persistence == MemoryEnvelopePersistence::PersistToDisk) {
        return Ok(None);
    }

    let task_snapshot = read_task_tracker_snapshot(workspace_root);
    let history_artifact_path = if should_persist && persistence == MemoryEnvelopePersistence::PersistToDisk {
        let mut hm = HistoryFileManager::new(workspace_root, session_id);
        let hm2 = messages_to_history_messages(original_history, 0);
        let hr = hm
            .write_history_sync(&hm2, original_history.len(), "compaction", touched_files, &[])
            .context("write compaction history artifact")?;
        Some(hr.file_path)
    } else {
        None
    };
    let loaded = if seed_envelope.is_none() {
        load_latest_memory_envelope(workspace_root, session_id)
    } else {
        None
    };
    let prior = seed_envelope.or(loaded.as_ref());
    let envelope = build_session_memory_envelope(
        session_id,
        workspace_root,
        original_history,
        touched_files,
        extract_compaction_summary(compacted, original_history),
        history_artifact_path.as_deref(),
        prior,
        &task_snapshot,
        envelope_update,
    );

    if let Some(hap) = history_artifact_path.as_ref() {
        write_memory_envelope_to_path(&memory_envelope_path_from_history_path(workspace_root, hap), &envelope)?;
    }
    apply_memory_envelope(compacted, &envelope, placement);
    Ok(Some(envelope))
}

/// Async counterpart to [`persist_memory_envelope`]. Compaction runs on the
/// async agent loop, so filesystem work must not block the runtime while the
/// recoverable history artifact or envelope is being written.
pub async fn persist_memory_envelope_async(
    workspace_root: &Path,
    session_id: &str,
    vt_cfg: Option<&VTCodeConfig>,
    original_history: &[Message],
    touched_files: &[String],
    compacted: &mut Vec<Message>,
    persistence: MemoryEnvelopePersistence,
    placement: MemoryEnvelopePlacement,
    seed_envelope: Option<&SessionMemoryEnvelope>,
) -> anyhow::Result<Option<SessionMemoryEnvelope>> {
    persist_memory_envelope_async_with_update(
        workspace_root,
        session_id,
        vt_cfg,
        original_history,
        touched_files,
        compacted,
        persistence,
        placement,
        seed_envelope,
        None,
    )
    .await
}

/// Async variant of [`persist_memory_envelope_async`] that applies a live
/// session update while constructing the envelope.
#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub async fn persist_memory_envelope_async_with_update(
    workspace_root: &Path,
    session_id: &str,
    vt_cfg: Option<&VTCodeConfig>,
    original_history: &[Message],
    touched_files: &[String],
    compacted: &mut Vec<Message>,
    persistence: MemoryEnvelopePersistence,
    placement: MemoryEnvelopePlacement,
    seed_envelope: Option<&SessionMemoryEnvelope>,
    envelope_update: Option<&SessionMemoryEnvelopeUpdate>,
) -> anyhow::Result<Option<SessionMemoryEnvelope>> {
    let should_persist = should_persist_memory_envelope(vt_cfg);
    if original_history.is_empty() || (!should_persist && persistence == MemoryEnvelopePersistence::PersistToDisk) {
        return Ok(None);
    }

    let task_snapshot = read_task_tracker_snapshot_async(workspace_root).await;
    let history_artifact_path = if should_persist && persistence == MemoryEnvelopePersistence::PersistToDisk {
        let mut history_manager = HistoryFileManager::new(workspace_root, session_id);
        let history_messages = messages_to_history_messages(original_history, 0);
        let history_result = history_manager
            .write_history(&history_messages, original_history.len(), "compaction", touched_files, &[])
            .await
            .context("write compaction history artifact")?;
        Some(history_result.file_path)
    } else {
        None
    };

    let loaded = if seed_envelope.is_none() {
        load_latest_memory_envelope_async(workspace_root, session_id).await
    } else {
        None
    };
    let prior = seed_envelope.or(loaded.as_ref());
    let envelope = build_session_memory_envelope(
        session_id,
        workspace_root,
        original_history,
        touched_files,
        extract_compaction_summary(compacted, original_history),
        history_artifact_path.as_deref(),
        prior,
        &task_snapshot,
        envelope_update,
    );

    if let Some(history_path) = history_artifact_path.as_ref() {
        write_memory_envelope_to_path_async(
            &memory_envelope_path_from_history_path(workspace_root, history_path),
            &envelope,
        )
        .await?;
    }
    apply_memory_envelope(compacted, &envelope, placement);
    Ok(Some(envelope))
}

pub fn should_persist_memory_envelope(vt_cfg: Option<&VTCodeConfig>) -> bool {
    vt_cfg.is_some_and(|cfg| cfg.context.dynamic.enabled && cfg.context.dynamic.persist_history)
}

fn memory_envelope_message(envelope: &SessionMemoryEnvelope) -> Message {
    let mut sections = Vec::new();
    sections.push(format!("{}\nSummary:\n{}", MEMORY_ENVELOPE_HEADER, envelope.summary.trim()));

    fn maybe_section(prefix: &str, content: Option<&str>) -> Option<String> {
        content
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("{prefix}\n{s}"))
    }

    fn list_section(prefix: &str, items: &[String]) -> Option<String> {
        (!items.is_empty()).then(|| format!("{prefix}\n- {}", items.join("\n- ")))
    }

    if let Some(s) = maybe_section("Objective", envelope.objective.as_deref()) {
        sections.push(s);
    }
    if let Some(s) = maybe_section("Task Tracker", envelope.task_summary.as_deref()) {
        sections.push(s);
    }
    if let Some(s) = maybe_section("Spec Summary", envelope.spec_summary.as_deref()) {
        sections.push(s);
    }
    if let Some(s) = maybe_section("Evaluation Summary", envelope.evaluation_summary.as_deref()) {
        sections.push(s);
    }
    if let Some(s) = maybe_section("Verification Status", envelope.verification_summary.as_deref()) {
        sections.push(s);
    }
    if let Some(s) = list_section("Constraints", &envelope.constraints) {
        sections.push(s);
    }
    if let Some(s) = list_section("Touched Files", &envelope.touched_files) {
        sections.push(s);
    }

    if !envelope.grounded_facts.is_empty() {
        let facts: Vec<_> = envelope
            .grounded_facts
            .iter()
            .map(|f| format!("[{}] {}", f.source, f.fact.trim()))
            .collect();
        sections.push(format!("Grounded Facts:\n{}", facts.join("\n")));
    }
    if let Some(s) = list_section("Open Questions", &envelope.open_questions) {
        sections.push(s);
    }
    if let Some(s) = list_section("Verification Todo", &envelope.verification_todo) {
        sections.push(s);
    }
    if let Some(s) = list_section("Delegation Notes", &envelope.delegation_notes) {
        sections.push(s);
    }
    if let Some(s) = maybe_section("History Artifact", envelope.history_artifact_path.as_deref()) {
        sections.push(s);
    }

    Message::system(sections.join("\n\n"))
}

fn is_compaction_summary_message(message: &Message) -> bool {
    message.role == MessageRole::System && message.content.as_text().starts_with("Previous conversation summary:\n")
}

pub fn strip_existing_memory_envelope(history: &mut Vec<Message>) {
    history.retain(|message| {
        !(message.role == MessageRole::System && message.content.as_text().starts_with(MEMORY_ENVELOPE_HEADER))
    });
}

fn extract_compaction_summary(compacted: &[Message], original_history: &[Message]) -> String {
    if let Some(summary) = compacted.iter().find_map(|message| {
        if message.role != MessageRole::System {
            return None;
        }

        let text = message.content.as_text();
        let trimmed = text.trim();
        trimmed
            .strip_prefix("Previous conversation summary:\n")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }) {
        return summary;
    }

    if let Some(summary) = compacted.iter().find_map(|message| {
        if message.role != MessageRole::Assistant {
            return None;
        }

        message.reasoning_details.as_ref()?.iter().find_map(|detail| {
            let value = match detail {
                Value::String(serialized) => serde_json::from_str::<Value>(serialized).ok()?,
                value => value.clone(),
            };
            let summary = value
                .get("content")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|summary| !summary.is_empty())?;
            (value.get("type").and_then(Value::as_str) == Some("compaction")).then_some(summary.to_string())
        })
    }) {
        return summary;
    }

    let mut recent = original_history
        .iter()
        .rev()
        .filter_map(|message| {
            let text = message.content.as_text();
            let trimmed = normalize_whitespace(text.as_ref());
            (!trimmed.is_empty()).then_some(format!(
                "{}: {}",
                message.role.as_generic_str(),
                truncate_for_fact(&trimmed, 160)
            ))
        })
        .take(4)
        .collect::<Vec<_>>();
    recent.reverse();

    if recent.is_empty() {
        "Compacted earlier conversation state and preserved continuity facts.".to_string()
    } else {
        format!("Compacted earlier conversation state. Recent preserved context: {}", recent.join(" | "))
    }
}

/// Sanitize a session id the same way history envelope filenames do
/// (32-char ASCII-safe prefix). Callers that match envelope names must use
/// this, not the raw session id.
pub fn sanitize_session_id(session_id: &str) -> String {
    session_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(32)
        .collect()
}

/// Whether a history envelope filename belongs to `session_id`.
///
/// Envelope names use the 32-char sanitized id, with optional `_<n>`
/// suffixes. Matching must use this exact rule — a loose `starts_with` on
/// the raw id over-preserves unrelated sessions.
pub fn memory_envelope_file_matches_session(name: &str, session_id: &str) -> bool {
    let session_prefix = sanitize_session_id(session_id);
    name == format!("{session_prefix}{MEMORY_ENVELOPE_SUFFIX}")
        || (name.starts_with(&format!("{session_prefix}_")) && name.ends_with(MEMORY_ENVELOPE_SUFFIX))
}

fn parse_task_tracker_snapshot(content: &str) -> TaskTrackerSnapshot {
    let title = content
        .lines()
        .find(|line| line.starts_with("# "))
        .map(|line| line.trim_start_matches("# ").trim().to_string());
    let checklist = content
        .lines()
        .filter(|line| line.trim_start().starts_with("- ["))
        .take(5)
        .map(normalize_whitespace)
        .collect::<Vec<_>>();
    let verification_summary = extract_verification_summary(content, &checklist);
    let verification_todo = content
        .lines()
        .filter(|line| line.trim_start().starts_with("- [ ]"))
        .take(MEMORY_LIST_LIMIT)
        .map(normalize_whitespace)
        .collect::<Vec<_>>();
    let summary = match (title.clone(), checklist.is_empty()) {
        (Some(title), false) => Some(format!("{title}: {}", checklist.join(" | "))),
        (Some(title), true) => Some(title),
        (None, false) => Some(checklist.join(" | ")),
        (None, true) => None,
    };

    TaskTrackerSnapshot {
        summary,
        objective: title,
        verification_summary,
        verification_todo,
    }
}

pub fn read_task_tracker_snapshot(workspace_root: &Path) -> TaskTrackerSnapshot {
    let tracker_path = current_task_path(workspace_root);
    fs::read_to_string(&tracker_path)
        .ok()
        .map(|content| parse_task_tracker_snapshot(&content))
        .unwrap_or_default()
}

/// Read the task tracker without blocking the async runtime.
pub async fn read_task_tracker_snapshot_async(workspace_root: &Path) -> TaskTrackerSnapshot {
    let tracker_path = current_task_path(workspace_root);
    async_fs::read_to_string(tracker_path)
        .await
        .ok()
        .map(|content| parse_task_tracker_snapshot(&content))
        .unwrap_or_default()
}

fn extract_verification_summary(content: &str, checklist: &[String]) -> Option<String> {
    let verify_commands = collect_structured_verify_commands(content);
    if !verify_commands.is_empty() {
        return Some(render_bullet_list(&verify_commands));
    }

    let fallback_lines = checklist
        .iter()
        .filter(|line| looks_like_verification_line(line))
        .cloned()
        .collect::<Vec<_>>();
    (!fallback_lines.is_empty()).then(|| fallback_lines.join("\n"))
}

/// Split the tail of a `verify:` line into clean commands.
///
/// Plan writers sometimes emit `verify: [a] and verify: [b]` on one line.
/// Strip at most one matching outer `[`…`]` pair per piece and drop empties so
/// a spliced marker can never leak into `verification_summary`.
fn split_verify_command_tail(rest: &str) -> Vec<String> {
    rest.split(" and verify:")
        .map(str::trim)
        .map(strip_one_outer_bracket_pair)
        .filter(|piece| !piece.is_empty())
        .map(normalize_whitespace)
        .filter(|piece| !piece.is_empty())
        .collect()
}

fn strip_one_outer_bracket_pair(piece: &str) -> &str {
    let trimmed = piece.trim();
    // Plan splices can leave the closing bracket off (`verify: [cmd`).
    let Some(inner) = trimmed.strip_prefix('[').map(str::trim_start) else {
        return trimmed;
    };
    match inner.strip_suffix(']') {
        Some(stripped) => stripped.trim_end(),
        None => inner,
    }
}

fn collect_structured_verify_commands(content: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut in_verify_block = false;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("verify:") {
            for command in split_verify_command_tail(rest) {
                commands.push(command);
            }
            in_verify_block = rest.trim().is_empty();
            continue;
        }

        if !in_verify_block {
            continue;
        }

        if trimmed.is_empty() {
            continue;
        }

        if (line.starts_with(' ') || line.starts_with('\t')) && trimmed.starts_with("- ") {
            commands.push(normalize_whitespace(trimmed.trim_start_matches("- ")));
            continue;
        }

        in_verify_block = false;
    }

    commands
}

fn render_bullet_list(items: &[String]) -> String {
    items.iter().map(|item| format!("- {item}")).collect::<Vec<_>>().join("\n")
}

fn looks_like_verification_line(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    [
        "verify",
        "verification",
        "test",
        "lint",
        "cargo check",
        "check-dev.sh",
        "check.sh",
    ]
    .iter()
    .any(|keyword| lowered.contains(keyword))
}

fn memory_envelope_path_from_history_path(workspace_root: &Path, history_path: &Path) -> PathBuf {
    let absolute_history_path = if history_path.is_absolute() {
        history_path.to_path_buf()
    } else {
        workspace_root.join(history_path)
    };

    let file_name = absolute_history_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            if let Some(stem) = name.strip_suffix(".jsonl") {
                format!("{stem}{MEMORY_ENVELOPE_SUFFIX}")
            } else {
                format!("{name}{MEMORY_ENVELOPE_SUFFIX}")
            }
        })
        .unwrap_or_else(|| format!("session_memory{MEMORY_ENVELOPE_SUFFIX}"));

    let parent = absolute_history_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| workspace_root.join(".vtcode").join("history"));
    parent.join(file_name)
}

pub fn default_memory_envelope_path_for_session(workspace_root: &Path, session_id: &str) -> PathBuf {
    workspace_root
        .join(".vtcode")
        .join("history")
        .join(format!("{}{MEMORY_ENVELOPE_SUFFIX}", sanitize_session_id(session_id)))
}

fn memory_envelope_paths_for_session(workspace_root: &Path, session_id: &str) -> Vec<PathBuf> {
    let history_dir = workspace_root.join(".vtcode").join("history");
    let mut candidates = fs::read_dir(history_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| memory_envelope_file_matches_session(name, session_id))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        let left_modified = fs::metadata(left).and_then(|metadata| metadata.modified()).ok();
        let right_modified = fs::metadata(right).and_then(|metadata| metadata.modified()).ok();
        right_modified
            .cmp(&left_modified)
            .then_with(|| right.file_name().cmp(&left.file_name()))
    });
    candidates
}

pub fn latest_memory_envelope_path_for_session(workspace_root: &Path, session_id: &str) -> Option<PathBuf> {
    memory_envelope_paths_for_session(workspace_root, session_id)
        .into_iter()
        .find(|path| {
            fs::read_to_string(path)
                .ok()
                .and_then(|content| serde_json::from_str::<SessionMemoryEnvelope>(&content).ok())
                .is_some_and(|envelope| envelope.session_id.is_empty() || envelope.session_id == session_id)
        })
}

pub fn load_latest_memory_envelope(workspace_root: &Path, session_id: &str) -> Option<SessionMemoryEnvelope> {
    let path = latest_memory_envelope_path_for_session(workspace_root, session_id)?;
    let content = fs::read_to_string(path).ok()?;
    let envelope: SessionMemoryEnvelope = serde_json::from_str(&content).ok()?;
    if !envelope.session_id.is_empty() && envelope.session_id != session_id {
        return None;
    }
    Some(envelope)
}

async fn memory_envelope_paths_for_session_async(workspace_root: &Path, session_id: &str) -> Vec<PathBuf> {
    let history_dir = workspace_root.join(".vtcode").join("history");
    let mut candidates = Vec::new();
    let Ok(mut entries) = async_fs::read_dir(history_dir).await else {
        return candidates;
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let matches = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| memory_envelope_file_matches_session(name, session_id));
        if matches {
            candidates.push(path);
        }
    }

    let mut modified = Vec::with_capacity(candidates.len());
    for path in candidates {
        let timestamp = async_fs::metadata(&path)
            .await
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        modified.push((timestamp, path));
    }
    modified.sort_by(|(left_time, left_path), (right_time, right_path)| {
        right_time
            .cmp(left_time)
            .then_with(|| right_path.file_name().cmp(&left_path.file_name()))
    });
    modified.into_iter().map(|(_, path)| path).collect()
}

/// Find and deserialize the newest valid envelope without synchronously
/// scanning historical candidates on the runtime thread.
pub async fn latest_memory_envelope_path_for_session_async(workspace_root: &Path, session_id: &str) -> Option<PathBuf> {
    for path in memory_envelope_paths_for_session_async(workspace_root, session_id).await {
        let Ok(content) = async_fs::read_to_string(&path).await else {
            continue;
        };
        let Ok(envelope) = serde_json::from_str::<SessionMemoryEnvelope>(&content) else {
            continue;
        };
        if envelope.session_id.is_empty() || envelope.session_id == session_id {
            return Some(path);
        }
    }
    None
}

pub async fn load_latest_memory_envelope_async(
    workspace_root: &Path,
    session_id: &str,
) -> Option<SessionMemoryEnvelope> {
    let path = latest_memory_envelope_path_for_session_async(workspace_root, session_id).await?;
    let content = async_fs::read_to_string(path).await.ok()?;
    let envelope: SessionMemoryEnvelope = serde_json::from_str(&content).ok()?;
    if !envelope.session_id.is_empty() && envelope.session_id != session_id {
        return None;
    }
    Some(envelope)
}

pub fn insert_memory_envelope_message(
    history: &mut Vec<Message>,
    envelope: &SessionMemoryEnvelope,
    placement: MemoryEnvelopePlacement,
) {
    let message = memory_envelope_message(envelope);
    match placement {
        MemoryEnvelopePlacement::Start => history.insert(0, message),
        MemoryEnvelopePlacement::BeforeLastUserOrSummary => {
            let insert_at = history
                .iter()
                .rposition(|item| item.role == MessageRole::User || is_compaction_summary_message(item))
                .unwrap_or(0);
            history.insert(insert_at, message);
        }
    }
}

fn apply_memory_envelope(
    compacted: &mut Vec<Message>,
    envelope: &SessionMemoryEnvelope,
    placement: MemoryEnvelopePlacement,
) {
    strip_existing_memory_envelope(compacted);
    insert_memory_envelope_message(compacted, envelope, placement);
}

pub fn inject_latest_memory_envelope(workspace_root: &Path, session_id: &str, history: &mut Vec<Message>) -> bool {
    let Some(envelope) = load_latest_memory_envelope(workspace_root, session_id) else {
        return false;
    };

    strip_existing_memory_envelope(history);
    insert_memory_envelope_message(history, &envelope, MemoryEnvelopePlacement::Start);
    true
}

pub fn write_memory_envelope_to_path(path: &Path, envelope: &SessionMemoryEnvelope) -> anyhow::Result<()> {
    let serialized = serde_json::to_vec_pretty(envelope).context("serialize memory envelope")?;
    vtcode_commons::VtCodePaths::write_private_file_atomic(path, &serialized)
        .with_context(|| format!("write memory envelope {}", path.display()))
}

/// Write an envelope through the shared private-file policy on a blocking worker.
/// The synced, exclusive temporary file is created beside the destination and
/// renamed over it without first removing the last committed envelope.
pub async fn write_memory_envelope_to_path_async(path: &Path, envelope: &SessionMemoryEnvelope) -> anyhow::Result<()> {
    let serialized = serde_json::to_vec_pretty(envelope).context("serialize memory envelope")?;
    vtcode_commons::fs::write_private_file_atomic(path, &serialized)
        .await
        .with_context(|| format!("write memory envelope {}", path.display()))
}

pub fn has_latest_memory_envelope(workspace_root: &Path, session_id: &str) -> bool {
    latest_memory_envelope_path_for_session(workspace_root, session_id).is_some()
}

// ---------------------------------------------------------------------------
// Local compaction configuration + zero-cost fork history
// ---------------------------------------------------------------------------

pub fn configured_retained_user_messages(vt_cfg: Option<&VTCodeConfig>) -> usize {
    vt_cfg.map(|cfg| cfg.context.dynamic.retained_user_messages).unwrap_or(4)
}

pub fn local_compaction_config(vt_cfg: Option<&VTCodeConfig>, always_summarize: bool) -> CompactionConfig {
    CompactionConfig {
        always_summarize,
        retained_user_messages: configured_retained_user_messages(vt_cfg),
        ..CompactionConfig::default()
    }
}

fn collect_zero_cost_retained_user_messages(
    history: &[Message],
    token_budget: usize,
    max_messages: usize,
) -> Vec<Message> {
    if token_budget == 0 || max_messages == 0 {
        return Vec::new();
    }

    let mut kept = Vec::new();
    let mut remaining = token_budget;

    for message in history.iter().rev() {
        if kept.len() >= max_messages {
            break;
        }
        if message.role != MessageRole::User || message.content.trim().is_empty() {
            continue;
        }

        let estimated = message.estimate_tokens();
        if estimated <= remaining {
            kept.push(message.clone());
            remaining = remaining.saturating_sub(estimated);
            continue;
        }

        if remaining > 4 {
            let truncated = truncate_to_token_limit(message.content.as_text().as_ref(), remaining.saturating_sub(4));
            let trimmed = truncated.trim();
            if !trimmed.is_empty() {
                kept.push(Message::user(trimmed.to_string()));
            }
        }
        break;
    }

    kept.reverse();
    kept
}

pub fn build_zero_cost_summarized_fork_history(
    source_history: &[Message],
    source_envelope: Option<&SessionMemoryEnvelope>,
    retained_user_messages: usize,
) -> Vec<Message> {
    let summary = source_envelope
        .map(|envelope| normalize_whitespace(&envelope.summary))
        .filter(|summary| !summary.is_empty())
        .unwrap_or_else(|| derive_continuity_summary(source_history, source_envelope, &TaskTrackerSnapshot::default()));

    let retained_users = collect_zero_cost_retained_user_messages(
        source_history,
        CompactionConfig::default().retained_user_message_tokens,
        retained_user_messages,
    );

    let mut compacted = Vec::with_capacity(retained_users.len().saturating_add(1));
    compacted.push(Message::system(format!("Previous conversation summary:\n{}", summary.trim())));
    compacted.extend(retained_users);
    compacted
}

// ---------------------------------------------------------------------------
// File-read de-duplication for local compaction
// ---------------------------------------------------------------------------

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

fn is_read_file_tool_name(tool_name: &str) -> bool {
    tool_name == tool_names::READ_FILE || tool_name.ends_with(".read_file")
}

fn collect_file_read_tool_kinds(history: &[Message]) -> HashMap<String, FileReadToolKind> {
    let mut kinds = HashMap::new();
    for message in history {
        let Some(tool_calls) = message.tool_calls.as_ref() else {
            continue;
        };
        for tc in tool_calls {
            let Some(tn) = tc.tool_name() else {
                continue;
            };
            let kind = if is_read_file_tool_name(tn) {
                Some(FileReadToolKind::ReadFile)
            } else if tn == tool_names::UNIFIED_FILE {
                tc.execution_arguments().ok().and_then(|args| {
                    args.get("action")
                        .and_then(Value::as_str)
                        .filter(|a| *a == "read")
                        .map(|_| FileReadToolKind::UnifiedFileRead)
                })
            } else {
                None
            };
            if let Some(k) = kind {
                kinds.insert(tc.id.clone(), k);
            }
        }
    }
    kinds
}

fn normalize_file_read_target(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.replace('\\', "/"))
}

fn build_file_read_dedup_key(payload: &Value) -> Option<FileReadDedupKey> {
    let obj = payload.as_object()?;
    if obj.get("items").is_some()
        || obj.get("error").is_some()
        || obj.get("spool_chunked").and_then(Value::as_bool).unwrap_or(false)
        || obj.get("has_more").and_then(Value::as_bool).unwrap_or(false)
    {
        return None;
    }
    let target = obj
        .get("file_path")
        .and_then(Value::as_str)
        .or_else(|| obj.get("path").and_then(Value::as_str))
        .and_then(normalize_file_read_target)?;
    Some(FileReadDedupKey {
        target,
        start_line: obj.get("start_line").and_then(Value::as_u64),
        end_line: obj.get("end_line").and_then(Value::as_u64),
        spool_path: obj
            .get("spool_path")
            .and_then(Value::as_str)
            .and_then(normalize_file_read_target),
    })
}

fn build_file_read_placeholder_content(payload: &Value, key: &FileReadDedupKey) -> String {
    let mut p = serde_json::Map::new();
    p.insert("deduped_read".into(), Value::Bool(true));
    p.insert("note".into(), Value::String(DEDUPED_FILE_READ_NOTE.to_string()));

    fn maybe_str(p: &mut serde_json::Map<String, Value>, payload: &Value, key: &str) {
        if let Some(s) = payload
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            p.insert(key.into(), Value::String(s.to_string()));
        }
    }

    maybe_str(&mut p, payload, "file_path");
    maybe_str(&mut p, payload, "path");
    if let Some(sl) = key.start_line {
        p.insert("start_line".into(), json!(sl));
    }
    if let Some(el) = key.end_line {
        p.insert("end_line".into(), json!(el));
    }
    if let Some(sp) = key.spool_path.as_deref() {
        p.insert("spool_path".into(), json!(sp));
    }
    Value::Object(p).to_string()
}

fn file_read_dedup_candidate(
    message: &Message,
    tool_kinds: &HashMap<String, FileReadToolKind>,
) -> Option<FileReadDedupCandidate> {
    if message.role != MessageRole::Tool {
        return None;
    }

    let kind = message
        .tool_call_id
        .as_deref()
        .and_then(|tool_call_id| tool_kinds.get(tool_call_id).copied())
        .or_else(|| {
            message
                .origin_tool
                .as_deref()
                .and_then(|tool_name| is_read_file_tool_name(tool_name).then_some(FileReadToolKind::ReadFile))
        })?;

    if !matches!(kind, FileReadToolKind::ReadFile | FileReadToolKind::UnifiedFileRead) {
        return None;
    }

    let payload: Value = serde_json::from_str(message.content.as_text().as_ref()).ok()?;
    let key = build_file_read_dedup_key(&payload)?;

    Some(FileReadDedupCandidate {
        placeholder_content: build_file_read_placeholder_content(&payload, &key),
        key,
    })
}

pub fn dedup_repeated_file_reads_for_local_compaction(history: &[Message]) -> Vec<Message> {
    let tool_kinds = collect_file_read_tool_kinds(history);
    let mut last_idx = HashMap::new();
    let mut candidates = Vec::new();
    for (i, msg) in history.iter().enumerate() {
        let Some(c) = file_read_dedup_candidate(msg, &tool_kinds) else {
            continue;
        };
        last_idx.insert(c.key.clone(), i);
        candidates.push((i, c));
    }
    let mut deduped = history.to_vec();
    let mut changed = false;
    for (idx, c) in candidates {
        if last_idx.get(&c.key).copied() == Some(idx) {
            continue;
        }
        if let Some(msg) = deduped.get_mut(idx) {
            msg.content = c.placeholder_content.into();
            changed = true;
        }
    }
    if changed { deduped } else { history.to_vec() }
}

// ---------------------------------------------------------------------------
// Threshold resolution (shared by every compaction trigger)
// ---------------------------------------------------------------------------

/// Conservative output reservation when the request does not declare a limit.
pub const DEFAULT_OUTPUT_RESERVE_TOKENS: usize = 4096;

/// Resolve a prompt threshold while preserving room for the next response.
pub fn resolve_compaction_threshold(configured_threshold: Option<u64>, context_size: usize) -> Option<u64> {
    resolve_compaction_threshold_with_reserve(configured_threshold, context_size, DEFAULT_OUTPUT_RESERVE_TOKENS)
}

pub fn resolve_compaction_threshold_with_reserve(
    configured_threshold: Option<u64>,
    context_size: usize,
    reserved_output_tokens: usize,
) -> Option<u64> {
    let configured = configured_threshold.filter(|value| *value > 0);
    if context_size == 0 {
        return configured;
    }
    let prompt_budget = context_size.saturating_sub(reserved_output_tokens).max(1) as u64;
    // Default trigger is a ratio of the prompt budget so long runs compact
    // before the expensive near-full zone (HarnessTax / session-efficiency).
    // An explicit `auto_compaction_threshold_tokens` still wins, capped at the
    // prompt budget so compaction never exceeds the usable window.
    // Integer percent avoids f64→u64 cast lint; `default_compaction_trigger_uses_ratio_of_prompt_budget`
    // pins this to `DEFAULT_COMPACTION_TRIGGER_RATIO`.
    const TRIGGER_RATIO_PERCENT: u64 = 75;
    let default_trigger = prompt_budget.saturating_mul(TRIGGER_RATIO_PERCENT) / 100;
    let default_trigger = default_trigger.clamp(1, prompt_budget);
    Some(configured.map_or(default_trigger, |value| value.min(prompt_budget)))
}

/// Explicit trigger overrides can reduce, but never bypass, the session ceiling.
pub fn resolve_effective_compaction_threshold(
    configured_threshold: Option<u64>,
    provider_context_size: usize,
    session_context_budget: usize,
) -> Option<u64> {
    resolve_compaction_threshold(
        configured_threshold,
        effective_session_context_budget(provider_context_size, session_context_budget),
    )
}

/// Zero means unknown/unconfigured; positive limits are intersected.
#[must_use]
pub fn effective_session_context_budget(provider_context_size: usize, session_context_budget: usize) -> usize {
    match (provider_context_size > 0, session_context_budget > 0) {
        (true, true) => provider_context_size.min(session_context_budget),
        (true, false) => provider_context_size,
        (false, true) => session_context_budget,
        (false, false) => 0,
    }
}

/// Resolve catalog/dynamic model capacity and the provider's route-specific ceiling.
#[must_use]
pub fn effective_context_budget(vt_cfg: Option<&VTCodeConfig>, provider: &dyn LLMProvider, model: &str) -> usize {
    use crate::llm::model_resolver::{DynamicModelMeta, ModelResolver};

    let provider_capacity = provider.effective_context_size(model);
    // Resolve through the shared model capability path so catalog entries and
    // discovered model metadata participate in exactly the same calculation as
    // request construction. The provider remains a hard route-specific ceiling
    // because a catalog can describe a larger platform window than this endpoint
    // exposes (or an explicit `ContextWindowProvider` override can narrow it).
    let resolved_capacity = ModelResolver::resolve(
        Some(provider.name()),
        model,
        &[],
        Some(DynamicModelMeta {
            display_name: model.to_owned(),
            description: None,
            context_window: (provider_capacity > 0).then_some(provider_capacity),
        }),
    )
    .and_then(|resolved| resolved.context_window())
    // Custom provider names and dynamic model ids are intentionally not
    // required to exist in the built-in catalog. The provider trait has
    // already resolved the route-specific capacity, so keep that value when
    // catalog resolution cannot identify the route.
    .unwrap_or(provider_capacity);
    let capacity = effective_session_context_budget(resolved_capacity, provider_capacity);
    let session_budget = vt_cfg.map_or_else(default_max_context_tokens, |cfg| cfg.context.max_context_tokens);
    effective_session_context_budget(capacity, session_budget)
}

pub fn effective_compaction_threshold_with_reserve(
    vt_cfg: Option<&VTCodeConfig>,
    provider: &dyn LLMProvider,
    model: &str,
    reserved_output_tokens: usize,
) -> Option<usize> {
    resolve_compaction_threshold_with_reserve(
        vt_cfg.and_then(|cfg| cfg.agent.harness.auto_compaction_threshold_tokens),
        effective_context_budget(vt_cfg, provider, model),
        reserved_output_tokens,
    )
    .and_then(|value| usize::try_from(value).ok())
}

pub fn effective_compaction_threshold(
    vt_cfg: Option<&VTCodeConfig>,
    provider: &dyn LLMProvider,
    model: &str,
) -> Option<usize> {
    let reserve = provider
        .sampling_overrides(model)
        .max_tokens
        .map_or(DEFAULT_OUTPUT_RESERVE_TOKENS, |value| value as usize);
    effective_compaction_threshold_with_reserve(vt_cfg, provider, model, reserve)
}

#[cfg(test)]
mod persistence_tests;

#[cfg(test)]
mod tests {
    use super::extract_compaction_summary;
    use super::{
        SessionMemoryEnvelope, TaskTrackerSnapshot, build_session_memory_envelope, parse_task_tracker_snapshot,
        resolve_compaction_threshold_with_reserve,
    };
    use crate::llm::provider::Message;
    use std::path::Path;

    #[test]
    fn default_compaction_trigger_uses_ratio_of_prompt_budget() {
        // 100_000 context, 4_096 reserve → prompt budget 95_904.
        // Default trigger is 75% of the prompt budget, not the full budget.
        let threshold = resolve_compaction_threshold_with_reserve(None, 100_000, 4_096).expect("threshold");
        let prompt_budget = 100_000u64 - 4_096;
        let expected = prompt_budget * 75 / 100;
        assert_eq!(threshold, expected);
        // Keep the integer percent in sync with the shared ratio constant.
        assert!((vtcode_config::constants::context::DEFAULT_COMPACTION_TRIGGER_RATIO - 0.75).abs() < f64::EPSILON);
        assert!(threshold < prompt_budget, "default trigger must fire before the full prompt budget");
        // Explicit config still wins and is capped at the prompt budget.
        assert_eq!(resolve_compaction_threshold_with_reserve(Some(10_000), 100_000, 4_096), Some(10_000));
        assert_eq!(resolve_compaction_threshold_with_reserve(Some(200_000), 100_000, 4_096), Some(prompt_budget));
    }

    fn tracker_snapshot(summary: &str, objective: &str, todo: &[&str]) -> TaskTrackerSnapshot {
        let markdown = format!("# {objective}\n\n- [ ] {}\n", todo.join("\n- [ ] "));
        let mut snap = parse_task_tracker_snapshot(&markdown);
        snap.summary = Some(summary.to_string());
        snap.objective = Some(objective.to_string());
        snap.verification_todo = todo.iter().map(|s| (*s).to_string()).collect();
        snap
    }

    #[test]
    fn envelope_prefers_live_objective_over_stale_prior() {
        let prior = SessionMemoryEnvelope {
            session_id: "sess-a".to_string(),
            objective: Some("1789-mighty-harbor".to_string()),
            task_summary: Some("mighty-harbor: README checklist".to_string()),
            verification_todo: vec!["mighty todo".to_string()],
            ..Default::default()
        };
        let live = tracker_snapshot("bright-squid analyze fix", "1789-bright-squid", &["cargo check analyze"]);

        let envelope = build_session_memory_envelope(
            "sess-b",
            Path::new("."),
            &[],
            &[],
            "Working on bright-squid".to_string(),
            None,
            Some(&prior),
            &live,
            None,
        );

        assert_eq!(envelope.objective.as_deref(), Some("1789-bright-squid"));
        assert!(
            envelope.task_summary.as_deref().unwrap_or_default().contains("bright-squid"),
            "task_summary must not keep prior-task narrative: {:?}",
            envelope.task_summary
        );
        assert_eq!(envelope.verification_todo, vec!["cargo check analyze".to_string()]);
    }

    #[test]
    fn envelope_objective_change_drops_prior_task_summary_when_live_empty() {
        let prior = SessionMemoryEnvelope {
            session_id: "sess-a".to_string(),
            objective: Some("task-a".to_string()),
            task_summary: Some("task-a: stale checklist".to_string()),
            verification_todo: vec!["stale".to_string()],
            ..Default::default()
        };
        let live = TaskTrackerSnapshot {
            objective: Some("task-b".to_string()),
            summary: None,
            ..Default::default()
        };

        let envelope = build_session_memory_envelope(
            "sess-b",
            Path::new("."),
            &[],
            &[],
            "summary".to_string(),
            None,
            Some(&prior),
            &live,
            None,
        );

        assert_eq!(envelope.objective.as_deref(), Some("task-b"));
        assert_eq!(envelope.task_summary, None);
        assert!(envelope.verification_todo.is_empty());
    }

    #[test]
    fn envelope_same_objective_keeps_todo_merge() {
        let prior = SessionMemoryEnvelope {
            session_id: "sess-a".to_string(),
            objective: Some("task-1".to_string()),
            task_summary: Some("task-1: shared".to_string()),
            verification_todo: vec!["keep me".to_string()],
            ..Default::default()
        };
        let live = tracker_snapshot("task-1 continued", "task-1", &["new item"]);

        let envelope = build_session_memory_envelope(
            "sess-a",
            Path::new("."),
            &[],
            &[],
            "summary".to_string(),
            None,
            Some(&prior),
            &live,
            None,
        );

        assert_eq!(envelope.objective.as_deref(), Some("task-1"));
        assert!(envelope.verification_todo.contains(&"keep me".to_string()));
        assert!(envelope.verification_todo.contains(&"new item".to_string()));
    }

    #[test]
    fn envelope_stale_artifact_resets_prior_constraints() {
        let workspace = tempfile::tempdir().expect("tempdir");
        let tasks_dir = workspace.path().join(".vtcode").join("tasks");
        std::fs::create_dir_all(&tasks_dir).expect("tasks dir");
        std::fs::create_dir_all(workspace.path().join(".vtcode").join("sessions").join("sess-stale"))
            .expect("session dir anchors the artifact cutoff");
        let spec_path = tasks_dir.join("current_spec.md");
        std::fs::write(&spec_path, "# Spec\n- Do not touch prod config\n").expect("write stale spec");
        let stale = std::time::SystemTime::now() - std::time::Duration::from_secs(48 * 60 * 60);
        let file = std::fs::File::options().write(true).open(&spec_path).expect("open");
        file.set_modified(stale).expect("set mtime");

        let prior = SessionMemoryEnvelope {
            session_id: "sess-stale".to_string(),
            constraints: vec!["Do not touch prod config".to_string(), "Keep user budget".to_string()],
            ..Default::default()
        };

        let envelope = build_session_memory_envelope(
            "sess-stale",
            workspace.path(),
            &[],
            &[],
            "summary".to_string(),
            None,
            Some(&prior),
            &tracker_snapshot("continuing", "sess-stale", &["cargo check"]),
            None,
        );

        assert!(envelope.spec_summary.is_none(), "stale spec must not describe the session");
        assert!(
            envelope.constraints.is_empty(),
            "a stale artifact resets the constraints channel so its lines cannot re-enter: {:?}",
            envelope.constraints
        );
    }

    #[test]
    fn envelope_fresh_artifact_merges_prior_constraints() {
        let workspace = tempfile::tempdir().expect("tempdir");
        let tasks_dir = workspace.path().join(".vtcode").join("tasks");
        std::fs::create_dir_all(&tasks_dir).expect("tasks dir");
        std::fs::create_dir_all(workspace.path().join(".vtcode").join("sessions").join("sess-fresh"))
            .expect("session dir anchors the artifact cutoff");
        std::fs::write(tasks_dir.join("current_spec.md"), "# Spec\n- Keep coverage at 70%\n")
            .expect("write fresh spec");

        let prior = SessionMemoryEnvelope {
            session_id: "sess-fresh".to_string(),
            constraints: vec!["Do not redesign the harness".to_string()],
            ..Default::default()
        };

        let envelope = build_session_memory_envelope(
            "sess-fresh",
            workspace.path(),
            &[],
            &[],
            "summary".to_string(),
            None,
            Some(&prior),
            &tracker_snapshot("continuing", "sess-fresh", &["cargo check"]),
            None,
        );

        assert!(
            envelope.constraints.contains(&"Do not redesign the harness".to_string()),
            "fresh artifacts keep the continuity merge of inherited constraints: {:?}",
            envelope.constraints
        );
        assert!(
            envelope.constraints.contains(&"Keep coverage at 70%".to_string()),
            "fresh artifact constraints are extracted: {:?}",
            envelope.constraints
        );
    }

    #[test]
    fn local_summary_takes_precedence_over_retained_provider_detail() {
        let compacted = vec![
            Message::system("Previous conversation summary:\nnew local summary".to_string()),
            Message::user("latest request".to_string()),
            Message::assistant(String::new()).with_reasoning_details(Some(vec![serde_json::json!({
                "type": "compaction",
                "content": "stale provider summary",
            })])),
        ];

        assert_eq!(extract_compaction_summary(&compacted, &[]), "new local summary");
    }

    #[test]
    fn split_verify_command_tail_splits_and_strips_brackets() {
        let tail = " [cargo nextest run -E 'test(turn_loop_helpers) or test(tool_outcomes) or test(blocked_handoff)'] and verify: [cargo check --locked";
        let commands = super::split_verify_command_tail(tail);
        assert_eq!(
            commands,
            vec![
                "cargo nextest run -E 'test(turn_loop_helpers) or test(tool_outcomes) or test(blocked_handoff)'"
                    .to_string(),
                "cargo check --locked".to_string(),
            ]
        );
    }

    #[test]
    fn split_verify_command_tail_keeps_legitimate_inner_brackets() {
        // Only one outer pair is stripped; array/subscript syntax inside the
        // command must survive.
        let commands = super::split_verify_command_tail(" cargo test --features 'a[b]' ");
        assert_eq!(commands, vec!["cargo test --features 'a[b]'".to_string()]);
    }

    #[test]
    fn structured_verify_commands_do_not_splice_bracketed_pairs() {
        let content =
            "# Fix\n\n- [x] step\n  files: src/x.rs\n  verify: [cargo check --locked] and verify: [cargo fmt --check\n";
        let commands = super::collect_structured_verify_commands(content);
        assert_eq!(commands, vec!["cargo check --locked".to_string(), "cargo fmt --check".to_string()]);
        let summary = super::extract_verification_summary(content, &[]).expect("summary");
        assert!(!summary.contains("] and verify:"), "splice marker must not leak: {summary}");
    }
}
