use anyhow::{Context, Result, anyhow, bail};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{Duration, sleep};

use crate::config::loader::VTCodeConfig;
use crate::config::types::AgentConfig as RuntimeAgentConfig;
use crate::config::{ConfigManager, PersistentMemoryConfig};
use crate::llm::factory::infer_provider_from_model;
use crate::llm::provider::{LLMProvider, LLMRequest, Message, MessageRole};
use crate::llm::{
    LightweightFeature, collect_single_response, create_provider_for_model_route, resolve_lightweight_route,
};
use vtcode_commons::VtCodePaths;

mod batch;
mod fact_extraction;
mod legacy_migration;
mod llm_ops;
mod lock;
mod reader;
mod rendering;

pub use batch::{BatchMemoryReport, batch_parameters_from_config, run_batch_memory_extraction};
pub use fact_extraction::{
    dedup_latest_facts, maybe_extract_tool_fact, maybe_extract_user_fact, normalize_whitespace, truncate_for_fact,
};
use legacy_migration::{
    migrate_legacy_persistent_memory_dir_if_needed, persistent_memory_base_dir, persistent_memory_project_name,
    sanitize_project_name,
};
use llm_ops::{classify_facts_strict, plan_memory_operation, summarize_memory};
use lock::MemoryLock;
use reader::{
    MemoryNoteSummary, classify_fact, collect_all_memory_matches, collect_cleanup_candidates, collect_memory_matches,
    count_pending_rollout_summaries, count_pending_rollout_summaries_async, decode_topic_source, encode_topic_source,
    list_pending_rollout_files_async, list_rollout_markdown_files, list_rollout_markdown_files_async,
    normalize_memory_query, parse_fact_line, parse_topic_file, read_note_summaries, read_rollout_records,
    read_topic_records,
};
use rendering::{
    render_memory_index, render_memory_summary, render_memory_summary_bullets, render_rollout_summary,
    render_topic_file, unique_rollout_id,
};

// Re-exported at crate-visible-but-restricted scope solely so that
// `persistent_memory_tests` (a descendant module of `persistent_memory`, see
// `#[cfg(test)] mod persistent_memory_tests;` below) can reach these
// otherwise-internal submodule items through its `use super::*;`. They are
// not referenced by any non-test code in this module, so the imports are
// gated behind `#[cfg(test)]` to avoid unused-import warnings in normal
// builds.
#[cfg(test)]
use legacy_migration::migrate_legacy_memory_dir;
#[cfg(test)]
use llm_ops::{
    MemoryModelRoute, MemoryPhase, classify_facts_with_provider, plan_memory_operation_with_provider,
    resolve_memory_model_routes, summarize_memory_with_provider,
};
#[cfg(test)]
use lock::{LOCK_STALE_AFTER_SECS, lock_age};

