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

fn should_persist_memory_envelope(vt_cfg: Option<&VTCodeConfig>) -> bool {
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

    if let Some(s) = maybe_section("Objective", envelope.objective.as_deref()) { sections.push(s); }
    if let Some(s) = maybe_section("Task Tracker", envelope.task_summary.as_deref()) { sections.push(s); }
    if let Some(s) = maybe_section("Spec Summary", envelope.spec_summary.as_deref()) { sections.push(s); }
    if let Some(s) = maybe_section("Evaluation Summary", envelope.evaluation_summary.as_deref()) { sections.push(s); }
    if let Some(s) = list_section("Constraints", &envelope.constraints) { sections.push(s); }
    if let Some(s) = list_section("Touched Files", &envelope.touched_files) { sections.push(s); }

    if !envelope.grounded_facts.is_empty() {
        let facts: Vec<_> = envelope.grounded_facts.iter()
            .map(|f| format!("[{}] {}", f.source, f.fact.trim())).collect();
        sections.push(format!("Grounded Facts:\n{}", facts.join("\n")));
    }
    if let Some(s) = list_section("Open Questions", &envelope.open_questions) { sections.push(s); }
    if let Some(s) = list_section("Verification Todo", &envelope.verification_todo) { sections.push(s); }
    if let Some(s) = list_section("Delegation Notes", &envelope.delegation_notes) { sections.push(s); }
    if let Some(s) = maybe_section("History Artifact", envelope.history_artifact_path.as_deref()) { sections.push(s); }

    Message::system(sections.join("\n\n"))
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

fn merge_touched_files(
    prior_envelope: Option<&SessionMemoryEnvelope>,
    touched_files: &[String],
) -> Vec<String> {
    let prior = prior_envelope
        .map(|e| e.touched_files.as_slice())
        .unwrap_or(&[]);
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

fn resolve_workspace_spool_path(workspace_root: &Path, raw_path: &str) -> Option<PathBuf> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path = Path::new(trimmed);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    };
    let normalized = vtcode_core::utils::path::normalize_path(&absolute);
    let normalized_workspace = vtcode_core::utils::path::normalize_path(workspace_root);
    normalized
        .starts_with(&normalized_workspace)
        .then_some(normalized)
}

fn structured_tool_preview_from_spool(
    obj: &serde_json::Map<String, Value>,
    workspace_root: &Path,
) -> Option<String> {
    let spool_path = obj.get("spool_path")?.as_str()?.trim();
    let resolved = resolve_workspace_spool_path(workspace_root, spool_path)?;
    let spool_content = String::from_utf8_lossy(&fs::read(&resolved).ok()?).into_owned();

    let mut parts = Vec::new();
    if let Some(stderr) = obj.get("stderr_preview").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        parts.push(normalize_whitespace(stderr));
    }

    let is_exec_like = obj.get("exit_code").is_some() || obj.get("stderr_preview").is_some()
        || obj.get("result_ref_only").and_then(Value::as_bool) == Some(true)
        || obj.get("spool_ref_only").and_then(Value::as_bool) == Some(true);

    if is_exec_like {
        parts.push(format!("Spool excerpt: {}", normalize_whitespace(&tail_preview_text(&spool_content, RECOVERY_PREVIEW_SPOOL_EXEC_TAIL_BYTES, RECOVERY_PREVIEW_SPOOL_EXEC_MAX_LINES))));
    } else {
        let mut excerpt_parts = Vec::new();
        if let Some(path) = obj.get("source_path").and_then(Value::as_str).or_else(|| obj.get("path").and_then(Value::as_str)).map(str::trim).filter(|s| !s.is_empty()) {
            excerpt_parts.push(format!("source_path: {path}"));
        }
        excerpt_parts.push(format!("Spool excerpt: {}", normalize_whitespace(&condense_text_bytes(&spool_content, RECOVERY_PREVIEW_SPOOL_READ_HEAD_BYTES, RECOVERY_PREVIEW_SPOOL_READ_TAIL_BYTES))));
        parts.push(excerpt_parts.join(" | "));
    }
    (!parts.is_empty()).then(|| parts.join(" | "))
}

