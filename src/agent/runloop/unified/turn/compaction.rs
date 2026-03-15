use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use vtcode_config::constants::context::TOKEN_BUDGET_HIGH_THRESHOLD;
use vtcode_core::compaction::CompactionConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::context::history_files::{HistoryFileManager, messages_to_history_messages};
use vtcode_core::llm::provider::{LLMProvider, Message, MessageRole};

use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::state::SessionStats;

const MEMORY_ENVELOPE_HEADER: &str = "[Session Memory Envelope]";
const MEMORY_ENVELOPE_SUFFIX: &str = ".memory.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemoryEnvelopePersistence {
    PersistToDisk,
    InMemoryOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompactionOutcome {
    pub original_len: usize,
    pub compacted_len: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct GroundedFactRecord {
    pub fact: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SessionMemoryEnvelope {
    #[serde(default)]
    pub session_id: String,
    pub summary: String,
    pub task_summary: Option<String>,
    pub grounded_facts: Vec<GroundedFactRecord>,
    pub touched_files: Vec<String>,
    pub history_artifact_path: Option<String>,
    pub generated_at: String,
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

    if let Some(task_summary) = envelope.task_summary.as_deref()
        && !task_summary.trim().is_empty()
    {
        text.push_str("\n\nTask Tracker:\n");
        text.push_str(task_summary.trim());
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

    if let Some(history_path) = envelope.history_artifact_path.as_deref() {
        text.push_str("\n\nHistory Artifact:\n");
        text.push_str(history_path);
    }

    Message::system(text)
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

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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

fn truncate_for_fact(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let truncated = trimmed
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    format!("{truncated}...")
}

fn maybe_extract_tool_fact(message: &Message) -> Option<GroundedFactRecord> {
    if message.role != MessageRole::Tool {
        return None;
    }

    let tool_name = message.origin_tool.as_deref().unwrap_or("tool");
    let text = message.content.as_text();
    let raw = text.trim();
    if raw.is_empty() {
        return None;
    }

    let candidate = serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| {
            if value.get("error").is_some() || value.get("success") == Some(&Value::Bool(false)) {
                return None;
            }

            for key in ["summary", "message", "result", "output", "stdout"] {
                if let Some(value) = value.get(key) {
                    if let Some(text) = value.as_str() {
                        let normalized = normalize_whitespace(text);
                        if !normalized.is_empty() {
                            return Some(normalized);
                        }
                    } else if !value.is_null() {
                        let normalized = normalize_whitespace(&value.to_string());
                        if !normalized.is_empty() {
                            return Some(normalized);
                        }
                    }
                }
            }

            let compact = normalize_whitespace(&value.to_string());
            (!compact.is_empty()).then_some(compact)
        })
        .or_else(|| {
            let lowered = raw.to_ascii_lowercase();
            if lowered.contains("error")
                || lowered.contains("failed")
                || lowered.contains("denied")
                || lowered.contains("timeout")
            {
                return None;
            }
            Some(normalize_whitespace(raw))
        })?;

    Some(GroundedFactRecord {
        fact: truncate_for_fact(&candidate, 180),
        source: format!("tool:{tool_name}"),
    })
}

fn maybe_extract_user_fact(message: &Message) -> Option<GroundedFactRecord> {
    if message.role != MessageRole::User {
        return None;
    }

    let text = normalize_whitespace(message.content.as_text().as_ref());
    if text.is_empty() {
        return None;
    }

    let lowered = text.to_ascii_lowercase();
    let looks_explicit = lowered.contains("remember")
        || lowered.contains("note that")
        || lowered.starts_with("important:")
        || lowered.starts_with("i am ")
        || lowered.starts_with("i'm ")
        || lowered.starts_with("my ");
    looks_explicit.then(|| GroundedFactRecord {
        fact: truncate_for_fact(&text, 180),
        source: "user_assertion".to_string(),
    })
}

fn dedup_latest_facts(history: &[Message]) -> Vec<GroundedFactRecord> {
    let mut facts = Vec::new();
    for message in history {
        if let Some(fact) =
            maybe_extract_tool_fact(message).or_else(|| maybe_extract_user_fact(message))
        {
            let normalized = normalize_whitespace(&fact.fact).to_ascii_lowercase();
            if let Some(existing_idx) = facts.iter().position(|entry: &GroundedFactRecord| {
                normalize_whitespace(&entry.fact).to_ascii_lowercase() == normalized
            }) {
                facts.remove(existing_idx);
            }
            facts.push(fact);
        }
    }

    let keep_from = facts.len().saturating_sub(5);
    facts.into_iter().skip(keep_from).collect()
}

fn read_task_summary(workspace_root: &Path) -> Option<String> {
    let tracker_path = workspace_root
        .join(".vtcode")
        .join("tasks")
        .join("current_task.md");
    let content = fs::read_to_string(&tracker_path).ok()?;

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

    match (title, checklist.is_empty()) {
        (Some(title), false) => Some(format!("{title}: {}", checklist.join(" | "))),
        (Some(title), true) => Some(title),
        (None, false) => Some(checklist.join(" | ")),
        (None, true) => None,
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

fn apply_memory_envelope(compacted: &mut Vec<Message>, envelope: &SessionMemoryEnvelope) {
    strip_existing_memory_envelope(compacted);
    compacted.insert(0, memory_envelope_message(envelope));
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
    history.insert(0, memory_envelope_message(&envelope));
    true
}

fn persist_memory_envelope(
    workspace_root: &Path,
    session_id: &str,
    vt_cfg: Option<&VTCodeConfig>,
    original_history: &[Message],
    touched_files: &[String],
    compacted: &mut Vec<Message>,
    persistence: MemoryEnvelopePersistence,
) -> Result<Option<SessionMemoryEnvelope>> {
    let should_persist = should_persist_memory_envelope(vt_cfg);
    if original_history.is_empty()
        || (!should_persist && persistence == MemoryEnvelopePersistence::PersistToDisk)
    {
        return Ok(None);
    }

    let task_summary = read_task_summary(workspace_root);
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
    let envelope = SessionMemoryEnvelope {
        session_id: session_id.to_string(),
        summary: extract_compaction_summary(compacted, original_history),
        task_summary,
        grounded_facts: dedup_latest_facts(original_history),
        touched_files: touched_files.to_vec(),
        history_artifact_path: history_artifact_path
            .as_ref()
            .map(|path| path.display().to_string()),
        generated_at: Utc::now().to_rfc3339(),
    };

    if let Some(history_artifact_path) = history_artifact_path {
        let envelope_path =
            memory_envelope_path_from_history_path(workspace_root, &history_artifact_path);
        if let Some(parent) = envelope_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("create memory envelope directory {}", parent.display())
            })?;
        }
        let serialized = serde_json::to_string_pretty(&envelope)?;
        fs::write(&envelope_path, serialized)
            .with_context(|| format!("write memory envelope {}", envelope_path.display()))?;
    }

    apply_memory_envelope(compacted, &envelope);

    Ok(Some(envelope))
}

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
    compact_history_segment_in_place(
        provider,
        model,
        session_id,
        workspace_root,
        vt_cfg,
        history,
        session_stats,
        context_manager,
        MemoryEnvelopePersistence::PersistToDisk,
    )
    .await
}

async fn compact_history_segment_in_place(
    provider: &dyn LLMProvider,
    model: &str,
    session_id: &str,
    workspace_root: &Path,
    vt_cfg: Option<&VTCodeConfig>,
    history: &mut Vec<Message>,
    session_stats: &mut SessionStats,
    context_manager: &mut ContextManager,
    persistence: MemoryEnvelopePersistence,
) -> Result<Option<CompactionOutcome>> {
    strip_existing_memory_envelope(history);
    let original_len = history.len();
    let original_history = history.clone();
    let previous_response_chain_present = session_stats
        .previous_response_id_for(provider.name(), model)
        .is_some();
    let compacted = vtcode_core::compaction::compact_history(
        provider,
        model,
        history,
        &CompactionConfig::default(),
    )
    .await?;

    if compacted == *history {
        return Ok(None);
    }

    let compaction_mode = if provider.supports_responses_compaction(model) {
        "server"
    } else {
        "local"
    };
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
        persistence,
    )?;
    *history = compacted;
    session_stats.clear_previous_response_chain();
    context_manager
        .cap_token_usage_after_compaction(configured_compaction_threshold(vt_cfg, provider, model));
    if let Some(ref envelope) = envelope {
        tracing::info!(
            provider = %provider.name(),
            model = %model,
            turn = compacted_len,
            tool_count = 0usize,
            parallelized = false,
            compaction_mode = %compaction_mode,
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
        compaction_mode = %compaction_mode,
        grounded_fact_count = envelope.as_ref().map_or(0, |item| item.grounded_facts.len()),
        previous_response_chain_present,
        "Applied conversation compaction"
    );

    Ok(Some(CompactionOutcome {
        original_len,
        compacted_len,
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
    if start_index == 0 {
        return compact_history_segment_in_place(
            provider,
            model,
            session_id,
            workspace_root,
            vt_cfg,
            history,
            session_stats,
            context_manager,
            MemoryEnvelopePersistence::InMemoryOnly,
        )
        .await;
    }

    let prefix = history[..start_index].to_vec();
    let mut suffix = history[start_index..].to_vec();
    let Some(suffix_outcome) = compact_history_segment_in_place(
        provider,
        model,
        session_id,
        workspace_root,
        vt_cfg,
        &mut suffix,
        session_stats,
        context_manager,
        MemoryEnvelopePersistence::InMemoryOnly,
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
    }))
}

pub(crate) async fn maybe_auto_compact_history(
    provider: &dyn LLMProvider,
    model: &str,
    session_id: &str,
    workspace_root: &Path,
    vt_cfg: Option<&VTCodeConfig>,
    history: &mut Vec<Message>,
    session_stats: &mut SessionStats,
    context_manager: &mut ContextManager,
) -> Result<Option<CompactionOutcome>> {
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

    if context_manager.current_token_usage() < compact_threshold {
        return Ok(None);
    }

    compact_history_in_place(
        provider,
        model,
        session_id,
        workspace_root,
        Some(vt_cfg),
        history,
        session_stats,
        context_manager,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{
        GroundedFactRecord, build_server_compaction_context_management,
        compact_history_from_index_in_place, compact_history_in_place,
        inject_latest_memory_envelope, latest_memory_envelope_path_for_session,
        maybe_auto_compact_history, resolve_compaction_threshold,
    };
    use crate::agent::runloop::unified::context_manager::ContextManager;
    use crate::agent::runloop::unified::state::SessionStats;
    use async_trait::async_trait;
    use hashbrown::HashMap;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;
    use tokio::sync::RwLock;
    use vtcode_commons::llm::Usage;
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, Message};

    struct LocalCompactionProvider;

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

    fn test_history() -> Vec<Message> {
        (0..12)
            .map(|index| Message::user(format!("message-{index}")))
            .collect()
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
        session_stats.set_previous_response_chain("stub", "stub-model", Some("resp_123"));
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
        assert_eq!(outcome.compacted_len, 11);
        assert!(
            history[0]
                .content
                .as_text()
                .contains("[Session Memory Envelope]")
        );
        assert_eq!(
            session_stats.previous_response_id_for("stub", "stub-model"),
            None
        );
        assert!(context_manager.current_token_usage() < 900);
        assert!(latest_memory_envelope_path_for_session(temp.path(), "session-alpha").is_some());
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
        session_stats.set_previous_response_chain("stub", "stub-model", Some("resp_123"));
        let mut context_manager = test_context_manager();
        context_manager.update_token_usage(&Some(Usage {
            prompt_tokens: 900,
            completion_tokens: 10,
            total_tokens: 910,
            ..Usage::default()
        }));

        let outcome = maybe_auto_compact_history(
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
        .expect("auto compaction succeeds")
        .expect("history should compact");

        assert_eq!(outcome.original_len, 12);
        assert_eq!(outcome.compacted_len, 11);
        assert!(
            history[0]
                .content
                .as_text()
                .contains("[Session Memory Envelope]")
        );
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
        assert!(outcome.compacted_len <= outcome.original_len);
        assert!(
            history[1]
                .content
                .as_text()
                .contains("[Session Memory Envelope]")
        );
        assert!(latest_memory_envelope_path_for_session(temp.path(), "session-alpha").is_none());
    }

    #[test]
    fn inject_latest_memory_envelope_rehydrates_resume_history() {
        let temp = tempdir().expect("tempdir");
        let history_dir = temp.path().join(".vtcode").join("history");
        fs::create_dir_all(&history_dir).expect("history dir");
        let envelope_path = history_dir.join("resume-session_001.memory.json");
        let envelope = super::SessionMemoryEnvelope {
            session_id: "resume-session".to_string(),
            summary: "Persisted summary".to_string(),
            task_summary: Some("Tracker: - [ ] Follow up".to_string()),
            grounded_facts: vec![GroundedFactRecord {
                fact: "Cargo.toml declares vtcode-core".to_string(),
                source: "tool:read_file".to_string(),
            }],
            touched_files: vec!["Cargo.toml".to_string()],
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
                summary: summary.to_string(),
                task_summary: None,
                grounded_facts: Vec::new(),
                touched_files: Vec::new(),
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
                summary: summary.to_string(),
                task_summary: None,
                grounded_facts: Vec::new(),
                touched_files: Vec::new(),
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
                summary: summary.to_string(),
                task_summary: None,
                grounded_facts: Vec::new(),
                touched_files: Vec::new(),
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
        assert_eq!(outcome.compacted_len, 11);
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

        let facts = super::dedup_latest_facts(&history);
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