mod facts;
use facts::{
    CLEANUP_NOTE_PREFIXES, FactsInput, SELF_FACT_PREFIXES, STRIP_PREFIXES, consolidate_memory_files,
    detect_memory_cleanup_status, detect_memory_cleanup_status_async, ensure_memory_layout, is_regular_file,
    looks_like_legacy_prompt, memory_plan_facts, persist_memory_internal, remove_rollout_markdown_files,
    rewrite_topic_without_selected, scrub_rollout_file_by_selection, selected_memory_candidates,
    truncate_memory_excerpt,
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
    /// Rough token estimate of the returned excerpt (0 when no token budget
    /// is configured; whitespace-based approximation, not exact tokenization).
    pub tokens_estimated: usize,
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
        if normalized.is_empty() || highlights.iter().any(|e| e == normalized) {
            continue;
        }
        highlights.push(normalized.to_string());
        if highlights.len() >= limit {
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
#[repr(usize)]
enum MemoryTopic {
    Preferences = 0,
    RepositoryFacts = 1,
}

impl MemoryTopic {
    fn title(self) -> &'static str {
        ["Preferences", "Repository Facts"][self as usize]
    }

    fn description(self) -> &'static str {
        [
            "Durable user preferences and workflow notes.",
            "Grounded repository facts and recurring tooling notes.",
        ][self as usize]
    }

    fn slug(self) -> &'static str {
        ["preferences", "repository_facts"][self as usize]
    }

    fn from_slug(value: &str) -> Option<Self> {
        match value {
            "preferences" => Some(Self::Preferences),
            "repository_facts" => Some(Self::RepositoryFacts),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PersistentMemoryFiles {
    pub(super) directory: PathBuf,
    pub(super) private_global: bool,
    pub(super) summary_file: PathBuf,
    pub(super) memory_file: PathBuf,
    pub(super) preferences_file: PathBuf,
    pub(super) repository_facts_file: PathBuf,
    pub(super) notes_dir: PathBuf,
    pub(super) rollout_summaries_dir: PathBuf,
    pub(super) lock_file: PathBuf,
}

impl PersistentMemoryFiles {
    fn new(directory: PathBuf, private_global: bool) -> Self {
        Self {
            summary_file: directory.join(MEMORY_SUMMARY_FILENAME),
            memory_file: directory.join(MEMORY_FILENAME),
            preferences_file: directory.join(PREFERENCES_FILENAME),
            repository_facts_file: directory.join(REPOSITORY_FACTS_FILENAME),
            notes_dir: directory.join(NOTES_DIRNAME),
            rollout_summaries_dir: directory.join(ROLLOUT_SUMMARIES_DIRNAME),
            lock_file: directory.join(MEMORY_LOCK_FILENAME),
            directory,
            private_global,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ClassifiedFacts {
    pub(super) preferences: Vec<GroundedFactRecord>,
    pub(super) repository_facts: Vec<GroundedFactRecord>,
}

impl ClassifiedFacts {
    pub(super) fn total(&self) -> usize {
        self.preferences.len() + self.repository_facts.len()
    }
}

/// Resolves the persistent memory directory for a project.
///
/// **Blocking**: This function may perform filesystem I/O (directory migration).
/// Callers in async contexts must wrap this in `tokio::task::spawn_blocking`.
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

/// Returns the current persistent memory status.
///
/// **Blocking**: This function performs filesystem I/O (directory migration,
/// file existence checks, reading topic files). Callers in async contexts
/// must wrap this in `tokio::task::spawn_blocking`.
pub fn persistent_memory_status(
    config: &PersistentMemoryConfig,
    workspace_root: &Path,
) -> Result<PersistentMemoryStatus> {
    let directory = resolve_persistent_memory_dir(config, workspace_root)?
        .ok_or_else(|| anyhow!("persistent memory path policy returned no storage directory"))?;
    let files = PersistentMemoryFiles::new(directory, config.directory_override.is_none());
    let pending_rollout_summaries = count_pending_rollout_summaries(&files.rollout_summaries_dir)?;
    let cleanup_status = detect_memory_cleanup_status(&files)?;

    Ok(PersistentMemoryStatus {
        enabled: config.enabled,
        auto_write: config.auto_write,
        summary_exists: is_regular_file(&files.summary_file),
        registry_exists: is_regular_file(&files.memory_file),
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

    let config_clone = config.clone();
    let workspace_root = workspace_root.to_path_buf();
    let status = tokio::task::spawn_blocking(move || persistent_memory_status(&config_clone, &workspace_root))
        .await
        .context("Persistent memory status task panicked")??;
    if !tokio::fs::try_exists(&status.summary_file).await.unwrap_or(false) {
        return Ok(None);
    }

    let raw = String::from_utf8(vtcode_commons::fs::read_private_file_no_follow(&status.summary_file).await?)
        .with_context(|| format!("Failed to decode persistent memory summary {}", status.summary_file.display()))?;

    let (contents, truncated, bytes_read, lines_read) =
        truncate_memory_excerpt(&raw, config.startup_line_limit, config.startup_byte_limit);
    // Apply the token budget last (it sees the already line/byte-truncated
    // text) so the injected excerpt always fits the configured prompt budget.
    let (contents, truncated, tokens_estimated) = if config.startup_token_budget > 0 {
        let trimmed = crate::llm::utils::truncate_to_token_limit(&contents, config.startup_token_budget);
        let truncated = truncated || trimmed != contents;
        // Estimate AFTER truncation so the count describes the excerpt that is
        // actually injected, not the pre-truncation text.
        let tokens = trimmed.split_whitespace().count();
        (trimmed, truncated, tokens)
    } else {
        (contents, truncated, 0)
    };

    Ok(Some(PersistentMemoryExcerpt {
        status,
        contents,
        truncated,
        bytes_read,
        lines_read,
        tokens_estimated,
    }))
}

pub async fn read_persistent_memory_excerpt_for_config(
    vt_cfg: Option<&VTCodeConfig>,
    workspace_root: &Path,
) -> Result<Option<PersistentMemoryExcerpt>> {
    let config = effective_persistent_memory_config(vt_cfg);
    read_persistent_memory_excerpt(&config, workspace_root).await
}

pub async fn finalize_persistent_memory(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    history: &[Message],
    session_id: &str,
) -> Result<Option<PersistentMemoryWriteReport>> {
    let config = effective_generated_memory_config(vt_cfg);
    if !config.enabled || !config.auto_write {
        return Ok(None);
    }

    // Live producer for the per-session derived memory view: without this,
    // cross-session queries (`vtcode_memory::query_facts` / `search_memory`)
    // only ever observe legacy-migrated sessions. Cheap file I/O — run before
    // the LLM-backed global persist so it completes even when finalization is
    // cut short.
    write_session_derived_view(runtime_config.workspace.as_path(), session_id, history);

    let cfg_status = config.clone();
    let ws_status = runtime_config.workspace.clone();
    if tokio::task::spawn_blocking(move || persistent_memory_status(&cfg_status, ws_status.as_path()))
        .await
        .context("Persistent memory status task panicked")??
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

/// Build the per-session memory view: prefer the compaction envelope's
/// grounded facts when one was persisted, else re-run the lightweight
/// extraction over the session history.
fn session_memory_view(
    workspace_root: &Path,
    session_id: &str,
    history: &[Message],
) -> vtcode_memory::SessionMemoryView {
    if let Some(envelope) = crate::compaction::memory_envelope::load_latest_memory_envelope(workspace_root, session_id)
        && !envelope.grounded_facts.is_empty()
    {
        return vtcode_memory::SessionMemoryView {
            summary: envelope.summary,
            facts: envelope.grounded_facts.into_iter().map(|record| record.fact).collect(),
        };
    }
    vtcode_memory::SessionMemoryView {
        summary: String::new(),
        facts: dedup_latest_facts(history, DEFAULT_FACT_LIMIT)
            .into_iter()
            .map(|record| record.fact)
            .collect(),
    }
}

/// Best-effort write of the derived memory view into the session store.
fn write_session_derived_view(workspace_root: &Path, session_id: &str, history: &[Message]) {
    let view = session_memory_view(workspace_root, session_id, history);
    if let Err(error) = vtcode_memory::write_session_memory_view(workspace_root, session_id, &view) {
        tracing::warn!(target: "vtcode.memory", session_id, error = %error, "failed to persist derived session memory view");
    }
}

pub async fn rebuild_persistent_memory_summary(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<Option<PersistentMemoryWriteReport>> {
    let config = effective_persistent_memory_config(vt_cfg);
    if !config.enabled {
        return Ok(None);
    }
    let cfg_rb = config.clone();
    let ws_rb = runtime_config.workspace.clone();
    if tokio::task::spawn_blocking(move || persistent_memory_status(&cfg_rb, ws_rb.as_path()))
        .await
        .context("Persistent memory status task panicked")??
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

pub async fn rebuild_generated_memory_files(config: &PersistentMemoryConfig, workspace_root: &Path) -> Result<()> {
    let cfg = config.clone();
    let ws = workspace_root.to_path_buf();
    let directory = tokio::task::spawn_blocking(move || resolve_persistent_memory_dir(&cfg, &ws))
        .await
        .context("Persistent memory directory resolution task panicked")??
        .context("persistent memory directory should resolve")?;
    let files = PersistentMemoryFiles::new(directory, config.directory_override.is_none());
    let mut created_files = Vec::new();
    ensure_memory_layout(&files, &mut created_files).await?;
    let _lock = MemoryLock::acquire(&files.lock_file).await?;
    // Consolidation is best-effort; log errors but don't fail initialization
    if let Err(e) = consolidate_memory_files(None, None, workspace_root, &files).await {
        tracing::warn!("Failed to consolidate memory files during init: {}", e);
    }
    Ok(())
}

pub async fn scaffold_persistent_memory(
    config: &PersistentMemoryConfig,
    workspace_root: &Path,
) -> Result<Option<PersistentMemoryStatus>> {
    let cfg = config.clone();
    let ws = workspace_root.to_path_buf();
    let status = tokio::task::spawn_blocking(move || persistent_memory_status(&cfg, &ws))
        .await
        .context("Persistent memory status task panicked")??;
    let files = PersistentMemoryFiles::new(status.directory.clone(), config.directory_override.is_none());
    let mut created_files = Vec::new();
    ensure_memory_layout(&files, &mut created_files).await?;
    let cfg2 = config.clone();
    let ws2 = workspace_root.to_path_buf();
    let final_status = tokio::task::spawn_blocking(move || persistent_memory_status(&cfg2, &ws2))
        .await
        .context("Persistent memory status task panicked")??;
    Ok(Some(final_status))
}

/// Write classified facts to all memory files (topic files, index, summary).
async fn write_classified_memory(
    files: &PersistentMemoryFiles,
    classified: &ClassifiedFacts,
    notes: &[MemoryNoteSummary],
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    workspace_root: &Path,
) -> Result<Vec<PathBuf>> {
    let mut created_files = Vec::new();
    async fn write_memory_file(
        path: &Path,
        contents: String,
        created_files: &mut Vec<PathBuf>,
        private_global: bool,
    ) -> Result<()> {
        if !tokio::fs::try_exists(path).await.unwrap_or(false) {
            created_files.push(path.to_path_buf());
        }
        if private_global {
            vtcode_commons::fs::write_private_file_atomic(path, contents.as_bytes())
                .await
                .with_context(|| format!("Failed to write private memory file {}", path.display()))
        } else {
            tokio::fs::write(path, contents)
                .await
                .with_context(|| format!("Failed to write {}", path.display()))
        }
    }
    write_memory_file(
        &files.preferences_file,
        render_topic_file(MemoryTopic::Preferences, &classified.preferences),
        &mut created_files,
        files.private_global,
    )
    .await?;
    write_memory_file(
        &files.repository_facts_file,
        render_topic_file(MemoryTopic::RepositoryFacts, &classified.repository_facts),
        &mut created_files,
        files.private_global,
    )
    .await?;
    write_memory_file(
        &files.memory_file,
        render_memory_index(&classified.preferences, &classified.repository_facts, notes, 0),
        &mut created_files,
        files.private_global,
    )
    .await?;
    let summary = summarize_memory(
        runtime_config,
        vt_cfg,
        workspace_root,
        &classified.preferences,
        &classified.repository_facts,
        notes,
    )
    .await
    .unwrap_or_else(|| render_memory_summary(&classified.preferences, &classified.repository_facts, notes));
    write_memory_file(&files.summary_file, summary, &mut created_files, files.private_global).await?;
    Ok(created_files)
}

pub async fn cleanup_persistent_memory(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    include_summary_only_signals: bool,
) -> Result<Option<PersistentMemoryCleanupReport>> {
    let config = effective_persistent_memory_config(vt_cfg);
    if !config.enabled {
        return Ok(None);
    }

    let cfg_dir = config.clone();
    let ws_dir = runtime_config.workspace.clone();
    let directory = tokio::task::spawn_blocking(move || resolve_persistent_memory_dir(&cfg_dir, ws_dir.as_path()))
        .await
        .context("Persistent memory directory resolution task panicked")??
        .context("persistent memory directory should resolve when enabled")?;
    let files = PersistentMemoryFiles::new(directory, config.directory_override.is_none());
    let mut created_files = Vec::new();
    ensure_memory_layout(&files, &mut created_files).await?;

    let status = detect_memory_cleanup_status_async(&files).await?;
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
    let (candidates, notes) =
        tokio::try_join!(collect_cleanup_candidates(&files), read_note_summaries(&files.notes_dir))?;
    let classified = if candidates.is_empty() {
        ClassifiedFacts {
            preferences: Vec::new(),
            repository_facts: Vec::new(),
        }
    } else {
        classify_facts_strict(Some(runtime_config), vt_cfg, runtime_config.workspace.as_path(), &candidates).await?
    };

    let removed_rollout_files = remove_rollout_markdown_files(&files.rollout_summaries_dir).await?;
    // Write is critical; propagate errors
    write_classified_memory(
        &files,
        &classified,
        &notes,
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

    let cfg = config.clone();
    let ws = workspace_root.to_path_buf();
    let directory = tokio::task::spawn_blocking(move || resolve_persistent_memory_dir(&cfg, &ws))
        .await
        .context("Persistent memory directory resolution task panicked")??
        .context("persistent memory directory should resolve when enabled")?;
    if !tokio::fs::try_exists(&directory).await.unwrap_or(false) {
        return Ok(Some(Vec::new()));
    }

    let files = PersistentMemoryFiles::new(directory, config.directory_override.is_none());
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
    let cfg = config.clone();
    let ws = workspace_root.to_path_buf();
    let directory = tokio::task::spawn_blocking(move || resolve_persistent_memory_dir(&cfg, &ws))
        .await
        .context("Persistent memory directory resolution task panicked")??
        .context("persistent memory directory should resolve when enabled")?;
    if !tokio::fs::try_exists(&directory).await.unwrap_or(false) {
        return Ok(Some(Vec::new()));
    }

    let files = PersistentMemoryFiles::new(directory, config.directory_override.is_none());
    collect_memory_matches(&files, &normalized_query).await.map(Some)
}

/// Plan a user-approved persistent-memory save.
///
/// `prior_assistant_reply` is reference material for deictic requests such as
/// "remember it" and must contain only the immediately preceding assistant
/// reply. The caller remains responsible for limiting it to those requests and
/// for obtaining confirmation before calling [`persist_remembered_memory_plan`].
pub async fn plan_remember_persistent_memory(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    request: &str,
    supplemental_answer: Option<&str>,
    prior_assistant_reply: Option<&str>,
) -> Result<Option<MemoryOpPlan>> {
    let config = effective_persistent_memory_config(vt_cfg);
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
        prior_assistant_reply,
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
    let config = effective_persistent_memory_config(vt_cfg);
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
    let config = effective_persistent_memory_config(vt_cfg);
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
    let config = effective_persistent_memory_config(vt_cfg);
    if !config.enabled || plan.kind != MemoryOpKind::Forget {
        return Ok(None);
    }

    let selected = selected_memory_candidates(candidates, &plan.selected_ids)?;
    let cfg_dir = config.clone();
    let ws_dir = runtime_config.workspace.clone();
    let directory = tokio::task::spawn_blocking(move || resolve_persistent_memory_dir(&cfg_dir, ws_dir.as_path()))
        .await
        .context("Persistent memory directory resolution task panicked")??
        .context("persistent memory directory should resolve when enabled")?;
    let files = PersistentMemoryFiles::new(directory, config.directory_override.is_none());
    if !tokio::fs::try_exists(&files.directory).await.unwrap_or(false) {
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
        files.private_global,
    )
    .await?;
    removed_facts += rewrite_topic_without_selected(
        &files.repository_facts_file,
        MemoryTopic::RepositoryFacts,
        &selected,
        files.private_global,
    )
    .await?;

    let rollout_files = list_rollout_markdown_files_async(&files.rollout_summaries_dir).await?;
    for path in rollout_files {
        removed_facts += scrub_rollout_file_by_selection(&path, &selected, files.private_global).await?;
    }

    if removed_facts > 0 {
        // Consolidation is best-effort; log errors but don't fail the forget operation
        if let Err(e) =
            consolidate_memory_files(Some(runtime_config), vt_cfg, runtime_config.workspace.as_path(), &files).await
        {
            tracing::warn!("Failed to consolidate memory files after forget: {}", e);
        }
    }

    Ok(Some(PersistentMemoryForgetReport {
        directory: files.directory,
        summary_file: files.summary_file,
        memory_file: files.memory_file,
        removed_facts,
        pending_rollout_summaries: count_pending_rollout_summaries_async(&files.rollout_summaries_dir).await?,
    }))
}

fn effective_persistent_memory_config(vt_cfg: Option<&VTCodeConfig>) -> PersistentMemoryConfig {
    let mut config = vt_cfg.map(|cfg| cfg.agent.persistent_memory.clone()).unwrap_or_default();
    if let Some(cfg) = vt_cfg {
        config.enabled = cfg.persistent_memory_enabled();
    }
    config
}

fn effective_generated_memory_config(vt_cfg: Option<&VTCodeConfig>) -> PersistentMemoryConfig {
    let mut config = effective_persistent_memory_config(vt_cfg);
    if let Some(cfg) = vt_cfg {
        config.enabled = cfg.should_generate_memories();
    }
    config
}

#[cfg(test)]
mod persistent_memory_tests;
