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
    MAX_APPLIED_FOLLOW_UP_INTENT_IDS, MAX_PENDING_FOLLOW_UP_INTENTS, QueuedFollowUpIntent,
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

mod persistence;
#[cfg(test)]
use persistence::{
    collect_structured_verify_commands, extract_compaction_summary, extract_verification_summary,
    parse_task_tracker_snapshot, split_verify_command_tail,
};
pub use persistence::{
    configured_retained_user_messages, default_memory_envelope_path_for_session, has_latest_memory_envelope,
    inject_latest_memory_envelope, insert_memory_envelope_message, latest_memory_envelope_path_for_session,
    latest_memory_envelope_path_for_session_async, load_latest_memory_envelope, load_latest_memory_envelope_async,
    local_compaction_config, memory_envelope_file_matches_session, persist_memory_envelope,
    persist_memory_envelope_async, persist_memory_envelope_async_with_update, persist_memory_envelope_with_update,
    read_task_tracker_snapshot, read_task_tracker_snapshot_async, sanitize_session_id, should_persist_memory_envelope,
    strip_existing_memory_envelope, write_memory_envelope_to_path, write_memory_envelope_to_path_async,
};

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
        .map(|intents| {
            if intents.len() > MAX_PENDING_FOLLOW_UP_INTENTS {
                // Flood corner: repeated mid-turn drains accumulate in-flight
                // intents until the post-turn checkpoint acknowledges them, so a
                // sustained flood can exceed the durable slots. Oldest-first is
                // kept and the newest tail is dropped; say so loudly instead of
                // losing accepted input silently.
                tracing::warn!(
                    pending = intents.len(),
                    capacity = MAX_PENDING_FOLLOW_UP_INTENTS,
                    "pending follow-up intents exceed durable envelope capacity; dropping newest"
                );
            }
            intents.into_iter().take(MAX_PENDING_FOLLOW_UP_INTENTS).collect()
        })
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
mod tests;