pub(crate) fn build_recovery_context_previews_with_workspace(
    history: &[Message],
    workspace_root: Option<&Path>,
) -> Vec<String> {
    fn truncate_preview(text: &str) -> String {
        if text.chars().count() <= RECOVERY_PREVIEW_MAX_CHARS {
            return text.to_string();
        }
        let end = text.char_indices().nth(RECOVERY_PREVIEW_MAX_CHARS).map_or(text.len(), |(i, _)| i);
        let mut t = text[..end].trim_end().to_string();
        t.push_str("...");
        t
    }
    fn preview_line(label: &str, text: &str) -> String {
        format!("{label}: {}", truncate_preview(text))
    }
    fn trimmed_json_str(obj: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
        obj.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(normalize_whitespace)
    }
    fn error_preview_text(obj: &serde_json::Map<String, Value>) -> Option<String> {
        match obj.get("error") {
            Some(Value::String(t)) => { let t = t.trim(); (!t.is_empty()).then(|| normalize_whitespace(t)) }
            Some(Value::Object(e)) => e.get("message").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(normalize_whitespace),
            _ => None,
        }
    }
    fn push_unique(parts: &mut Vec<String>, text: Option<String>) {
        if let Some(t) = text { if !parts.iter().any(|e| e == &t) { parts.push(t); } }
    }
    fn compact_json_preview(v: &Value) -> Option<String> {
        let s = serde_json::to_string(v).ok()?;
        let n = normalize_whitespace(&s);
        (!n.is_empty()).then_some(n)
    }
    fn structured_tool_preview(raw_text: &str, workspace_root: Option<&Path>) -> Option<(String, u8)> {
        let obj = serde_json::from_str::<Value>(raw_text).ok()?.as_object()?.clone();
        let guidance = obj.get("error").and_then(Value::as_object).unwrap_or(&obj);
        let mut parts = Vec::new();
        let mut priority = 0u8;
        if let Some(matches) = obj.get("matches").and_then(Value::as_array) {
            let path = trimmed_json_str(&obj, "path");
            let summary = if matches.is_empty() {
                path.map_or_else(|| "No matches found".to_string(), |p| format!("No matches found in {p}"))
            } else {
                let total = obj.get("total_match_count").or_else(|| obj.get("matched_count")).or_else(|| obj.get("count")).and_then(Value::as_u64).unwrap_or(matches.len() as u64);
                path.map_or_else(|| format!("Found {total} matches"), |p| format!("Found {total} matches in {p}"))
            };
            push_unique(&mut parts, Some(summary));
            priority = priority.max(20);
        } else if let Some(items) = obj.get("items").and_then(Value::as_array) {
            let total = obj.get("total").or_else(|| obj.get("count")).and_then(Value::as_u64).unwrap_or(items.len() as u64);
            push_unique(&mut parts, Some(format!("Listed {total} items")));
            priority = priority.max(10);
        } else if let Some(files) = obj.get("files").and_then(Value::as_array) {
            let total = obj.get("total").and_then(Value::as_u64).unwrap_or(files.len() as u64);
            push_unique(&mut parts, Some(format!("Listed {total} files")));
            priority = priority.max(10);
        }
        push_unique(&mut parts, error_preview_text(&obj));
        for key in ["critical_note", "message", "hint"] {
            push_unique(&mut parts, trimmed_json_str(&obj, key).or_else(|| trimmed_json_str(guidance, key)));
        }
        if parts.iter().any(|p| !(p.starts_with("Listed ") || p.starts_with("Found ") || p.starts_with("No matches found"))) {
            priority = priority.max(55);
        }
        push_unique(&mut parts, trimmed_json_str(&obj, "next_action").or_else(|| trimmed_json_str(guidance, "next_action")).map(|n| format!("Next action: {n}")));
        if parts.iter().any(|p| p.starts_with("Next action: ")) { priority = priority.max(60); }
        if let Some(tool) = trimmed_json_str(&obj, "fallback_tool").or_else(|| trimmed_json_str(guidance, "fallback_tool")) {
            let fallback = obj.get("fallback_tool_args").or_else(|| guidance.get("fallback_tool_args")).and_then(compact_json_preview)
                .map(|a| format!("Fallback tool: {tool} {a}")).unwrap_or_else(|| format!("Fallback tool: {tool}"));
            push_unique(&mut parts, Some(fallback));
            priority = priority.max(60);
        }
        if let Some(ws) = workspace_root {
            let spool = structured_tool_preview_from_spool(&obj, ws);
            if spool.is_some() { priority = priority.max(100); }
            push_unique(&mut parts, spool);
        }
        if parts.is_empty() {
            for key in ["output", "content", "stdout", "stderr"] {
                push_unique(&mut parts, trimmed_json_str(&obj, key));
                if !parts.is_empty() { priority = priority.max(90); break; }
            }
        }
        (!parts.is_empty()).then(|| (parts.join(" | "), priority.max(1)))
    }

    let latest_user_request = history.iter().rev().find_map(|m| {
        if m.role != MessageRole::User { return None; }
        let text = normalize_whitespace(m.content.as_text().trim());
        if text.is_empty() { return None; }
        Some(preview_line(RECOVERY_PREVIEW_USER_LABEL, &text))
    });

    let mut tool_previews = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (recency_rank, message) in history.iter().rev().enumerate() {
        if message.role != MessageRole::Tool { continue; }
        let raw_text = message.content.as_text();
        let (text, priority) = structured_tool_preview(raw_text.as_ref(), workspace_root)
            .unwrap_or_else(|| (normalize_whitespace(raw_text.trim()), 50));
        if text.is_empty() || !seen.insert(text.clone()) { continue; }
        tool_previews.push((text, priority, recency_rank));
    }
    tool_previews.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));
    tool_previews.truncate(RECOVERY_PREVIEW_MAX_TOOL_OUTPUTS);

    let mut previews = Vec::new();
    if let Some(ur) = latest_user_request { previews.push(ur); }
    previews.extend(tool_previews.into_iter().enumerate().map(|(i, (t, _, _))| format!("Tool output {}: {}", i + 1, truncate_preview(&t))));

    if previews.is_empty() {
        if let Some(text) = history.iter().rev().find_map(|m| {
            let text = normalize_whitespace(m.content.as_text().trim());
            if text.is_empty() { return None; }
            let label = match m.role {
                MessageRole::Tool => RECOVERY_PREVIEW_TOOL_LABEL,
                MessageRole::Assistant => RECOVERY_PREVIEW_ASSISTANT_LABEL,
                MessageRole::User => RECOVERY_PREVIEW_USER_LABEL,
                _ => return None,
            };
            Some(preview_line(label, &text))
        }) {
            previews.push(text);
        }
    }
    previews
}

