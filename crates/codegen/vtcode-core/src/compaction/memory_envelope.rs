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

use vtcode_config::constants::context::DEFAULT_COMPACTION_TRIGGER_RATIO;
use vtcode_config::loader::VTCodeConfig;

use crate::compaction::CompactionConfig;
use crate::config::constants::tools as tool_names;
use crate::context::history_files::{HistoryFileManager, messages_to_history_messages};
use crate::core::agent::harness_artifacts::{current_task_path, read_evaluation_summary, read_spec_summary};
use crate::llm::provider::{LLMProvider, Message, MessageRole};
use crate::llm::utils::truncate_to_token_limit;
use crate::persistent_memory::{GroundedFactRecord, dedup_latest_facts, normalize_whitespace, truncate_for_fact};

pub const MEMORY_ENVELOPE_HEADER: &str = "[Session Memory Envelope]";
pub const MEMORY_ENVELOPE_SUFFIX: &str = ".memory.json";
pub const SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION: u32 = 2;
pub const MEMORY_LIST_LIMIT: usize = 5;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[allow(clippy::too_many_arguments)]
pub fn build_session_memory_envelope(
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
    let evaluation_summary =
        read_evaluation_summary(workspace_root).or_else(|| pe.and_then(|e| e.evaluation_summary.clone()));
    let merge = |prior: &[String], updates: &[String]| merge_recent_strings(prior, updates, MEMORY_LIST_LIMIT);
    let constraints = merge(
        pe.map(|e| e.constraints.as_slice()).unwrap_or(&[]),
        &extract_constraints_from_summary(spec_summary.as_deref()),
    );
    let constraints = merge(&constraints, &extract_constraints_from_summary(evaluation_summary.as_deref()));
    let update = envelope_update.cloned().unwrap_or_default();

    SessionMemoryEnvelope {
        session_id: session_id.to_string(),
        schema_version: Some(SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
        summary,
        objective: update
            .objective
            .or_else(|| pe.and_then(|e| e.objective.clone()).or_else(|| task_snapshot.objective.clone())),
        task_summary: pe
            .and_then(|e| e.task_summary.clone())
            .or_else(|| task_snapshot.summary.clone()),
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
        verification_todo: merge(
            pe.map(|e| e.verification_todo.as_slice()).unwrap_or(&[]),
            &task_snapshot
                .verification_todo
                .iter()
                .cloned()
                .chain(update.verification_todo)
                .collect::<Vec<_>>(),
        ),
        delegation_notes: merge(pe.map(|e| e.delegation_notes.as_slice()).unwrap_or(&[]), &update.delegation_notes),
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
        history_artifact_path.as_ref(),
        prior,
        &task_snapshot,
        None,
    );

    if let Some(hap) = history_artifact_path.as_ref() {
        write_memory_envelope_to_path(&memory_envelope_path_from_history_path(workspace_root, hap), &envelope)?;
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
        || (name.starts_with(&format!("{session_prefix}_")) && name.ends_with(MEMORY_ENVELOPE_SUFFIX))
}

pub fn read_task_tracker_snapshot(workspace_root: &Path) -> TaskTrackerSnapshot {
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
    let verification_summary = extract_verification_summary(&content, &checklist);
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

fn collect_structured_verify_commands(content: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut in_verify_block = false;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("verify:") {
            let command = normalize_whitespace(rest);
            if command.is_empty() {
                in_verify_block = true;
            } else {
                commands.push(command);
                in_verify_block = false;
            }
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
    candidates.sort();
    candidates
}

pub fn latest_memory_envelope_path_for_session(workspace_root: &Path, session_id: &str) -> Option<PathBuf> {
    memory_envelope_paths_for_session(workspace_root, session_id)
        .into_iter()
        .rev()
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create memory envelope directory {}", parent.display()))?;
    }
    let serialized = serde_json::to_string_pretty(envelope)?;
    fs::write(path, serialized).with_context(|| format!("write memory envelope {}", path.display()))?;
    Ok(())
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

#[allow(clippy::cast_sign_loss)] // context_size is usize (non-negative), ratio is positive
pub fn resolve_compaction_threshold(configured_threshold: Option<u64>, context_size: usize) -> Option<u64> {
    let configured_threshold = configured_threshold.filter(|threshold| *threshold > 0);
    let derived_threshold = if context_size > 0 {
        Some(((context_size as f64) * DEFAULT_COMPACTION_TRIGGER_RATIO).round().max(0.0) as u64)
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

pub fn effective_compaction_threshold(
    vt_cfg: Option<&VTCodeConfig>,
    provider: &dyn LLMProvider,
    model: &str,
) -> Option<usize> {
    let context_size = provider.effective_context_size(model);
    let configured_threshold = vt_cfg.and_then(|cfg| cfg.agent.harness.auto_compaction_threshold_tokens);

    resolve_compaction_threshold(configured_threshold, context_size)
        .and_then(|threshold| usize::try_from(threshold).ok())
}
