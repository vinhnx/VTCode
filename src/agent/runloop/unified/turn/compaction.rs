use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use vtcode_config::constants::context::TOKEN_BUDGET_HIGH_THRESHOLD;
use vtcode_core::compaction::CompactionConfig;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::context::history_files::{HistoryFileManager, messages_to_history_messages};
use vtcode_core::core::agent::harness_artifacts::{
    current_task_path, read_evaluation_summary, read_spec_summary,
};
use vtcode_core::hooks::LifecycleHookEngine;
use vtcode_core::llm::provider::{LLMProvider, Message, MessageRole, ResponsesCompactionOptions};
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
        Some(((context_size as f64) * TOKEN_BUDGET_HIGH_THRESHOLD).round() as u64)
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

fn configured_compaction_threshold(
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

fn should_persist_memory_envelope(vt_cfg: Option<&VTCodeConfig>) -> bool {
    vt_cfg.is_some_and(|cfg| cfg.context.dynamic.enabled && cfg.context.dynamic.persist_history)
}

fn memory_envelope_message(envelope: &SessionMemoryEnvelope) -> Message {
    let mut text = String::new();
    text.push_str(MEMORY_ENVELOPE_HEADER);
    text.push_str("\nSummary:\n");
    text.push_str(envelope.summary.trim());

    if let Some(objective) = envelope.objective.as_deref()
        && !objective.trim().is_empty()
    {
        text.push_str("\n\nObjective:\n");
        text.push_str(objective.trim());
    }

    if let Some(task_summary) = envelope.task_summary.as_deref()
        && !task_summary.trim().is_empty()
    {
        text.push_str("\n\nTask Tracker:\n");
        text.push_str(task_summary.trim());
    }

    if let Some(spec_summary) = envelope.spec_summary.as_deref()
        && !spec_summary.trim().is_empty()
    {
        text.push_str("\n\nSpec Summary:\n");
        text.push_str(spec_summary.trim());
    }

    if let Some(evaluation_summary) = envelope.evaluation_summary.as_deref()
        && !evaluation_summary.trim().is_empty()
    {
        text.push_str("\n\nEvaluation Summary:\n");
        text.push_str(evaluation_summary.trim());
    }

    if !envelope.constraints.is_empty() {
        text.push_str("\n\nConstraints:\n- ");
        text.push_str(&envelope.constraints.join("\n- "));
    }

    if !envelope.touched_files.is_empty() {
        text.push_str("\n\nTouched Files:\n- ");
        text.push_str(&envelope.touched_files.join("\n- "));
    }

    if !envelope.grounded_facts.is_empty() {
        text.push_str("\n\nGrounded Facts:\n");
        for fact in &envelope.grounded_facts {
            text.push_str("- [");
            text.push_str(&fact.source);
            text.push_str("] ");
            text.push_str(fact.fact.trim());
            text.push('\n');
        }
        while text.ends_with('\n') {
            text.pop();
        }
    }

    if !envelope.open_questions.is_empty() {
        text.push_str("\n\nOpen Questions:\n- ");
        text.push_str(&envelope.open_questions.join("\n- "));
    }

    if !envelope.verification_todo.is_empty() {
        text.push_str("\n\nVerification Todo:\n- ");
        text.push_str(&envelope.verification_todo.join("\n- "));
    }

    if !envelope.delegation_notes.is_empty() {
        text.push_str("\n\nDelegation Notes:\n- ");
        text.push_str(&envelope.delegation_notes.join("\n- "));
    }

    if let Some(history_path) = envelope.history_artifact_path.as_deref() {
        text.push_str("\n\nHistory Artifact:\n");
        text.push_str(history_path);
    }

    Message::system(text)
}

fn is_compaction_summary_message(message: &Message) -> bool {
    message.role == MessageRole::System
        && message
            .content
            .as_text()
            .starts_with("Previous conversation summary:\n")
}

fn strip_existing_memory_envelope(history: &mut Vec<Message>) {
    history.retain(|message| {
        !(message.role == MessageRole::System
            && message
                .content
                .as_text()
                .starts_with(MEMORY_ENVELOPE_HEADER))
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
        format!(
            "Compacted earlier conversation state. Recent preserved context: {}",
            recent.join(" | ")
        )
    }
}

fn sanitize_session_id(session_id: &str) -> String {
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

fn memory_envelope_file_matches_session(name: &str, session_id: &str) -> bool {
    let session_prefix = sanitize_session_id(session_id);
    name == format!("{session_prefix}{MEMORY_ENVELOPE_SUFFIX}")
        || (name.starts_with(&format!("{session_prefix}_"))
            && name.ends_with(MEMORY_ENVELOPE_SUFFIX))
}

fn read_task_tracker_snapshot(workspace_root: &Path) -> TaskTrackerSnapshot {
    let tracker_path = current_task_path(workspace_root);
    let Ok(content) = fs::read_to_string(&tracker_path) else {
        return TaskTrackerSnapshot::default();
    };

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
        verification_todo,
    }
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

fn default_memory_envelope_path_for_session(workspace_root: &Path, session_id: &str) -> PathBuf {
    workspace_root.join(".vtcode").join("history").join(format!(
        "{}{MEMORY_ENVELOPE_SUFFIX}",
        sanitize_session_id(session_id)
    ))
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
    candidates.sort();
    candidates
}

fn latest_memory_envelope_path_for_session(
    workspace_root: &Path,
    session_id: &str,
) -> Option<PathBuf> {
    memory_envelope_paths_for_session(workspace_root, session_id)
        .into_iter()
        .rev()
        .find(|path| {
            fs::read_to_string(path)
                .ok()
                .and_then(|content| serde_json::from_str::<SessionMemoryEnvelope>(&content).ok())
                .is_some_and(|envelope| {
                    envelope.session_id.is_empty() || envelope.session_id == session_id
                })
        })
}

fn load_latest_memory_envelope(
    workspace_root: &Path,
    session_id: &str,
) -> Option<SessionMemoryEnvelope> {
    let path = latest_memory_envelope_path_for_session(workspace_root, session_id)?;
    let content = fs::read_to_string(path).ok()?;
    let envelope: SessionMemoryEnvelope = serde_json::from_str(&content).ok()?;
    if !envelope.session_id.is_empty() && envelope.session_id != session_id {
        return None;
    }
    Some(envelope)
}

fn insert_memory_envelope_message(
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
                .rposition(|item| {
                    item.role == MessageRole::User || is_compaction_summary_message(item)
                })
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

pub(crate) fn inject_latest_memory_envelope(
    workspace_root: &Path,
    session_id: &str,
    history: &mut Vec<Message>,
) -> bool {
    let Some(envelope) = load_latest_memory_envelope(workspace_root, session_id) else {
        return false;
    };

    strip_existing_memory_envelope(history);
    insert_memory_envelope_message(history, &envelope, MemoryEnvelopePlacement::Start);
    true
}

fn merge_touched_files(
    prior_envelope: Option<&SessionMemoryEnvelope>,
    touched_files: &[String],
) -> Vec<String> {
    let mut merged = prior_envelope
        .map(|envelope| envelope.touched_files.clone())
        .unwrap_or_default();

    for path in touched_files {
        if let Some(existing_idx) = merged.iter().position(|item| item == path) {
            merged.remove(existing_idx);
        }
        merged.push(path.clone());
    }

    merged
}

fn merge_recent_strings(prior: &[String], updates: &[String], limit: usize) -> Vec<String> {
    let mut merged = prior
        .iter()
        .map(|value| normalize_whitespace(value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    for value in updates
        .iter()
        .map(|value| normalize_whitespace(value))
        .filter(|value| !value.is_empty())
    {
        let key = value.to_ascii_lowercase();
        if let Some(existing_idx) = merged
            .iter()
            .position(|item| item.to_ascii_lowercase() == key)
        {
            merged.remove(existing_idx);
        }
        merged.push(value);
    }

    let keep_from = merged.len().saturating_sub(limit);
    merged.into_iter().skip(keep_from).collect()
}

fn extract_constraints_from_summary(text: Option<&str>) -> Vec<String> {
    text.into_iter()
        .flat_map(|value| value.lines())
        .map(normalize_whitespace)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            if let Some(rest) = line.strip_prefix("- ") {
                return Some(rest.trim().to_string());
            }
            line.strip_prefix("* ").map(|rest| rest.trim().to_string())
        })
        .take(MEMORY_LIST_LIMIT)
        .collect()
}

fn derive_continuity_summary(
    history: &[Message],
    prior_envelope: Option<&SessionMemoryEnvelope>,
) -> String {
    let mut recent = history
        .iter()
        .rev()
        .filter(|message| {
            !(message.role == MessageRole::System
                && message
                    .content
                    .as_text()
                    .starts_with(MEMORY_ENVELOPE_HEADER))
        })
        .filter_map(|message| {
            let trimmed = normalize_whitespace(message.content.as_text().as_ref());
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
        prior_envelope
            .map(|envelope| envelope.summary.clone())
            .unwrap_or_else(|| "Session continuity facts preserved.".to_string())
    } else {
        format!("Recent session context: {}", recent.join(" | "))
    }
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

        for tool_call in tool_calls {
            let Some(tool_name) = tool_call.tool_name() else {
                continue;
            };

            let kind = if is_read_file_tool_name(tool_name) {
                Some(FileReadToolKind::ReadFile)
            } else if tool_name == tool_names::UNIFIED_FILE {
                tool_call.execution_arguments().ok().and_then(|args| {
                    args.get("action")
                        .and_then(Value::as_str)
                        .filter(|action| *action == "read")
                        .map(|_| FileReadToolKind::UnifiedFileRead)
                })
            } else {
                None
            };

            if let Some(kind) = kind {
                kinds.insert(tool_call.id.clone(), kind);
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
    let object = payload.as_object()?;
    if object.get("items").is_some()
        || object.get("error").is_some()
        || object
            .get("spool_chunked")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || object
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return None;
    }

    let target = object
        .get("file_path")
        .and_then(Value::as_str)
        .or_else(|| object.get("path").and_then(Value::as_str))
        .and_then(normalize_file_read_target)?;

    Some(FileReadDedupKey {
        target,
        start_line: object.get("start_line").and_then(Value::as_u64),
        end_line: object.get("end_line").and_then(Value::as_u64),
        spool_path: object
            .get("spool_path")
            .and_then(Value::as_str)
            .and_then(normalize_file_read_target),
    })
}

fn build_file_read_placeholder_content(payload: &Value, key: &FileReadDedupKey) -> String {
    let mut placeholder = serde_json::Map::new();
    placeholder.insert("deduped_read".to_string(), Value::Bool(true));
    placeholder.insert(
        "note".to_string(),
        Value::String(DEDUPED_FILE_READ_NOTE.to_string()),
    );
    if let Some(file_path) = payload.get("file_path").and_then(Value::as_str) {
        let trimmed = file_path.trim();
        if !trimmed.is_empty() {
            placeholder.insert("file_path".to_string(), Value::String(trimmed.to_string()));
        }
    }
    if let Some(path) = payload.get("path").and_then(Value::as_str) {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            placeholder.insert("path".to_string(), Value::String(trimmed.to_string()));
        }
    }
    if let Some(start_line) = key.start_line {
        placeholder.insert("start_line".to_string(), json!(start_line));
    }
    if let Some(end_line) = key.end_line {
        placeholder.insert("end_line".to_string(), json!(end_line));
    }
    if let Some(spool_path) = key.spool_path.as_deref() {
        placeholder.insert("spool_path".to_string(), json!(spool_path));
    }

    Value::Object(placeholder).to_string()
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
            message.origin_tool.as_deref().and_then(|tool_name| {
                is_read_file_tool_name(tool_name).then_some(FileReadToolKind::ReadFile)
            })
        })?;

    if !matches!(
        kind,
        FileReadToolKind::ReadFile | FileReadToolKind::UnifiedFileRead
    ) {
        return None;
    }

    let payload: Value = serde_json::from_str(message.content.as_text().as_ref()).ok()?;
    let key = build_file_read_dedup_key(&payload)?;

    Some(FileReadDedupCandidate {
        placeholder_content: build_file_read_placeholder_content(&payload, &key),
        key,
    })
}

fn dedup_repeated_file_reads_for_local_compaction(history: &[Message]) -> Vec<Message> {
    let tool_kinds = collect_file_read_tool_kinds(history);
    let mut last_index_by_key = HashMap::new();
    let mut candidates = Vec::new();

    for (index, message) in history.iter().enumerate() {
        let Some(candidate) = file_read_dedup_candidate(message, &tool_kinds) else {
            continue;
        };
        last_index_by_key.insert(candidate.key.clone(), index);
        candidates.push((index, candidate));
    }

    let mut deduped = history.to_vec();
    let mut changed = false;
    for (index, candidate) in candidates {
        let Some(last_index) = last_index_by_key.get(&candidate.key).copied() else {
            continue;
        };
        if last_index == index {
            continue;
        }

        if let Some(message) = deduped.get_mut(index) {
            message.content = candidate.placeholder_content.into();
            changed = true;
        }
    }

    if changed { deduped } else { history.to_vec() }
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

fn build_session_memory_envelope(
    session_id: &str,
    workspace_root: &Path,
    original_history: &[Message],
    touched_files: &[String],
    summary: String,
    history_artifact_path: Option<&PathBuf>,
    prior_envelope: Option<&SessionMemoryEnvelope>,
    task_snapshot: &TaskTrackerSnapshot,
    envelope_update: Option<&SessionMemoryEnvelopeUpdate>,
) -> SessionMemoryEnvelope {
    let spec_summary = read_spec_summary(workspace_root)
        .or_else(|| prior_envelope.and_then(|envelope| envelope.spec_summary.clone()));
    let evaluation_summary = read_evaluation_summary(workspace_root)
        .or_else(|| prior_envelope.and_then(|envelope| envelope.evaluation_summary.clone()));
    let derived_constraints = merge_recent_strings(
        prior_envelope
            .map(|envelope| envelope.constraints.as_slice())
            .unwrap_or(&[]),
        &extract_constraints_from_summary(spec_summary.as_deref()),
        MEMORY_LIST_LIMIT,
    );
    let derived_constraints = merge_recent_strings(
        &derived_constraints,
        &extract_constraints_from_summary(evaluation_summary.as_deref()),
        MEMORY_LIST_LIMIT,
    );
    let update = envelope_update.cloned().unwrap_or_default();

    SessionMemoryEnvelope {
        session_id: session_id.to_string(),
        schema_version: Some(SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
        summary,
        objective: update.objective.or_else(|| {
            task_snapshot
                .objective
                .clone()
                .or_else(|| prior_envelope.and_then(|envelope| envelope.objective.clone()))
        }),
        task_summary: task_snapshot
            .summary
            .clone()
            .or_else(|| prior_envelope.and_then(|envelope| envelope.task_summary.clone())),
        spec_summary,
        evaluation_summary,
        constraints: merge_recent_strings(
            &derived_constraints,
            &update.constraints,
            MEMORY_LIST_LIMIT,
        ),
        grounded_facts: merge_grounded_facts(
            prior_envelope,
            original_history,
            &update.grounded_facts,
        ),
        touched_files: merge_touched_files(
            prior_envelope,
            &touched_files
                .iter()
                .cloned()
                .chain(update.touched_files)
                .collect::<Vec<_>>(),
        ),
        open_questions: merge_recent_strings(
            prior_envelope
                .map(|envelope| envelope.open_questions.as_slice())
                .unwrap_or(&[]),
            &update.open_questions,
            MEMORY_LIST_LIMIT,
        ),
        verification_todo: merge_recent_strings(
            prior_envelope
                .map(|envelope| envelope.verification_todo.as_slice())
                .unwrap_or(&[]),
            &task_snapshot
                .verification_todo
                .iter()
                .cloned()
                .chain(update.verification_todo)
                .collect::<Vec<_>>(),
            MEMORY_LIST_LIMIT,
        ),
        delegation_notes: merge_recent_strings(
            prior_envelope
                .map(|envelope| envelope.delegation_notes.as_slice())
                .unwrap_or(&[]),
            &update.delegation_notes,
            MEMORY_LIST_LIMIT,
        ),
        history_artifact_path: history_artifact_path
            .map(|path| path.display().to_string())
            .or_else(|| prior_envelope.and_then(|envelope| envelope.history_artifact_path.clone())),
        generated_at: Utc::now().to_rfc3339(),
    }
}

fn write_memory_envelope_to_path(path: &Path, envelope: &SessionMemoryEnvelope) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create memory envelope directory {}", parent.display()))?;
    }
    let serialized = serde_json::to_string_pretty(envelope)?;
    fs::write(path, serialized)
        .with_context(|| format!("write memory envelope {}", path.display()))?;
    Ok(())
}

fn persist_memory_envelope(
    workspace_root: &Path,
    session_id: &str,
    vt_cfg: Option<&VTCodeConfig>,
    original_history: &[Message],
    touched_files: &[String],
    compacted: &mut Vec<Message>,
    persistence: MemoryEnvelopePersistence,
    placement: MemoryEnvelopePlacement,
    seed_envelope: Option<&SessionMemoryEnvelope>,
) -> Result<Option<SessionMemoryEnvelope>> {
    let should_persist = should_persist_memory_envelope(vt_cfg);
    if original_history.is_empty()
        || (!should_persist && persistence == MemoryEnvelopePersistence::PersistToDisk)
    {
        return Ok(None);
    }

    let task_snapshot = read_task_tracker_snapshot(workspace_root);
    let history_artifact_path =
        if should_persist && persistence == MemoryEnvelopePersistence::PersistToDisk {
            let mut history_manager = HistoryFileManager::new(workspace_root, session_id);
            let history_messages = messages_to_history_messages(original_history, 0);
            let history_result = history_manager
                .write_history_sync(
                    &history_messages,
                    original_history.len(),
                    "compaction",
                    touched_files,
                    &[],
                )
                .context("write compaction history artifact")?;
            Some(history_result.file_path)
        } else {
            None
        };
    let loaded_prior_envelope = if seed_envelope.is_none() {
        load_latest_memory_envelope(workspace_root, session_id)
    } else {
        None
    };
    let prior_envelope = seed_envelope.or(loaded_prior_envelope.as_ref());
    let envelope = build_session_memory_envelope(
        session_id,
        workspace_root,
        original_history,
        touched_files,
        extract_compaction_summary(compacted, original_history),
        history_artifact_path.as_ref(),
        prior_envelope,
        &task_snapshot,
        None,
    );

    if let Some(history_artifact_path) = history_artifact_path.as_ref() {
        let envelope_path =
            memory_envelope_path_from_history_path(workspace_root, history_artifact_path);
        write_memory_envelope_to_path(&envelope_path, &envelope)?;
    }

    apply_memory_envelope(compacted, &envelope, placement);

    Ok(Some(envelope))
}

fn configured_retained_user_messages(vt_cfg: Option<&VTCodeConfig>) -> usize {
    vt_cfg
        .map(|cfg| cfg.context.dynamic.retained_user_messages)
        .unwrap_or(4)
}

fn local_compaction_config(
    vt_cfg: Option<&VTCodeConfig>,
    always_summarize: bool,
) -> CompactionConfig {
    CompactionConfig {
        always_summarize,
        retained_user_messages: configured_retained_user_messages(vt_cfg),
        ..CompactionConfig::default()
    }
}

pub(crate) fn refresh_session_memory_envelope(
    workspace_root: &Path,
    session_id: &str,
    vt_cfg: Option<&VTCodeConfig>,
    history: &mut Vec<Message>,
    session_stats: &SessionStats,
    envelope_update: Option<&SessionMemoryEnvelopeUpdate>,
) -> Result<Option<SessionMemoryEnvelope>> {
    if history.is_empty() || !should_persist_memory_envelope(vt_cfg) {
        return Ok(None);
    }

    let prior_envelope = load_latest_memory_envelope(workspace_root, session_id);
    let task_snapshot = read_task_tracker_snapshot(workspace_root);
    let touched_files = session_stats.recent_touched_files();
    let envelope = build_session_memory_envelope(
        session_id,
        workspace_root,
        history,
        &touched_files,
        derive_continuity_summary(history, prior_envelope.as_ref()),
        None,
        prior_envelope.as_ref(),
        &task_snapshot,
        envelope_update,
    );
    let envelope_path = latest_memory_envelope_path_for_session(workspace_root, session_id)
        .unwrap_or_else(|| default_memory_envelope_path_for_session(workspace_root, session_id));
    write_memory_envelope_to_path(&envelope_path, &envelope)?;
    apply_memory_envelope(history, &envelope, MemoryEnvelopePlacement::Start);
    Ok(Some(envelope))
}

pub(crate) async fn build_summarized_fork_history(
    provider: &dyn LLMProvider,
    model: &str,
    source_session_id: &str,
    target_session_id: &str,
    workspace_root: &Path,
    vt_cfg: Option<&VTCodeConfig>,
    source_history: &[Message],
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

    let compaction_input = dedup_repeated_file_reads_for_local_compaction(&source_history);
    let mut compacted = vtcode_core::compaction::compact_history(
        provider,
        model,
        &compaction_input,
        &local_compaction_config(vt_cfg, true),
    )
    .await?;

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
        .cap_token_usage_after_compaction(configured_compaction_threshold(vt_cfg, provider, model));
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
    let current_token_usage = state.context_manager.current_token_usage();
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

    let Some(compact_threshold) = configured_compaction_threshold(Some(vt_cfg), provider, model)
    else {
        return Ok(None);
    };

    if current_token_usage < compact_threshold {
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
mod tests {
    use super::{
        CompactionContext, CompactionState, GroundedFactRecord,
        build_server_compaction_context_management, build_summarized_fork_history,
        compact_history_for_recovery_in_place, compact_history_from_index_in_place,
        compact_history_in_place, compact_history_in_place_with_events,
        inject_latest_memory_envelope, latest_memory_envelope_path_for_session,
        manual_openai_compact_history_in_place, maybe_auto_compact_history,
        resolve_compaction_threshold,
    };
    use crate::agent::runloop::unified::context_manager::ContextManager;
    use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
    use crate::agent::runloop::unified::state::SessionStats;
    use async_trait::async_trait;
    use hashbrown::HashMap;
    use serde_json::json;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::RwLock;
    use vtcode_commons::llm::Usage;
    use vtcode_core::config::constants::tools as tool_names;
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::llm::provider::{
        LLMError, LLMProvider, LLMRequest, LLMResponse, Message, MessageRole,
        ResponsesCompactionOptions, ToolCall,
    };

    struct LocalCompactionProvider;

    struct ProviderCompactionProvider;

    struct NoOpProviderCompactionProvider;

    struct FailingProviderCompactionProvider;

    struct RecordingProviderCompactionProvider {
        seen_history: Arc<RwLock<Vec<Message>>>,
    }

    #[async_trait]
    impl LLMProvider for LocalCompactionProvider {
        fn name(&self) -> &str {
            "stub"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(LLMResponse::new("stub-model", "summary"))
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        fn effective_context_size(&self, _model: &str) -> usize {
            1_000
        }
    }

    #[async_trait]
    impl LLMProvider for ProviderCompactionProvider {
        fn name(&self) -> &str {
            "provider-stub"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(LLMResponse::new("stub-model", "summary"))
        }

        async fn compact_history(
            &self,
            _model: &str,
            history: &[Message],
        ) -> Result<Vec<Message>, LLMError> {
            let mut compacted = Vec::new();
            compacted.push(Message::system(
                "Previous conversation summary:\nProvider compacted history".to_string(),
            ));
            compacted.extend(history.iter().rev().take(2).cloned().collect::<Vec<_>>());
            compacted.reverse();
            Ok(compacted)
        }

        async fn compact_history_with_options(
            &self,
            model: &str,
            history: &[Message],
            _options: &ResponsesCompactionOptions,
        ) -> Result<Vec<Message>, LLMError> {
            self.compact_history(model, history).await
        }

        fn supports_responses_compaction(&self, _model: &str) -> bool {
            true
        }

        fn supports_manual_openai_compaction(&self, _model: &str) -> bool {
            true
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        fn effective_context_size(&self, _model: &str) -> usize {
            1_000
        }
    }

    #[async_trait]
    impl LLMProvider for NoOpProviderCompactionProvider {
        fn name(&self) -> &str {
            "noop-provider-stub"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(LLMResponse::new("stub-model", "summary"))
        }

        async fn compact_history(
            &self,
            _model: &str,
            history: &[Message],
        ) -> Result<Vec<Message>, LLMError> {
            Ok(history.to_vec())
        }

        async fn compact_history_with_options(
            &self,
            _model: &str,
            history: &[Message],
            _options: &ResponsesCompactionOptions,
        ) -> Result<Vec<Message>, LLMError> {
            Ok(history.to_vec())
        }

        fn supports_responses_compaction(&self, _model: &str) -> bool {
            true
        }

        fn supports_manual_openai_compaction(&self, _model: &str) -> bool {
            true
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        fn effective_context_size(&self, _model: &str) -> usize {
            1_000
        }
    }

    #[async_trait]
    impl LLMProvider for FailingProviderCompactionProvider {
        fn name(&self) -> &str {
            "failing-provider-stub"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(LLMResponse::new("stub-model", "summary"))
        }

        async fn compact_history(
            &self,
            _model: &str,
            _history: &[Message],
        ) -> Result<Vec<Message>, LLMError> {
            Err(LLMError::Provider {
                message: "provider compaction failed".to_string(),
                metadata: None,
            })
        }

        fn supports_responses_compaction(&self, _model: &str) -> bool {
            true
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        fn effective_context_size(&self, _model: &str) -> usize {
            1_000
        }
    }

    #[async_trait]
    impl LLMProvider for RecordingProviderCompactionProvider {
        fn name(&self) -> &str {
            "recording-provider-stub"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(LLMResponse::new("stub-model", "summary"))
        }

        async fn compact_history(
            &self,
            _model: &str,
            history: &[Message],
        ) -> Result<Vec<Message>, LLMError> {
            *self.seen_history.write().await = history.to_vec();
            Ok(history.to_vec())
        }

        async fn compact_history_with_options(
            &self,
            _model: &str,
            history: &[Message],
            _options: &ResponsesCompactionOptions,
        ) -> Result<Vec<Message>, LLMError> {
            self.compact_history("stub-model", history).await
        }

        fn supports_responses_compaction(&self, _model: &str) -> bool {
            true
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        fn effective_context_size(&self, _model: &str) -> usize {
            1_000
        }
    }

    fn test_history() -> Vec<Message> {
        vec![
            Message::user("message-0".to_string()),
            Message::assistant("assistant-0".to_string()),
            Message::tool_response("call-0".to_string(), "tool-0".to_string()),
            Message::user("message-1".to_string()),
            Message::assistant("assistant-1".to_string()),
            Message::tool_response("call-1".to_string(), "tool-1".to_string()),
            Message::user("message-2".to_string()),
            Message::assistant("assistant-2".to_string()),
            Message::tool_response("call-2".to_string(), "tool-2".to_string()),
            Message::user("message-3".to_string()),
            Message::assistant("assistant-3".to_string()),
            Message::tool_response("call-3".to_string(), "tool-3".to_string()),
        ]
    }

    fn test_history_with_memory_envelope() -> Vec<Message> {
        let mut history = vec![Message::system(
            "[Session Memory Envelope]\nSummary:\nExisting summary".to_string(),
        )];
        history.extend(test_history());
        history
    }

    fn assert_local_compaction_history(history: &[Message], envelope_index: usize) {
        assert_local_compaction_history_with_user_count(history, envelope_index, 4);
    }

    fn assert_local_compaction_history_with_user_count(
        history: &[Message],
        envelope_index: usize,
        retained_user_messages: usize,
    ) {
        assert_eq!(history.len(), retained_user_messages + 2);
        assert!(
            history[envelope_index]
                .content
                .as_text()
                .contains("[Session Memory Envelope]")
        );
        assert_eq!(
            history.len(),
            history
                .iter()
                .filter(|message| {
                    message.role == MessageRole::System || message.role == MessageRole::User
                })
                .count()
        );
        assert!(history.iter().any(|message| {
            message.role == MessageRole::System
                && message
                    .content
                    .as_text()
                    .contains("Previous conversation summary")
        }));
        assert_eq!(
            history
                .iter()
                .filter(|message| message.role == MessageRole::User)
                .count(),
            retained_user_messages
        );
    }

    fn read_file_tool_call(id: &str, path: &str) -> ToolCall {
        ToolCall::function(
            id.to_string(),
            tool_names::READ_FILE.to_string(),
            json!({ "path": path }).to_string(),
        )
    }

    fn unified_file_read_tool_call(id: &str, path: &str) -> ToolCall {
        ToolCall::function(
            id.to_string(),
            tool_names::UNIFIED_FILE.to_string(),
            json!({ "action": "read", "path": path }).to_string(),
        )
    }

    fn assistant_with_tool_call(tool_call: ToolCall) -> Message {
        let mut message = Message::assistant(String::new());
        message.tool_calls = Some(vec![tool_call]);
        message
    }

    fn test_context_manager() -> ContextManager {
        ContextManager::new(
            "You are VT Code.".to_string(),
            (),
            std::sync::Arc::new(RwLock::new(HashMap::new())),
            None,
        )
    }

    #[tokio::test]
    async fn manual_compaction_succeeds_without_server_side_support() {
        let temp = tempdir().expect("tempdir");
        let provider = LocalCompactionProvider;
        let mut history = test_history();
        let mut session_stats = SessionStats::default();
        session_stats.set_previous_response_chain("stub", "stub-model", Some("resp_123"), &[]);
        let mut context_manager = test_context_manager();
        context_manager.update_token_usage(&Some(Usage {
            prompt_tokens: 900,
            completion_tokens: 10,
            total_tokens: 910,
            ..Usage::default()
        }));

        let outcome = compact_history_in_place(
            &provider,
            "stub-model",
            "session-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            &mut history,
            &mut session_stats,
            &mut context_manager,
        )
        .await
        .expect("manual compaction succeeds")
        .expect("history should compact");

        assert_eq!(outcome.original_len, 12);
        assert_eq!(outcome.compacted_len, 5);
        assert_local_compaction_history(&history, 0);
        assert_eq!(
            session_stats.previous_response_id_for("stub", "stub-model"),
            None
        );
        assert!(context_manager.current_token_usage() < 900);
        assert!(latest_memory_envelope_path_for_session(temp.path(), "session-alpha").is_some());
    }

    #[tokio::test]
    async fn manual_compaction_emits_local_compaction_boundary_event() {
        let temp = tempdir().expect("tempdir");
        let provider = LocalCompactionProvider;
        let harness_path = temp.path().join("harness.jsonl");
        let harness_emitter = HarnessEventEmitter::new(harness_path.clone()).expect("emitter");
        let mut history = test_history();
        let mut session_stats = SessionStats::default();
        let mut context_manager = test_context_manager();

        let outcome = compact_history_in_place_with_events(
            CompactionContext::new(
                &provider,
                "stub-model",
                "session-alpha",
                "thread-alpha",
                temp.path(),
                Some(&VTCodeConfig::default()),
                None,
                Some(&harness_emitter),
            ),
            CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
            vtcode_core::exec::events::CompactionTrigger::Manual,
        )
        .await
        .expect("compaction succeeds")
        .expect("history should compact");

        assert_eq!(
            outcome.mode,
            vtcode_core::exec::events::CompactionMode::Local
        );
        let content = fs::read_to_string(harness_path).expect("read harness log");
        assert!(content.contains("\"type\":\"thread.compact_boundary\""));
        assert!(content.contains("\"mode\":\"local\""));
    }

    #[tokio::test]
    async fn provider_compaction_emits_provider_boundary_event() {
        let temp = tempdir().expect("tempdir");
        let provider = ProviderCompactionProvider;
        let harness_path = temp.path().join("provider-harness.jsonl");
        let harness_emitter = HarnessEventEmitter::new(harness_path.clone()).expect("emitter");
        let mut history = test_history();
        let mut session_stats = SessionStats::default();
        let mut context_manager = test_context_manager();

        let outcome = compact_history_in_place_with_events(
            CompactionContext::new(
                &provider,
                "stub-model",
                "session-alpha",
                "thread-alpha",
                temp.path(),
                Some(&VTCodeConfig::default()),
                None,
                Some(&harness_emitter),
            ),
            CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
            vtcode_core::exec::events::CompactionTrigger::Manual,
        )
        .await
        .expect("compaction succeeds")
        .expect("history should compact");

        assert_eq!(
            outcome.mode,
            vtcode_core::exec::events::CompactionMode::Provider
        );
        let content = fs::read_to_string(harness_path).expect("read harness log");
        assert!(content.contains("\"type\":\"thread.compact_boundary\""));
        assert!(content.contains("\"mode\":\"provider\""));
    }

    #[tokio::test]
    async fn manual_openai_compaction_clears_previous_response_chain() {
        let temp = tempdir().expect("tempdir");
        let provider = ProviderCompactionProvider;
        let mut history = test_history();
        let mut session_stats = SessionStats::default();
        session_stats.set_previous_response_chain(
            "provider-stub",
            "stub-model",
            Some("resp_123"),
            &[],
        );
        let mut context_manager = test_context_manager();

        let outcome = manual_openai_compact_history_in_place(
            CompactionContext::new(
                &provider,
                "stub-model",
                "session-alpha",
                "thread-alpha",
                temp.path(),
                Some(&VTCodeConfig::default()),
                None,
                None,
            ),
            CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
            &ResponsesCompactionOptions::default(),
        )
        .await
        .expect("manual OpenAI compaction succeeds")
        .expect("history should compact");

        assert_eq!(
            outcome.mode,
            vtcode_core::exec::events::CompactionMode::Provider
        );
        assert_eq!(
            session_stats.previous_response_id_for("provider-stub", "stub-model"),
            None
        );
    }

    #[tokio::test]
    async fn manual_openai_compaction_rejects_unsupported_provider_without_local_fallback() {
        let temp = tempdir().expect("tempdir");
        let provider = LocalCompactionProvider;
        let mut history = test_history();
        let original_history = history.clone();
        let mut session_stats = SessionStats::default();
        let mut context_manager = test_context_manager();

        let err = manual_openai_compact_history_in_place(
            CompactionContext::new(
                &provider,
                "stub-model",
                "session-alpha",
                "thread-alpha",
                temp.path(),
                Some(&VTCodeConfig::default()),
                None,
                None,
            ),
            CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
            &ResponsesCompactionOptions::default(),
        )
        .await
        .expect_err("unsupported provider should fail");

        assert!(err.to_string().contains(
            "Manual `/compact` is available only for the native OpenAI provider on api.openai.com"
        ));
        assert_eq!(history, original_history);
    }

    #[tokio::test]
    async fn manual_openai_compaction_noop_preserves_existing_history() {
        let temp = tempdir().expect("tempdir");
        let provider = NoOpProviderCompactionProvider;
        let mut history = test_history_with_memory_envelope();
        let original_history = history.clone();
        let mut session_stats = SessionStats::default();
        let mut context_manager = test_context_manager();

        let outcome = manual_openai_compact_history_in_place(
            CompactionContext::new(
                &provider,
                "stub-model",
                "session-alpha",
                "thread-alpha",
                temp.path(),
                Some(&VTCodeConfig::default()),
                None,
                None,
            ),
            CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
            &ResponsesCompactionOptions::default(),
        )
        .await
        .expect("noop compaction succeeds");

        assert!(outcome.is_none());
        assert_eq!(history, original_history);
    }

    #[tokio::test]
    async fn provider_compaction_noop_preserves_existing_history() {
        let temp = tempdir().expect("tempdir");
        let provider = NoOpProviderCompactionProvider;
        let mut history = test_history_with_memory_envelope();
        let original_history = history.clone();
        let mut session_stats = SessionStats::default();
        let mut context_manager = test_context_manager();

        let outcome = compact_history_in_place_with_events(
            CompactionContext::new(
                &provider,
                "stub-model",
                "session-alpha",
                "thread-alpha",
                temp.path(),
                Some(&VTCodeConfig::default()),
                None,
                None,
            ),
            CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
            vtcode_core::exec::events::CompactionTrigger::Manual,
        )
        .await
        .expect("noop compaction succeeds");

        assert!(outcome.is_none());
        assert_eq!(history, original_history);
    }

    #[tokio::test]
    async fn provider_compaction_preserves_original_repeated_file_reads() {
        let temp = tempdir().expect("tempdir");
        let seen_history = Arc::new(RwLock::new(Vec::new()));
        let provider = RecordingProviderCompactionProvider {
            seen_history: Arc::clone(&seen_history),
        };
        let mut history = vec![
            assistant_with_tool_call(read_file_tool_call("call-1", "src/lib.rs")),
            Message::tool_response_with_origin(
                "call-1".to_string(),
                json!({
                    "file_path": "src/lib.rs",
                    "start_line": 1,
                    "end_line": 40,
                    "result": "older contents"
                })
                .to_string(),
                tool_names::READ_FILE.to_string(),
            ),
            assistant_with_tool_call(read_file_tool_call("call-2", "src/lib.rs")),
            Message::tool_response_with_origin(
                "call-2".to_string(),
                json!({
                    "file_path": "src/lib.rs",
                    "start_line": 1,
                    "end_line": 40,
                    "result": "newer contents"
                })
                .to_string(),
                tool_names::READ_FILE.to_string(),
            ),
        ];
        history.extend(test_history());
        let original_history = history.clone();
        let mut session_stats = SessionStats::default();
        let mut context_manager = test_context_manager();

        let outcome = compact_history_in_place_with_events(
            CompactionContext::new(
                &provider,
                "stub-model",
                "session-alpha",
                "thread-alpha",
                temp.path(),
                Some(&VTCodeConfig::default()),
                None,
                None,
            ),
            CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
            vtcode_core::exec::events::CompactionTrigger::Manual,
        )
        .await
        .expect("provider compaction succeeds");

        assert!(outcome.is_none());
        assert_eq!(history, original_history);

        let seen = seen_history.read().await.clone();
        assert_eq!(seen.len(), original_history.len());
        assert!(seen[1].content.as_text().contains("older contents"));
        assert!(!seen[1].content.as_text().contains("deduped_read"));
    }

    #[test]
    fn dedup_repeated_file_reads_rewrites_only_older_exact_matches() {
        let history = vec![
            assistant_with_tool_call(read_file_tool_call("call-1", "src/lib.rs")),
            Message::tool_response_with_origin(
                "call-1".to_string(),
                json!({
                    "file_path": "src/lib.rs",
                    "start_line": 1,
                    "end_line": 40,
                    "result": "older contents"
                })
                .to_string(),
                tool_names::READ_FILE.to_string(),
            ),
            assistant_with_tool_call(unified_file_read_tool_call("call-2", "src/lib.rs")),
            Message::tool_response(
                "call-2".to_string(),
                json!({
                    "path": "src/lib.rs",
                    "start_line": 1,
                    "end_line": 40,
                    "result": "newer contents"
                })
                .to_string(),
            ),
        ];

        let deduped = super::dedup_repeated_file_reads_for_local_compaction(&history);

        let older_payload: serde_json::Value =
            serde_json::from_str(deduped[1].content.as_text().as_ref()).expect("json payload");
        assert_eq!(
            older_payload
                .get("deduped_read")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            older_payload
                .get("note")
                .and_then(serde_json::Value::as_str),
            Some(super::DEDUPED_FILE_READ_NOTE)
        );
        assert_eq!(
            older_payload
                .get("file_path")
                .and_then(serde_json::Value::as_str),
            Some("src/lib.rs")
        );
        assert!(deduped[3].content.as_text().contains("newer contents"));
        assert!(!deduped[3].content.as_text().contains("deduped_read"));
    }

    #[test]
    fn dedup_repeated_file_reads_keeps_different_slices_and_chunked_reads() {
        let different_slice_history = vec![
            assistant_with_tool_call(read_file_tool_call("call-1", "src/lib.rs")),
            Message::tool_response_with_origin(
                "call-1".to_string(),
                json!({
                    "file_path": "src/lib.rs",
                    "start_line": 1,
                    "end_line": 20,
                    "result": "slice one"
                })
                .to_string(),
                tool_names::READ_FILE.to_string(),
            ),
            assistant_with_tool_call(read_file_tool_call("call-2", "src/lib.rs")),
            Message::tool_response_with_origin(
                "call-2".to_string(),
                json!({
                    "file_path": "src/lib.rs",
                    "start_line": 21,
                    "end_line": 40,
                    "result": "slice two"
                })
                .to_string(),
                tool_names::READ_FILE.to_string(),
            ),
        ];
        let chunked_history = vec![
            assistant_with_tool_call(read_file_tool_call("call-3", "src/lib.rs")),
            Message::tool_response_with_origin(
                "call-3".to_string(),
                json!({
                    "file_path": "src/lib.rs",
                    "start_line": 1,
                    "end_line": 40,
                    "result": "first chunk",
                    "spool_chunked": true,
                    "has_more": true
                })
                .to_string(),
                tool_names::READ_FILE.to_string(),
            ),
            assistant_with_tool_call(read_file_tool_call("call-4", "src/lib.rs")),
            Message::tool_response_with_origin(
                "call-4".to_string(),
                json!({
                    "file_path": "src/lib.rs",
                    "start_line": 1,
                    "end_line": 40,
                    "result": "second chunk",
                    "spool_chunked": true,
                    "has_more": false
                })
                .to_string(),
                tool_names::READ_FILE.to_string(),
            ),
        ];

        assert_eq!(
            super::dedup_repeated_file_reads_for_local_compaction(&different_slice_history),
            different_slice_history
        );
        assert_eq!(
            super::dedup_repeated_file_reads_for_local_compaction(&chunked_history),
            chunked_history
        );
    }

    #[test]
    fn legacy_memory_envelope_deserializes_with_new_fields_defaulted() {
        let envelope: super::SessionMemoryEnvelope = serde_json::from_value(json!({
            "session_id": "session-alpha",
            "summary": "Persisted summary",
            "task_summary": "Task tracker",
            "spec_summary": null,
            "evaluation_summary": null,
            "grounded_facts": [{
                "fact": "fact",
                "source": "tool:read_file"
            }],
            "touched_files": ["src/lib.rs"],
            "history_artifact_path": ".vtcode/history/session-alpha.jsonl",
            "generated_at": "2026-03-14T00:00:00Z"
        }))
        .expect("legacy envelope should deserialize");

        assert_eq!(envelope.schema_version, None);
        assert_eq!(envelope.objective, None);
        assert!(envelope.constraints.is_empty());
        assert!(envelope.open_questions.is_empty());
        assert!(envelope.verification_todo.is_empty());
        assert!(envelope.delegation_notes.is_empty());
    }

    #[test]
    fn refresh_session_memory_envelope_merges_existing_continuity_fields() {
        let temp = tempdir().expect("tempdir");
        let history_dir = temp.path().join(".vtcode").join("history");
        fs::create_dir_all(&history_dir).expect("history dir");
        fs::create_dir_all(temp.path().join(".vtcode").join("tasks")).expect("tasks dir");
        fs::write(
            temp.path()
                .join(".vtcode")
                .join("tasks")
                .join("current_task.md"),
            "# Ship compaction cleanup\n- [ ] Run cargo nextest\n- [x] Wire in config\n",
        )
        .expect("write task");
        fs::write(
            temp.path()
                .join(".vtcode")
                .join("tasks")
                .join("current_spec.md"),
            "# Spec\nKeep local compaction aligned with summarized forks.\n",
        )
        .expect("write spec");
        fs::write(
            temp.path()
                .join(".vtcode")
                .join("tasks")
                .join("current_evaluation.md"),
            "# Eval\nNeed a regression test for repeated reads.\n",
        )
        .expect("write eval");

        let prior_envelope = super::SessionMemoryEnvelope {
            session_id: "session-alpha".to_string(),
            schema_version: Some(super::SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
            summary: "Prior summary".to_string(),
            objective: Some("Keep continuity".to_string()),
            task_summary: Some("Older task summary".to_string()),
            spec_summary: None,
            evaluation_summary: None,
            constraints: vec!["Do not redesign the harness".to_string()],
            grounded_facts: vec![GroundedFactRecord {
                fact: "Existing grounded fact".to_string(),
                source: "tool:read_file".to_string(),
            }],
            touched_files: vec!["src/old.rs".to_string()],
            open_questions: vec!["What should summarized forks retain?".to_string()],
            verification_todo: vec!["Confirm refresh runs at turn boundaries.".to_string()],
            delegation_notes: vec!["explorer: looked at compaction flow".to_string()],
            history_artifact_path: Some(".vtcode/history/session-alpha_0001.jsonl".to_string()),
            generated_at: "2026-03-14T00:00:00Z".to_string(),
        };
        fs::write(
            history_dir.join("session-alpha.memory.json"),
            serde_json::to_string_pretty(&prior_envelope).expect("serialize envelope"),
        )
        .expect("write envelope");

        let mut history = vec![
            Message::user("Continue the compaction work.".to_string()),
            Message::assistant("I will update the local compaction path.".to_string()),
        ];
        let mut session_stats = SessionStats::default();
        session_stats.record_touched_files(["src/new.rs".to_string()]);

        let update = super::SessionMemoryEnvelopeUpdate {
            grounded_facts: vec![GroundedFactRecord {
                fact: "Child agent confirmed the parser contract.".to_string(),
                source: "subagent:reviewer".to_string(),
            }],
            touched_files: vec!["src/child.rs".to_string()],
            open_questions: vec!["Should dedup cover batch reads?".to_string()],
            verification_todo: vec!["Run cargo check".to_string()],
            delegation_notes: vec!["reviewer: parser contract validated".to_string()],
            ..Default::default()
        };

        let envelope = super::refresh_session_memory_envelope(
            temp.path(),
            "session-alpha",
            Some(&VTCodeConfig::default()),
            &mut history,
            &session_stats,
            Some(&update),
        )
        .expect("refresh succeeds")
        .expect("envelope should be refreshed");

        assert_eq!(
            envelope.objective.as_deref(),
            Some("Ship compaction cleanup")
        );
        assert!(
            envelope
                .constraints
                .contains(&"Do not redesign the harness".to_string())
        );
        assert!(
            envelope
                .spec_summary
                .as_deref()
                .is_some_and(|summary| summary.contains("Keep local compaction aligned"))
        );
        assert!(
            envelope
                .evaluation_summary
                .as_deref()
                .is_some_and(|summary| summary.contains("Need a regression test"))
        );
        assert!(
            envelope
                .open_questions
                .contains(&"Should dedup cover batch reads?".to_string())
        );
        assert!(
            envelope
                .verification_todo
                .iter()
                .any(|item| item.contains("Run cargo nextest"))
        );
        assert!(
            envelope
                .verification_todo
                .contains(&"Run cargo check".to_string())
        );
        assert!(
            envelope
                .delegation_notes
                .contains(&"reviewer: parser contract validated".to_string())
        );
        assert!(envelope.touched_files.contains(&"src/new.rs".to_string()));
        assert!(envelope.touched_files.contains(&"src/child.rs".to_string()));
        assert!(
            history[0]
                .content
                .as_text()
                .contains("[Session Memory Envelope]")
        );
    }

    #[tokio::test]
    async fn provider_compaction_error_preserves_existing_history() {
        let temp = tempdir().expect("tempdir");
        let provider = FailingProviderCompactionProvider;
        let mut history = test_history_with_memory_envelope();
        let original_history = history.clone();
        let mut session_stats = SessionStats::default();
        let mut context_manager = test_context_manager();

        let err = compact_history_in_place_with_events(
            CompactionContext::new(
                &provider,
                "stub-model",
                "session-alpha",
                "thread-alpha",
                temp.path(),
                Some(&VTCodeConfig::default()),
                None,
                None,
            ),
            CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
            vtcode_core::exec::events::CompactionTrigger::Manual,
        )
        .await
        .expect_err("failing provider should fail");

        assert!(!err.to_string().is_empty());
        assert_eq!(history, original_history);
    }

    #[tokio::test]
    async fn auto_compaction_replaces_history_and_clears_response_chain() {
        let temp = tempdir().expect("tempdir");
        let provider = LocalCompactionProvider;
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.harness.auto_compaction_enabled = true;
        vt_cfg.agent.harness.auto_compaction_threshold_tokens = Some(700);

        let mut history = test_history();
        let mut session_stats = SessionStats::default();
        session_stats.set_previous_response_chain("stub", "stub-model", Some("resp_123"), &[]);
        let mut context_manager = test_context_manager();
        context_manager.update_token_usage(&Some(Usage {
            prompt_tokens: 900,
            completion_tokens: 10,
            total_tokens: 910,
            ..Usage::default()
        }));

        let outcome = maybe_auto_compact_history(
            CompactionContext::new(
                &provider,
                "stub-model",
                "session-alpha",
                "thread-alpha",
                temp.path(),
                Some(&vt_cfg),
                None,
                None,
            ),
            CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        )
        .await
        .expect("auto compaction succeeds")
        .expect("history should compact");

        assert_eq!(outcome.original_len, 12);
        assert_eq!(outcome.compacted_len, 5);
        assert_local_compaction_history(&history, 4);
        assert!(
            history[0]
                .content
                .as_text()
                .contains("Previous conversation summary")
        );
        assert_eq!(history[5].role, MessageRole::User);
        assert_eq!(
            session_stats.previous_response_id_for("stub", "stub-model"),
            None
        );
        assert!(context_manager.current_token_usage() < 700);
        assert!(latest_memory_envelope_path_for_session(temp.path(), "session-alpha").is_some());
    }

    #[tokio::test]
    async fn targeted_compaction_preserves_prefix_and_replaces_suffix() {
        let temp = tempdir().expect("tempdir");
        let provider = LocalCompactionProvider;
        let mut history = test_history();
        let preserved_prefix = history[..1].to_vec();
        let mut session_stats = SessionStats::default();
        let mut context_manager = test_context_manager();
        context_manager.update_token_usage(&Some(Usage {
            prompt_tokens: 900,
            completion_tokens: 10,
            total_tokens: 910,
            ..Usage::default()
        }));

        let outcome = compact_history_from_index_in_place(
            &provider,
            "stub-model",
            "session-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            &mut history,
            1,
            &mut session_stats,
            &mut context_manager,
        )
        .await
        .expect("targeted compaction succeeds")
        .expect("history should compact");

        assert_eq!(&history[..1], preserved_prefix.as_slice());
        assert_eq!(outcome.original_len, 12);
        assert_eq!(outcome.compacted_len, 5);
        assert_eq!(history.len(), 6);
        assert!(
            history[1]
                .content
                .as_text()
                .contains("[Session Memory Envelope]")
        );
        assert!(
            history[2]
                .content
                .as_text()
                .contains("Previous conversation summary")
        );
        assert!(latest_memory_envelope_path_for_session(temp.path(), "session-alpha").is_none());
    }

    #[tokio::test]
    async fn recovery_compaction_preserves_current_turn_suffix_and_emits_event() {
        let temp = tempdir().expect("tempdir");
        let provider = LocalCompactionProvider;
        let harness_path = temp.path().join("recovery-harness.jsonl");
        let harness_emitter = HarnessEventEmitter::new(harness_path.clone()).expect("emitter");
        let mut history = test_history();
        history.push(Message::system("Previous turn already completed tool execution. Reuse the latest tool outputs in history instead of rerunning the same exploration. If those tool outputs include `critical_note`, `next_action`, or `rerun_hint`, follow that guidance first.".to_string()));
        history.push(Message::system("Model follow-up failed after tool activity. Tools are disabled on the next pass; provide a direct textual response from the current context and reuse the latest tool outputs already in history.".to_string()));
        history.push(Message::user("current-turn".to_string()));
        history.push(Message::assistant("".to_string()));
        history.push(Message::tool_response(
            "call-current".to_string(),
            "{\"ok\":true}".to_string(),
        ));
        let preserved_suffix = history[12..].to_vec();
        let mut session_stats = SessionStats::default();
        let mut context_manager = test_context_manager();

        let outcome = compact_history_for_recovery_in_place(
            CompactionContext::new(
                &provider,
                "stub-model",
                "session-alpha",
                "thread-alpha",
                temp.path(),
                Some(&VTCodeConfig::default()),
                None,
                Some(&harness_emitter),
            ),
            CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
            12,
        )
        .await
        .expect("recovery compaction succeeds")
        .expect("history should compact");

        assert_eq!(
            history[history.len() - preserved_suffix.len()..],
            preserved_suffix
        );
        assert!(outcome.compacted_len < outcome.original_len);

        let content = fs::read_to_string(harness_path).expect("read harness log");
        assert!(content.contains("\"type\":\"thread.compact_boundary\""));
        assert!(content.contains("\"trigger\":\"recovery\""));
        assert!(content.contains("\"mode\":\"local\""));
    }

    #[tokio::test]
    async fn recovery_compaction_uses_provider_mode_when_supported() {
        let temp = tempdir().expect("tempdir");
        let provider = ProviderCompactionProvider;
        let harness_path = temp.path().join("provider-recovery-harness.jsonl");
        let harness_emitter = HarnessEventEmitter::new(harness_path.clone()).expect("emitter");
        let mut history = test_history();
        history.push(Message::user("current-turn".to_string()));
        history.push(Message::assistant("".to_string()));
        history.push(Message::tool_response(
            "call-current".to_string(),
            "{\"ok\":true}".to_string(),
        ));
        let preserved_suffix = history[12..].to_vec();
        let mut session_stats = SessionStats::default();
        let mut context_manager = test_context_manager();

        let outcome = compact_history_for_recovery_in_place(
            CompactionContext::new(
                &provider,
                "stub-model",
                "session-alpha",
                "thread-alpha",
                temp.path(),
                Some(&VTCodeConfig::default()),
                None,
                Some(&harness_emitter),
            ),
            CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
            12,
        )
        .await
        .expect("provider recovery compaction succeeds")
        .expect("history should compact");

        assert_eq!(
            outcome.mode,
            vtcode_core::exec::events::CompactionMode::Provider
        );
        assert_eq!(
            history[history.len() - preserved_suffix.len()..],
            preserved_suffix
        );

        let content = fs::read_to_string(harness_path).expect("read harness log");
        assert!(content.contains("\"trigger\":\"recovery\""));
        assert!(content.contains("\"mode\":\"provider\""));
    }

    #[test]
    fn inject_latest_memory_envelope_rehydrates_resume_history() {
        let temp = tempdir().expect("tempdir");
        let history_dir = temp.path().join(".vtcode").join("history");
        fs::create_dir_all(&history_dir).expect("history dir");
        let envelope_path = history_dir.join("resume-session_001.memory.json");
        let envelope = super::SessionMemoryEnvelope {
            session_id: "resume-session".to_string(),
            schema_version: Some(super::SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
            summary: "Persisted summary".to_string(),
            objective: None,
            task_summary: Some("Tracker: - [ ] Follow up".to_string()),
            spec_summary: None,
            evaluation_summary: None,
            constraints: Vec::new(),
            grounded_facts: vec![GroundedFactRecord {
                fact: "Cargo.toml declares vtcode-core".to_string(),
                source: "tool:read_file".to_string(),
            }],
            touched_files: vec!["Cargo.toml".to_string()],
            open_questions: Vec::new(),
            verification_todo: Vec::new(),
            delegation_notes: Vec::new(),
            history_artifact_path: Some(".vtcode/history/resume-session_001.jsonl".to_string()),
            generated_at: "2026-03-14T00:00:00Z".to_string(),
        };
        fs::write(
            &envelope_path,
            serde_json::to_string_pretty(&envelope).expect("serialize envelope"),
        )
        .expect("write envelope");

        let mut history = vec![Message::user("resume".to_string())];
        assert!(inject_latest_memory_envelope(
            temp.path(),
            "resume-session",
            &mut history
        ));
        assert!(history[0].content.as_text().contains("Persisted summary"));
        assert!(history[0].content.as_text().contains("Cargo.toml"));
    }

    #[test]
    fn inject_latest_memory_envelope_is_session_scoped() {
        let temp = tempdir().expect("tempdir");
        let history_dir = temp.path().join(".vtcode").join("history");
        fs::create_dir_all(&history_dir).expect("history dir");

        for (session_id, summary) in [
            ("session-alpha", "Alpha summary"),
            ("session-beta", "Beta summary"),
        ] {
            let envelope_path = history_dir.join(format!("{session_id}_0001.memory.json"));
            let envelope = super::SessionMemoryEnvelope {
                session_id: session_id.to_string(),
                schema_version: Some(super::SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
                summary: summary.to_string(),
                objective: None,
                task_summary: None,
                spec_summary: None,
                evaluation_summary: None,
                constraints: Vec::new(),
                grounded_facts: Vec::new(),
                touched_files: Vec::new(),
                open_questions: Vec::new(),
                verification_todo: Vec::new(),
                delegation_notes: Vec::new(),
                history_artifact_path: None,
                generated_at: "2026-03-14T00:00:00Z".to_string(),
            };
            fs::write(
                envelope_path,
                serde_json::to_string_pretty(&envelope).expect("serialize envelope"),
            )
            .expect("write envelope");
        }

        let mut history = vec![Message::user("resume".to_string())];
        assert!(inject_latest_memory_envelope(
            temp.path(),
            "session-beta",
            &mut history
        ));
        assert!(history[0].content.as_text().contains("Beta summary"));
        assert!(!history[0].content.as_text().contains("Alpha summary"));
    }

    #[test]
    fn inject_latest_memory_envelope_requires_exact_session_prefix_match() {
        let temp = tempdir().expect("tempdir");
        let history_dir = temp.path().join(".vtcode").join("history");
        fs::create_dir_all(&history_dir).expect("history dir");

        for (file_name, summary) in [
            ("session-a_0001.memory.json", "Exact summary"),
            ("session-alpha_0002.memory.json", "Wrong summary"),
        ] {
            let envelope = super::SessionMemoryEnvelope {
                session_id: "session-a".to_string(),
                schema_version: Some(super::SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
                summary: summary.to_string(),
                objective: None,
                task_summary: None,
                spec_summary: None,
                evaluation_summary: None,
                constraints: Vec::new(),
                grounded_facts: Vec::new(),
                touched_files: Vec::new(),
                open_questions: Vec::new(),
                verification_todo: Vec::new(),
                delegation_notes: Vec::new(),
                history_artifact_path: None,
                generated_at: "2026-03-14T00:00:00Z".to_string(),
            };
            fs::write(
                history_dir.join(file_name),
                serde_json::to_string_pretty(&envelope).expect("serialize envelope"),
            )
            .expect("write envelope");
        }

        let mut history = vec![Message::user("resume".to_string())];
        assert!(inject_latest_memory_envelope(
            temp.path(),
            "session-a",
            &mut history
        ));
        assert!(history[0].content.as_text().contains("Exact summary"));
        assert!(!history[0].content.as_text().contains("Wrong summary"));
    }

    #[tokio::test]
    async fn no_envelope_written_when_dynamic_history_is_disabled() {
        let temp = tempdir().expect("tempdir");
        let provider = LocalCompactionProvider;
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.context.dynamic.enabled = false;

        let mut history = test_history();
        let mut session_stats = SessionStats::default();
        let mut context_manager = test_context_manager();

        compact_history_in_place(
            &provider,
            "stub-model",
            "session-alpha",
            temp.path(),
            Some(&vt_cfg),
            &mut history,
            &mut session_stats,
            &mut context_manager,
        )
        .await
        .expect("compaction succeeds");

        assert!(latest_memory_envelope_path_for_session(temp.path(), "session-alpha").is_none());
        assert!(
            history[0]
                .content
                .as_text()
                .contains("Previous conversation summary")
        );
    }

    #[tokio::test]
    async fn persisted_envelope_uses_recorded_touched_files_only() {
        let temp = tempdir().expect("tempdir");
        let provider = LocalCompactionProvider;
        let mut history = test_history();
        history.push(Message::user(
            "Mentioning docs/example.md in prose should not populate touched files.".to_string(),
        ));
        let mut session_stats = SessionStats::default();
        session_stats.record_touched_files(["src/main.rs".to_string(), "Cargo.toml".to_string()]);
        let mut context_manager = test_context_manager();

        compact_history_in_place(
            &provider,
            "stub-model",
            "session-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            &mut history,
            &mut session_stats,
            &mut context_manager,
        )
        .await
        .expect("compaction succeeds");

        let envelope_path = latest_memory_envelope_path_for_session(temp.path(), "session-alpha")
            .expect("envelope path");
        let envelope: super::SessionMemoryEnvelope =
            serde_json::from_str(&fs::read_to_string(envelope_path).expect("read envelope"))
                .expect("parse envelope");

        assert_eq!(
            envelope.touched_files,
            vec!["src/main.rs".to_string(), "Cargo.toml".to_string()]
        );
        assert_eq!(envelope.session_id, "session-alpha");
    }

    #[test]
    fn inject_latest_memory_envelope_uses_exact_session_id_when_prefixes_collide() {
        let temp = tempdir().expect("tempdir");
        let history_dir = temp.path().join(".vtcode").join("history");
        fs::create_dir_all(&history_dir).expect("history dir");

        let session_alpha = "01234567890123456789012345678901-alpha";
        let session_beta = "01234567890123456789012345678901-beta";

        for (session_id, summary, suffix) in [
            (session_alpha, "Alpha summary", "0001"),
            (session_beta, "Beta summary", "0002"),
        ] {
            let envelope = super::SessionMemoryEnvelope {
                session_id: session_id.to_string(),
                schema_version: Some(super::SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
                summary: summary.to_string(),
                objective: None,
                task_summary: None,
                spec_summary: None,
                evaluation_summary: None,
                constraints: Vec::new(),
                grounded_facts: Vec::new(),
                touched_files: Vec::new(),
                open_questions: Vec::new(),
                verification_todo: Vec::new(),
                delegation_notes: Vec::new(),
                history_artifact_path: None,
                generated_at: "2026-03-14T00:00:00Z".to_string(),
            };
            let file_name = format!("{}_{suffix}.memory.json", &session_id[..32]);
            fs::write(
                history_dir.join(file_name),
                serde_json::to_string_pretty(&envelope).expect("serialize envelope"),
            )
            .expect("write envelope");
        }

        let mut history = vec![Message::user("resume".to_string())];
        assert!(inject_latest_memory_envelope(
            temp.path(),
            session_alpha,
            &mut history
        ));
        assert!(history[0].content.as_text().contains("Alpha summary"));
        assert!(!history[0].content.as_text().contains("Beta summary"));
    }

    #[tokio::test]
    async fn compaction_strips_existing_memory_envelope_before_recompacting() {
        let temp = tempdir().expect("tempdir");
        let provider = LocalCompactionProvider;
        let mut history = test_history();
        history.insert(
            0,
            Message::system("[Session Memory Envelope]\nSummary:\nPersisted summary".to_string()),
        );
        let mut session_stats = SessionStats::default();
        let mut context_manager = test_context_manager();

        let outcome = compact_history_in_place(
            &provider,
            "stub-model",
            "session-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            &mut history,
            &mut session_stats,
            &mut context_manager,
        )
        .await
        .expect("compaction succeeds")
        .expect("history should compact");

        assert_eq!(outcome.original_len, 12);
        assert_eq!(outcome.compacted_len, 5);
        assert_eq!(
            history
                .iter()
                .filter(|message| message
                    .content
                    .as_text()
                    .contains("[Session Memory Envelope]"))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn summarized_fork_history_reuses_compaction_pipeline_and_prior_envelope() {
        let temp = tempdir().expect("tempdir");
        let history_dir = temp.path().join(".vtcode").join("history");
        fs::create_dir_all(&history_dir).expect("history dir");
        let source_envelope = super::SessionMemoryEnvelope {
            session_id: "session-source".to_string(),
            schema_version: Some(super::SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
            summary: "Prior source summary".to_string(),
            objective: Some("Keep the source session moving".to_string()),
            task_summary: Some("Tracker: keep going".to_string()),
            spec_summary: None,
            evaluation_summary: None,
            constraints: Vec::new(),
            grounded_facts: vec![GroundedFactRecord {
                fact: "src/lib.rs was updated".to_string(),
                source: "tool:write_file".to_string(),
            }],
            touched_files: vec!["src/lib.rs".to_string()],
            open_questions: Vec::new(),
            verification_todo: Vec::new(),
            delegation_notes: Vec::new(),
            history_artifact_path: Some(".vtcode/history/session-source_0001.jsonl".to_string()),
            generated_at: "2026-03-14T00:00:00Z".to_string(),
        };
        fs::write(
            history_dir.join("session-source_0001.memory.json"),
            serde_json::to_string_pretty(&source_envelope).expect("serialize envelope"),
        )
        .expect("write envelope");

        let compacted = build_summarized_fork_history(
            &LocalCompactionProvider,
            "stub-model",
            "session-source",
            "session-target",
            temp.path(),
            Some(&VTCodeConfig::default()),
            &test_history(),
        )
        .await
        .expect("summarized fork history");

        assert_eq!(compacted.len(), 6);
        assert!(
            compacted[0]
                .content
                .as_text()
                .contains("[Session Memory Envelope]")
        );
        assert!(compacted[0].content.as_text().contains("src/lib.rs"));
        assert!(
            compacted[1]
                .content
                .as_text()
                .contains("Previous conversation summary")
        );
        assert_eq!(
            compacted
                .iter()
                .filter(|message| message.role == MessageRole::User)
                .count(),
            4
        );
        assert!(compacted.iter().all(
            |message| message.role == MessageRole::System || message.role == MessageRole::User
        ));
    }

    #[tokio::test]
    async fn local_and_fork_compaction_share_retained_user_budget() {
        let temp = tempdir().expect("tempdir");
        let provider = LocalCompactionProvider;
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.context.dynamic.retained_user_messages = 2;

        let mut history = test_history();
        let mut session_stats = SessionStats::default();
        let mut context_manager = test_context_manager();

        compact_history_in_place(
            &provider,
            "stub-model",
            "session-alpha",
            temp.path(),
            Some(&vt_cfg),
            &mut history,
            &mut session_stats,
            &mut context_manager,
        )
        .await
        .expect("compaction succeeds")
        .expect("history should compact");

        assert_local_compaction_history_with_user_count(&history, 0, 2);

        let compacted = build_summarized_fork_history(
            &provider,
            "stub-model",
            "session-alpha",
            "session-beta",
            temp.path(),
            Some(&vt_cfg),
            &test_history(),
        )
        .await
        .expect("summarized fork history");

        assert_eq!(
            compacted
                .iter()
                .filter(|message| message.role == MessageRole::User)
                .count(),
            2
        );
    }

    #[test]
    fn grounded_fact_extraction_dedupes_caps_and_skips_errors() {
        let history = vec![
            Message::tool_response_with_origin(
                "call_1".to_string(),
                "{\"result\":\"Cargo.toml declares vtcode-core\"}".to_string(),
                "read_file".to_string(),
            ),
            Message::tool_response_with_origin(
                "call_2".to_string(),
                "{\"result\":\"cargo.toml declares vtcode-core\"}".to_string(),
                "read_file".to_string(),
            ),
            Message::tool_response_with_origin(
                "call_3".to_string(),
                "{\"error\":\"denied\"}".to_string(),
                "read_file".to_string(),
            ),
            Message::user("Remember I prefer concise answers.".to_string()),
        ];

        let facts = super::dedup_latest_facts(&history, 5);
        assert_eq!(facts.len(), 2);
        assert!(facts.iter().any(|fact| fact.source == "tool:read_file"));
        assert!(facts.iter().any(|fact| fact.source == "user_assertion"));
    }

    #[test]
    fn resolve_compaction_threshold_prefers_configured_value() {
        assert_eq!(resolve_compaction_threshold(Some(42), 200_000), Some(42));
    }

    #[test]
    fn resolve_compaction_threshold_uses_context_ratio_when_unset() {
        assert_eq!(resolve_compaction_threshold(None, 200_000), Some(180_000));
    }

    #[test]
    fn resolve_compaction_threshold_clamps_to_context_size() {
        assert_eq!(
            resolve_compaction_threshold(Some(300_000), 200_000),
            Some(200_000)
        );
    }

    #[test]
    fn resolve_compaction_threshold_requires_context_or_override() {
        assert_eq!(resolve_compaction_threshold(None, 0), None);
    }

    #[test]
    fn build_server_compaction_context_management_creates_openai_payload() {
        assert_eq!(
            build_server_compaction_context_management(Some(512), 2_000),
            Some(json!([{
                "type": "compaction",
                "compact_threshold": 512,
            }]))
        );
    }
}