fn is_read_file_tool_name(tool_name: &str) -> bool {
    tool_name == tool_names::READ_FILE || tool_name.ends_with(".read_file")
}

fn collect_file_read_tool_kinds(history: &[Message]) -> HashMap<String, FileReadToolKind> {
    let mut kinds = HashMap::new();
    for message in history {
        let Some(tool_calls) = message.tool_calls.as_ref() else { continue; };
        for tc in tool_calls {
            let Some(tn) = tc.tool_name() else { continue; };
            let kind = if is_read_file_tool_name(tn) {
                Some(FileReadToolKind::ReadFile)
            } else if tn == tool_names::UNIFIED_FILE {
                tc.execution_arguments().ok().and_then(|args| {
                    args.get("action").and_then(Value::as_str).filter(|a| *a == "read").map(|_| FileReadToolKind::UnifiedFileRead)
                })
            } else { None };
            if let Some(k) = kind { kinds.insert(tc.id.clone(), k); }
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
    if obj.get("items").is_some() || obj.get("error").is_some()
        || obj.get("spool_chunked").and_then(Value::as_bool).unwrap_or(false)
        || obj.get("has_more").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let target = obj.get("file_path").and_then(Value::as_str)
        .or_else(|| obj.get("path").and_then(Value::as_str))
        .and_then(normalize_file_read_target)?;
    Some(FileReadDedupKey {
        target,
        start_line: obj.get("start_line").and_then(Value::as_u64),
        end_line: obj.get("end_line").and_then(Value::as_u64),
        spool_path: obj.get("spool_path").and_then(Value::as_str).and_then(normalize_file_read_target),
    })
}

fn build_file_read_placeholder_content(payload: &Value, key: &FileReadDedupKey) -> String {
    let mut p = serde_json::Map::new();
    p.insert("deduped_read".into(), Value::Bool(true));
    p.insert("note".into(), Value::String(DEDUPED_FILE_READ_NOTE.to_string()));
    fn maybe_str(p: &mut serde_json::Map<String, Value>, payload: &Value, key: &str) {
        if let Some(s) = payload.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
            p.insert(key.into(), Value::String(s.to_string()));
        }
    }
    maybe_str(&mut p, payload, "file_path");
    maybe_str(&mut p, payload, "path");
    if let Some(sl) = key.start_line { p.insert("start_line".into(), json!(sl)); }
    if let Some(el) = key.end_line { p.insert("end_line".into(), json!(el)); }
    if let Some(sp) = key.spool_path.as_deref() { p.insert("spool_path".into(), json!(sp)); }
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
    let mut last_idx = HashMap::new();
    let mut candidates = Vec::new();
    for (i, msg) in history.iter().enumerate() {
        let Some(c) = file_read_dedup_candidate(msg, &tool_kinds) else { continue; };
        last_idx.insert(c.key.clone(), i);
        candidates.push((i, c));
    }
    let mut deduped = history.to_vec();
    let mut changed = false;
    for (idx, c) in candidates {
        if last_idx.get(&c.key).copied() == Some(idx) { continue; }
        if let Some(msg) = deduped.get_mut(idx) { msg.content = c.placeholder_content.into(); changed = true; }
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
    let pe = prior_envelope;
    let spec_summary = read_spec_summary(workspace_root).or_else(|| pe.and_then(|e| e.spec_summary.clone()));
    let evaluation_summary = read_evaluation_summary(workspace_root).or_else(|| pe.and_then(|e| e.evaluation_summary.clone()));
    let merge = |prior: &[String], updates: &[String]| merge_recent_strings(prior, updates, MEMORY_LIST_LIMIT);
    let constraints = merge(pe.map(|e| e.constraints.as_slice()).unwrap_or(&[]), &extract_constraints_from_summary(spec_summary.as_deref()));
    let constraints = merge(&constraints, &extract_constraints_from_summary(evaluation_summary.as_deref()));
    let update = envelope_update.cloned().unwrap_or_default();

    SessionMemoryEnvelope {
        session_id: session_id.to_string(),
        schema_version: Some(SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
        summary,
        objective: update.objective.or_else(|| task_snapshot.objective.clone().or_else(|| pe.and_then(|e| e.objective.clone()))),
        task_summary: task_snapshot.summary.clone().or_else(|| pe.and_then(|e| e.task_summary.clone())),
        spec_summary,
        evaluation_summary,
        constraints: merge(&constraints, &update.constraints),
        grounded_facts: merge_grounded_facts(pe, original_history, &update.grounded_facts),
        touched_files: merge_touched_files(pe, &touched_files.iter().cloned().chain(update.touched_files).collect::<Vec<_>>()),
        open_questions: merge(pe.map(|e| e.open_questions.as_slice()).unwrap_or(&[]), &update.open_questions),
        verification_todo: merge(pe.map(|e| e.verification_todo.as_slice()).unwrap_or(&[]), &task_snapshot.verification_todo.iter().cloned().chain(update.verification_todo).collect::<Vec<_>>()),
        delegation_notes: merge(pe.map(|e| e.delegation_notes.as_slice()).unwrap_or(&[]), &update.delegation_notes),
        history_artifact_path: history_artifact_path.map(|p| p.display().to_string()).or_else(|| pe.and_then(|e| e.history_artifact_path.clone())),
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
    if original_history.is_empty() || (!should_persist && persistence == MemoryEnvelopePersistence::PersistToDisk) {
        return Ok(None);
    }

    let task_snapshot = read_task_tracker_snapshot(workspace_root);
    let history_artifact_path = if should_persist && persistence == MemoryEnvelopePersistence::PersistToDisk {
        let mut hm = HistoryFileManager::new(workspace_root, session_id);
        let hm2 = messages_to_history_messages(original_history, 0);
        let hr = hm.write_history_sync(&hm2, original_history.len(), "compaction", touched_files, &[]).context("write compaction history artifact")?;
        Some(hr.file_path)
    } else {
        None
    };
    let loaded = if seed_envelope.is_none() { load_latest_memory_envelope(workspace_root, session_id) } else { None };
    let prior = seed_envelope.or(loaded.as_ref());
    let envelope = build_session_memory_envelope(session_id, workspace_root, original_history, touched_files,
        extract_compaction_summary(compacted, original_history), history_artifact_path.as_ref(), prior, &task_snapshot, None);

    if let Some(hap) = history_artifact_path.as_ref() {
        write_memory_envelope_to_path(&memory_envelope_path_from_history_path(workspace_root, hap), &envelope)?;
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
    if history.is_empty() || !should_persist_memory_envelope(vt_cfg) { return Ok(None); }

    let prior = load_latest_memory_envelope(workspace_root, session_id);
    let task_snapshot = read_task_tracker_snapshot(workspace_root);
    let touched_files = session_stats.recent_touched_files();
    let envelope = build_session_memory_envelope(session_id, workspace_root, history, &touched_files,
        derive_continuity_summary(history, prior.as_ref()), None, prior.as_ref(), &task_snapshot, envelope_update);
    let path = latest_memory_envelope_path_for_session(workspace_root, session_id)
        .unwrap_or_else(|| default_memory_envelope_path_for_session(workspace_root, session_id));
    write_memory_envelope_to_path(&path, &envelope)?;
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
