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
    if limit == 0 { return Vec::new(); }
    let mut highlights = Vec::with_capacity(limit);
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') { continue; }
        let normalized = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")).or_else(|| trimmed.strip_prefix("+ ")).unwrap_or(trimmed);
        if normalized.is_empty() || highlights.iter().any(|e| e == normalized) { continue; }
        highlights.push(normalized.to_string());
        if highlights.len() == limit { break; }
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
    fn total(&self) -> usize { self.preferences.len() + self.repository_facts.len() }
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
    if trimmed.is_empty() { bail!("{context} returned empty content"); }
    if let Ok(parsed) = serde_json::from_str::<T>(trimmed) { return Ok(parsed); }
    if let Some(json_block) = extract_first_json_block(trimmed) {
        return serde_json::from_str::<T>(json_block).with_context(|| format!("failed to parse {context} response"));
    }
    serde_json::from_str::<T>(trimmed).with_context(|| format!("failed to parse {context} response"))
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
    if message.role != MessageRole::Tool { return None; }
    let tool_name = message.origin_tool.as_deref().unwrap_or("tool");
    let text = message.content.as_text();
    let raw = text.trim();
    if raw.is_empty() { return None; }

    let candidate = serde_json::from_str::<serde_json::Value>(raw).ok().and_then(|value| {
        if value.get("error").is_some() || value.get("success") == Some(&serde_json::Value::Bool(false)) { return None; }
        for key in ["summary", "message", "result", "output", "stdout"] {
            if let Some(v) = value.get(key) {
                if let Some(text) = v.as_str() {
                    let normalized = normalize_whitespace(text);
                    if !normalized.is_empty() { return Some(normalized); }
                } else if !v.is_null() {
                    let normalized = normalize_whitespace(&v.to_string());
                    if !normalized.is_empty() { return Some(normalized); }
                }
            }
        }
        let compact = normalize_whitespace(&value.to_string());
        (!compact.is_empty()).then_some(compact)
    }).or_else(|| {
        let lowered = raw.to_ascii_lowercase();
        if lowered.contains("error") || lowered.contains("failed") || lowered.contains("denied") || lowered.contains("timeout") { return None; }
        Some(normalize_whitespace(raw))
    })?;

    Some(GroundedFactRecord { fact: truncate_for_fact(&candidate, 180), source: format!("tool:{tool_name}") })
}

pub fn maybe_extract_user_fact(message: &Message) -> Option<GroundedFactRecord> {
    if message.role != MessageRole::User { return None; }
    let text = normalize_whitespace(message.content.as_text().as_ref());
    if text.is_empty() { return None; }
    let candidate_text = strip_user_memory_candidate_prefixes(&text);
    let (candidate_text, looks_authored_note) = strip_user_memory_note_marker(candidate_text).map(|fact| (fact, true)).unwrap_or((candidate_text, false));
    let looks_durable_self_fact = SELF_FACT_PREFIXES.iter().any(|p| candidate_text.to_ascii_lowercase().starts_with(*p));
    let should_extract = looks_authored_note || looks_durable_self_fact;
    should_extract.then(|| GroundedFactRecord { fact: truncate_for_fact(candidate_text, 180), source: "user_assertion".to_string() })
}

fn strip_user_memory_candidate_prefixes(text: &str) -> &str {
    let mut trimmed = text.trim();
    loop {
        let lowered = trimmed.to_ascii_lowercase();
        let Some(prefix) = STRIP_PREFIXES.iter().find(|p| lowered.starts_with(**p)) else { return trimmed };
        trimmed = trimmed.get(prefix.len()..).unwrap_or("").trim_start_matches([',', ':', '-', ' ']).trim_start();
    }
}

