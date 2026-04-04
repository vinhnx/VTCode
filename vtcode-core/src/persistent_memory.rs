use anyhow::{Context, Result, anyhow, bail};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{Duration, sleep};

use crate::config::loader::VTCodeConfig;
use crate::config::types::AgentConfig as RuntimeAgentConfig;
use crate::config::{ConfigManager, PersistentMemoryConfig, get_config_dir};
use crate::llm::factory::infer_provider_from_model;
use crate::llm::provider::{LLMProvider, LLMRequest, Message, MessageRole};
use crate::llm::{
    LightweightFeature, collect_single_response, create_provider_for_model_route,
    resolve_lightweight_route,
};

pub const MEMORY_FILENAME: &str = "MEMORY.md";
pub const MEMORY_SUMMARY_FILENAME: &str = "memory_summary.md";
pub const ROLLOUT_SUMMARIES_DIRNAME: &str = "rollout_summaries";
pub const NOTES_DIRNAME: &str = "notes";

const MEMORY_LOCK_FILENAME: &str = ".memory.lock";
const PREFERENCES_FILENAME: &str = "preferences.md";
const REPOSITORY_FACTS_FILENAME: &str = "repository-facts.md";
const DEFAULT_FACT_LIMIT: usize = 24;
const MEMORY_HIGHLIGHT_LIMIT: usize = 10;
const TOPIC_FACT_LIMIT: usize = 32;
const LOCK_RETRY_ATTEMPTS: usize = 40;
const LOCK_RETRY_DELAY_MS: u64 = 50;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroundedFactRecord {
    pub fact: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PersistentMemoryStatus {
    pub enabled: bool,
    pub auto_write: bool,
    pub directory: PathBuf,
    pub summary_file: PathBuf,
    pub memory_file: PathBuf,
    pub preferences_file: PathBuf,
    pub repository_facts_file: PathBuf,
    pub notes_dir: PathBuf,
    pub rollout_summaries_dir: PathBuf,
    pub summary_exists: bool,
    pub registry_exists: bool,
    pub pending_rollout_summaries: usize,
    pub cleanup_status: MemoryCleanupStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct PersistentMemoryExcerpt {
    pub status: PersistentMemoryStatus,
    pub contents: String,
    pub truncated: bool,
    pub bytes_read: usize,
    pub lines_read: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PersistentMemoryWriteReport {
    pub directory: PathBuf,
    pub summary_file: PathBuf,
    pub memory_file: PathBuf,
    pub rollout_summary_file: Option<PathBuf>,
    pub created_files: Vec<PathBuf>,
    pub added_facts: usize,
    pub pending_rollout_summaries: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PersistentMemoryMatch {
    pub source: String,
    pub fact: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PersistentMemoryForgetReport {
    pub directory: PathBuf,
    pub summary_file: PathBuf,
    pub memory_file: PathBuf,
    pub removed_facts: usize,
    pub pending_rollout_summaries: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MemoryCleanupStatus {
    pub needed: bool,
    pub suspicious_facts: usize,
    pub suspicious_summary_lines: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PersistentMemoryCleanupReport {
    pub directory: PathBuf,
    pub summary_file: PathBuf,
    pub memory_file: PathBuf,
    pub rewritten_facts: usize,
    pub removed_rollout_files: usize,
}

pub fn extract_memory_highlights(contents: &str, limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }

    let mut highlights = Vec::with_capacity(limit);
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let normalized = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
            .unwrap_or(trimmed);
        if normalized.is_empty() || highlights.iter().any(|existing| existing == normalized) {
            continue;
        }

        highlights.push(normalized.to_string());
        if highlights.len() == limit {
            break;
        }
    }

    highlights
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryOpKind {
    Remember,
    Forget,
    AskMissing,
    Noop,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryOpCandidate {
    pub id: usize,
    pub source: String,
    pub fact: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPlannedTopic {
    Preferences,
    RepositoryFacts,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryPlannedFact {
    pub topic: MemoryPlannedTopic,
    pub fact: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryMissingField {
    pub field: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryOpPlan {
    pub kind: MemoryOpKind,
    #[serde(default)]
    pub facts: Vec<MemoryPlannedFact>,
    #[serde(default)]
    pub selected_ids: Vec<usize>,
    #[serde(default)]
    pub missing: Option<MemoryMissingField>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemoryTopic {
    Preferences,
    RepositoryFacts,
}

impl MemoryTopic {
    fn title(self) -> &'static str {
        match self {
            Self::Preferences => "Preferences",
            Self::RepositoryFacts => "Repository Facts",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Preferences => "Durable user preferences and workflow notes.",
            Self::RepositoryFacts => "Grounded repository facts and recurring tooling notes.",
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Self::Preferences => "preferences",
            Self::RepositoryFacts => "repository_facts",
        }
    }

    fn from_slug(value: &str) -> Option<Self> {
        match value {
            "preferences" => Some(Self::Preferences),
            "repository_facts" => Some(Self::RepositoryFacts),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct PersistentMemoryFiles {
    directory: PathBuf,
    summary_file: PathBuf,
    memory_file: PathBuf,
    preferences_file: PathBuf,
    repository_facts_file: PathBuf,
    notes_dir: PathBuf,
    rollout_summaries_dir: PathBuf,
    lock_file: PathBuf,
}

impl PersistentMemoryFiles {
    fn new(directory: PathBuf) -> Self {
        Self {
            summary_file: directory.join(MEMORY_SUMMARY_FILENAME),
            memory_file: directory.join(MEMORY_FILENAME),
            preferences_file: directory.join(PREFERENCES_FILENAME),
            repository_facts_file: directory.join(REPOSITORY_FACTS_FILENAME),
            notes_dir: directory.join(NOTES_DIRNAME),
            rollout_summaries_dir: directory.join(ROLLOUT_SUMMARIES_DIRNAME),
            lock_file: directory.join(MEMORY_LOCK_FILENAME),
            directory,
        }
    }
}

#[derive(Debug, Clone)]
struct ClassifiedFacts {
    preferences: Vec<GroundedFactRecord>,
    repository_facts: Vec<GroundedFactRecord>,
}

impl ClassifiedFacts {
    fn total(&self) -> usize {
        self.preferences.len() + self.repository_facts.len()
    }
}

#[derive(Debug, Deserialize)]
struct MemorySummaryResponse {
    #[serde(default)]
    bullets: Vec<String>,
}

#[derive(Debug, Clone)]
struct MemoryModelRoute {
    provider_name: String,
    model: String,
    temperature: f32,
}

#[derive(Debug, Clone)]
struct ResolvedMemoryRoutes {
    primary: MemoryModelRoute,
    fallback: Option<MemoryModelRoute>,
    warning: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryClassificationItem {
    id: usize,
    topic: MemoryPlannedTopic,
    #[serde(default)]
    fact: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryClassificationPlan {
    #[serde(default)]
    keep: Vec<MemoryClassificationItem>,
}

pub fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn truncate_for_fact(text: &str, max_chars: usize) -> String {
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

fn memory_supports_native_json(provider: &dyn LLMProvider, route: &MemoryModelRoute) -> bool {
    provider.supports_structured_output(&route.model)
}

fn build_memory_json_request(
    provider: &dyn LLMProvider,
    route: &MemoryModelRoute,
    prompt: String,
    schema_name: &str,
    schema: &serde_json::Value,
) -> Result<LLMRequest> {
    let supports_native_json = memory_supports_native_json(provider, route);
    let prompt = if supports_native_json {
        prompt
    } else {
        let schema = serde_json::to_string_pretty(schema)
            .context("failed to serialize persistent memory JSON schema")?;
        format!(
            "{prompt}\n\nReturn JSON only. Do not add markdown fences or explanatory text. The response must be a single JSON object that matches this schema:\n{schema}"
        )
    };

    Ok(LLMRequest {
        model: route.model.clone(),
        temperature: Some(route.temperature),
        output_format: supports_native_json.then(|| {
            json!({
                "type": "json_schema",
                "json_schema": {
                    "name": schema_name,
                    "schema": schema,
                }
            })
        }),
        messages: vec![Message::user(prompt)],
        ..Default::default()
    })
}

fn parse_memory_json_response<T>(text: &str, context: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    let trimmed = text.trim();
    if trimmed.is_empty() {
        bail!("{context} returned empty content");
    }

    if let Ok(parsed) = serde_json::from_str::<T>(trimmed) {
        return Ok(parsed);
    }

    if let Some(json_block) = extract_first_json_block(trimmed) {
        return serde_json::from_str::<T>(json_block)
            .with_context(|| format!("failed to parse {context} response"));
    }

    serde_json::from_str::<T>(trimmed)
        .with_context(|| format!("failed to parse {context} response"))
}

fn extract_first_json_block(text: &str) -> Option<&str> {
    let (start, opening) = text
        .char_indices()
        .find(|(_, ch)| matches!(ch, '{' | '['))?;
    let mut stack = vec![opening];
    let mut in_string = false;
    let mut escaped = false;

    for (offset, ch) in text[start + opening.len_utf8()..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' | '[' => stack.push(ch),
            '}' => {
                if stack.pop() != Some('{') {
                    return None;
                }
                if stack.is_empty() {
                    let end = start + opening.len_utf8() + offset + ch.len_utf8();
                    return Some(&text[start..end]);
                }
            }
            ']' => {
                if stack.pop() != Some('[') {
                    return None;
                }
                if stack.is_empty() {
                    let end = start + opening.len_utf8() + offset + ch.len_utf8();
                    return Some(&text[start..end]);
                }
            }
            _ => {}
        }
    }

    None
}

pub fn maybe_extract_tool_fact(message: &Message) -> Option<GroundedFactRecord> {
    if message.role != MessageRole::Tool {
        return None;
    }

    let tool_name = message.origin_tool.as_deref().unwrap_or("tool");
    let text = message.content.as_text();
    let raw = text.trim();
    if raw.is_empty() {
        return None;
    }

    let candidate = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| {
            if value.get("error").is_some()
                || value.get("success") == Some(&serde_json::Value::Bool(false))
            {
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

pub fn maybe_extract_user_fact(message: &Message) -> Option<GroundedFactRecord> {
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

pub fn dedup_latest_facts(history: &[Message], limit: usize) -> Vec<GroundedFactRecord> {
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

    let keep_from = facts.len().saturating_sub(limit);
    facts.into_iter().skip(keep_from).collect()
}

pub fn resolve_persistent_memory_dir(
    config: &PersistentMemoryConfig,
    workspace_root: &Path,
) -> Result<Option<PathBuf>> {
    let project_name = persistent_memory_project_name(workspace_root);
    let directory = persistent_memory_base_dir(config)?
        .join("projects")
        .join(sanitize_project_name(&project_name))
        .join("memory");
    migrate_legacy_persistent_memory_dir_if_needed(config, &project_name, &directory)?;
    Ok(Some(directory))
}

pub fn persistent_memory_status(
    config: &PersistentMemoryConfig,
    workspace_root: &Path,
) -> Result<PersistentMemoryStatus> {
    let directory = resolve_persistent_memory_dir(config, workspace_root)?.unwrap_or_else(|| {
        dirs::home_dir()
            .map(|home| home.join(".vtcode"))
            .unwrap_or_else(|| PathBuf::from(".vtcode"))
            .join("projects")
            .join("workspace")
            .join("memory")
    });
    let files = PersistentMemoryFiles::new(directory);
    let pending_rollout_summaries = count_pending_rollout_summaries(&files.rollout_summaries_dir)?;
    let cleanup_status = detect_memory_cleanup_status(&files)?;

    Ok(PersistentMemoryStatus {
        enabled: config.enabled,
        auto_write: config.auto_write,
        summary_exists: files.summary_file.exists(),
        registry_exists: files.memory_file.exists(),
        pending_rollout_summaries,
        cleanup_status,
        directory: files.directory,
        summary_file: files.summary_file,
        memory_file: files.memory_file,
        preferences_file: files.preferences_file,
        repository_facts_file: files.repository_facts_file,
        notes_dir: files.notes_dir,
        rollout_summaries_dir: files.rollout_summaries_dir,
    })
}

pub async fn read_persistent_memory_excerpt(
    config: &PersistentMemoryConfig,
    workspace_root: &Path,
) -> Result<Option<PersistentMemoryExcerpt>> {
    if !config.enabled {
        return Ok(None);
    }

    let status = persistent_memory_status(config, workspace_root)?;
    if !status.summary_file.exists() {
        return Ok(None);
    }

    let raw = tokio::fs::read_to_string(&status.summary_file)
        .await
        .with_context(|| {
            format!(
                "Failed to read persistent memory summary {}",
                status.summary_file.display()
            )
        })?;

    let (contents, truncated, bytes_read, lines_read) =
        truncate_memory_excerpt(&raw, config.startup_line_limit, config.startup_byte_limit);

    Ok(Some(PersistentMemoryExcerpt {
        status,
        contents,
        truncated,
        bytes_read,
        lines_read,
    }))
}

pub async fn finalize_persistent_memory(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    history: &[Message],
) -> Result<Option<PersistentMemoryWriteReport>> {
    let config = vt_cfg
        .map(|cfg| cfg.agent.persistent_memory.clone())
        .unwrap_or_default();
    if !config.enabled || !config.auto_write {
        return Ok(None);
    }
    if persistent_memory_status(&config, runtime_config.workspace.as_path())?
        .cleanup_status
        .needed
    {
        return Ok(None);
    }

    let facts = dedup_latest_facts(history, DEFAULT_FACT_LIMIT);
    persist_memory_internal(
        &config,
        runtime_config.workspace.as_path(),
        Some(runtime_config),
        vt_cfg,
        &facts,
        true,
        false,
    )
    .await
}

pub async fn rebuild_persistent_memory_summary(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<Option<PersistentMemoryWriteReport>> {
    let config = vt_cfg
        .map(|cfg| cfg.agent.persistent_memory.clone())
        .unwrap_or_default();
    if !config.enabled {
        return Ok(None);
    }
    if persistent_memory_status(&config, runtime_config.workspace.as_path())?
        .cleanup_status
        .needed
    {
        bail!("persistent memory cleanup is required before rebuilding the summary");
    }

    persist_memory_internal(
        &config,
        runtime_config.workspace.as_path(),
        Some(runtime_config),
        vt_cfg,
        &[],
        false,
        true,
    )
    .await
}

pub async fn rebuild_generated_memory_files(
    config: &PersistentMemoryConfig,
    workspace_root: &Path,
) -> Result<()> {
    let directory = resolve_persistent_memory_dir(config, workspace_root)?
        .expect("persistent memory directory should resolve");
    let files = PersistentMemoryFiles::new(directory);
    let mut created_files = Vec::new();
    ensure_memory_layout(&files, &mut created_files).await?;
    let _lock = MemoryLock::acquire(&files.lock_file).await?;
    let _ = consolidate_memory_files(None, None, workspace_root, &files).await?;
    Ok(())
}

pub async fn scaffold_persistent_memory(
    config: &PersistentMemoryConfig,
    workspace_root: &Path,
) -> Result<Option<PersistentMemoryStatus>> {
    let status = persistent_memory_status(config, workspace_root)?;
    let files = PersistentMemoryFiles::new(status.directory.clone());
    let mut created_files = Vec::new();
    ensure_memory_layout(&files, &mut created_files).await?;
    Ok(Some(persistent_memory_status(config, workspace_root)?))
}

pub async fn cleanup_persistent_memory(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    include_summary_only_signals: bool,
) -> Result<Option<PersistentMemoryCleanupReport>> {
    let config = vt_cfg
        .map(|cfg| cfg.agent.persistent_memory.clone())
        .unwrap_or_default();
    if !config.enabled {
        return Ok(None);
    }

    let directory = resolve_persistent_memory_dir(&config, runtime_config.workspace.as_path())?
        .expect("persistent memory directory should resolve when enabled");
    let files = PersistentMemoryFiles::new(directory);
    let mut created_files = Vec::new();
    ensure_memory_layout(&files, &mut created_files).await?;

    let status = detect_memory_cleanup_status(&files)?;
    if !status.needed && !include_summary_only_signals {
        return Ok(Some(PersistentMemoryCleanupReport {
            directory: files.directory,
            summary_file: files.summary_file,
            memory_file: files.memory_file,
            rewritten_facts: 0,
            removed_rollout_files: 0,
        }));
    }

    let _lock = MemoryLock::acquire(&files.lock_file).await?;
    let candidates = collect_cleanup_candidates(&files).await?;
    let classified = if candidates.is_empty() {
        ClassifiedFacts {
            preferences: Vec::new(),
            repository_facts: Vec::new(),
        }
    } else {
        classify_facts_strict(
            Some(runtime_config),
            vt_cfg,
            runtime_config.workspace.as_path(),
            &candidates,
        )
        .await?
    };

    let removed_rollout_files = remove_rollout_markdown_files(&files.rollout_summaries_dir).await?;
    let notes = read_note_summaries(&files.notes_dir).await?;
    let mut created_files = Vec::new();
    write_topic_file(
        &files.preferences_file,
        MemoryTopic::Preferences,
        &classified.preferences,
        &mut created_files,
    )
    .await?;
    write_topic_file(
        &files.repository_facts_file,
        MemoryTopic::RepositoryFacts,
        &classified.repository_facts,
        &mut created_files,
    )
    .await?;
    write_memory_index(
        &files.memory_file,
        &classified.preferences,
        &classified.repository_facts,
        &notes,
        0,
        &mut created_files,
    )
    .await?;
    write_memory_summary(
        &files.summary_file,
        Some(runtime_config),
        vt_cfg,
        runtime_config.workspace.as_path(),
        &classified.preferences,
        &classified.repository_facts,
        &notes,
        &mut created_files,
    )
    .await?;

    Ok(Some(PersistentMemoryCleanupReport {
        directory: files.directory,
        summary_file: files.summary_file,
        memory_file: files.memory_file,
        rewritten_facts: classified.total(),
        removed_rollout_files,
    }))
}

pub async fn list_persistent_memory_candidates(
    config: &PersistentMemoryConfig,
    workspace_root: &Path,
) -> Result<Option<Vec<PersistentMemoryMatch>>> {
    if !config.enabled {
        return Ok(None);
    }

    let directory = resolve_persistent_memory_dir(config, workspace_root)?
        .expect("persistent memory directory should resolve when enabled");
    if !directory.exists() {
        return Ok(Some(Vec::new()));
    }

    let files = PersistentMemoryFiles::new(directory);
    collect_all_memory_matches(&files).await.map(Some)
}

pub async fn find_persistent_memory_matches(
    config: &PersistentMemoryConfig,
    workspace_root: &Path,
    query: &str,
) -> Result<Option<Vec<PersistentMemoryMatch>>> {
    if !config.enabled {
        return Ok(None);
    }
    let Some(normalized_query) = normalize_memory_query(query) else {
        return Ok(Some(Vec::new()));
    };
    let directory = resolve_persistent_memory_dir(config, workspace_root)?
        .expect("persistent memory directory should resolve when enabled");
    if !directory.exists() {
        return Ok(Some(Vec::new()));
    }

    let files = PersistentMemoryFiles::new(directory);
    collect_memory_matches(&files, &normalized_query)
        .await
        .map(Some)
}

pub async fn plan_remember_persistent_memory(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    request: &str,
    supplemental_answer: Option<&str>,
) -> Result<Option<MemoryOpPlan>> {
    let config = vt_cfg
        .map(|cfg| cfg.agent.persistent_memory.clone())
        .unwrap_or_default();
    if !config.enabled {
        return Ok(None);
    }

    let plan = plan_memory_operation(
        runtime_config,
        vt_cfg,
        runtime_config.workspace.as_path(),
        MemoryOpKind::Remember,
        request,
        supplemental_answer,
        &[],
    )
    .await?;
    Ok(Some(plan))
}

pub async fn persist_remembered_memory_plan(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    plan: &MemoryOpPlan,
) -> Result<Option<PersistentMemoryWriteReport>> {
    let config = vt_cfg
        .map(|cfg| cfg.agent.persistent_memory.clone())
        .unwrap_or_default();
    if !config.enabled || plan.kind != MemoryOpKind::Remember {
        return Ok(None);
    }

    let facts = memory_plan_facts(plan)?;
    persist_preclassified_memory_internal(
        &config,
        runtime_config.workspace.as_path(),
        Some(runtime_config),
        vt_cfg,
        &facts,
        true,
        false,
    )
    .await
}

pub async fn plan_forget_persistent_memory(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    request: &str,
    candidates: &[MemoryOpCandidate],
) -> Result<Option<MemoryOpPlan>> {
    let config = vt_cfg
        .map(|cfg| cfg.agent.persistent_memory.clone())
        .unwrap_or_default();
    if !config.enabled {
        return Ok(None);
    }

    let plan = plan_memory_operation(
        runtime_config,
        vt_cfg,
        runtime_config.workspace.as_path(),
        MemoryOpKind::Forget,
        request,
        None,
        candidates,
    )
    .await?;
    Ok(Some(plan))
}

pub async fn forget_planned_persistent_memory_matches(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    candidates: &[MemoryOpCandidate],
    plan: &MemoryOpPlan,
) -> Result<Option<PersistentMemoryForgetReport>> {
    let config = vt_cfg
        .map(|cfg| cfg.agent.persistent_memory.clone())
        .unwrap_or_default();
    if !config.enabled || plan.kind != MemoryOpKind::Forget {
        return Ok(None);
    }

    let selected = selected_memory_candidates(candidates, &plan.selected_ids)?;
    let directory = resolve_persistent_memory_dir(&config, runtime_config.workspace.as_path())?
        .expect("persistent memory directory should resolve when enabled");
    let files = PersistentMemoryFiles::new(directory);
    if !files.directory.exists() {
        return Ok(Some(PersistentMemoryForgetReport {
            directory: files.directory,
            summary_file: files.summary_file,
            memory_file: files.memory_file,
            removed_facts: 0,
            pending_rollout_summaries: 0,
        }));
    }

    let _lock = MemoryLock::acquire(&files.lock_file).await?;
    let mut removed_facts = 0usize;
    removed_facts += rewrite_topic_without_selected(
        &files.preferences_file,
        MemoryTopic::Preferences,
        &selected,
    )
    .await?;
    removed_facts += rewrite_topic_without_selected(
        &files.repository_facts_file,
        MemoryTopic::RepositoryFacts,
        &selected,
    )
    .await?;

    let rollout_files = list_rollout_markdown_files(&files.rollout_summaries_dir).await?;
    for path in rollout_files {
        removed_facts += scrub_rollout_file_by_selection(&path, &selected).await?;
    }

    if removed_facts > 0 {
        let _ = consolidate_memory_files(
            Some(runtime_config),
            vt_cfg,
            runtime_config.workspace.as_path(),
            &files,
        )
        .await?;
    }

    Ok(Some(PersistentMemoryForgetReport {
        directory: files.directory,
        summary_file: files.summary_file,
        memory_file: files.memory_file,
        removed_facts,
        pending_rollout_summaries: count_pending_rollout_summaries(&files.rollout_summaries_dir)?,
    }))
}

async fn persist_preclassified_memory_internal(
    config: &PersistentMemoryConfig,
    workspace_root: &Path,
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    facts: &[GroundedFactRecord],
    write_rollout: bool,
    force_rebuild: bool,
) -> Result<Option<PersistentMemoryWriteReport>> {
    let directory = resolve_persistent_memory_dir(config, workspace_root)?
        .expect("persistent memory directory should resolve when enabled");
    let files = PersistentMemoryFiles::new(directory);
    let mut created_files = Vec::new();
    ensure_memory_layout(&files, &mut created_files).await?;
    if detect_memory_cleanup_status(&files)?.needed && (write_rollout || !facts.is_empty()) {
        bail!("persistent memory cleanup is required before mutating memory");
    }

    let _lock = MemoryLock::acquire(&files.lock_file).await?;
    let existing_lines = read_existing_memory_lines(&files.directory).await?;
    let deduped_facts = facts
        .iter()
        .filter(|fact| {
            let normalized = normalize_whitespace(&fact.fact).to_ascii_lowercase();
            !existing_lines.contains(&normalized)
        })
        .cloned()
        .collect::<Vec<_>>();
    let classified = classified_facts_from_records(&deduped_facts);

    let staged_rollout = if write_rollout && classified.total() > 0 {
        Some(
            write_rollout_summary_pending(&files.rollout_summaries_dir, &classified)
                .await
                .with_context(|| {
                    format!(
                        "Failed to write rollout summary under {}",
                        files.rollout_summaries_dir.display()
                    )
                })?,
        )
    } else {
        None
    };

    let pending_before = list_pending_rollout_files(&files.rollout_summaries_dir).await?;
    let should_consolidate = force_rebuild
        || staged_rollout.is_some()
        || !pending_before.is_empty()
        || !files.summary_file.exists()
        || !files.memory_file.exists();
    if !should_consolidate {
        return Ok(None);
    }

    let consolidated =
        consolidate_memory_files(runtime_config, vt_cfg, workspace_root, &files).await?;
    created_files.extend(consolidated.created_files);
    created_files.sort();
    created_files.dedup();

    Ok(Some(PersistentMemoryWriteReport {
        directory: files.directory,
        summary_file: files.summary_file,
        memory_file: files.memory_file,
        rollout_summary_file: staged_rollout.map(finalize_rollout_summary_path),
        created_files,
        added_facts: consolidated.added_facts,
        pending_rollout_summaries: count_pending_rollout_summaries(&files.rollout_summaries_dir)?,
    }))
}

async fn persist_memory_internal(
    config: &PersistentMemoryConfig,
    workspace_root: &Path,
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    candidate_facts: &[GroundedFactRecord],
    write_rollout: bool,
    force_rebuild: bool,
) -> Result<Option<PersistentMemoryWriteReport>> {
    let directory = resolve_persistent_memory_dir(config, workspace_root)?
        .expect("persistent memory directory should resolve when enabled");
    let files = PersistentMemoryFiles::new(directory);
    let mut created_files = Vec::new();
    ensure_memory_layout(&files, &mut created_files).await?;
    if detect_memory_cleanup_status(&files)?.needed
        && (write_rollout || !candidate_facts.is_empty())
    {
        bail!("persistent memory cleanup is required before mutating memory");
    }

    let _lock = MemoryLock::acquire(&files.lock_file).await?;
    let existing_lines = read_existing_memory_lines(&files.directory).await?;

    let deduped_candidates = candidate_facts
        .iter()
        .filter(|fact| {
            let normalized = normalize_whitespace(&fact.fact).to_ascii_lowercase();
            !existing_lines.contains(&normalized)
        })
        .cloned()
        .collect::<Vec<_>>();

    let staged_rollout = if write_rollout && !deduped_candidates.is_empty() {
        let classified =
            classify_facts_strict(runtime_config, vt_cfg, workspace_root, &deduped_candidates)
                .await?;
        if classified.total() == 0 {
            None
        } else {
            Some(
                write_rollout_summary_pending(&files.rollout_summaries_dir, &classified)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to write rollout summary under {}",
                            files.rollout_summaries_dir.display()
                        )
                    })?,
            )
        }
    } else {
        None
    };

    let pending_before = list_pending_rollout_files(&files.rollout_summaries_dir).await?;
    let should_consolidate = force_rebuild
        || staged_rollout.is_some()
        || !pending_before.is_empty()
        || !files.summary_file.exists()
        || !files.memory_file.exists();
    if !should_consolidate {
        return Ok(None);
    }

    let consolidated =
        consolidate_memory_files(runtime_config, vt_cfg, workspace_root, &files).await?;
    created_files.extend(consolidated.created_files);
    created_files.sort();
    created_files.dedup();

    let pending_rollout_summaries = count_pending_rollout_summaries(&files.rollout_summaries_dir)?;

    Ok(Some(PersistentMemoryWriteReport {
        directory: files.directory,
        summary_file: files.summary_file,
        memory_file: files.memory_file,
        rollout_summary_file: staged_rollout.map(finalize_rollout_summary_path),
        created_files,
        added_facts: consolidated.added_facts,
        pending_rollout_summaries,
    }))
}

fn classified_facts_from_records(records: &[GroundedFactRecord]) -> ClassifiedFacts {
    let mut preferences = Vec::new();
    let mut repository_facts = Vec::new();
    for fact in records {
        match decode_topic_source(&fact.source)
            .0
            .unwrap_or_else(|| classify_fact(fact))
        {
            MemoryTopic::Preferences => preferences.push(fact.clone()),
            MemoryTopic::RepositoryFacts => repository_facts.push(fact.clone()),
        }
    }

    ClassifiedFacts {
        preferences: merge_topic_facts(preferences),
        repository_facts: merge_topic_facts(repository_facts),
    }
}

async fn ensure_memory_layout(
    files: &PersistentMemoryFiles,
    created_files: &mut Vec<PathBuf>,
) -> Result<()> {
    tokio::fs::create_dir_all(&files.directory)
        .await
        .with_context(|| {
            format!(
                "Failed to create persistent memory directory {}",
                files.directory.display()
            )
        })?;
    tokio::fs::create_dir_all(&files.rollout_summaries_dir)
        .await
        .with_context(|| {
            format!(
                "Failed to create rollout summaries directory {}",
                files.rollout_summaries_dir.display()
            )
        })?;
    tokio::fs::create_dir_all(&files.notes_dir)
        .await
        .with_context(|| {
            format!(
                "Failed to create notes directory {}",
                files.notes_dir.display()
            )
        })?;

    ensure_file(
        &files.preferences_file,
        render_topic_file(MemoryTopic::Preferences, &[]),
        created_files,
    )
    .await?;
    ensure_file(
        &files.repository_facts_file,
        render_topic_file(MemoryTopic::RepositoryFacts, &[]),
        created_files,
    )
    .await?;
    ensure_file(
        &files.memory_file,
        render_memory_index(&[], &[], &[], 0),
        created_files,
    )
    .await?;
    ensure_file(
        &files.summary_file,
        render_memory_summary(&[], &[], &[]),
        created_files,
    )
    .await?;

    Ok(())
}

async fn ensure_file(
    path: &Path,
    contents: String,
    created_files: &mut Vec<PathBuf>,
) -> Result<()> {
    if path.exists() {
        return Ok(());
    }

    tokio::fs::write(path, contents)
        .await
        .with_context(|| format!("Failed to write {}", path.display()))?;
    created_files.push(path.to_path_buf());
    Ok(())
}

fn persistent_memory_base_dir(config: &PersistentMemoryConfig) -> Result<PathBuf> {
    if let Some(override_dir) = config.directory_override.as_deref() {
        if let Some(stripped) = override_dir.strip_prefix("~/") {
            let home = dirs::home_dir().context("Could not resolve home directory")?;
            return Ok(home.join(stripped));
        }
        return Ok(PathBuf::from(override_dir));
    }

    dirs::home_dir()
        .map(|home| home.join(".vtcode"))
        .context("Could not resolve VT Code home directory")
}

fn persistent_memory_project_name(workspace_root: &Path) -> String {
    ConfigManager::current_project_name(workspace_root)
        .or_else(|| {
            workspace_root
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.to_string())
        })
        .unwrap_or_else(|| "workspace".to_string())
}

fn migrate_legacy_persistent_memory_dir_if_needed(
    config: &PersistentMemoryConfig,
    project_name: &str,
    target_dir: &Path,
) -> Result<()> {
    if config.directory_override.is_some() {
        return Ok(());
    }

    let Some(legacy_dir) = legacy_persistent_memory_dir(project_name)? else {
        return Ok(());
    };
    if legacy_dir == target_dir || !legacy_dir.exists() {
        return Ok(());
    }

    migrate_legacy_memory_dir(&legacy_dir, target_dir)
}

fn migrate_legacy_memory_dir(legacy_dir: &Path, target_dir: &Path) -> Result<()> {
    if target_dir.exists() && memory_directory_has_stored_content(target_dir)? {
        if !memory_directory_has_stored_content(legacy_dir)? {
            remove_empty_legacy_memory_hierarchy(legacy_dir)?;
        }
        return Ok(());
    }

    if target_dir.exists() {
        std::fs::remove_dir_all(target_dir)
            .with_context(|| format!("Failed to clear {}", target_dir.display()))?;
    }

    let target_parent = target_dir
        .parent()
        .context("Persistent memory directory is missing a parent")?;
    std::fs::create_dir_all(target_parent)
        .with_context(|| format!("Failed to create {}", target_parent.display()))?;

    std::fs::rename(legacy_dir, target_dir).with_context(|| {
        format!(
            "Failed to migrate persistent memory from {} to {}",
            legacy_dir.display(),
            target_dir.display()
        )
    })?;
    remove_empty_legacy_memory_hierarchy(legacy_dir)?;
    Ok(())
}

fn legacy_persistent_memory_dir(project_name: &str) -> Result<Option<PathBuf>> {
    let Some(legacy_base) = get_config_dir() else {
        return Ok(None);
    };
    let current_base = dirs::home_dir()
        .map(|home| home.join(".vtcode"))
        .context("Could not resolve VT Code home directory")?;
    if legacy_base == current_base {
        return Ok(None);
    }

    Ok(Some(
        legacy_base
            .join("projects")
            .join(sanitize_project_name(project_name))
            .join("memory"),
    ))
}

fn memory_directory_has_stored_content(directory: &Path) -> Result<bool> {
    if !directory.exists() {
        return Ok(false);
    }

    for path in [
        directory.join(PREFERENCES_FILENAME),
        directory.join(REPOSITORY_FACTS_FILENAME),
    ] {
        if !path.exists() {
            continue;
        }

        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if !parse_topic_file(&contents).is_empty() {
            return Ok(true);
        }
    }

    let rollout_dir = directory.join(ROLLOUT_SUMMARIES_DIRNAME);
    if !rollout_dir.exists() {
        return Ok(false);
    }

    for entry in std::fs::read_dir(&rollout_dir)
        .with_context(|| format!("Failed to list {}", rollout_dir.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if !parse_topic_file(&contents).is_empty() {
            return Ok(true);
        }
    }

    Ok(false)
}

fn remove_empty_legacy_memory_hierarchy(legacy_memory_dir: &Path) -> Result<()> {
    let mut current = legacy_memory_dir.parent();
    for _ in 0..3 {
        let Some(path) = current else {
            break;
        };
        match std::fs::remove_dir(path) {
            Ok(()) => current = path.parent(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => current = path.parent(),
            Err(err) if err.kind() == std::io::ErrorKind::DirectoryNotEmpty => break,
            Err(err) => {
                return Err(err).with_context(|| format!("Failed to remove {}", path.display()));
            }
        }
    }

    Ok(())
}

fn sanitize_project_name(project_name: &str) -> String {
    let sanitized = project_name
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' => '_',
            other => other,
        })
        .collect::<String>();
    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        "workspace".to_string()
    } else {
        trimmed.to_string()
    }
}

fn truncate_memory_excerpt(
    contents: &str,
    line_limit: usize,
    byte_limit: usize,
) -> (String, bool, usize, usize) {
    let all_lines = contents.lines().collect::<Vec<_>>();
    let mut selected = String::new();
    let mut bytes_read = 0usize;
    let mut lines_read = 0usize;
    let mut truncated = false;

    for (index, line) in all_lines.iter().enumerate() {
        if lines_read >= line_limit {
            truncated = true;
            break;
        }

        let line_bytes = line.len();
        let trailing_newline = usize::from(index + 1 < all_lines.len());
        if bytes_read + line_bytes + trailing_newline > byte_limit {
            truncated = true;
            break;
        }

        selected.push_str(line);
        selected.push('\n');
        bytes_read += line_bytes + trailing_newline;
        lines_read += 1;
    }

    if !truncated && contents.len() > bytes_read {
        truncated = true;
    }

    (
        selected.trim_end().to_string(),
        truncated,
        bytes_read,
        lines_read,
    )
}

async fn read_existing_memory_lines(directory: &Path) -> Result<BTreeSet<String>> {
    let mut lines = BTreeSet::new();
    if !directory.exists() {
        return Ok(lines);
    }

    let mut stack = vec![directory.to_path_buf()];
    while let Some(next_dir) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&next_dir)
            .await
            .with_context(|| format!("Failed to list {}", next_dir.display()))?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let metadata = entry.metadata().await?;
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|value| value.to_str()) != Some("md") {
                continue;
            }
            let content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
            for line in content.lines() {
                let Some((_, fact)) = parse_fact_line(line) else {
                    continue;
                };
                lines.insert(normalize_whitespace(&fact).to_ascii_lowercase());
            }
        }
    }

    Ok(lines)
}

fn default_cleanup_status() -> MemoryCleanupStatus {
    MemoryCleanupStatus {
        needed: false,
        suspicious_facts: 0,
        suspicious_summary_lines: 0,
    }
}

fn detect_memory_cleanup_status(files: &PersistentMemoryFiles) -> Result<MemoryCleanupStatus> {
    if !files.directory.exists() {
        return Ok(default_cleanup_status());
    }

    let mut suspicious_facts = 0usize;
    for path in [
        &files.preferences_file,
        &files.repository_facts_file,
        &files.memory_file,
    ] {
        suspicious_facts += count_suspicious_facts_in_file(path)?;
    }
    suspicious_facts += count_suspicious_rollout_facts(&files.rollout_summaries_dir)?;

    let suspicious_summary_lines = count_suspicious_summary_lines(&files.summary_file)?;
    Ok(MemoryCleanupStatus {
        needed: suspicious_facts > 0 || suspicious_summary_lines > 0,
        suspicious_facts,
        suspicious_summary_lines,
    })
}

fn count_suspicious_facts_in_file(path: &Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(parse_topic_file(&content)
        .into_iter()
        .filter(is_legacy_polluted_fact)
        .count())
}

fn count_suspicious_rollout_facts(rollout_dir: &Path) -> Result<usize> {
    if !rollout_dir.exists() {
        return Ok(0);
    }

    let mut count = 0usize;
    for entry in std::fs::read_dir(rollout_dir)
        .with_context(|| format!("Failed to list {}", rollout_dir.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        count += count_suspicious_facts_in_file(&path)?;
    }
    Ok(count)
}

fn count_suspicious_summary_lines(path: &Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("- "))
        .map(|line| line.trim_start_matches("- ").trim())
        .filter(|line| looks_like_legacy_prompt(line) || looks_like_serialized_payload(line))
        .count())
}

fn is_legacy_polluted_fact(fact: &GroundedFactRecord) -> bool {
    looks_like_legacy_prompt(&fact.fact) || looks_like_serialized_payload(&fact.fact)
}

fn looks_like_legacy_prompt(text: &str) -> bool {
    let mut lowered = normalize_whitespace(text).to_ascii_lowercase();
    while let Some(stripped) = [
        "please ",
        "can you ",
        "could you ",
        "would you ",
        "vt code, ",
        "vt code ",
    ]
    .iter()
    .find_map(|prefix| lowered.strip_prefix(prefix))
    {
        lowered = stripped.trim_start().to_string();
    }
    remember_markers_for_cleanup()
        .iter()
        .chain(forget_markers_for_cleanup())
        .any(|marker| lowered.starts_with(marker))
}

fn looks_like_serialized_payload(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with('{')
        || trimmed.starts_with('[')
        || trimmed.contains("\"query\":")
        || trimmed.contains("\"matches\":")
        || trimmed.contains("\"path\":")
        || trimmed.contains("</parameter>")
        || trimmed.contains("</invoke>")
        || trimmed.contains("<</invoke>")
}

fn remember_markers_for_cleanup() -> &'static [&'static str] {
    &[
        "save to memory",
        "remember that",
        "remember my",
        "remember ",
        "add to memory",
        "store in memory",
    ]
}

fn forget_markers_for_cleanup() -> &'static [&'static str] {
    &["forget ", "remove from memory", "delete from memory"]
}

fn normalize_memory_query(query: &str) -> Option<String> {
    let normalized = normalize_whitespace(query).to_ascii_lowercase();
    (!normalized.is_empty()).then_some(normalized)
}

async fn collect_memory_matches(
    files: &PersistentMemoryFiles,
    normalized_query: &str,
) -> Result<Vec<PersistentMemoryMatch>> {
    Ok(collect_all_memory_matches(files)
        .await?
        .into_iter()
        .filter(|record| {
            let normalized_fact = normalize_whitespace(&record.fact).to_ascii_lowercase();
            let normalized_source = normalize_whitespace(&record.source).to_ascii_lowercase();
            normalized_fact.contains(normalized_query)
                || normalized_source.contains(normalized_query)
        })
        .collect())
}

async fn collect_all_memory_matches(
    files: &PersistentMemoryFiles,
) -> Result<Vec<PersistentMemoryMatch>> {
    let preferences = read_topic_records(&files.preferences_file, MemoryTopic::Preferences).await?;
    let repository_facts =
        read_topic_records(&files.repository_facts_file, MemoryTopic::RepositoryFacts).await?;
    let rollout_records = read_rollout_records(&files.rollout_summaries_dir).await?;
    let notes = read_note_summaries(&files.notes_dir).await?;

    let mut matches = Vec::new();
    for record in preferences
        .into_iter()
        .chain(repository_facts)
        .chain(rollout_records.0)
        .chain(rollout_records.1)
    {
        let (_topic, source) = decode_topic_source(&record.source);
        matches.push(PersistentMemoryMatch {
            source,
            fact: record.fact,
        });
    }
    for note in notes {
        for highlight in note.highlights {
            matches.push(PersistentMemoryMatch {
                source: note.relative_path.clone(),
                fact: highlight,
            });
        }
    }

    let mut deduped = Vec::new();
    for record in matches {
        let normalized = normalize_whitespace(&record.fact).to_ascii_lowercase();
        if let Some(existing_idx) = deduped.iter().position(|entry: &PersistentMemoryMatch| {
            normalize_whitespace(&entry.fact).to_ascii_lowercase() == normalized
        }) {
            deduped.remove(existing_idx);
        }
        deduped.push(record);
    }
    Ok(deduped)
}

async fn collect_cleanup_candidates(
    files: &PersistentMemoryFiles,
) -> Result<Vec<GroundedFactRecord>> {
    let preferences = read_topic_records(&files.preferences_file, MemoryTopic::Preferences).await?;
    let repository_facts =
        read_topic_records(&files.repository_facts_file, MemoryTopic::RepositoryFacts).await?;
    let rollout_records = read_rollout_records(&files.rollout_summaries_dir).await?;

    Ok(preferences
        .into_iter()
        .chain(repository_facts)
        .chain(rollout_records.0)
        .chain(rollout_records.1)
        .collect())
}

async fn write_rollout_summary_pending(
    rollout_dir: &Path,
    classified: &ClassifiedFacts,
) -> Result<PathBuf> {
    tokio::fs::create_dir_all(rollout_dir)
        .await
        .with_context(|| format!("Failed to create {}", rollout_dir.display()))?;

    let file_name = format!("{}.pending.md", unique_rollout_id());
    let path = rollout_dir.join(file_name);
    let contents = render_rollout_summary(classified);
    tokio::fs::write(&path, contents)
        .await
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(path)
}

fn finalize_rollout_summary_path(path: PathBuf) -> PathBuf {
    if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
        path.with_file_name(name.trim_end_matches(".pending.md").to_string() + ".md")
    } else {
        path
    }
}

async fn list_pending_rollout_files(rollout_dir: &Path) -> Result<Vec<PathBuf>> {
    if !rollout_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = tokio::fs::read_dir(rollout_dir)
        .await
        .with_context(|| format!("Failed to list {}", rollout_dir.display()))?;
    let mut paths = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.ends_with(".pending.md"))
        {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

async fn list_rollout_markdown_files(rollout_dir: &Path) -> Result<Vec<PathBuf>> {
    if !rollout_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = tokio::fs::read_dir(rollout_dir)
        .await
        .with_context(|| format!("Failed to list {}", rollout_dir.display()))?;
    let mut paths = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("md") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn count_pending_rollout_summaries(rollout_dir: &Path) -> Result<usize> {
    if !rollout_dir.exists() {
        return Ok(0);
    }

    let mut count = 0usize;
    for entry in std::fs::read_dir(rollout_dir)
        .with_context(|| format!("Failed to list {}", rollout_dir.display()))?
    {
        let path = entry?.path();
        if path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.ends_with(".pending.md"))
        {
            count += 1;
        }
    }
    Ok(count)
}

fn list_note_markdown_files(notes_dir: &Path) -> Result<Vec<PathBuf>> {
    fn walk(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        if !dir.exists() {
            return Ok(());
        }

        for entry in
            std::fs::read_dir(dir).with_context(|| format!("Failed to list {}", dir.display()))?
        {
            let path = entry?.path();
            if path.is_dir() {
                walk(&path, files)?;
            } else if path.extension().and_then(|value| value.to_str()) == Some("md") {
                files.push(path);
            }
        }

        Ok(())
    }

    let mut files = Vec::new();
    walk(notes_dir, &mut files)?;
    files.sort();
    Ok(files)
}

async fn read_note_summaries(notes_dir: &Path) -> Result<Vec<MemoryNoteSummary>> {
    let note_files = list_note_markdown_files(notes_dir)?;
    let mut notes = Vec::with_capacity(note_files.len());
    for path in note_files {
        let content = tokio::fs::read_to_string(&path)
            .await
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let relative = path
            .strip_prefix(notes_dir)
            .with_context(|| format!("Failed to relativize {}", path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        notes.push(MemoryNoteSummary {
            relative_path: format!("{NOTES_DIRNAME}/{relative}"),
            highlights: extract_memory_highlights(&content, 3),
        });
    }
    Ok(notes)
}

struct ConsolidationResult {
    created_files: Vec<PathBuf>,
    added_facts: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MemoryNoteSummary {
    relative_path: String,
    highlights: Vec<String>,
}

async fn consolidate_memory_files(
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    workspace_root: &Path,
    files: &PersistentMemoryFiles,
) -> Result<ConsolidationResult> {
    let pending_files = list_pending_rollout_files(&files.rollout_summaries_dir).await?;
    let preferences_existing =
        read_topic_records(&files.preferences_file, MemoryTopic::Preferences).await?;
    let repository_existing =
        read_topic_records(&files.repository_facts_file, MemoryTopic::RepositoryFacts).await?;
    let rollout_records = read_rollout_records(&files.rollout_summaries_dir).await?;
    let notes = read_note_summaries(&files.notes_dir).await?;

    let preferences = merge_topic_facts(
        preferences_existing
            .into_iter()
            .chain(rollout_records.0)
            .collect(),
    );
    let repository_facts = merge_topic_facts(
        repository_existing
            .into_iter()
            .chain(rollout_records.1)
            .collect(),
    );

    let mut created_files = Vec::new();
    write_topic_file(
        &files.preferences_file,
        MemoryTopic::Preferences,
        &preferences,
        &mut created_files,
    )
    .await?;
    write_topic_file(
        &files.repository_facts_file,
        MemoryTopic::RepositoryFacts,
        &repository_facts,
        &mut created_files,
    )
    .await?;

    write_memory_index(
        &files.memory_file,
        &preferences,
        &repository_facts,
        &notes,
        0,
        &mut created_files,
    )
    .await?;
    write_memory_summary(
        &files.summary_file,
        runtime_config,
        vt_cfg,
        workspace_root,
        &preferences,
        &repository_facts,
        &notes,
        &mut created_files,
    )
    .await?;

    let added_facts = pending_files
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .flat_map(|content| {
            content
                .lines()
                .filter_map(parse_fact_line)
                .collect::<Vec<_>>()
        })
        .count();

    for pending in &pending_files {
        let finalized = finalize_rollout_summary_path(pending.clone());
        if !finalized.exists() {
            tokio::fs::rename(pending, &finalized)
                .await
                .with_context(|| {
                    format!("Failed to finalize rollout summary {}", pending.display())
                })?;
        } else {
            tokio::fs::remove_file(pending)
                .await
                .with_context(|| format!("Failed to remove {}", pending.display()))?;
        }
    }

    Ok(ConsolidationResult {
        created_files,
        added_facts,
    })
}

async fn read_topic_records(path: &Path, topic: MemoryTopic) -> Result<Vec<GroundedFactRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let contents = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(parse_topic_file(&contents)
        .into_iter()
        .map(|record| GroundedFactRecord {
            fact: record.fact,
            source: encode_topic_source(topic, &record.source),
        })
        .collect())
}

async fn read_rollout_records(
    rollout_dir: &Path,
) -> Result<(Vec<GroundedFactRecord>, Vec<GroundedFactRecord>)> {
    if !rollout_dir.exists() {
        return Ok((Vec::new(), Vec::new()));
    }

    let mut entries = tokio::fs::read_dir(rollout_dir)
        .await
        .with_context(|| format!("Failed to list {}", rollout_dir.display()))?;
    let mut preferences = Vec::new();
    let mut repository_facts = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        let contents = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        for record in parse_topic_file(&contents) {
            let (topic, _display_source) = decode_topic_source(&record.source);
            match topic.unwrap_or_else(|| classify_fact(&record)) {
                MemoryTopic::Preferences => preferences.push(record),
                MemoryTopic::RepositoryFacts => repository_facts.push(record),
            }
        }
    }

    Ok((preferences, repository_facts))
}

fn merge_topic_facts(records: Vec<GroundedFactRecord>) -> Vec<GroundedFactRecord> {
    let mut facts = Vec::new();
    for fact in records {
        let normalized = normalize_whitespace(&fact.fact).to_ascii_lowercase();
        if let Some(existing_idx) = facts.iter().position(|entry: &GroundedFactRecord| {
            normalize_whitespace(&entry.fact).to_ascii_lowercase() == normalized
        }) {
            facts.remove(existing_idx);
        }
        facts.push(fact);
    }

    let keep_from = facts.len().saturating_sub(TOPIC_FACT_LIMIT);
    facts.into_iter().skip(keep_from).collect()
}

fn normalized_selection_key(source: &str, fact: &str) -> String {
    format!(
        "{}::{}",
        normalize_whitespace(source).to_ascii_lowercase(),
        normalize_whitespace(fact).to_ascii_lowercase()
    )
}

fn selection_key_for_record(record: &GroundedFactRecord) -> String {
    let (_topic, source) = decode_topic_source(&record.source);
    normalized_selection_key(&source, &record.fact)
}

fn selection_keys(selected: &[MemoryOpCandidate]) -> BTreeSet<String> {
    selected
        .iter()
        .map(|entry| normalized_selection_key(&entry.source, &entry.fact))
        .collect()
}

async fn rewrite_topic_without_selected(
    path: &Path,
    topic: MemoryTopic,
    selected: &[MemoryOpCandidate],
) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }

    let selected = selection_keys(selected);
    let facts = read_topic_records(path, topic).await?;
    let removed = facts
        .iter()
        .filter(|fact| selected.contains(&selection_key_for_record(fact)))
        .count();
    if removed == 0 {
        return Ok(0);
    }

    let kept = facts
        .into_iter()
        .filter(|fact| !selected.contains(&selection_key_for_record(fact)))
        .collect::<Vec<_>>();
    tokio::fs::write(path, render_topic_file(topic, &kept))
        .await
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(removed)
}

async fn scrub_rollout_file_by_selection(
    path: &Path,
    selected: &[MemoryOpCandidate],
) -> Result<usize> {
    let contents = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let selected = selection_keys(selected);
    let mut removed = 0usize;
    let mut filtered = Vec::new();

    for line in contents.lines() {
        let keep = parse_fact_line(line).is_none_or(|(source, fact)| {
            let record = GroundedFactRecord { source, fact };
            let matches = selected.contains(&selection_key_for_record(&record));
            if matches {
                removed += 1;
            }
            !matches
        });
        if keep {
            filtered.push(line);
        }
    }

    if removed == 0 {
        return Ok(0);
    }

    let mut rewritten = filtered.join("\n");
    if contents.ends_with('\n') {
        rewritten.push('\n');
    }
    tokio::fs::write(path, rewritten)
        .await
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(removed)
}

async fn remove_rollout_markdown_files(rollout_dir: &Path) -> Result<usize> {
    let rollout_files = list_rollout_markdown_files(rollout_dir).await?;
    let count = rollout_files.len();
    for path in rollout_files {
        tokio::fs::remove_file(&path)
            .await
            .with_context(|| format!("Failed to remove {}", path.display()))?;
    }
    Ok(count)
}

async fn write_topic_file(
    path: &Path,
    topic: MemoryTopic,
    facts: &[GroundedFactRecord],
    created_files: &mut Vec<PathBuf>,
) -> Result<()> {
    if !path.exists() {
        created_files.push(path.to_path_buf());
    }
    let contents = render_topic_file(topic, facts);
    tokio::fs::write(path, contents)
        .await
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

async fn write_memory_index(
    path: &Path,
    preferences: &[GroundedFactRecord],
    repository_facts: &[GroundedFactRecord],
    notes: &[MemoryNoteSummary],
    pending_rollouts: usize,
    created_files: &mut Vec<PathBuf>,
) -> Result<()> {
    if !path.exists() {
        created_files.push(path.to_path_buf());
    }
    let contents = render_memory_index(preferences, repository_facts, notes, pending_rollouts);
    tokio::fs::write(path, contents)
        .await
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

async fn write_memory_summary(
    path: &Path,
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    workspace_root: &Path,
    preferences: &[GroundedFactRecord],
    repository_facts: &[GroundedFactRecord],
    notes: &[MemoryNoteSummary],
    created_files: &mut Vec<PathBuf>,
) -> Result<()> {
    let contents = summarize_memory(
        runtime_config,
        vt_cfg,
        workspace_root,
        preferences,
        repository_facts,
        notes,
    )
    .await
    .unwrap_or_else(|| render_memory_summary(preferences, repository_facts, notes));
    if !path.exists() {
        created_files.push(path.to_path_buf());
    }
    tokio::fs::write(path, contents)
        .await
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

fn parse_topic_file(contents: &str) -> Vec<GroundedFactRecord> {
    contents
        .lines()
        .filter_map(parse_fact_line)
        .map(|(source, fact)| GroundedFactRecord { source, fact })
        .collect()
}

fn parse_fact_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let remainder = trimmed.strip_prefix("- [")?;
    let (source, fact) = remainder.split_once("] ")?;
    let fact = fact.trim();
    if fact.is_empty() {
        return None;
    }
    Some((source.trim().to_string(), fact.to_string()))
}

fn classify_fact(fact: &GroundedFactRecord) -> MemoryTopic {
    if fact.source == "user_assertion" {
        MemoryTopic::Preferences
    } else {
        MemoryTopic::RepositoryFacts
    }
}

fn encode_topic_source(topic: MemoryTopic, source: &str) -> String {
    format!("{}:{}", topic.slug(), source)
}

fn decode_topic_source(source: &str) -> (Option<MemoryTopic>, String) {
    let Some((topic, display_source)) = source.split_once(':') else {
        return (None, source.to_string());
    };
    (
        MemoryTopic::from_slug(topic),
        display_source.trim().to_string(),
    )
}

async fn classify_facts_strict(
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    workspace_root: &Path,
    candidates: &[GroundedFactRecord],
) -> Result<ClassifiedFacts> {
    if candidates.is_empty() {
        return Ok(ClassifiedFacts {
            preferences: Vec::new(),
            repository_facts: Vec::new(),
        });
    }

    classify_facts_with_llm(runtime_config, vt_cfg, workspace_root, candidates).await
}

async fn classify_facts_with_llm(
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    workspace_root: &Path,
    candidates: &[GroundedFactRecord],
) -> Result<ClassifiedFacts> {
    let runtime_config = runtime_config
        .ok_or_else(|| anyhow!("runtime config is required for persistent memory LLM routing"))?;
    let routes = resolve_memory_model_routes(runtime_config, vt_cfg);
    log_memory_route_warning(&routes);

    let provider = create_memory_provider(&routes.primary, runtime_config, vt_cfg)?;
    match classify_facts_with_provider(
        provider.as_ref(),
        &routes.primary,
        workspace_root,
        candidates,
    )
    .await
    {
        Ok(result) => Ok(result),
        Err(primary_err) => {
            let Some(fallback) = routes.fallback.as_ref() else {
                return Err(primary_err);
            };

            tracing::warn!(
                model = %routes.primary.model,
                fallback_model = %fallback.model,
                error = %primary_err,
                "persistent memory classification failed on lightweight route; retrying with main model"
            );
            let provider = create_memory_provider(fallback, runtime_config, vt_cfg)?;
            classify_facts_with_provider(provider.as_ref(), fallback, workspace_root, candidates)
                .await
        }
    }
}

async fn classify_facts_with_provider(
    provider: &dyn LLMProvider,
    route: &MemoryModelRoute,
    workspace_root: &Path,
    candidates: &[GroundedFactRecord],
) -> Result<ClassifiedFacts> {
    let payload = candidates
        .iter()
        .enumerate()
        .map(|(index, fact)| {
            json!({
                "id": index,
                "source": fact.source,
                "fact": fact.fact,
            })
        })
        .collect::<Vec<_>>();

    let schema = json!({
        "type": "object",
        "properties": {
            "keep": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"},
                        "topic": {
                            "type": "string",
                            "enum": ["preferences", "repository_facts"]
                        },
                        "fact": {"type": "string"}
                    },
                    "required": ["id", "topic", "fact"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["keep"],
        "additionalProperties": false
    });
    let request = build_memory_json_request(
        provider,
        route,
        format!(
            "Classify VT Code memory evidence. Keep only durable reusable preferences or repository facts. Rewrite each kept fact into one concise canonical sentence. Drop transient, conversational, or noisy entries by omitting them.\n\nWorkspace: {}\nCandidates:\n{}",
            workspace_root.display(),
            serde_json::to_string_pretty(&payload)
                .context("failed to serialize memory classification payload")?
        ),
        "memory_classification",
        &schema,
    )?;

    let response = collect_single_response(provider, request)
        .await
        .context("persistent memory classification LLM request failed")?;
    let content = response
        .content
        .context("persistent memory classification returned no content")?;
    let parsed = parse_memory_json_response::<MemoryClassificationPlan>(
        content.trim(),
        "persistent memory classification",
    )?;

    let mut preferences = Vec::new();
    let mut repository_facts = Vec::new();
    for item in parsed.keep {
        let candidate = candidates.get(item.id).ok_or_else(|| {
            anyhow!(
                "memory classification referenced unknown candidate id {}",
                item.id
            )
        })?;
        let normalized_fact = normalize_whitespace(item.fact.as_deref().unwrap_or(&candidate.fact));
        if normalized_fact.is_empty() || looks_like_legacy_prompt(&normalized_fact) {
            continue;
        }
        let topic = match item.topic {
            MemoryPlannedTopic::Preferences => MemoryTopic::Preferences,
            MemoryPlannedTopic::RepositoryFacts => MemoryTopic::RepositoryFacts,
        };
        let record = GroundedFactRecord {
            fact: truncate_for_fact(&normalized_fact, 180),
            source: {
                let (_existing_topic, display_source) = decode_topic_source(&candidate.source);
                encode_topic_source(topic, &display_source)
            },
        };
        match topic {
            MemoryTopic::Preferences => preferences.push(record),
            MemoryTopic::RepositoryFacts => repository_facts.push(record),
        };
    }

    Ok(ClassifiedFacts {
        preferences,
        repository_facts,
    })
}

async fn summarize_memory(
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    workspace_root: &Path,
    preferences: &[GroundedFactRecord],
    repository_facts: &[GroundedFactRecord],
    notes: &[MemoryNoteSummary],
) -> Option<String> {
    let runtime_config = runtime_config?;
    let routes = resolve_memory_model_routes(runtime_config, vt_cfg);
    log_memory_route_warning(&routes);

    let provider = create_memory_provider(&routes.primary, runtime_config, vt_cfg).ok()?;
    match summarize_memory_with_provider(
        provider.as_ref(),
        &routes.primary,
        workspace_root,
        preferences,
        repository_facts,
        notes,
    )
    .await
    {
        Ok(summary) => Some(summary),
        Err(primary_err) => {
            let fallback = routes.fallback.as_ref()?;
            tracing::warn!(
                model = %routes.primary.model,
                fallback_model = %fallback.model,
                error = %primary_err,
                "persistent memory summary failed on lightweight route; retrying with main model"
            );
            let provider = create_memory_provider(fallback, runtime_config, vt_cfg).ok()?;
            summarize_memory_with_provider(
                provider.as_ref(),
                fallback,
                workspace_root,
                preferences,
                repository_facts,
                notes,
            )
            .await
            .ok()
        }
    }
}

async fn summarize_memory_with_provider(
    provider: &dyn LLMProvider,
    route: &MemoryModelRoute,
    workspace_root: &Path,
    preferences: &[GroundedFactRecord],
    repository_facts: &[GroundedFactRecord],
    notes: &[MemoryNoteSummary],
) -> Result<String> {
    let schema = json!({
        "type": "object",
        "properties": {
            "bullets": {
                "type": "array",
                "items": {"type": "string"}
            }
        },
        "required": ["bullets"],
        "additionalProperties": false
    });
    let request = build_memory_json_request(
        provider,
        route,
        format!(
            "Write a concise VT Code persistent memory summary for startup injection. Return 4-10 short bullets only. Focus on stable preferences, repository facts, and durable user-authored notes.\n\nWorkspace: {}\nPreferences:\n{}\n\nRepository facts:\n{}\n\nNotes:\n{}",
            workspace_root.display(),
            facts_for_prompt(preferences),
            facts_for_prompt(repository_facts),
            notes_for_prompt(notes),
        ),
        "memory_summary",
        &schema,
    )?;

    let response = collect_single_response(provider, request)
        .await
        .context("persistent memory summary LLM request failed")?
        .content
        .context("persistent memory summary returned no content")?;
    let parsed = parse_memory_json_response::<MemorySummaryResponse>(
        response.trim(),
        "persistent memory summary",
    )?;
    let bullets = parsed
        .bullets
        .into_iter()
        .map(|bullet| normalize_whitespace(&bullet))
        .filter(|bullet| !bullet.is_empty())
        .take(MEMORY_HIGHLIGHT_LIMIT)
        .collect::<Vec<_>>();
    if bullets.is_empty() {
        bail!("persistent memory summary returned no bullets");
    }

    Ok(render_memory_summary_bullets(&bullets))
}

async fn plan_memory_operation(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    workspace_root: &Path,
    expected_kind: MemoryOpKind,
    request: &str,
    supplemental_answer: Option<&str>,
    candidates: &[MemoryOpCandidate],
) -> Result<MemoryOpPlan> {
    let routes = resolve_memory_model_routes(runtime_config, vt_cfg);
    log_memory_route_warning(&routes);

    let provider = create_memory_provider(&routes.primary, runtime_config, vt_cfg)?;
    match plan_memory_operation_with_provider(
        provider.as_ref(),
        &routes.primary,
        workspace_root,
        expected_kind.clone(),
        request,
        supplemental_answer,
        candidates,
    )
    .await
    {
        Ok(plan) => Ok(plan),
        Err(primary_err) => {
            let Some(fallback) = routes.fallback.as_ref() else {
                return Err(primary_err);
            };

            tracing::warn!(
                model = %routes.primary.model,
                fallback_model = %fallback.model,
                error = %primary_err,
                "persistent memory planner failed on lightweight route; retrying with main model"
            );
            let provider = create_memory_provider(fallback, runtime_config, vt_cfg)?;
            plan_memory_operation_with_provider(
                provider.as_ref(),
                fallback,
                workspace_root,
                expected_kind,
                request,
                supplemental_answer,
                candidates,
            )
            .await
        }
    }
}

async fn plan_memory_operation_with_provider(
    provider: &dyn LLMProvider,
    route: &MemoryModelRoute,
    workspace_root: &Path,
    expected_kind: MemoryOpKind,
    request: &str,
    supplemental_answer: Option<&str>,
    candidates: &[MemoryOpCandidate],
) -> Result<MemoryOpPlan> {
    let payload = serde_json::to_string_pretty(candidates)
        .context("failed to serialize memory operation candidates")?;
    let supplemental = supplemental_answer.unwrap_or("").trim();
    let schema = json!({
        "type": "object",
        "properties": {
            "kind": {
                "type": "string",
                "enum": ["remember", "forget", "ask_missing", "noop"]
            },
            "facts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "topic": {
                            "type": "string",
                            "enum": ["preferences", "repository_facts"]
                        },
                        "fact": {"type": "string"},
                        "source": {"type": "string"}
                    },
                    "required": ["topic", "fact"],
                    "additionalProperties": false
                }
            },
            "selected_ids": {
                "type": "array",
                "items": {"type": "integer"}
            },
            "missing": {
                "type": ["object", "null"],
                "properties": {
                    "field": {"type": "string"},
                    "prompt": {"type": "string"}
                },
                "required": ["field", "prompt"],
                "additionalProperties": false
            },
            "message": {"type": ["string", "null"]}
        },
        "required": ["kind", "facts", "selected_ids", "missing", "message"],
        "additionalProperties": false
    });
    let llm_request = build_memory_json_request(
        provider,
        route,
        format!(
            "Plan a VT Code persistent memory operation.\n\nExpected operation: {:?}\nWorkspace: {}\nUser request: {}\nSupplemental answer: {}\nCurrent candidates:\n{}\n\nRules:\n- Never echo the raw request back as a saved fact.\n- For remember: extract only durable canonical facts. If a required value is missing, return ask_missing.\n- For forget: choose only ids from Current candidates. Do not invent ids.\n- For ask_missing: include one concise field label and one concise human-facing prompt.\n- For noop: do not include facts or selected ids.\n- Saved facts must be standalone sentences, not imperative prompts.",
            expected_kind,
            workspace_root.display(),
            request.trim(),
            if supplemental.is_empty() {
                "(none)"
            } else {
                supplemental
            },
            payload
        ),
        "memory_operation_plan",
        &schema,
    )?;

    let response = collect_single_response(provider, llm_request)
        .await
        .context("persistent memory planner LLM request failed")?;
    let content = response
        .content
        .context("persistent memory planner returned no content")?;
    let plan =
        parse_memory_json_response::<MemoryOpPlan>(content.trim(), "persistent memory planner")?;
    validate_memory_op_plan(&plan, expected_kind, candidates)?;
    Ok(plan)
}

fn validate_memory_op_plan(
    plan: &MemoryOpPlan,
    expected_kind: MemoryOpKind,
    candidates: &[MemoryOpCandidate],
) -> Result<()> {
    match plan.kind {
        MemoryOpKind::Remember => {
            if expected_kind != MemoryOpKind::Remember {
                bail!("memory planner returned remember for a non-remember request");
            }
            if plan.facts.is_empty() {
                bail!("memory planner returned remember with no facts");
            }
            if plan
                .facts
                .iter()
                .any(|fact| normalize_whitespace(&fact.fact).is_empty())
            {
                bail!("memory planner returned an empty fact");
            }
        }
        MemoryOpKind::Forget => {
            if expected_kind != MemoryOpKind::Forget {
                bail!("memory planner returned forget for a non-forget request");
            }
            let valid_ids = candidates
                .iter()
                .map(|candidate| candidate.id)
                .collect::<BTreeSet<_>>();
            if plan.selected_ids.iter().any(|id| !valid_ids.contains(id)) {
                bail!("memory planner selected an unknown memory candidate");
            }
        }
        MemoryOpKind::AskMissing => {
            let missing = plan
                .missing
                .as_ref()
                .ok_or_else(|| anyhow!("memory planner returned ask_missing without a prompt"))?;
            if normalize_whitespace(&missing.field).is_empty()
                || normalize_whitespace(&missing.prompt).is_empty()
            {
                bail!("memory planner returned an incomplete missing-field request");
            }
        }
        MemoryOpKind::Noop => {}
    }

    if matches!(plan.kind, MemoryOpKind::AskMissing | MemoryOpKind::Noop)
        && (!plan.facts.is_empty() || !plan.selected_ids.is_empty())
    {
        bail!("memory planner returned extra mutations for a non-mutating plan");
    }

    Ok(())
}

fn memory_plan_facts(plan: &MemoryOpPlan) -> Result<Vec<GroundedFactRecord>> {
    if plan.kind != MemoryOpKind::Remember {
        bail!("memory plan is not a remember operation");
    }

    Ok(plan
        .facts
        .iter()
        .map(|fact| {
            let topic = match fact.topic {
                MemoryPlannedTopic::Preferences => MemoryTopic::Preferences,
                MemoryPlannedTopic::RepositoryFacts => MemoryTopic::RepositoryFacts,
            };
            let source = if fact.source.trim().is_empty() {
                "manual_memory".to_string()
            } else {
                normalize_whitespace(&fact.source)
            };
            GroundedFactRecord {
                fact: truncate_for_fact(&normalize_whitespace(&fact.fact), 180),
                source: encode_topic_source(topic, &source),
            }
        })
        .filter(|fact| !fact.fact.is_empty())
        .collect())
}

fn selected_memory_candidates(
    candidates: &[MemoryOpCandidate],
    selected_ids: &[usize],
) -> Result<Vec<MemoryOpCandidate>> {
    let selected = selected_ids
        .iter()
        .filter_map(|id| {
            candidates
                .iter()
                .find(|candidate| candidate.id == *id)
                .cloned()
        })
        .collect::<Vec<_>>();
    if selected_ids.len() != selected.len() {
        bail!("memory plan selected a missing candidate");
    }
    Ok(selected)
}

fn facts_for_prompt(facts: &[GroundedFactRecord]) -> String {
    if facts.is_empty() {
        return "- none".to_string();
    }

    facts
        .iter()
        .map(|fact| {
            let (_topic, source) = decode_topic_source(&fact.source);
            format!("- [{}] {}", source, fact.fact)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn notes_for_prompt(notes: &[MemoryNoteSummary]) -> String {
    if notes.is_empty() {
        return "- none".to_string();
    }

    notes
        .iter()
        .map(|note| {
            let preview = if note.highlights.is_empty() {
                "no extracted highlights".to_string()
            } else {
                note.highlights.join("; ")
            };
            format!("- [{}] {}", note.relative_path, preview)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn resolve_memory_model_routes(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> ResolvedMemoryRoutes {
    let resolution =
        resolve_lightweight_route(runtime_config, vt_cfg, LightweightFeature::Memory, None);
    let primary = memory_model_route_from_resolution(&resolution.primary, runtime_config, vt_cfg);
    let fallback = resolution
        .fallback
        .as_ref()
        .map(|route| memory_model_route_from_resolution(route, runtime_config, vt_cfg));

    ResolvedMemoryRoutes {
        primary,
        fallback,
        warning: resolution.warning,
    }
}

fn memory_model_route_from_resolution(
    route: &crate::llm::ModelRoute,
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> MemoryModelRoute {
    let temperature = if route.model == runtime_config.model
        && route
            .provider_name
            .eq_ignore_ascii_case(runtime_provider_name(runtime_config).as_str())
    {
        0.0
    } else {
        vt_cfg
            .map(|cfg| cfg.agent.small_model.temperature)
            .unwrap_or(0.0)
    };

    MemoryModelRoute {
        provider_name: route.provider_name.clone(),
        model: route.model.clone(),
        temperature,
    }
}

fn log_memory_route_warning(routes: &ResolvedMemoryRoutes) {
    if let Some(warning) = &routes.warning {
        tracing::warn!(warning = %warning, "persistent memory route adjusted");
    }
}

fn create_memory_provider(
    route: &MemoryModelRoute,
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<Box<dyn LLMProvider>> {
    create_provider_for_model_route(
        &crate::llm::ModelRoute {
            provider_name: route.provider_name.clone(),
            model: route.model.clone(),
        },
        runtime_config,
        vt_cfg,
    )
    .context("Failed to initialize persistent memory LLM provider")
}

fn runtime_provider_name(runtime_config: &RuntimeAgentConfig) -> String {
    if !runtime_config.provider.trim().is_empty() {
        return runtime_config.provider.to_lowercase();
    }

    infer_provider_from_model(&runtime_config.model)
        .map(|provider| provider.to_string().to_lowercase())
        .unwrap_or_else(|| "gemini".to_string())
}

fn render_topic_file(topic: MemoryTopic, facts: &[GroundedFactRecord]) -> String {
    let mut output = String::new();
    output.push_str("# ");
    output.push_str(topic.title());
    output.push_str("\n\n");
    output.push_str(topic.description());
    output.push('\n');

    if facts.is_empty() {
        output.push_str("\n- No saved facts yet.\n");
        return output;
    }

    output.push('\n');
    for fact in facts {
        let (_topic, display_source) = decode_topic_source(&fact.source);
        output.push_str("- [");
        output.push_str(display_source.trim());
        output.push_str("] ");
        output.push_str(&fact.fact);
        output.push('\n');
    }

    output
}

fn render_memory_index(
    preferences: &[GroundedFactRecord],
    repository_facts: &[GroundedFactRecord],
    notes: &[MemoryNoteSummary],
    pending_rollouts: usize,
) -> String {
    let mut highlights = preferences
        .iter()
        .chain(repository_facts.iter())
        .cloned()
        .collect::<Vec<_>>();
    let keep_from = highlights.len().saturating_sub(MEMORY_HIGHLIGHT_LIMIT);
    highlights = highlights.into_iter().skip(keep_from).collect();

    let mut output = String::new();
    output.push_str("# VT Code Memory Registry\n\n");
    output.push_str("## Files\n");
    output.push_str("- `memory_summary.md`: Startup-injected summary for future sessions.\n");
    output.push_str("- `preferences.md`: Durable user preferences and workflow notes.\n");
    output.push_str(
        "- `repository-facts.md`: Grounded repository facts and recurring tooling notes.\n",
    );
    output
        .push_str("- `notes/`: User-authored durable notes available to the native memory tool.\n");
    output.push_str("- `rollout_summaries/`: Per-session evidence summaries.\n");
    output.push_str(&format!(
        "\n## Rollout Status\n- Pending rollout summaries: {}\n",
        pending_rollouts
    ));

    output.push_str("\n## Highlights\n");
    if highlights.is_empty() {
        output.push_str("- No persistent notes yet.\n");
    } else {
        for fact in highlights {
            let (_topic, display_source) = decode_topic_source(&fact.source);
            output.push_str("- [");
            output.push_str(display_source.trim());
            output.push_str("] ");
            output.push_str(&fact.fact);
            output.push('\n');
        }
    }

    if !notes.is_empty() {
        output.push_str("\n## Note Files\n");
        for note in notes {
            output.push_str("- ");
            output.push('`');
            output.push_str(&note.relative_path);
            output.push('`');
            if let Some(first) = note.highlights.first() {
                output.push_str(": ");
                output.push_str(first);
            }
            output.push('\n');
        }
    }

    output
}

fn render_memory_summary(
    preferences: &[GroundedFactRecord],
    repository_facts: &[GroundedFactRecord],
    notes: &[MemoryNoteSummary],
) -> String {
    let mut bullets = preferences
        .iter()
        .chain(repository_facts.iter())
        .map(|fact| fact.fact.clone())
        .collect::<Vec<_>>();
    bullets.extend(notes.iter().filter_map(|note| {
        note.highlights
            .first()
            .map(|highlight| format!("Note ({}): {}", note.relative_path, highlight))
    }));
    let keep_from = bullets.len().saturating_sub(MEMORY_HIGHLIGHT_LIMIT);
    bullets = bullets.into_iter().skip(keep_from).collect();
    if bullets.is_empty() {
        bullets.push("No durable memory notes have been consolidated yet.".to_string());
    }
    render_memory_summary_bullets(&bullets)
}

fn render_memory_summary_bullets(bullets: &[String]) -> String {
    let mut output = String::new();
    output.push_str("# VT Code Memory Summary\n\n");
    for bullet in bullets {
        output.push_str("- ");
        output.push_str(bullet.trim());
        output.push('\n');
    }
    output
}

fn render_rollout_summary(classified: &ClassifiedFacts) -> String {
    let mut output = String::new();
    output.push_str("# Rollout Summary\n\n");
    output.push_str(&format!(
        "- Generated: {}\n",
        chrono::Utc::now().to_rfc3339()
    ));
    output.push('\n');

    if classified.total() == 0 {
        output.push_str("- No durable facts captured.\n");
        return output;
    }

    for fact in classified
        .preferences
        .iter()
        .chain(classified.repository_facts.iter())
    {
        output.push_str("- [");
        output.push_str(&fact.source);
        output.push_str("] ");
        output.push_str(&fact.fact);
        output.push('\n');
    }

    output
}

fn unique_rollout_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("rollout-{millis}")
}

struct MemoryLock {
    path: PathBuf,
}

impl MemoryLock {
    async fn acquire(path: &Path) -> Result<Self> {
        for _ in 0..LOCK_RETRY_ATTEMPTS {
            match tokio::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path)
                .await
            {
                Ok(_) => {
                    return Ok(Self {
                        path: path.to_path_buf(),
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    sleep(Duration::from_millis(LOCK_RETRY_DELAY_MS)).await;
                }
                Err(err) => {
                    return Err(err)
                        .with_context(|| format!("Failed to acquire {}", path.display()));
                }
            }
        }

        Err(anyhow::anyhow!(
            "Timed out waiting for persistent memory lock {}",
            path.display()
        ))
    }
}

impl Drop for MemoryLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::models::ModelId;
    use crate::llm::provider::{
        FinishReason, LLMError, LLMNormalizedStream, LLMProvider, LLMRequest, LLMResponse,
        NormalizedStreamEvent,
    };
    use crate::llm::resolve_api_key_for_model_route;
    use async_trait::async_trait;
    use futures::stream;
    use std::sync::Mutex;
    use tempfile::tempdir;

    struct StaticProvider {
        response: &'static str,
        supports_structured_output: bool,
        last_request: Mutex<Option<LLMRequest>>,
    }

    impl StaticProvider {
        fn new(response: &'static str) -> Self {
            Self {
                response,
                supports_structured_output: true,
                last_request: Mutex::new(None),
            }
        }

        fn prompt_only_json(response: &'static str) -> Self {
            Self {
                response,
                supports_structured_output: false,
                last_request: Mutex::new(None),
            }
        }

        fn last_request(&self) -> LLMRequest {
            self.last_request
                .lock()
                .expect("request lock")
                .clone()
                .expect("request recorded")
        }
    }

    #[async_trait]
    impl LLMProvider for StaticProvider {
        fn name(&self) -> &str {
            "static"
        }

        async fn generate(
            &self,
            request: LLMRequest,
        ) -> std::result::Result<LLMResponse, LLMError> {
            *self.last_request.lock().expect("request lock") = Some(request);
            Ok(LLMResponse::new("stub-model", self.response))
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> std::result::Result<(), LLMError> {
            Ok(())
        }

        fn supports_structured_output(&self, _model: &str) -> bool {
            self.supports_structured_output
        }
    }

    fn message_history() -> Vec<Message> {
        vec![
            Message::user("remember that I prefer cargo nextest".to_string()),
            Message::tool_response_with_origin(
                "call-1".to_string(),
                serde_json::json!({"summary":"Tests live under vtcode-core/tests"}).to_string(),
                "unified_search".to_string(),
            ),
        ]
    }

    fn runtime_config(workspace: &Path) -> RuntimeAgentConfig {
        RuntimeAgentConfig {
            model: "gpt-5".to_string(),
            api_key: "test-key".to_string(),
            provider: "openai".to_string(),
            openai_chatgpt_auth: None,
            api_key_env: "OPENAI_API_KEY".to_string(),
            workspace: workspace.to_path_buf(),
            verbose: false,
            quiet: false,
            theme: "ciapre".to_string(),
            reasoning_effort: crate::config::types::ReasoningEffortLevel::None,
            ui_surface: crate::config::types::UiSurfacePreference::Auto,
            prompt_cache: crate::config::PromptCachingConfig::default(),
            model_source: crate::config::types::ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: Default::default(),
            checkpointing_enabled: true,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: 10,
            checkpointing_max_age_days: Some(7),
            max_conversation_turns: 10,
            model_behavior: None,
        }
    }

    fn enabled_memory_config() -> PersistentMemoryConfig {
        PersistentMemoryConfig {
            enabled: true,
            ..PersistentMemoryConfig::default()
        }
    }

    fn enabled_memory_config_for(workspace: &Path) -> PersistentMemoryConfig {
        PersistentMemoryConfig {
            enabled: true,
            directory_override: Some(workspace.join(".memory").display().to_string()),
            ..PersistentMemoryConfig::default()
        }
    }

    #[test]
    fn dedup_latest_facts_extracts_user_and_tool_memory() {
        let facts = dedup_latest_facts(&message_history(), 8);
        assert_eq!(facts.len(), 2);
        assert!(facts.iter().any(|fact| fact.source == "user_assertion"));
        assert!(
            facts
                .iter()
                .any(|fact| fact.source == "tool:unified_search")
        );
    }

    #[tokio::test]
    async fn llm_classification_rewrites_and_routes_candidates() {
        let workspace = tempdir().expect("workspace");
        let provider = StaticProvider::new(
            r#"{
              "keep": [
                {"id": 0, "topic": "preferences", "fact": "Prefer cargo nextest for test runs"},
                {"id": 1, "topic": "repository_facts", "fact": "Tests live under vtcode-core/tests"}
              ]
            }"#,
        );
        let route = MemoryModelRoute {
            provider_name: "stub".to_string(),
            model: "stub-model".to_string(),
            temperature: 0.0,
        };
        let classified = classify_facts_with_provider(
            &provider,
            &route,
            workspace.path(),
            &dedup_latest_facts(&message_history(), 8),
        )
        .await
        .expect("classify");

        assert_eq!(classified.preferences.len(), 1);
        assert_eq!(classified.repository_facts.len(), 1);
        assert_eq!(
            classified.preferences[0].fact,
            "Prefer cargo nextest for test runs"
        );
        assert_eq!(
            classified.repository_facts[0].fact,
            "Tests live under vtcode-core/tests"
        );
    }

    #[tokio::test]
    async fn remember_planner_requests_missing_details() {
        let workspace = tempdir().expect("workspace");
        let provider = StaticProvider::new(
            r#"{
              "kind": "ask_missing",
              "facts": [],
              "selected_ids": [],
              "missing": {"field": "name", "prompt": "What name should VT Code remember?"},
              "message": null
            }"#,
        );
        let route = MemoryModelRoute {
            provider_name: "stub".to_string(),
            model: "stub-model".to_string(),
            temperature: 0.0,
        };
        let plan = plan_memory_operation_with_provider(
            &provider,
            &route,
            workspace.path(),
            MemoryOpKind::Remember,
            "save to memory and remember my name",
            None,
            &[],
        )
        .await
        .expect("plan");

        assert_eq!(plan.kind, MemoryOpKind::AskMissing);
        assert_eq!(
            plan.missing.as_ref().map(|missing| missing.field.as_str()),
            Some("name")
        );
    }

    #[tokio::test]
    async fn forget_planner_selects_exact_candidate_ids() {
        let workspace = tempdir().expect("workspace");
        let provider = StaticProvider::new(
            r#"{
              "kind": "forget",
              "facts": [],
              "selected_ids": [1],
              "missing": null,
              "message": "Remove the pnpm preference."
            }"#,
        );
        let route = MemoryModelRoute {
            provider_name: "stub".to_string(),
            model: "stub-model".to_string(),
            temperature: 0.0,
        };
        let candidates = vec![
            MemoryOpCandidate {
                id: 0,
                source: "manual_memory".to_string(),
                fact: "Prefer cargo nextest".to_string(),
            },
            MemoryOpCandidate {
                id: 1,
                source: "manual_memory".to_string(),
                fact: "Prefer pnpm".to_string(),
            },
        ];
        let plan = plan_memory_operation_with_provider(
            &provider,
            &route,
            workspace.path(),
            MemoryOpKind::Forget,
            "forget my pnpm preference",
            None,
            &candidates,
        )
        .await
        .expect("plan");

        assert_eq!(plan.kind, MemoryOpKind::Forget);
        assert_eq!(plan.selected_ids, vec![1]);
    }

    #[tokio::test]
    async fn classification_falls_back_to_prompt_only_json_when_native_schema_is_unsupported() {
        let workspace = tempdir().expect("workspace");
        let provider = StaticProvider::prompt_only_json(
            "Here is the JSON:\n```json\n{\n  \"keep\": [\n    {\"id\": 0, \"topic\": \"preferences\", \"fact\": \"Prefer cargo nextest for test runs\"}\n  ]\n}\n```",
        );
        let route = MemoryModelRoute {
            provider_name: "stub".to_string(),
            model: "stub-model".to_string(),
            temperature: 0.0,
        };

        let classified = classify_facts_with_provider(
            &provider,
            &route,
            workspace.path(),
            &dedup_latest_facts(&message_history(), 8),
        )
        .await
        .expect("classify");

        assert_eq!(classified.preferences.len(), 1);
        let request = provider.last_request();
        assert!(request.output_format.is_none());
        assert!(
            request.messages[0]
                .content
                .as_text()
                .contains("Return JSON only.")
        );
    }

    #[tokio::test]
    async fn planner_falls_back_to_prompt_only_json_when_native_schema_is_unsupported() {
        let workspace = tempdir().expect("workspace");
        let provider = StaticProvider::prompt_only_json(
            "```json\n{\n  \"kind\": \"ask_missing\",\n  \"facts\": [],\n  \"selected_ids\": [],\n  \"missing\": {\"field\": \"name\", \"prompt\": \"What name should VT Code remember?\"},\n  \"message\": null\n}\n```",
        );
        let route = MemoryModelRoute {
            provider_name: "stub".to_string(),
            model: "stub-model".to_string(),
            temperature: 0.0,
        };

        let plan = plan_memory_operation_with_provider(
            &provider,
            &route,
            workspace.path(),
            MemoryOpKind::Remember,
            "remember my name",
            None,
            &[],
        )
        .await
        .expect("plan");

        assert_eq!(plan.kind, MemoryOpKind::AskMissing);
        let request = provider.last_request();
        assert!(request.output_format.is_none());
        assert!(
            request.messages[0]
                .content
                .as_text()
                .contains("Return JSON only.")
        );
    }

    #[derive(Clone)]
    struct StreamingOnlyMemoryProvider {
        response: &'static str,
    }

    #[async_trait]
    impl LLMProvider for StreamingOnlyMemoryProvider {
        fn name(&self) -> &str {
            "streaming-memory"
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        fn supports_non_streaming(&self, _model: &str) -> bool {
            false
        }

        async fn generate(
            &self,
            _request: LLMRequest,
        ) -> std::result::Result<LLMResponse, LLMError> {
            panic!("generate should not be called for streaming-only provider")
        }

        async fn stream_normalized(
            &self,
            _request: LLMRequest,
        ) -> std::result::Result<LLMNormalizedStream, LLMError> {
            Ok(Box::pin(stream::iter(vec![Ok(
                NormalizedStreamEvent::Done {
                    response: Box::new(LLMResponse {
                        content: Some(self.response.to_string()),
                        model: "stub-model".to_string(),
                        tool_calls: None,
                        usage: None,
                        finish_reason: FinishReason::Stop,
                        reasoning: None,
                        reasoning_details: None,
                        organization_id: None,
                        request_id: None,
                        tool_references: Vec::new(),
                    }),
                },
            )])))
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> std::result::Result<(), LLMError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn planner_supports_streaming_only_provider() {
        let workspace = tempdir().expect("workspace");
        let provider = StreamingOnlyMemoryProvider {
            response: "{\"kind\":\"ask_missing\",\"facts\":[],\"selected_ids\":[],\"missing\":{\"field\":\"name\",\"prompt\":\"What name should VT Code remember?\"},\"message\":null}",
        };
        let route = MemoryModelRoute {
            provider_name: "stub".to_string(),
            model: "stub-model".to_string(),
            temperature: 0.0,
        };

        let plan = plan_memory_operation_with_provider(
            &provider,
            &route,
            workspace.path(),
            MemoryOpKind::Remember,
            "remember my name",
            None,
            &[],
        )
        .await
        .expect("streaming planner should succeed");

        assert_eq!(plan.kind, MemoryOpKind::AskMissing);
    }

    #[tokio::test]
    async fn summary_falls_back_to_prompt_only_json_when_native_schema_is_unsupported() {
        let workspace = tempdir().expect("workspace");
        let provider = StaticProvider::prompt_only_json(
            "Summary:\n{\"bullets\":[\"Prefer cargo nextest\",\"Tests live under vtcode-core/tests\"]}",
        );
        let route = MemoryModelRoute {
            provider_name: "stub".to_string(),
            model: "stub-model".to_string(),
            temperature: 0.0,
        };

        let summary = summarize_memory_with_provider(
            &provider,
            &route,
            workspace.path(),
            &[GroundedFactRecord {
                fact: "Prefer cargo nextest".to_string(),
                source: encode_topic_source(MemoryTopic::Preferences, "manual_memory"),
            }],
            &[GroundedFactRecord {
                fact: "Tests live under vtcode-core/tests".to_string(),
                source: encode_topic_source(MemoryTopic::RepositoryFacts, "tool:unified_search"),
            }],
            &[],
        )
        .await
        .expect("summary");

        assert!(summary.contains("Prefer cargo nextest"));
        let request = provider.last_request();
        assert!(request.output_format.is_none());
    }

    #[tokio::test]
    async fn rebuild_summary_uses_summary_file_not_registry() {
        let workspace = tempdir().expect("workspace");
        std::fs::write(workspace.path().join(".git"), "gitdir: /tmp/git").expect("git marker");
        let memory_config = enabled_memory_config_for(workspace.path());
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.persistent_memory = memory_config.clone();

        let memory_dir = resolve_persistent_memory_dir(&memory_config, workspace.path())
            .expect("memory dir")
            .expect("resolved dir");
        let files = PersistentMemoryFiles::new(memory_dir.clone());
        let mut created_files = Vec::new();
        ensure_memory_layout(&files, &mut created_files)
            .await
            .expect("layout");
        tokio::fs::write(
            &files.preferences_file,
            render_topic_file(
                MemoryTopic::Preferences,
                &[GroundedFactRecord {
                    fact: "Prefer cargo nextest".to_string(),
                    source: encode_topic_source(MemoryTopic::Preferences, "manual_memory"),
                }],
            ),
        )
        .await
        .expect("write prefs");

        let excerpt = read_persistent_memory_excerpt(&memory_config, workspace.path())
            .await
            .expect("excerpt")
            .expect("present");
        assert!(excerpt.contents.contains("No durable memory notes"));

        rebuild_persistent_memory_summary(&runtime_config(workspace.path()), Some(&vt_cfg))
            .await
            .expect("rebuild")
            .expect("report");

        let excerpt = read_persistent_memory_excerpt(&memory_config, workspace.path())
            .await
            .expect("excerpt")
            .expect("present");
        assert!(excerpt.contents.contains("Prefer cargo nextest"));
    }

    #[tokio::test]
    async fn scaffold_creates_memory_layout_even_when_disabled() {
        let workspace = tempdir().expect("workspace");
        let config = PersistentMemoryConfig {
            enabled: false,
            directory_override: Some(workspace.path().join(".memory").display().to_string()),
            ..PersistentMemoryConfig::default()
        };

        let status = scaffold_persistent_memory(&config, workspace.path())
            .await
            .expect("scaffold succeeds")
            .expect("status");

        assert!(!status.enabled);
        assert!(status.summary_file.exists());
        assert!(status.memory_file.exists());
        assert!(status.preferences_file.exists());
        assert!(status.repository_facts_file.exists());
        assert!(status.notes_dir.exists());
        assert!(status.rollout_summaries_dir.exists());
    }

    #[tokio::test]
    async fn rebuild_generated_files_include_notes_as_canonical_inputs() {
        let workspace = tempdir().expect("workspace");
        let config = PersistentMemoryConfig {
            enabled: true,
            directory_override: Some(workspace.path().join(".memory").display().to_string()),
            ..PersistentMemoryConfig::default()
        };

        scaffold_persistent_memory(&config, workspace.path())
            .await
            .expect("scaffold")
            .expect("status");
        let memory_dir = resolve_persistent_memory_dir(&config, workspace.path())
            .expect("memory dir")
            .expect("resolved dir");
        let files = PersistentMemoryFiles::new(memory_dir);
        tokio::fs::write(
            files.notes_dir.join("project.md"),
            "# Project Notes\n\n- Keep Anthropic memory backed by shared storage.\n",
        )
        .await
        .expect("write note");

        rebuild_generated_memory_files(&config, workspace.path())
            .await
            .expect("rebuild");

        let summary = tokio::fs::read_to_string(&files.summary_file)
            .await
            .expect("summary");
        let index = tokio::fs::read_to_string(&files.memory_file)
            .await
            .expect("index");

        assert!(summary.contains("Keep Anthropic memory backed by shared storage"));
        assert!(index.contains("## Note Files"), "{index}");
        assert!(index.contains("Keep Anthropic memory backed by shared storage"));
    }

    #[tokio::test]
    async fn remember_plan_persists_normalized_manual_memory_update() {
        let workspace = tempdir().expect("workspace");
        std::fs::write(workspace.path().join(".git"), "gitdir: /tmp/git").expect("git marker");
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.persistent_memory = enabled_memory_config_for(workspace.path());
        let plan = MemoryOpPlan {
            kind: MemoryOpKind::Remember,
            facts: vec![MemoryPlannedFact {
                topic: MemoryPlannedTopic::Preferences,
                fact: "Prefer pnpm for workspace package management.".to_string(),
                source: "manual_memory".to_string(),
            }],
            selected_ids: Vec::new(),
            missing: None,
            message: None,
        };

        let report =
            persist_remembered_memory_plan(&runtime_config(workspace.path()), Some(&vt_cfg), &plan)
                .await
                .expect("remember plan")
                .expect("report");

        assert_eq!(report.added_facts, 1);
        let excerpt =
            read_persistent_memory_excerpt(&vt_cfg.agent.persistent_memory, workspace.path())
                .await
                .expect("excerpt")
                .expect("present");
        assert!(excerpt.contents.contains("Prefer pnpm"));
    }

    #[tokio::test]
    async fn forget_planned_matches_remove_notes_from_memory_files() {
        let workspace = tempdir().expect("workspace");
        std::fs::write(workspace.path().join(".git"), "gitdir: /tmp/git").expect("git marker");
        let runtime = runtime_config(workspace.path());
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.persistent_memory = enabled_memory_config_for(workspace.path());
        let remember_plan = MemoryOpPlan {
            kind: MemoryOpKind::Remember,
            facts: vec![MemoryPlannedFact {
                topic: MemoryPlannedTopic::Preferences,
                fact: "Prefer pnpm for workspace package management.".to_string(),
                source: "manual_memory".to_string(),
            }],
            selected_ids: Vec::new(),
            missing: None,
            message: None,
        };

        persist_remembered_memory_plan(&runtime, Some(&vt_cfg), &remember_plan)
            .await
            .expect("remember plan")
            .expect("report");

        let matches = find_persistent_memory_matches(
            &vt_cfg.agent.persistent_memory,
            workspace.path(),
            "pnpm",
        )
        .await
        .expect("find matches")
        .expect("enabled");
        assert!(!matches.is_empty());

        let candidates =
            list_persistent_memory_candidates(&vt_cfg.agent.persistent_memory, workspace.path())
                .await
                .expect("list")
                .expect("enabled")
                .into_iter()
                .enumerate()
                .map(|(index, entry)| MemoryOpCandidate {
                    id: index,
                    source: entry.source,
                    fact: entry.fact,
                })
                .collect::<Vec<_>>();
        let plan = MemoryOpPlan {
            kind: MemoryOpKind::Forget,
            facts: Vec::new(),
            selected_ids: vec![0],
            missing: None,
            message: None,
        };

        let report =
            forget_planned_persistent_memory_matches(&runtime, Some(&vt_cfg), &candidates, &plan)
                .await
                .expect("forget plan")
                .expect("report");
        assert!(report.removed_facts >= 1);

        let matches = find_persistent_memory_matches(
            &vt_cfg.agent.persistent_memory,
            workspace.path(),
            "pnpm",
        )
        .await
        .expect("find matches")
        .expect("enabled");
        assert!(matches.is_empty());

        let excerpt =
            read_persistent_memory_excerpt(&vt_cfg.agent.persistent_memory, workspace.path())
                .await
                .expect("excerpt")
                .expect("present");
        assert!(!excerpt.contents.contains("Prefer pnpm"));
    }

    #[test]
    fn cleanup_status_flags_legacy_prompt_lines() {
        let workspace = tempdir().expect("workspace");
        let memory_config = enabled_memory_config_for(workspace.path());
        let memory_dir = resolve_persistent_memory_dir(&memory_config, workspace.path())
            .expect("memory dir")
            .expect("resolved dir");
        let files = PersistentMemoryFiles::new(memory_dir);
        std::fs::create_dir_all(&files.directory).expect("dir");
        std::fs::create_dir_all(&files.rollout_summaries_dir).expect("rollout dir");
        std::fs::write(
            &files.preferences_file,
            "# Preferences\n\n- [user_assertion] save to memory and remember my name\n",
        )
        .expect("prefs");
        std::fs::write(
            &files.summary_file,
            "# VT Code Memory Summary\n\n- {\"query\":\"pnpm\"}\n",
        )
        .expect("summary");

        let status = detect_memory_cleanup_status(&files).expect("status");
        assert!(status.needed);
        assert!(status.suspicious_facts >= 1);
        assert!(status.suspicious_summary_lines >= 1);
    }

    #[test]
    fn cleanup_status_ignores_normalized_user_assertion_fact() {
        let workspace = tempdir().expect("workspace");
        let memory_config = enabled_memory_config_for(workspace.path());
        let memory_dir = resolve_persistent_memory_dir(&memory_config, workspace.path())
            .expect("memory dir")
            .expect("resolved dir");
        let files = PersistentMemoryFiles::new(memory_dir);
        std::fs::create_dir_all(&files.directory).expect("dir");
        std::fs::write(
            &files.preferences_file,
            "# Preferences\n\n- [user_assertion] My name is Vinh Nguyen\n",
        )
        .expect("prefs");

        let status = detect_memory_cleanup_status(&files).expect("status");
        assert!(!status.needed);
        assert_eq!(status.suspicious_facts, 0);
        assert_eq!(status.suspicious_summary_lines, 0);
    }

    #[test]
    fn cleanup_status_ignores_embedded_remember_word_in_fact() {
        let workspace = tempdir().expect("workspace");
        let memory_config = enabled_memory_config_for(workspace.path());
        let memory_dir = resolve_persistent_memory_dir(&memory_config, workspace.path())
            .expect("memory dir")
            .expect("resolved dir");
        let files = PersistentMemoryFiles::new(memory_dir);
        std::fs::create_dir_all(&files.directory).expect("dir");
        std::fs::write(
            &files.repository_facts_file,
            "# Repository Facts\n\n- [repository_fact] The docs remember prior design decisions in AGENTS.md.\n",
        )
        .expect("facts");

        let status = detect_memory_cleanup_status(&files).expect("status");
        assert!(!status.needed);
        assert_eq!(status.suspicious_facts, 0);
    }

    #[test]
    fn resolve_memory_model_route_prefers_explicit_small_model_provider() {
        let workspace = tempdir().expect("workspace");
        let runtime = runtime_config(workspace.path());
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.small_model.enabled = true;
        vt_cfg.agent.small_model.use_for_memory = true;
        vt_cfg.agent.small_model.model = "claude-4-5-haiku".to_string();

        let routes = resolve_memory_model_routes(&runtime, Some(&vt_cfg));

        assert_eq!(routes.primary.provider_name, "openai");
        assert_eq!(routes.primary.model, ModelId::GPT5Mini.as_str());
        assert!(routes.warning.is_some());
    }

    #[test]
    fn resolve_memory_route_api_key_uses_runtime_key_for_active_provider() {
        let workspace = tempdir().expect("workspace");
        let route = MemoryModelRoute {
            provider_name: "openai".to_string(),
            model: "gpt-5-mini".to_string(),
            temperature: 0.1,
        };

        let api_key = resolve_api_key_for_model_route(
            &crate::llm::ModelRoute {
                provider_name: route.provider_name.clone(),
                model: route.model.clone(),
            },
            &runtime_config(workspace.path()),
        );

        assert_eq!(api_key.as_deref(), Some("test-key"));
    }

    #[test]
    fn resolves_project_scoped_memory_directory() {
        let workspace = tempdir().expect("workspace");
        std::fs::write(workspace.path().join(".vtcode-project"), "renamed-project")
            .expect("project name");
        let config = enabled_memory_config();
        let directory = resolve_persistent_memory_dir(&config, workspace.path())
            .expect("memory dir")
            .expect("memory dir should resolve");
        assert!(
            directory
                .to_string_lossy()
                .contains(".vtcode/projects/renamed-project/memory")
        );
    }

    #[test]
    fn migrates_legacy_memory_into_empty_target_directory() {
        let root = tempdir().expect("root");
        let legacy_dir = root.path().join("legacy/projects/repo/memory");
        let target_dir = root.path().join("home/.vtcode/projects/repo/memory");
        std::fs::create_dir_all(legacy_dir.join(ROLLOUT_SUMMARIES_DIRNAME)).expect("legacy dir");
        std::fs::write(
            legacy_dir.join(PREFERENCES_FILENAME),
            render_topic_file(
                MemoryTopic::Preferences,
                &[GroundedFactRecord {
                    fact: "Prefer cargo nextest".to_string(),
                    source: encode_topic_source(MemoryTopic::Preferences, "manual_memory"),
                }],
            ),
        )
        .expect("legacy prefs");

        migrate_legacy_memory_dir(&legacy_dir, &target_dir).expect("migrate");

        assert!(!legacy_dir.exists());
        let migrated =
            std::fs::read_to_string(target_dir.join(PREFERENCES_FILENAME)).expect("target prefs");
        assert!(migrated.contains("Prefer cargo nextest"));
    }

    #[test]
    fn migrates_legacy_memory_over_scaffold_only_target() {
        let root = tempdir().expect("root");
        let legacy_dir = root.path().join("legacy/projects/repo/memory");
        let target_dir = root.path().join("home/.vtcode/projects/repo/memory");
        std::fs::create_dir_all(legacy_dir.join(ROLLOUT_SUMMARIES_DIRNAME)).expect("legacy dir");
        std::fs::write(
            legacy_dir.join(REPOSITORY_FACTS_FILENAME),
            render_topic_file(
                MemoryTopic::RepositoryFacts,
                &[GroundedFactRecord {
                    fact: "Tests live under vtcode-core/tests".to_string(),
                    source: encode_topic_source(
                        MemoryTopic::RepositoryFacts,
                        "tool:unified_search",
                    ),
                }],
            ),
        )
        .expect("legacy facts");

        std::fs::create_dir_all(target_dir.join(ROLLOUT_SUMMARIES_DIRNAME)).expect("target dir");
        std::fs::write(
            target_dir.join(PREFERENCES_FILENAME),
            render_topic_file(MemoryTopic::Preferences, &[]),
        )
        .expect("target prefs");
        std::fs::write(
            target_dir.join(REPOSITORY_FACTS_FILENAME),
            render_topic_file(MemoryTopic::RepositoryFacts, &[]),
        )
        .expect("target facts");
        std::fs::write(
            target_dir.join(MEMORY_FILENAME),
            render_memory_index(&[], &[], &[], 0),
        )
        .expect("target memory");
        std::fs::write(
            target_dir.join(MEMORY_SUMMARY_FILENAME),
            render_memory_summary(&[], &[], &[]),
        )
        .expect("target summary");

        migrate_legacy_memory_dir(&legacy_dir, &target_dir).expect("migrate");

        assert!(!legacy_dir.exists());
        let migrated = std::fs::read_to_string(target_dir.join(REPOSITORY_FACTS_FILENAME))
            .expect("target facts");
        assert!(migrated.contains("Tests live under vtcode-core/tests"));
    }
}