fn strip_user_memory_note_marker(text: &str) -> Option<&str> {
    let lowered = text.to_ascii_lowercase();
    CLEANUP_NOTE_PREFIXES.iter().find_map(|prefix| {
        lowered.starts_with(prefix).then(|| text.get(prefix.len()..).unwrap_or("").trim_start_matches([',', ':', '-', ' ']).trim_start())
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
        FactsInput::Candidates(&facts),
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
        FactsInput::Candidates(&[]),
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

/// Write classified facts to all memory files (topic files, index, summary).
async fn write_classified_memory(files: &PersistentMemoryFiles, classified: &ClassifiedFacts, runtime_config: Option<&RuntimeAgentConfig>, vt_cfg: Option<&VTCodeConfig>, workspace_root: &Path) -> Result<Vec<PathBuf>> {
    let notes = read_note_summaries(&files.notes_dir).await?;
    let mut created_files = Vec::new();
    async fn write_if_missing(path: &Path, contents: String, created_files: &mut Vec<PathBuf>) -> Result<()> {
        if !path.exists() { created_files.push(path.to_path_buf()); }
        tokio::fs::write(path, contents).await.with_context(|| format!("Failed to write {}", path.display()))
    }
    write_if_missing(&files.preferences_file, render_topic_file(MemoryTopic::Preferences, &classified.preferences), &mut created_files).await?;
    write_if_missing(&files.repository_facts_file, render_topic_file(MemoryTopic::RepositoryFacts, &classified.repository_facts), &mut created_files).await?;
    write_if_missing(&files.memory_file, render_memory_index(&classified.preferences, &classified.repository_facts, &notes, 0), &mut created_files).await?;
    let summary = summarize_memory(runtime_config, vt_cfg, workspace_root, &classified.preferences, &classified.repository_facts, &notes).await.unwrap_or_else(|| render_memory_summary(&classified.preferences, &classified.repository_facts, &notes));
    write_if_missing(&files.summary_file, summary, &mut created_files).await?;
    Ok(created_files)
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
    let _ = write_classified_memory(
        &files,
        &classified,
        Some(runtime_config),
        vt_cfg,
        runtime_config.workspace.as_path(),
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
    persist_memory_internal(
        &config,
        runtime_config.workspace.as_path(),
        Some(runtime_config),
        vt_cfg,
        FactsInput::Preclassified(&facts),
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

/// Distinguishes how facts are provided to the persistence layer.
enum FactsInput<'a> {
    /// Facts already classified into topics (no LLM call needed).
    Preclassified(&'a [GroundedFactRecord]),
    /// Raw candidate facts that must be classified via LLM.
    Candidates(&'a [GroundedFactRecord]),
}

impl FactsInput<'_> {
    fn as_slice(&self) -> &[GroundedFactRecord] {
        match self {
            FactsInput::Preclassified(facts) => facts,
            FactsInput::Candidates(facts) => facts,
        }
    }
}

async fn persist_memory_internal(
    config: &PersistentMemoryConfig,
    workspace_root: &Path,
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    facts_input: FactsInput<'_>,
    write_rollout: bool,
    force_rebuild: bool,
) -> Result<Option<PersistentMemoryWriteReport>> {
    let directory = resolve_persistent_memory_dir(config, workspace_root)?.expect("persistent memory directory should resolve when enabled");
    let files = PersistentMemoryFiles::new(directory);
    let mut created_files = Vec::new();
    ensure_memory_layout(&files, &mut created_files).await?;

    let facts_slice = facts_input.as_slice();
    if detect_memory_cleanup_status(&files)?.needed && (write_rollout || !facts_slice.is_empty()) {
        bail!("persistent memory cleanup is required before mutating memory");
    }

    let _lock = MemoryLock::acquire(&files.lock_file).await?;
    let existing_lines = read_existing_memory_lines(&files.directory).await?;
    let deduped_records: Vec<GroundedFactRecord> = facts_slice.iter()
        .filter(|f| !existing_lines.contains(&normalize_whitespace(&f.fact).to_ascii_lowercase()))
        .cloned().collect();

    let classified = match facts_input {
        FactsInput::Preclassified(_) => classified_facts_from_records(&deduped_records),
        FactsInput::Candidates(_) if deduped_records.is_empty() => ClassifiedFacts { preferences: Vec::new(), repository_facts: Vec::new() },
        FactsInput::Candidates(_) => classify_facts_strict(runtime_config, vt_cfg, workspace_root, &deduped_records).await?,
    };

    let staged_rollout = if write_rollout && classified.total() > 0 {
        Some(write_rollout_summary_pending(&files.rollout_summaries_dir, &classified).await
            .with_context(|| format!("Failed to write rollout summary under {}", files.rollout_summaries_dir.display()))?)
    } else { None };

    let pending_before = list_pending_rollout_files(&files.rollout_summaries_dir).await?;
    let should_consolidate = force_rebuild || staged_rollout.is_some() || !pending_before.is_empty()
        || !files.summary_file.exists() || !files.memory_file.exists();
    if !should_consolidate { return Ok(None); }

    let consolidated = consolidate_memory_files(runtime_config, vt_cfg, workspace_root, &files).await?;
    created_files.extend(consolidated.created_files);
    created_files.sort();
    created_files.dedup();

    Ok(Some(PersistentMemoryWriteReport {
        directory: files.directory, summary_file: files.summary_file, memory_file: files.memory_file,
        rollout_summary_file: staged_rollout.map(finalize_rollout_summary_path), created_files,
        added_facts: consolidated.added_facts,
        pending_rollout_summaries: count_pending_rollout_summaries(&files.rollout_summaries_dir)?,
    }))
}

fn classified_facts_from_records(records: &[GroundedFactRecord]) -> ClassifiedFacts {
    let mut preferences = Vec::new();
    let mut repository_facts = Vec::new();
    for fact in records {
        let topic = decode_topic_source(&fact.source).0.unwrap_or_else(|| classify_fact(fact));
        match topic {
            MemoryTopic::Preferences => preferences.push(fact.clone()),
            MemoryTopic::RepositoryFacts => repository_facts.push(fact.clone()),
        }
    }
    ClassifiedFacts { preferences: merge_topic_facts(preferences), repository_facts: merge_topic_facts(repository_facts) }
}

async fn ensure_memory_layout(files: &PersistentMemoryFiles, created_files: &mut Vec<PathBuf>) -> Result<()> {
    async fn ensure_file(path: &Path, contents: String, created_files: &mut Vec<PathBuf>) -> Result<()> {
        if path.exists() { return Ok(()); }
        tokio::fs::write(path, contents).await.with_context(|| format!("Failed to write {}", path.display()))?;
        created_files.push(path.to_path_buf());
        Ok(())
    }
    for (dir, desc) in [(&files.directory, "persistent memory"), (&files.rollout_summaries_dir, "rollout summaries"), (&files.notes_dir, "notes")] {
        tokio::fs::create_dir_all(dir).await.with_context(|| format!("Failed to create {desc} {}", dir.display()))?;
    }
    ensure_file(&files.preferences_file, render_topic_file(MemoryTopic::Preferences, &[]), created_files).await?;
    ensure_file(&files.repository_facts_file, render_topic_file(MemoryTopic::RepositoryFacts, &[]), created_files).await?;
    ensure_file(&files.memory_file, render_memory_index(&[], &[], &[], 0), created_files).await?;
    ensure_file(&files.summary_file, render_memory_summary(&[], &[], &[]), created_files).await?;
    Ok(())
}

fn persistent_memory_base_dir(config: &PersistentMemoryConfig) -> Result<PathBuf> {
    if let Some(override_dir) = config.directory_override.as_deref() {
        if let Some(stripped) = override_dir.strip_prefix("~/") {
            return Ok(dirs::home_dir().context("Could not resolve home directory")?.join(stripped));
        }
        return Ok(PathBuf::from(override_dir));
    }
    dirs::home_dir().map(|home| home.join(".vtcode")).context("Could not resolve VT Code home directory")
}

fn persistent_memory_project_name(workspace_root: &Path) -> String {
    ConfigManager::current_project_name(workspace_root)
        .or_else(|| workspace_root.file_name().and_then(|v| v.to_str()).map(|v| v.to_string()))
        .unwrap_or_else(|| "workspace".to_string())
}

fn migrate_legacy_persistent_memory_dir_if_needed(config: &PersistentMemoryConfig, project_name: &str, target_dir: &Path) -> Result<()> {
    if config.directory_override.is_some() { return Ok(()); }
    let Some(legacy_dir) = legacy_persistent_memory_dir(project_name)? else { return Ok(()) };
    if legacy_dir == target_dir || !legacy_dir.exists() { return Ok(()) }
    migrate_legacy_memory_dir(&legacy_dir, target_dir)
}

fn migrate_legacy_memory_dir(legacy_dir: &Path, target_dir: &Path) -> Result<()> {
    if target_dir.exists() && memory_directory_has_stored_content(target_dir)? {
        if !memory_directory_has_stored_content(legacy_dir)? { remove_empty_legacy_memory_hierarchy(legacy_dir)?; }
        return Ok(());
    }
    if target_dir.exists() {
        std::fs::remove_dir_all(target_dir).with_context(|| format!("Failed to clear {}", target_dir.display()))?;
    }
    let target_parent = target_dir.parent().context("Persistent memory directory is missing a parent")?;
    std::fs::create_dir_all(target_parent).with_context(|| format!("Failed to create {}", target_parent.display()))?;
    std::fs::rename(legacy_dir, target_dir).with_context(|| format!("Failed to migrate persistent memory from {} to {}", legacy_dir.display(), target_dir.display()))?;
    remove_empty_legacy_memory_hierarchy(legacy_dir)?;
    Ok(())
}

fn legacy_persistent_memory_dir(project_name: &str) -> Result<Option<PathBuf>> {
    let Some(legacy_base) = get_config_dir() else { return Ok(None) };
    let current_base = dirs::home_dir().map(|home| home.join(".vtcode")).context("Could not resolve VT Code home directory")?;
    if legacy_base == current_base { return Ok(None) }
    Ok(Some(legacy_base.join("projects").join(sanitize_project_name(project_name)).join("memory")))
}

fn memory_directory_has_stored_content(directory: &Path) -> Result<bool> {
    if !directory.exists() { return Ok(false); }
    for path in [directory.join(PREFERENCES_FILENAME), directory.join(REPOSITORY_FACTS_FILENAME)] {
        if !path.exists() { continue; }
        let contents = std::fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
        if !parse_topic_file(&contents).is_empty() { return Ok(true); }
    }
    let rollout_dir = directory.join(ROLLOUT_SUMMARIES_DIRNAME);
    if !rollout_dir.exists() { return Ok(false); }
    for entry in std::fs::read_dir(&rollout_dir).with_context(|| format!("Failed to list {}", rollout_dir.display()))? {
        let path = entry?.path();
        if path.extension().and_then(|v| v.to_str()) != Some("md") { continue; }
        let contents = std::fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
        if !parse_topic_file(&contents).is_empty() { return Ok(true); }
    }
    Ok(false)
}

fn remove_empty_legacy_memory_hierarchy(legacy_memory_dir: &Path) -> Result<()> {
    let mut current = legacy_memory_dir.parent();
    for _ in 0..3 {
        let Some(path) = current else { break };
        match std::fs::remove_dir(path) {
            Ok(()) => current = path.parent(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => current = path.parent(),
            Err(err) if err.kind() == std::io::ErrorKind::DirectoryNotEmpty => break,
            Err(err) => return Err(err).with_context(|| format!("Failed to remove {}", path.display())),
        }
    }
    Ok(())
}

fn sanitize_project_name(project_name: &str) -> String {
    let sanitized: String = project_name.chars().map(|ch| match ch { '/' | '\\' | ':' => '_', other => other }).collect();
    let trimmed = sanitized.trim();
    if trimmed.is_empty() { "workspace".to_string() } else { trimmed.to_string() }
}

fn truncate_memory_excerpt(contents: &str, line_limit: usize, byte_limit: usize) -> (String, bool, usize, usize) {
    let all_lines = contents.lines().collect::<Vec<_>>();
    let mut selected = String::new();
    let mut bytes_read = 0usize;
    let mut lines_read = 0usize;
    let mut truncated = false;
    for (index, line) in all_lines.iter().enumerate() {
        if lines_read >= line_limit { truncated = true; break; }
        let line_bytes = line.len();
        let trailing_newline = usize::from(index + 1 < all_lines.len());
        if bytes_read + line_bytes + trailing_newline > byte_limit { truncated = true; break; }
        selected.push_str(line);
        selected.push('\n');
        bytes_read += line_bytes + trailing_newline;
        lines_read += 1;
    }
    if !truncated && contents.len() > bytes_read { truncated = true; }
    (selected.trim_end().to_string(), truncated, bytes_read, lines_read)
}

async fn read_existing_memory_lines(directory: &Path) -> Result<BTreeSet<String>> {
    let mut lines = BTreeSet::new();
    if !directory.exists() { return Ok(lines); }
    let mut stack = vec![directory.to_path_buf()];
    while let Some(next_dir) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&next_dir).await
            .with_context(|| format!("Failed to list {}", next_dir.display()))?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if entry.metadata().await?.is_dir() { stack.push(path); continue; }
            if path.extension().and_then(|v| v.to_str()) != Some("md") { continue; }
            let content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
            for line in content.lines() {
                if let Some((_, fact)) = parse_fact_line(line) {
                    lines.insert(normalize_whitespace(&fact).to_ascii_lowercase());
                }
            }
        }
    }
    Ok(lines)
}

const CLEANUP_REMEMBER_MARKERS: &[&str] = &["save to memory", "remember that", "remember my", "remember ", "add to memory", "store in memory"];
const CLEANUP_FORGET_MARKERS: &[&str] = &["forget ", "remove from memory", "delete from memory"];
const STRIP_PREFIXES: &[&str] = &["please ", "please, ", "can you ", "could you ", "would you ", "vt code, ", "vt code "];
const CLEANUP_NOTE_PREFIXES: &[&str] = &["note that ", "important:"];
const SELF_FACT_PREFIXES: &[&str] = &["my name is ", "i prefer ", "my preferred ", "my pronouns are ", "my timezone is "];

fn detect_memory_cleanup_status(files: &PersistentMemoryFiles) -> Result<MemoryCleanupStatus> {
    if !files.directory.exists() {
        return Ok(MemoryCleanupStatus { needed: false, suspicious_facts: 0, suspicious_summary_lines: 0 });
    }
    let mut suspicious_facts = 0usize;
    for path in [&files.preferences_file, &files.repository_facts_file, &files.memory_file] {
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
    if !path.exists() { return Ok(0); }
    let content = std::fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(parse_topic_file(&content).into_iter().filter(|f| is_legacy_polluted_fact(f)).count())
}

fn count_suspicious_rollout_facts(rollout_dir: &Path) -> Result<usize> {
    if !rollout_dir.exists() { return Ok(0); }
    let mut count = 0usize;
    for entry in std::fs::read_dir(rollout_dir).with_context(|| format!("Failed to list {}", rollout_dir.display()))? {
        let path = entry?.path();
        if path.extension().and_then(|v| v.to_str()) == Some("md") {
            count += count_suspicious_facts_in_file(&path)?;
        }
    }
    Ok(count)
}

fn count_suspicious_summary_lines(path: &Path) -> Result<usize> {
    if !path.exists() { return Ok(0); }
    let content = std::fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(content.lines().map(str::trim).filter(|l| l.starts_with("- ")).map(|l| l.trim_start_matches("- ").trim())
        .filter(|l| looks_like_legacy_prompt(l) || looks_like_serialized_payload(l)).count())
}

fn is_legacy_polluted_fact(fact: &GroundedFactRecord) -> bool {
    looks_like_legacy_prompt(&fact.fact) || looks_like_serialized_payload(&fact.fact)
}

fn looks_like_legacy_prompt(text: &str) -> bool {
    let mut lowered = normalize_whitespace(text).to_ascii_lowercase();
    while let Some(stripped) = STRIP_PREFIXES.iter().find_map(|p| lowered.strip_prefix(p)) {
        lowered = stripped.trim_start().to_string();
    }
    CLEANUP_REMEMBER_MARKERS.iter().chain(CLEANUP_FORGET_MARKERS.iter()).any(|m| lowered.starts_with(m))
}

fn looks_like_serialized_payload(text: &str) -> bool {
    let t = text.trim();
    t.starts_with('{') || t.starts_with('[') || t.contains("\"query\":") || t.contains("\"matches\":")
        || t.contains("\"path\":") || t.contains("</parameter>")
        || t.contains("</invoke>")
        || t.contains("<</invoke>")
}

fn normalize_memory_query(query: &str) -> Option<String> {
    let normalized = normalize_whitespace(query).to_ascii_lowercase();
    (!normalized.is_empty()).then_some(normalized)
}

async fn collect_memory_matches(files: &PersistentMemoryFiles, normalized_query: &str) -> Result<Vec<PersistentMemoryMatch>> {
    Ok(collect_all_memory_matches(files).await?.into_iter()
        .filter(|r| {
            let nf = normalize_whitespace(&r.fact).to_ascii_lowercase();
            let ns = normalize_whitespace(&r.source).to_ascii_lowercase();
            nf.contains(normalized_query) || ns.contains(normalized_query)
        }).collect())
}

async fn collect_all_memory_matches(files: &PersistentMemoryFiles) -> Result<Vec<PersistentMemoryMatch>> {
    let prefs = read_topic_records(&files.preferences_file, MemoryTopic::Preferences).await?;
    let repo = read_topic_records(&files.repository_facts_file, MemoryTopic::RepositoryFacts).await?;
    let rollout = read_rollout_records(&files.rollout_summaries_dir).await?;
    let notes = read_note_summaries(&files.notes_dir).await?;

    let mut matches = Vec::new();
    for r in prefs.into_iter().chain(repo).chain(rollout.0).chain(rollout.1) {
        let (_, src) = decode_topic_source(&r.source);
        matches.push(PersistentMemoryMatch { source: src, fact: r.fact });
    }
    for n in notes {
        for h in n.highlights {
            matches.push(PersistentMemoryMatch { source: n.relative_path.clone(), fact: h });
        }
    }

    let mut deduped = Vec::new();
    for r in matches {
        let nf = normalize_whitespace(&r.fact).to_ascii_lowercase();
        if let Some(i) = deduped.iter().position(|e: &PersistentMemoryMatch| normalize_whitespace(&e.fact).to_ascii_lowercase() == nf) {
            deduped.remove(i);
        }
        deduped.push(r);
    }
    Ok(deduped)
}

async fn collect_cleanup_candidates(files: &PersistentMemoryFiles) -> Result<Vec<GroundedFactRecord>> {
    let prefs = read_topic_records(&files.preferences_file, MemoryTopic::Preferences).await?;
    let repo = read_topic_records(&files.repository_facts_file, MemoryTopic::RepositoryFacts).await?;
    let rollout = read_rollout_records(&files.rollout_summaries_dir).await?;
    Ok(prefs.into_iter().chain(repo).chain(rollout.0).chain(rollout.1).collect())
}

async fn write_rollout_summary_pending(rollout_dir: &Path, classified: &ClassifiedFacts) -> Result<PathBuf> {
    tokio::fs::create_dir_all(rollout_dir).await.with_context(|| format!("Failed to create {}", rollout_dir.display()))?;
    let path = rollout_dir.join(format!("{}.pending.md", unique_rollout_id()));
    tokio::fs::write(&path, render_rollout_summary(classified)).await.with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(path)
}

fn finalize_rollout_summary_path(path: PathBuf) -> PathBuf {
    match path.file_name().and_then(|v| v.to_str()) {
        Some(name) => path.with_file_name(name.trim_end_matches(".pending.md").to_string() + ".md"),
        None => path,
    }
}

/// List `.md` files under `dir`, optionally filtering by a predicate on the file name.
fn list_md_files(dir: &Path, filter: impl Fn(&str) -> bool) -> Result<Vec<PathBuf>> {
    fn walk(dir: &Path, files: &mut Vec<PathBuf>, filter: &impl Fn(&str) -> bool) -> Result<()> {
        if !dir.exists() { return Ok(()); }
        for entry in std::fs::read_dir(dir).with_context(|| format!("Failed to list {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() { walk(&path, files, filter)?; }
            else if path.extension().and_then(|v| v.to_str()) == Some("md") && filter(path.file_name().and_then(|v| v.to_str()).unwrap_or("")) {
                files.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(dir, &mut files, &filter)?;
    files.sort();
    Ok(files)
}

async fn list_pending_rollout_files(rollout_dir: &Path) -> Result<Vec<PathBuf>> {
    Ok(list_md_files(rollout_dir, |n| n.ends_with(".pending.md"))?)
}

async fn list_rollout_markdown_files(rollout_dir: &Path) -> Result<Vec<PathBuf>> {
    Ok(list_md_files(rollout_dir, |_| true)?)
}

fn list_note_markdown_files(notes_dir: &Path) -> Result<Vec<PathBuf>> {
    list_md_files(notes_dir, |_| true)
}

fn count_pending_rollout_summaries(rollout_dir: &Path) -> Result<usize> {
    Ok(list_md_files(rollout_dir, |n| n.ends_with(".pending.md"))?.len())
}

async fn read_note_summaries(notes_dir: &Path) -> Result<Vec<MemoryNoteSummary>> {
    let mut notes = Vec::new();
    for path in list_note_markdown_files(notes_dir)? {
        let content = tokio::fs::read_to_string(&path).await.with_context(|| format!("Failed to read {}", path.display()))?;
        let relative = path.strip_prefix(notes_dir).with_context(|| format!("Failed to relativize {}", path.display()))?.to_string_lossy().replace('\\', "/");
        notes.push(MemoryNoteSummary { relative_path: format!("{NOTES_DIRNAME}/{relative}"), highlights: extract_memory_highlights(&content, 3) });
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

async fn consolidate_memory_files(runtime_config: Option<&RuntimeAgentConfig>, vt_cfg: Option<&VTCodeConfig>, workspace_root: &Path, files: &PersistentMemoryFiles) -> Result<ConsolidationResult> {
    let pending_files = list_pending_rollout_files(&files.rollout_summaries_dir).await?;
    let prefs_existing = read_topic_records(&files.preferences_file, MemoryTopic::Preferences).await?;
    let repo_existing = read_topic_records(&files.repository_facts_file, MemoryTopic::RepositoryFacts).await?;
    let rollout = read_rollout_records(&files.rollout_summaries_dir).await?;
    let classified = ClassifiedFacts {
        preferences: merge_topic_facts(prefs_existing.into_iter().chain(rollout.0).collect()),
        repository_facts: merge_topic_facts(repo_existing.into_iter().chain(rollout.1).collect()),
    };
    let created_files = write_classified_memory(&files, &classified, runtime_config, vt_cfg, workspace_root).await?;
    let added_facts = pending_files.iter().filter_map(|p| std::fs::read_to_string(p).ok()).flat_map(|c| c.lines().filter_map(parse_fact_line).collect::<Vec<_>>()).count();
    for pending in &pending_files {
        let finalized = finalize_rollout_summary_path(pending.clone());
        if !finalized.exists() {
            tokio::fs::rename(pending, &finalized).await.with_context(|| format!("Failed to finalize rollout summary {}", pending.display()))?;
        } else {
            tokio::fs::remove_file(pending).await.with_context(|| format!("Failed to remove {}", pending.display()))?;
        }
    }
    Ok(ConsolidationResult { created_files, added_facts })
}

async fn read_topic_records(path: &Path, topic: MemoryTopic) -> Result<Vec<GroundedFactRecord>> {
    if !path.exists() { return Ok(Vec::new()); }
    let contents = tokio::fs::read_to_string(path).await.with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(parse_topic_file(&contents).into_iter().map(|r| GroundedFactRecord { fact: r.fact, source: encode_topic_source(topic, &r.source) }).collect())
}

async fn read_rollout_records(rollout_dir: &Path) -> Result<(Vec<GroundedFactRecord>, Vec<GroundedFactRecord>)> {
    if !rollout_dir.exists() { return Ok((Vec::new(), Vec::new())); }
    let mut prefs = Vec::new();
    let mut repo_facts = Vec::new();
    let mut entries = tokio::fs::read_dir(rollout_dir).await
        .with_context(|| format!("Failed to list {}", rollout_dir.display()))?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("md") { continue; }
        let contents = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        for record in parse_topic_file(&contents) {
            let (topic, _) = decode_topic_source(&record.source);
            match topic.unwrap_or_else(|| classify_fact(&record)) {
                MemoryTopic::Preferences => prefs.push(record),
                MemoryTopic::RepositoryFacts => repo_facts.push(record),
            }
        }
    }
    Ok((prefs, repo_facts))
}

fn merge_topic_facts(records: Vec<GroundedFactRecord>) -> Vec<GroundedFactRecord> {
    let mut facts = Vec::new();
    for fact in records {
        let normalized = normalize_whitespace(&fact.fact).to_ascii_lowercase();
        if let Some(i) = facts.iter().position(|e: &GroundedFactRecord| normalize_whitespace(&e.fact).to_ascii_lowercase() == normalized) {
            facts.remove(i);
        }
        facts.push(fact);
    }
    let skip = facts.len().saturating_sub(TOPIC_FACT_LIMIT);
    facts.into_iter().skip(skip).collect()
}

fn normalized_selection_key(source: &str, fact: &str) -> String {
    format!("{}::{}", normalize_whitespace(source).to_ascii_lowercase(), normalize_whitespace(fact).to_ascii_lowercase())
}

fn selection_key_for_record(record: &GroundedFactRecord) -> String {
    let (_topic, source) = decode_topic_source(&record.source);
    normalized_selection_key(&source, &record.fact)
}

fn selection_keys(selected: &[MemoryOpCandidate]) -> BTreeSet<String> {
    selected.iter().map(|e| normalized_selection_key(&e.source, &e.fact)).collect()
}

async fn rewrite_topic_without_selected(path: &Path, topic: MemoryTopic, selected: &[MemoryOpCandidate]) -> Result<usize> {
    if !path.exists() { return Ok(0); }
    let keys = selection_keys(selected);
    let facts = read_topic_records(path, topic).await?;
    let removed = facts.iter().filter(|f| keys.contains(&selection_key_for_record(f))).count();
    if removed == 0 { return Ok(0); }
    let kept: Vec<_> = facts.into_iter().filter(|f| !keys.contains(&selection_key_for_record(f))).collect();
    tokio::fs::write(path, render_topic_file(topic, &kept)).await.with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(removed)
}

async fn scrub_rollout_file_by_selection(path: &Path, selected: &[MemoryOpCandidate]) -> Result<usize> {
    let contents = tokio::fs::read_to_string(path).await.with_context(|| format!("Failed to read {}", path.display()))?;
    let keys = selection_keys(selected);
    let mut removed = 0usize;
    let mut filtered = Vec::new();
    for line in contents.lines() {
        let keep = parse_fact_line(line).is_none_or(|(source, fact)| {
            let m = keys.contains(&selection_key_for_record(&GroundedFactRecord { source, fact }));
            if m { removed += 1; }
            !m
        });
        if keep { filtered.push(line); }
    }
    if removed == 0 { return Ok(0); }
    let mut rewritten = filtered.join("\n");
    if contents.ends_with('\n') { rewritten.push('\n'); }
    tokio::fs::write(path, rewritten).await.with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(removed)
}

async fn remove_rollout_markdown_files(rollout_dir: &Path) -> Result<usize> {
    let files = list_rollout_markdown_files(rollout_dir).await?;
    let count = files.len();
    for p in files {
        tokio::fs::remove_file(&p).await.with_context(|| format!("Failed to remove {}", p.display()))?;
    }
    Ok(count)
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
    if fact.source == "user_assertion" { MemoryTopic::Preferences } else { MemoryTopic::RepositoryFacts }
}

fn encode_topic_source(topic: MemoryTopic, source: &str) -> String { format!("{}:{}", topic.slug(), source) }

fn decode_topic_source(source: &str) -> (Option<MemoryTopic>, String) {
    match source.split_once(':') {
        Some((topic, rest)) => (MemoryTopic::from_slug(topic), rest.trim().to_string()),
        None => (None, source.to_string()),
    }
}

async fn classify_facts_strict(
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    workspace_root: &Path,
    candidates: &[GroundedFactRecord],
) -> Result<ClassifiedFacts> {
    if candidates.is_empty() {
        return Ok(ClassifiedFacts { preferences: Vec::new(), repository_facts: Vec::new() });
    }
    classify_facts_with_llm(runtime_config, vt_cfg, workspace_root, candidates).await
}

/// Try a memory LLM operation with primary route, falling back to the fallback route on error.
/// This macro expands to the full routing/fallback pattern used by all memory LLM calls.
macro_rules! try_with_memory_routes {
    ($runtime_config:expr, $vt_cfg:expr, $workspace_root:expr, $provider_fn:expr) => {
        async {
            let __rt_cfg: &RuntimeAgentConfig = $runtime_config;
            let __routes = resolve_memory_model_routes(__rt_cfg, $vt_cfg);
            log_memory_route_warning(&__routes);

            let __provider = create_memory_provider(&__routes.primary, __rt_cfg, $vt_cfg)?;
            match $provider_fn(__provider.as_ref(), &__routes.primary).await {
                Ok(result) => Ok(result),
                Err(__primary_err) => {
                    let Some(__fallback) = __routes.fallback.as_ref() else {
                        return Err(__primary_err);
                    };

                    tracing::warn!(
                        model = %__routes.primary.model,
                        fallback_model = %__fallback.model,
                        error = %__primary_err,
                        "persistent memory LLM call failed on lightweight route; retrying with main model"
                    );
                    let __provider = create_memory_provider(__fallback, __rt_cfg, $vt_cfg)?;
                    $provider_fn(__provider.as_ref(), __fallback).await
                }
            }
        }
    };
}

async fn classify_facts_with_llm(
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    workspace_root: &Path,
    candidates: &[GroundedFactRecord],
) -> Result<ClassifiedFacts> {
    let rt_cfg = runtime_config.ok_or_else(|| anyhow!("runtime config is required for persistent memory LLM routing"))?;
    try_with_memory_routes!(rt_cfg, vt_cfg, workspace_root, |provider, route| {
        classify_facts_with_provider(provider, route, workspace_root, candidates)
    }).await
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
    try_with_memory_routes!(runtime_config, vt_cfg, workspace_root, |provider, route| {
        summarize_memory_with_provider(
            provider,
            route,
            workspace_root,
            preferences,
            repository_facts,
            notes,
        )
    })
    .await
    .ok()
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
    try_with_memory_routes!(runtime_config, vt_cfg, workspace_root, |provider, route| {
        plan_memory_operation_with_provider(
            provider,
            route,
            workspace_root,
            expected_kind.clone(),
            request,
            supplemental_answer,
            candidates,
        )
    })
    .await
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

fn validate_memory_op_plan(plan: &MemoryOpPlan, expected_kind: MemoryOpKind, candidates: &[MemoryOpCandidate]) -> Result<()> {
    match plan.kind {
        MemoryOpKind::Remember => {
            if expected_kind != MemoryOpKind::Remember { bail!("memory planner returned remember for a non-remember request"); }
            if plan.facts.is_empty() { bail!("memory planner returned remember with no facts"); }
            if plan.facts.iter().any(|f| normalize_whitespace(&f.fact).is_empty()) { bail!("memory planner returned an empty fact"); }
        }
        MemoryOpKind::Forget => {
            if expected_kind != MemoryOpKind::Forget { bail!("memory planner returned forget for a non-forget request"); }
            let valid_ids: BTreeSet<_> = candidates.iter().map(|c| c.id).collect();
            if plan.selected_ids.iter().any(|id| !valid_ids.contains(id)) { bail!("memory planner selected an unknown memory candidate"); }
        }
        MemoryOpKind::AskMissing => {
            let m = plan.missing.as_ref().ok_or_else(|| anyhow!("memory planner returned ask_missing without a prompt"))?;
            if normalize_whitespace(&m.field).is_empty() || normalize_whitespace(&m.prompt).is_empty() {
                bail!("memory planner returned an incomplete missing-field request");
            }
        }
        MemoryOpKind::Noop => {}
    }
    if matches!(plan.kind, MemoryOpKind::AskMissing | MemoryOpKind::Noop) && (!plan.facts.is_empty() || !plan.selected_ids.is_empty()) {
        bail!("memory planner returned extra mutations for a non-mutating plan");
    }
    Ok(())
}

fn memory_plan_facts(plan: &MemoryOpPlan) -> Result<Vec<GroundedFactRecord>> {
    if plan.kind != MemoryOpKind::Remember { bail!("memory plan is not a remember operation"); }
    Ok(plan.facts.iter().map(|f| {
        let topic = match f.topic { MemoryPlannedTopic::Preferences => MemoryTopic::Preferences, MemoryPlannedTopic::RepositoryFacts => MemoryTopic::RepositoryFacts };
        let source = if f.source.trim().is_empty() { "manual_memory".to_string() } else { normalize_whitespace(&f.source) };
        GroundedFactRecord { fact: truncate_for_fact(&normalize_whitespace(&f.fact), 180), source: encode_topic_source(topic, &source) }
    }).filter(|f| !f.fact.is_empty()).collect())
}

fn selected_memory_candidates(candidates: &[MemoryOpCandidate], selected_ids: &[usize]) -> Result<Vec<MemoryOpCandidate>> {
    let selected: Vec<_> = selected_ids.iter().filter_map(|id| candidates.iter().find(|c| c.id == *id).cloned()).collect();
    if selected_ids.len() != selected.len() { bail!("memory plan selected a missing candidate"); }
    Ok(selected)
}

fn facts_for_prompt(facts: &[GroundedFactRecord]) -> String {
    if facts.is_empty() { return "- none".to_string(); }
    facts.iter().map(|f| { let (_, s) = decode_topic_source(&f.source); format!("- [{}] {}", s, f.fact) }).collect::<Vec<_>>().join("\n")
}

fn notes_for_prompt(notes: &[MemoryNoteSummary]) -> String {
    if notes.is_empty() { return "- none".to_string(); }
    notes.iter().map(|n| {
        let preview = if n.highlights.is_empty() { "no extracted highlights".to_string() } else { n.highlights.join("; ") };
        format!("- [{}] {}", n.relative_path, preview)
    }).collect::<Vec<_>>().join("\n")
}

fn resolve_memory_model_routes(runtime_config: &RuntimeAgentConfig, vt_cfg: Option<&VTCodeConfig>) -> ResolvedMemoryRoutes {
    let resolution = resolve_lightweight_route(runtime_config, vt_cfg, LightweightFeature::Memory, None);
    let primary = memory_model_route_from_resolution(&resolution.primary, runtime_config, vt_cfg);
    let fallback = resolution.fallback.as_ref().map(|r| memory_model_route_from_resolution(r, runtime_config, vt_cfg));
    ResolvedMemoryRoutes { primary, fallback, warning: resolution.warning }
}

fn memory_model_route_from_resolution(route: &crate::llm::ModelRoute, runtime_config: &RuntimeAgentConfig, vt_cfg: Option<&VTCodeConfig>) -> MemoryModelRoute {
    let temperature = if route.model == runtime_config.model && route.provider_name.eq_ignore_ascii_case(&runtime_provider_name(runtime_config)) {
        0.0
    } else {
        vt_cfg.map(|cfg| cfg.agent.small_model.temperature).unwrap_or(0.0)
    };
    MemoryModelRoute { provider_name: route.provider_name.clone(), model: route.model.clone(), temperature }
}

fn log_memory_route_warning(routes: &ResolvedMemoryRoutes) {
    if let Some(warning) = &routes.warning { tracing::warn!(warning = %warning, "persistent memory route adjusted"); }
}

fn create_memory_provider(route: &MemoryModelRoute, runtime_config: &RuntimeAgentConfig, vt_cfg: Option<&VTCodeConfig>) -> Result<Box<dyn LLMProvider>> {
    create_provider_for_model_route(&crate::llm::ModelRoute { provider_name: route.provider_name.clone(), model: route.model.clone() }, runtime_config, vt_cfg)
        .context("Failed to initialize persistent memory LLM provider")
}

fn runtime_provider_name(runtime_config: &RuntimeAgentConfig) -> String {
    if !runtime_config.provider.trim().is_empty() { return runtime_config.provider.to_lowercase(); }
    infer_provider_from_model(&runtime_config.model).map(|p| p.to_string().to_lowercase()).unwrap_or_else(|| "gemini".to_string())
}

fn render_topic_file(topic: MemoryTopic, facts: &[GroundedFactRecord]) -> String {
    let mut out = format!("# {}\n\n{}\n", topic.title(), topic.description());
    if facts.is_empty() {
        out.push_str("\n- No saved facts yet.\n");
    } else {
        out.push('\n');
        for f in facts {
            let (_, src) = decode_topic_source(&f.source);
            out.push_str(&format!("- [{}] {}\n", src.trim(), f.fact));
        }
    }
    out
}

fn render_memory_index(preferences: &[GroundedFactRecord], repository_facts: &[GroundedFactRecord], notes: &[MemoryNoteSummary], pending_rollouts: usize) -> String {
    let mut highlights: Vec<_> = preferences.iter().chain(repository_facts.iter()).cloned().collect();
    let skip = highlights.len().saturating_sub(MEMORY_HIGHLIGHT_LIMIT);
    highlights = highlights.into_iter().skip(skip).collect();
    let mut out = String::from("# VT Code Memory Registry\n\n## Files\n");
    out.push_str("- `memory_summary.md`: Startup-injected summary for future sessions.\n");
    out.push_str("- `preferences.md`: Durable user preferences and workflow notes.\n");
    out.push_str("- `repository-facts.md`: Grounded repository facts and recurring tooling notes.\n");
    out.push_str("- `notes/`: User-authored durable notes available to the native memory tool.\n");
    out.push_str("- `rollout_summaries/`: Per-session evidence summaries.\n");
    out.push_str(&format!("\n## Rollout Status\n- Pending rollout summaries: {pending_rollouts}\n"));
    out.push_str("\n## Highlights\n");
    if highlights.is_empty() {
        out.push_str("- No persistent notes yet.\n");
    } else {
        for f in &highlights {
            let (_, src) = decode_topic_source(&f.source);
            out.push_str(&format!("- [{}] {}\n", src.trim(), f.fact));
        }
    }
    if !notes.is_empty() {
        out.push_str("\n## Note Files\n");
        for n in notes {
            out.push_str(&format!("- `{}`", n.relative_path));
            if let Some(first) = n.highlights.first() { out.push_str(&format!(": {first}")); }
            out.push('\n');
        }
    }
    out
}

fn render_memory_summary(preferences: &[GroundedFactRecord], repository_facts: &[GroundedFactRecord], notes: &[MemoryNoteSummary]) -> String {
    let mut bullets: Vec<_> = preferences.iter().chain(repository_facts.iter()).map(|f| f.fact.clone()).collect();
    bullets.extend(notes.iter().filter_map(|n| n.highlights.first().map(|h| format!("Note ({}): {}", n.relative_path, h))));
    let skip = bullets.len().saturating_sub(MEMORY_HIGHLIGHT_LIMIT);
    bullets = bullets.into_iter().skip(skip).collect();
    if bullets.is_empty() { bullets.push("No durable memory notes have been consolidated yet.".to_string()); }
    render_memory_summary_bullets(&bullets)
}

fn render_memory_summary_bullets(bullets: &[String]) -> String {
    let mut out = String::from("# VT Code Memory Summary\n");
    for b in bullets { out.push_str(&format!("- {}\n", b.trim())); }
    out
}

fn render_rollout_summary(classified: &ClassifiedFacts) -> String {
    let mut out = format!("# Rollout Summary\n\n- Generated: {}\n", chrono::Utc::now().to_rfc3339());
    if classified.total() == 0 {
        out.push_str("\n- No durable facts captured.\n");
    } else {
        out.push('\n');
        for f in classified.preferences.iter().chain(&classified.repository_facts) {
            out.push_str(&format!("- [{}] {}\n", f.source, f.fact));
        }
    }
    out
}

fn unique_rollout_id() -> String {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    format!("rollout-{millis}")
}

struct MemoryLock { path: PathBuf }

impl MemoryLock {
    async fn acquire(path: &Path) -> Result<Self> {
        for _ in 0..LOCK_RETRY_ATTEMPTS {
            match tokio::fs::OpenOptions::new().create_new(true).write(true).open(path).await {
                Ok(_) => return Ok(Self { path: path.to_path_buf() }),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => sleep(Duration::from_millis(LOCK_RETRY_DELAY_MS)).await,
                Err(err) => return Err(err).with_context(|| format!("Failed to acquire {}", path.display())),
            }
        }
        Err(anyhow::anyhow!("Timed out waiting for persistent memory lock {}", path.display()))
    }
}

impl Drop for MemoryLock {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.path); }
}

#[cfg(test)]
mod persistent_memory_tests;
