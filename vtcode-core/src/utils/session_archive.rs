use crate::config::constants::{defaults, tools as tool_names};
use crate::llm::provider::{Message, MessageContent, MessageRole, ToolCall};
use crate::telemetry::perf::PerfSpan;
use crate::utils::dot_config::DotManager;
use crate::utils::error_log_collector::ErrorLogEntry;
use crate::utils::file_utils::{
    ensure_dir_exists, read_json_file, read_json_file_sync, write_json_file, write_json_file_sync,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SESSION_FILE_PREFIX: &str = "session";
const SESSION_FILE_EXTENSION: &str = "json";
pub const SESSION_DIR_ENV: &str = "VT_SESSION_DIR";
pub const SESSION_MAX_FILES_ENV: &str = "VT_SESSION_MAX_FILES";
pub const SESSION_MAX_AGE_DAYS_ENV: &str = "VT_SESSION_MAX_AGE_DAYS";
pub const SESSION_MAX_SIZE_MB_ENV: &str = "VT_SESSION_MAX_SIZE_MB";
const DEFAULT_SESSION_MAX_FILES: usize = 50;
const DEFAULT_SESSION_MAX_AGE_DAYS: u64 = 30;
const DEFAULT_SESSION_MAX_SIZE_MB: u64 = 100;
const BYTES_PER_MB: u64 = 1024 * 1024;
const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionArchiveMetadata {
    pub workspace_label: String,
    pub workspace_path: String,
    pub model: String,
    pub provider: String,
    pub theme: String,
    pub reasoning_effort: String,
    /// Names of skills loaded in this session
    #[serde(default)]
    pub loaded_skills: Vec<String>,
}

impl SessionArchiveMetadata {
    pub fn new(
        workspace_label: impl Into<String>,
        workspace_path: impl Into<String>,
        model: impl Into<String>,
        provider: impl Into<String>,
        theme: impl Into<String>,
        reasoning_effort: impl Into<String>,
    ) -> Self {
        Self {
            workspace_label: workspace_label.into(),
            workspace_path: workspace_path.into(),
            model: model.into(),
            provider: provider.into(),
            theme: theme.into(),
            reasoning_effort: reasoning_effort.into(),
            loaded_skills: Vec::new(),
        }
    }

    /// Set loaded skills for this session
    pub fn with_loaded_skills(mut self, skills: Vec<String>) -> Self {
        self.loaded_skills = skills;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionMessage {
    pub role: MessageRole,
    pub content: MessageContent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_details: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_tool: Option<String>,
}

impl Eq for SessionMessage {}

impl SessionMessage {
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: MessageContent::Text(content.into()),
            reasoning: None,
            reasoning_details: None,
            tool_calls: None,
            tool_call_id: None,
            origin_tool: None,
        }
    }

    pub fn with_content(role: MessageRole, content: MessageContent) -> Self {
        Self {
            role,
            content,
            reasoning: None,
            reasoning_details: None,
            tool_calls: None,
            tool_call_id: None,
            origin_tool: None,
        }
    }

    pub fn with_tool_call_id(
        role: MessageRole,
        content: impl Into<String>,
        tool_call_id: Option<String>,
    ) -> Self {
        Self::with_tool_call_id_content(role, MessageContent::Text(content.into()), tool_call_id)
    }

    pub fn with_tool_call_id_content(
        role: MessageRole,
        content: MessageContent,
        tool_call_id: Option<String>,
    ) -> Self {
        Self {
            role,
            content,
            reasoning: None,
            reasoning_details: None,
            tool_calls: None,
            tool_call_id,
            origin_tool: None,
        }
    }
}

impl From<&Message> for SessionMessage {
    fn from(message: &Message) -> Self {
        Self {
            role: message.role.clone(),
            content: message.content.clone(),
            reasoning: message.reasoning.clone(),
            reasoning_details: message.reasoning_details.clone(),
            tool_calls: message.tool_calls.clone(),
            tool_call_id: message.tool_call_id.clone(),
            origin_tool: message.origin_tool.clone(),
        }
    }
}

impl From<&SessionMessage> for Message {
    fn from(message: &SessionMessage) -> Self {
        Self {
            role: message.role.clone(),
            content: message.content.clone(),
            reasoning: message.reasoning.clone(),
            reasoning_details: message.reasoning_details.clone(),
            tool_calls: message.tool_calls.clone(),
            tool_call_id: message.tool_call_id.clone(),
            origin_tool: message.origin_tool.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub metadata: SessionArchiveMetadata,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub total_messages: usize,
    pub distinct_tools: Vec<String>,
    pub transcript: Vec<String>,
    #[serde(default)]
    pub messages: Vec<SessionMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<SessionProgress>,
    /// ERROR-level log entries captured during the session for post-mortem debugging.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub error_logs: Vec<ErrorLogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SessionProgress {
    pub turn_number: usize,
    #[serde(default)]
    pub recent_messages: Vec<SessionMessage>,
    #[serde(default)]
    pub tool_summaries: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_context_tokens: Option<usize>,
    /// Names of skills loaded at checkpoint time
    #[serde(default)]
    pub loaded_skills: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SessionListing {
    pub path: PathBuf,
    pub snapshot: SessionSnapshot,
}

#[derive(Debug, Clone)]
pub struct SessionProgressArgs {
    pub total_messages: usize,
    pub distinct_tools: Vec<String>,
    pub recent_messages: Vec<SessionMessage>,
    pub turn_number: usize,
    pub token_usage: Option<String>,
    pub max_context_tokens: Option<usize>,
    pub loaded_skills: Option<Vec<String>>,
}

impl SessionListing {
    pub fn identifier(&self) -> String {
        self.path
            .file_stem()
            .and_then(|value| value.to_str())
            .map(|value| value.to_string())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    pub fn first_prompt_preview(&self) -> Option<String> {
        self.preview_for_role(MessageRole::User)
    }

    pub fn first_reply_preview(&self) -> Option<String> {
        self.preview_for_role(MessageRole::Assistant)
    }

    fn preview_for_role(&self, role: MessageRole) -> Option<String> {
        self.snapshot.messages.iter().find_map(|message| {
            if message.role != role {
                return None;
            }

            let text_projection = message.content.as_text();
            if text_projection.trim().is_empty() {
                return None;
            }

            text_projection.lines().find_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(truncate_preview(trimmed, 80))
                }
            })
        })
    }
}

fn generate_unique_archive_path(
    sessions_dir: &Path,
    metadata: &SessionArchiveMetadata,
    started_at: DateTime<Utc>,
    custom_suffix: Option<&str>,
) -> PathBuf {
    let sanitized_label = sanitize_component(&metadata.workspace_label);
    let timestamp = started_at.format("%Y%m%dT%H%M%SZ").to_string();

    if let Some(suffix) = custom_suffix {
        // Custom suffix format: session-{label}-{timestamp}-{suffix}.json
        let file_name = format!(
            "{}-{}-{}-{}.{}",
            SESSION_FILE_PREFIX,
            sanitized_label,
            timestamp,
            sanitize_component(suffix),
            SESSION_FILE_EXTENSION
        );
        sessions_dir.join(file_name)
    } else {
        // Original format with collision detection: session-{label}-{timestamp}_{micros}-{pid}{-NN}.json
        let micros = started_at.timestamp_subsec_micros();
        let pid = process::id();
        let mut attempt = 0u32;

        loop {
            let suffix = if attempt == 0 {
                String::new()
            } else {
                format!("-{:02}", attempt)
            };
            let file_name = format!(
                "{}-{}-{}_{:06}-{:05}{}.{}",
                SESSION_FILE_PREFIX,
                sanitized_label,
                timestamp,
                micros,
                pid,
                suffix,
                SESSION_FILE_EXTENSION
            );
            let candidate = sessions_dir.join(file_name);
            if !candidate.exists() {
                return candidate;
            }
            attempt = attempt.wrapping_add(1);
        }
    }
}

fn progress_transcript_from_recent_messages(recent_messages: &[SessionMessage]) -> Vec<String> {
    let mut transcript = Vec::new();

    for message in recent_messages {
        if !matches!(message.role, MessageRole::User | MessageRole::Assistant) {
            continue;
        }

        let content = message.content.trim();
        let content: &str = content.as_ref();
        if !content.is_empty()
            && transcript
                .last()
                .is_none_or(|last: &String| last.as_str() != content)
        {
            transcript.push(content.to_string());
        }

        if matches!(message.role, MessageRole::Assistant)
            && let Some(reasoning) = message.reasoning.as_deref()
        {
            let reasoning = reasoning.trim();
            if !reasoning.is_empty()
                && reasoning != content
                && transcript
                    .last()
                    .is_none_or(|last: &String| last.as_str() != reasoning)
            {
                transcript.push(reasoning.to_string());
            }
        }
    }

    transcript
}

fn normalize_session_tool_name(name: &str) -> String {
    match name {
        n if n == tool_names::UNIFIED_EXEC
            || n == tool_names::SHELL
            || n == tool_names::EXEC_PTY_CMD =>
        {
            tool_names::RUN_PTY_CMD.to_string()
        }
        _ => name.to_string(),
    }
}

fn normalize_distinct_tools_for_summary(distinct_tools: &[String]) -> Vec<String> {
    let mut normalized = Vec::with_capacity(distinct_tools.len());
    let mut seen = std::collections::BTreeSet::new();

    for tool in distinct_tools {
        let mapped = normalize_session_tool_name(tool);
        if seen.insert(mapped.clone()) {
            normalized.push(mapped);
        }
    }

    normalized
}

#[derive(Debug, Clone)]
pub struct SessionArchive {
    path: PathBuf,
    metadata: SessionArchiveMetadata,
    started_at: DateTime<Utc>,
    progress_throttle: Arc<Mutex<ProgressThrottle>>,
}

#[derive(Debug)]
struct ProgressThrottle {
    last_written: Instant,
    last_turn: usize,
}

impl ProgressThrottle {
    fn new() -> Self {
        let min_interval =
            Duration::from_millis(defaults::DEFAULT_SESSION_PROGRESS_MIN_INTERVAL_MS);
        let last_written = Instant::now()
            .checked_sub(min_interval)
            .unwrap_or_else(Instant::now);
        Self {
            last_written,
            last_turn: 0,
        }
    }
}

impl SessionArchive {
    pub async fn new(
        metadata: SessionArchiveMetadata,
        custom_suffix: Option<String>,
    ) -> Result<Self> {
        let sessions_dir = resolve_sessions_dir().await?;
        apply_session_retention_best_effort(&sessions_dir);
        let started_at = Utc::now();
        let path = generate_unique_archive_path(
            &sessions_dir,
            &metadata,
            started_at,
            custom_suffix.as_deref(),
        );

        Ok(Self {
            path,
            metadata,
            started_at,
            progress_throttle: Arc::new(Mutex::new(ProgressThrottle::new())),
        })
    }

    pub fn finalize(
        &self,
        transcript: Vec<String>,
        total_messages: usize,
        distinct_tools: Vec<String>,
        messages: Vec<SessionMessage>,
    ) -> Result<PathBuf> {
        use crate::utils::error_log_collector::drain_error_logs;

        let distinct_tools = normalize_distinct_tools_for_summary(&distinct_tools);
        let snapshot = SessionSnapshot {
            metadata: self.metadata.clone(),
            started_at: self.started_at,
            ended_at: Utc::now(),
            total_messages,
            distinct_tools,
            transcript,
            messages,
            progress: None,
            error_logs: drain_error_logs(),
        };

        let path = self.write_snapshot(snapshot)?;
        if let Some(parent) = path.parent() {
            apply_session_retention_best_effort(parent);
        }
        Ok(path)
    }

    pub fn persist_progress(&self, args: SessionProgressArgs) -> Result<PathBuf> {
        use crate::utils::error_log_collector::drain_error_logs;

        let mut perf = PerfSpan::new("vtcode.perf.session_progress_write_ms");
        perf.tag("mode", "sync");

        let progress_transcript = progress_transcript_from_recent_messages(&args.recent_messages);
        let distinct_tools = normalize_distinct_tools_for_summary(&args.distinct_tools);
        let tool_summaries = distinct_tools.clone();
        let snapshot = SessionSnapshot {
            metadata: self.metadata.clone(),
            started_at: self.started_at,
            ended_at: Utc::now(),
            total_messages: args.total_messages,
            distinct_tools,
            transcript: progress_transcript,
            messages: args.recent_messages.clone(),
            progress: Some(SessionProgress {
                turn_number: args.turn_number,
                recent_messages: args.recent_messages,
                tool_summaries,
                token_usage: args.token_usage,
                max_context_tokens: args.max_context_tokens,
                loaded_skills: args.loaded_skills.unwrap_or_default(),
            }),
            error_logs: drain_error_logs(),
        };

        self.write_snapshot(snapshot)
    }

    pub async fn persist_progress_async(&self, args: SessionProgressArgs) -> Result<PathBuf> {
        use crate::utils::error_log_collector::drain_error_logs;

        let mut perf = PerfSpan::new("vtcode.perf.session_progress_write_ms");
        perf.tag("mode", "async");

        if !self.should_persist_progress(args.turn_number)? {
            return Ok(self.path.clone());
        }

        let progress_transcript = progress_transcript_from_recent_messages(&args.recent_messages);
        let distinct_tools = normalize_distinct_tools_for_summary(&args.distinct_tools);
        let tool_summaries = distinct_tools.clone();
        let snapshot = SessionSnapshot {
            metadata: self.metadata.clone(),
            started_at: self.started_at,
            ended_at: Utc::now(),
            total_messages: args.total_messages,
            distinct_tools,
            transcript: progress_transcript,
            messages: args.recent_messages.clone(),
            progress: Some(SessionProgress {
                turn_number: args.turn_number,
                recent_messages: args.recent_messages,
                tool_summaries,
                token_usage: args.token_usage,
                max_context_tokens: args.max_context_tokens,
                loaded_skills: args.loaded_skills.unwrap_or_default(),
            }),
            error_logs: drain_error_logs(),
        };

        self.write_snapshot_async(snapshot).await
    }

    fn write_snapshot(&self, snapshot: SessionSnapshot) -> Result<PathBuf> {
        write_json_file_sync(&self.path, &snapshot)?;
        Ok(self.path.clone())
    }

    async fn write_snapshot_async(&self, snapshot: SessionSnapshot) -> Result<PathBuf> {
        write_json_file(&self.path, &snapshot).await?;
        Ok(self.path.clone())
    }

    fn should_persist_progress(&self, turn_number: usize) -> Result<bool> {
        let min_interval =
            Duration::from_millis(defaults::DEFAULT_SESSION_PROGRESS_MIN_INTERVAL_MS);
        let min_turns = defaults::DEFAULT_SESSION_PROGRESS_MIN_TURN_DELTA;

        let mut throttle = self
            .progress_throttle
            .lock()
            .map_err(|err| anyhow::anyhow!("session progress throttle lock poisoned: {err}"))
            .context("Failed to evaluate session progress persistence throttle")?;
        if turn_number <= throttle.last_turn {
            return Ok(false);
        }
        if throttle.last_written.elapsed() < min_interval
            && turn_number.saturating_sub(throttle.last_turn) < min_turns
        {
            return Ok(false);
        }

        throttle.last_written = Instant::now();
        throttle.last_turn = turn_number;
        Ok(true)
    }
    /// Update loaded skills in the archive metadata
    pub fn set_loaded_skills(&mut self, skills: Vec<String>) {
        self.metadata.loaded_skills = skills;
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Create a forked session from an existing session snapshot
    ///
    /// This creates a new session archive that inherits metadata from the source
    /// session but operates independently. The forked session will have a new
    /// archive file with a custom suffix if provided.
    ///
    /// # Arguments
    /// * `source_snapshot` - The snapshot of the session to fork from
    /// * `custom_suffix` - Optional custom suffix for the new session ID
    ///
    /// # Returns
    /// A new SessionArchive instance for the forked session
    pub async fn fork(
        source_snapshot: &SessionSnapshot,
        custom_suffix: Option<String>,
    ) -> Result<Self> {
        let sessions_dir = resolve_sessions_dir().await?;
        apply_session_retention_best_effort(&sessions_dir);
        let started_at = Utc::now();

        // Preserve workspace metadata from source
        let forked_metadata = SessionArchiveMetadata {
            workspace_label: source_snapshot.metadata.workspace_label.clone(),
            workspace_path: source_snapshot.metadata.workspace_path.clone(),
            model: source_snapshot.metadata.model.clone(),
            provider: source_snapshot.metadata.provider.clone(),
            theme: source_snapshot.metadata.theme.clone(),
            reasoning_effort: source_snapshot.metadata.reasoning_effort.clone(),
            loaded_skills: source_snapshot.metadata.loaded_skills.clone(),
        };

        let path = generate_unique_archive_path(
            &sessions_dir,
            &forked_metadata,
            started_at,
            custom_suffix.as_deref(),
        );

        Ok(Self {
            path,
            metadata: forked_metadata,
            started_at,
            progress_throttle: Arc::new(Mutex::new(ProgressThrottle::new())),
        })
    }
}

pub async fn list_recent_sessions(limit: usize) -> Result<Vec<SessionListing>> {
    let sessions_dir = match resolve_sessions_dir().await {
        Ok(dir) => dir,
        Err(_) => return Ok(Vec::new()),
    };

    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    // Collect all session file paths first
    let mut session_paths = Vec::new();
    for entry in fs::read_dir(&sessions_dir).with_context(|| {
        format!(
            "failed to read session directory: {}",
            sessions_dir.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!("failed to read session entry in {}", sessions_dir.display())
        })?;
        let path = entry.path();
        if is_session_file(&path) {
            session_paths.push(path);
        }
    }

    // Process session files in parallel for better performance with large archives
    // Batch processing to avoid overwhelming the system with too many concurrent tasks
    const BATCH_SIZE: usize = 10;
    let mut all_listings = Vec::new();

    for batch in session_paths.chunks(BATCH_SIZE) {
        let mut tasks = Vec::with_capacity(batch.len());

        for path in batch {
            let path = path.clone();
            let task = tokio::task::spawn(async move {
                read_json_file::<SessionSnapshot>(&path)
                    .await
                    .ok()
                    .map(|snapshot| SessionListing { path, snapshot })
            });
            tasks.push(task);
        }

        // Collect results from this batch
        for task in tasks {
            if let Ok(Some(listing)) = task.await {
                all_listings.push(listing);
            }
        }
    }

    // Sort and limit results
    all_listings.sort_by(|a, b| b.snapshot.ended_at.cmp(&a.snapshot.ended_at));
    if limit > 0 && all_listings.len() > limit {
        all_listings.truncate(limit);
    }

    Ok(all_listings)
}

/// Find a session archive by its identifier (file stem) without needing to list all sessions.
pub async fn find_session_by_identifier(identifier: &str) -> Result<Option<SessionListing>> {
    let sessions_dir = match resolve_sessions_dir().await {
        Ok(dir) => dir,
        Err(_) => return Ok(None),
    };

    if !sessions_dir.exists() {
        return Ok(None);
    }

    for entry in fs::read_dir(&sessions_dir).with_context(|| {
        format!(
            "failed to read session directory: {}",
            sessions_dir.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!("failed to read session entry in {}", sessions_dir.display())
        })?;
        let path = entry.path();
        if !is_session_file(&path) {
            continue;
        }

        let stem_matches = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(|stem| stem == identifier)
            .unwrap_or(false);
        if !stem_matches {
            continue;
        }

        let snapshot: SessionSnapshot = read_json_file_sync(&path)?;
        return Ok(Some(SessionListing { path, snapshot }));
    }

    Ok(None)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub session_id: String,
    pub session_path: PathBuf,
    pub timestamp: DateTime<Utc>,
    pub message_index: usize,
    pub role: MessageRole,
    pub content_snippet: String,
    pub score: f32, // Simple matching score (e.g., term frequency or just 1.0)
}

/// Search for a query string across recent sessions.
/// Returns a list of `SearchResult` sorted by relevance (or recency).
pub async fn search_sessions(
    query: &str,
    session_limit: usize,
    max_results: usize,
) -> Result<Vec<SearchResult>> {
    let listings = list_recent_sessions(session_limit).await?;
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    // Naive search implementation
    for listing in listings {
        for (idx, msg) in listing.snapshot.messages.iter().enumerate() {
            let content = match &msg.content {
                MessageContent::Text(t) => t.as_str(),
                MessageContent::Parts(_) => continue, // Skip parts for simple text search for now
            };

            if let Some(pos) = content.to_lowercase().find(&query_lower) {
                // Create a snippet around the match
                let start = pos.saturating_sub(50);
                let end = (pos + query_lower.len() + 50).min(content.len());
                let snippet = format!("...{}...", &content[start..end].replace('\n', " "));

                results.push(SearchResult {
                    session_id: listing.identifier(),
                    session_path: listing.path.clone(),
                    timestamp: listing.snapshot.started_at, // Use session start time
                    message_index: idx,
                    role: msg.role.clone(),
                    content_snippet: snippet,
                    score: 1.0, // Baseline score
                });

                if results.len() >= max_results {
                    break;
                }
            }
        }
        if results.len() >= max_results {
            break;
        }
    }

    Ok(results)
}

async fn resolve_sessions_dir() -> Result<PathBuf> {
    if let Some(custom) = env::var_os(SESSION_DIR_ENV) {
        let path = PathBuf::from(custom);
        ensure_dir_exists(&path).await?;
        return Ok(path);
    }

    let manager = DotManager::new().context("failed to load VT Code dot manager")?;
    manager
        .initialize()
        .await
        .context("failed to initialize VT Code dot directory structure")?;
    let dir = manager.sessions_dir();
    ensure_dir_exists(&dir).await?;
    Ok(dir)
}

#[derive(Debug, Clone)]
struct SessionFileEntry {
    path: PathBuf,
    modified: SystemTime,
    size: u64,
}

#[derive(Debug, Clone, Copy)]
struct SessionRetentionLimits {
    max_files: usize,
    max_age_days: u64,
    max_total_size_bytes: u64,
}

impl Default for SessionRetentionLimits {
    fn default() -> Self {
        Self {
            max_files: DEFAULT_SESSION_MAX_FILES,
            max_age_days: DEFAULT_SESSION_MAX_AGE_DAYS,
            max_total_size_bytes: DEFAULT_SESSION_MAX_SIZE_MB.saturating_mul(BYTES_PER_MB),
        }
    }
}

fn parse_env_usize(key: &str) -> Option<usize> {
    env::var(key).ok()?.trim().parse::<usize>().ok()
}

fn parse_env_u64(key: &str) -> Option<u64> {
    env::var(key).ok()?.trim().parse::<u64>().ok()
}

fn session_retention_limits() -> SessionRetentionLimits {
    let defaults = SessionRetentionLimits::default();
    SessionRetentionLimits {
        max_files: parse_env_usize(SESSION_MAX_FILES_ENV).unwrap_or(defaults.max_files),
        max_age_days: parse_env_u64(SESSION_MAX_AGE_DAYS_ENV).unwrap_or(defaults.max_age_days),
        max_total_size_bytes: parse_env_u64(SESSION_MAX_SIZE_MB_ENV)
            .map(|value| value.saturating_mul(BYTES_PER_MB))
            .unwrap_or(defaults.max_total_size_bytes),
    }
}

fn apply_session_retention_best_effort(sessions_dir: &Path) {
    if let Err(err) = apply_session_retention(sessions_dir) {
        eprintln!(
            "Warning: failed to prune session archives in {}: {}",
            sessions_dir.display(),
            err
        );
    }
}

fn apply_session_retention(sessions_dir: &Path) -> Result<()> {
    apply_session_retention_with_limits(sessions_dir, session_retention_limits())
}

fn apply_session_retention_with_limits(
    sessions_dir: &Path,
    limits: SessionRetentionLimits,
) -> Result<()> {
    let mut entries = collect_session_entries(sessions_dir)?;

    if entries.is_empty() {
        return Ok(());
    }

    let now = SystemTime::now();
    let age_cutoff = if limits.max_age_days == 0 {
        now
    } else {
        now.checked_sub(Duration::from_secs(
            limits.max_age_days.saturating_mul(SECONDS_PER_DAY),
        ))
        .unwrap_or(UNIX_EPOCH)
    };

    let (expired, retained): (Vec<_>, Vec<_>) = entries
        .into_iter()
        .partition(|entry| entry.modified <= age_cutoff);
    remove_session_files(expired);
    entries = retained;

    entries.sort_by(|a, b| b.modified.cmp(&a.modified));

    if limits.max_files > 0 && entries.len() > limits.max_files {
        let overflow = entries.split_off(limits.max_files);
        remove_session_files(overflow);
    }

    if limits.max_total_size_bytes == 0 || entries.is_empty() {
        return Ok(());
    }

    let mut total_size = 0u64;
    let mut keep_entries = Vec::with_capacity(entries.len());
    let mut size_overflow = Vec::new();

    for entry in entries {
        let projected = total_size.saturating_add(entry.size);
        if keep_entries.is_empty() || projected <= limits.max_total_size_bytes {
            total_size = projected;
            keep_entries.push(entry);
        } else {
            size_overflow.push(entry);
        }
    }

    remove_session_files(size_overflow);
    Ok(())
}

fn collect_session_entries(sessions_dir: &Path) -> Result<Vec<SessionFileEntry>> {
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(sessions_dir).with_context(|| {
        format!(
            "failed to read session directory for retention: {}",
            sessions_dir.display()
        )
    })? {
        let entry = match entry {
            Ok(value) => value,
            Err(err) => {
                eprintln!(
                    "Warning: failed to read a session archive entry in {}: {}",
                    sessions_dir.display(),
                    err
                );
                continue;
            }
        };
        let path = entry.path();
        if !is_session_file(&path) {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(value) => value,
            Err(err) => {
                eprintln!(
                    "Warning: failed to read metadata for session archive {}: {}",
                    path.display(),
                    err
                );
                continue;
            }
        };
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        entries.push(SessionFileEntry {
            path,
            modified,
            size: metadata.len(),
        });
    }

    Ok(entries)
}

fn remove_session_files(entries: Vec<SessionFileEntry>) {
    for entry in entries {
        if let Err(err) = fs::remove_file(&entry.path) {
            eprintln!(
                "Warning: failed to remove session archive {}: {}",
                entry.path.display(),
                err
            );
        }
    }
}

fn truncate_preview(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_owned();
    }

    let mut truncated = String::new();
    for ch in input.chars().take(max_chars.saturating_sub(1)) {
        truncated.push(ch);
    }
    truncated.push('…');
    truncated
}

fn sanitize_component(value: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if matches!(ch, '-' | '_') {
            if !last_was_separator {
                normalized.push(ch);
                last_was_separator = true;
            }
        } else if !last_was_separator {
            normalized.push('-');
            last_was_separator = true;
        }
    }

    let trimmed = normalized.trim_matches(|c| c == '-' || c == '_');
    if trimmed.is_empty() {
        "workspace".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn is_session_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case(SESSION_FILE_EXTENSION))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::{ContentPart, ToolCall};
    use anyhow::anyhow;
    use chrono::{TimeZone, Timelike};
    use std::time::Duration;

    struct EnvGuard {
        key: &'static str,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            // SAFETY: Tests control the lifetime of this environment mutation
            // and provide UTF-8-compatible values derived from filesystem
            // paths, restoring the previous state via the guard's Drop impl.
            unsafe {
                env::set_var(key, value);
            }
            Self { key }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: The guard owns the only mutation to this variable during
            // the test, so removing it when the guard drops is sound.
            unsafe {
                env::remove_var(self.key);
            }
        }
    }

    #[tokio::test]
    async fn session_archive_persists_snapshot() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

        let metadata = SessionArchiveMetadata::new(
            "ExampleWorkspace",
            "/tmp/example",
            "model-x",
            "provider-y",
            "dark",
            "medium",
        );
        let archive = SessionArchive::new(metadata.clone(), None).await?;
        let transcript = vec!["line one".to_owned(), "line two".to_owned()];
        let messages = vec![
            SessionMessage::new(MessageRole::User, "Hello world"),
            SessionMessage::new(MessageRole::Assistant, "Hi there"),
        ];
        let path = archive.finalize(
            transcript.clone(),
            4,
            vec!["tool_a".to_owned()],
            messages.clone(),
        )?;

        let stored = fs::read_to_string(&path)
            .with_context(|| format!("failed to read stored session: {}", path.display()))?;
        let snapshot: SessionSnapshot =
            serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

        assert_eq!(snapshot.metadata, metadata);
        assert_eq!(snapshot.transcript, transcript);
        assert_eq!(snapshot.total_messages, 4);
        assert_eq!(snapshot.distinct_tools, vec!["tool_a".to_owned()]);
        assert_eq!(snapshot.messages, messages);
        Ok(())
    }

    #[test]
    fn session_message_converts_back_and_forth() {
        let _original = Message::assistant("Test response".to_owned());
        let mut original = Message::assistant("Test response".to_owned());
        original.reasoning = Some("Model thoughts".to_owned());
        original.reasoning = Some("Model thoughts".to_owned());
        let stored = SessionMessage::from(&original);
        let restored = Message::from(&stored);

        assert_eq!(original.role, restored.role);
        assert_eq!(original.content, restored.content);
        assert_eq!(original.reasoning, stored.reasoning);
        assert_eq!(original.reasoning, restored.reasoning);
        assert_eq!(original.tool_call_id, restored.tool_call_id);
    }

    #[test]
    fn session_message_preserves_tool_calls_reasoning_details_and_origin_tool() {
        let mut original = Message::assistant("Calling a tool".to_owned());
        original.reasoning_details = Some(vec![serde_json::json!({
            "summary": "tool call planning"
        })]);
        original.tool_calls = Some(vec![ToolCall::function(
            "call_1".to_string(),
            "unified_exec".to_string(),
            "{\"cmd\":\"cargo fmt\"}".to_string(),
        )]);
        original.origin_tool = Some("unified_exec".to_string());

        let stored = SessionMessage::from(&original);
        let restored = Message::from(&stored);

        assert_eq!(stored.reasoning_details, original.reasoning_details);
        assert_eq!(stored.tool_calls, original.tool_calls);
        assert_eq!(stored.origin_tool, original.origin_tool);
        assert_eq!(restored.reasoning_details, original.reasoning_details);
        assert_eq!(restored.tool_calls, original.tool_calls);
        assert_eq!(restored.origin_tool, original.origin_tool);
    }

    #[test]
    fn session_message_preserves_parts() {
        let original = Message::assistant_with_parts(vec![
            ContentPart::text("See attached image".to_owned()),
            ContentPart::text("See attached image".to_owned()),
            ContentPart::image("encoded-image".to_owned(), "image/png".to_owned()),
            ContentPart::image("encoded-image".to_owned(), "image/png".to_owned()),
            ContentPart::image("encoded-image".to_owned(), "image/png".to_owned()),
        ]);
        let stored = SessionMessage::from(&original);

        assert_eq!(stored.content, original.content);

        let restored = Message::from(&stored);
        assert_eq!(restored.content, original.content);
    }

    #[tokio::test]
    async fn session_progress_persists_budget_and_recent_messages() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

        let metadata = SessionArchiveMetadata::new(
            "ExampleWorkspace",
            "/tmp/example",
            "model-x",
            "provider-y",
            "dark",
            "medium",
        );
        let archive = SessionArchive::new(metadata, None).await?;
        let recent = vec![SessionMessage::new(MessageRole::Assistant, "recent")];

        let path = archive.persist_progress(SessionProgressArgs {
            total_messages: 1,
            distinct_tools: vec!["tool_a".to_owned()],
            recent_messages: recent.clone(),
            turn_number: 2,
            token_usage: Some("10 tokens".to_string()),
            max_context_tokens: Some(128),
            loaded_skills: None, // loaded_skills
        })?;

        let stored = fs::read_to_string(&path)
            .with_context(|| format!("failed to read stored session: {}", path.display()))?;
        let snapshot: SessionSnapshot =
            serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

        let progress = snapshot.progress.expect("progress should exist");
        assert_eq!(progress.turn_number, 2);
        assert_eq!(progress.recent_messages, recent);
        assert_eq!(progress.token_usage, Some("10 tokens".to_string()));
        assert_eq!(progress.tool_summaries, vec!["tool_a".to_string()]);
        assert_eq!(progress.max_context_tokens, Some(128));
        assert_eq!(snapshot.transcript, vec!["recent".to_string()]);
        Ok(())
    }

    #[tokio::test]
    async fn session_progress_transcript_skips_tool_noise_and_duplicates() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

        let metadata = SessionArchiveMetadata::new(
            "ExampleWorkspace",
            "/tmp/example",
            "model-x",
            "provider-y",
            "dark",
            "medium",
        );
        let archive = SessionArchive::new(metadata, None).await?;
        let mut assistant = SessionMessage::new(MessageRole::Assistant, "done");
        assistant.reasoning = Some("reasoned".to_string());
        let recent = vec![
            SessionMessage::new(MessageRole::User, "run cargo check"),
            SessionMessage::new(MessageRole::Tool, "{\"output\":\"...\"}"),
            SessionMessage::new(MessageRole::Assistant, "done"),
            assistant,
        ];

        let path = archive.persist_progress(SessionProgressArgs {
            total_messages: recent.len(),
            distinct_tools: vec!["unified_exec".to_owned()],
            recent_messages: recent,
            turn_number: 2,
            token_usage: Some("10 tokens".to_string()),
            max_context_tokens: Some(128),
            loaded_skills: None,
        })?;

        let stored = fs::read_to_string(&path)
            .with_context(|| format!("failed to read stored session: {}", path.display()))?;
        let snapshot: SessionSnapshot =
            serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

        assert_eq!(
            snapshot.transcript,
            vec![
                "run cargo check".to_string(),
                "done".to_string(),
                "reasoned".to_string()
            ]
        );
        Ok(())
    }

    #[tokio::test]
    async fn session_progress_normalizes_pty_tool_aliases_in_summaries() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

        let metadata = SessionArchiveMetadata::new(
            "ExampleWorkspace",
            "/tmp/example",
            "model-x",
            "provider-y",
            "dark",
            "medium",
        );
        let archive = SessionArchive::new(metadata, None).await?;
        let recent = vec![SessionMessage::new(MessageRole::Assistant, "done")];

        let path = archive.persist_progress(SessionProgressArgs {
            total_messages: 1,
            distinct_tools: vec![
                tool_names::UNIFIED_EXEC.to_string(),
                tool_names::RUN_PTY_CMD.to_string(),
                tool_names::SHELL.to_string(),
                tool_names::EXEC_PTY_CMD.to_string(),
            ],
            recent_messages: recent,
            turn_number: 2,
            token_usage: Some("10 tokens".to_string()),
            max_context_tokens: Some(128),
            loaded_skills: None,
        })?;

        let stored = fs::read_to_string(&path)
            .with_context(|| format!("failed to read stored session: {}", path.display()))?;
        let snapshot: SessionSnapshot =
            serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

        assert_eq!(
            snapshot.distinct_tools,
            vec![tool_names::RUN_PTY_CMD.to_string()]
        );
        let progress = snapshot.progress.expect("progress should exist");
        assert_eq!(
            progress.tool_summaries,
            vec![tool_names::RUN_PTY_CMD.to_string()]
        );
        Ok(())
    }

    #[tokio::test]
    async fn find_session_by_identifier_returns_match() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

        let metadata = SessionArchiveMetadata::new(
            "Sample",
            "/tmp/sample",
            "model-x",
            "provider-y",
            "dark",
            "medium",
        );
        let archive = SessionArchive::new(metadata.clone(), None).await?;
        let messages = vec![
            SessionMessage::new(MessageRole::User, "Hi"),
            SessionMessage::new(MessageRole::Assistant, "Hello"),
        ];
        let path = archive.finalize(Vec::new(), messages.len(), Vec::new(), messages)?;
        let identifier = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| anyhow!("missing file stem"))?
            .to_string();

        let listing = find_session_by_identifier(&identifier)
            .await?
            .ok_or_else(|| anyhow!("expected session to be found"))?;
        assert_eq!(listing.identifier(), identifier);
        assert_eq!(listing.snapshot.metadata, metadata);

        Ok(())
    }

    #[tokio::test]
    async fn session_archive_path_collision_adds_suffix() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

        let metadata = SessionArchiveMetadata::new(
            "ExampleWorkspace",
            "/tmp/example",
            "model-x",
            "provider-y",
            "dark",
            "medium",
        );

        let started_at = Utc
            .with_ymd_and_hms(2025, 9, 25, 10, 15, 30)
            .unwrap()
            .with_nanosecond(123_456_000)
            .unwrap();

        let first_path = generate_unique_archive_path(temp_dir.path(), &metadata, started_at, None);
        fs::write(&first_path, "{}").context("failed to create sentinel file")?;

        let second_path =
            generate_unique_archive_path(temp_dir.path(), &metadata, started_at, None);

        assert_ne!(first_path, second_path);
        let second_name = second_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("file name");
        assert!(second_name.contains("-01"));

        Ok(())
    }

    #[test]
    fn session_archive_filename_includes_microseconds_and_pid() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let metadata = SessionArchiveMetadata::new(
            "ExampleWorkspace",
            "/tmp/example",
            "model-x",
            "provider-y",
            "dark",
            "medium",
        );

        let started_at = Utc
            .with_ymd_and_hms(2025, 9, 25, 10, 15, 30)
            .unwrap()
            .with_nanosecond(654_321_000)
            .expect("nanosecond set");

        let path = generate_unique_archive_path(temp_dir.path(), &metadata, started_at, None);
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("file name string");

        assert!(name.contains("20250925T101530Z_654321"));
        let pid_fragment = format!("{:05}", process::id());
        assert!(name.contains(&pid_fragment));

        Ok(())
    }

    #[tokio::test]
    async fn list_recent_sessions_orders_entries() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

        let first_metadata = SessionArchiveMetadata::new(
            "First",
            "/tmp/first",
            "model-a",
            "provider-a",
            "light",
            "medium",
        );
        let first_archive = SessionArchive::new(first_metadata.clone(), None).await?;
        first_archive.finalize(
            vec!["first".to_owned()],
            1,
            Vec::new(),
            vec![SessionMessage::new(MessageRole::User, "First")],
        )?;

        tokio::time::sleep(Duration::from_millis(10)).await;

        let second_metadata = SessionArchiveMetadata::new(
            "Second",
            "/tmp/second",
            "model-b",
            "provider-b",
            "dark",
            "high",
        );
        let second_archive = SessionArchive::new(second_metadata.clone(), None).await?;
        second_archive.finalize(
            vec!["second".to_owned()],
            2,
            vec!["tool_b".to_owned()],
            vec![SessionMessage::new(MessageRole::User, "Second")],
        )?;

        let listings = list_recent_sessions(10).await?;
        assert_eq!(listings.len(), 2);
        assert_eq!(listings[0].snapshot.metadata, second_metadata);
        assert_eq!(listings[1].snapshot.metadata, first_metadata);
        Ok(())
    }

    #[test]
    fn session_archive_retention_prunes_oldest_by_count() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        for idx in 0..3 {
            let path = temp_dir.path().join(format!("session-{idx}.json"));
            fs::write(&path, format!("{{\"idx\":{idx}}}"))
                .with_context(|| format!("failed to write {}", path.display()))?;
            std::thread::sleep(Duration::from_millis(5));
        }

        apply_session_retention_with_limits(
            temp_dir.path(),
            SessionRetentionLimits {
                max_files: 2,
                max_age_days: 365,
                max_total_size_bytes: 10 * BYTES_PER_MB,
            },
        )?;

        let mut remaining = fs::read_dir(temp_dir.path())
            .context("failed to list retained session files")?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .filter_map(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.to_string())
            })
            .collect::<Vec<_>>();
        remaining.sort();

        assert_eq!(remaining.len(), 2);
        assert!(!remaining.iter().any(|name| name == "session-0.json"));
        Ok(())
    }

    #[test]
    fn session_archive_retention_prunes_by_total_size() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        for idx in 0..2 {
            let path = temp_dir.path().join(format!("session-{idx}.json"));
            fs::write(&path, "x".repeat(800_000))
                .with_context(|| format!("failed to write {}", path.display()))?;
            std::thread::sleep(Duration::from_millis(5));
        }

        apply_session_retention_with_limits(
            temp_dir.path(),
            SessionRetentionLimits {
                max_files: 10,
                max_age_days: 365,
                max_total_size_bytes: BYTES_PER_MB,
            },
        )?;

        let remaining = fs::read_dir(temp_dir.path())
            .context("failed to list retained session files")?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect::<Vec<_>>();

        assert_eq!(remaining.len(), 1);
        let remaining_name = remaining[0]
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        assert_eq!(remaining_name, "session-1.json");
        Ok(())
    }

    #[test]
    fn listing_previews_return_first_non_empty_lines() {
        let metadata = SessionArchiveMetadata::new(
            "Workspace",
            "/tmp/ws",
            "model",
            "provider",
            "dark",
            "medium",
        );
        let long_response = "response snippet ".repeat(6);
        let snapshot = SessionSnapshot {
            metadata,
            started_at: Utc::now(),
            ended_at: Utc::now(),
            total_messages: 2,
            distinct_tools: Vec::new(),
            transcript: Vec::new(),
            messages: vec![
                SessionMessage::new(MessageRole::System, ""),
                SessionMessage::new(MessageRole::User, "  prompt line\nsecond"),
                SessionMessage::new(MessageRole::Assistant, long_response.clone()),
            ],
            progress: None,
            error_logs: Vec::new(),
        };
        let listing = SessionListing {
            path: PathBuf::from("session-workspace.json"),
            snapshot,
        };

        assert_eq!(
            listing.first_prompt_preview(),
            Some("prompt line".to_owned())
        );
        let expected = super::truncate_preview(&long_response, 80);
        assert_eq!(listing.first_reply_preview(), Some(expected));
    }

    #[tokio::test]
    async fn search_sessions_finds_keyword() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

        let metadata = SessionArchiveMetadata::new("SearchWS", "/tmp/s", "mod", "prov", "d", "m");
        let archive = SessionArchive::new(metadata, None).await?;

        let messages = vec![
            SessionMessage::new(MessageRole::User, "Where is the secret API key?"),
            SessionMessage::new(
                MessageRole::Assistant,
                "The secret key is defined in .env.local",
            ),
        ];
        archive.finalize(vec![], 2, vec![], messages)?;

        // Search
        let results = search_sessions("secret key", 10, 5).await?;
        assert!(!results.is_empty());
        assert!(results[0].content_snippet.contains("secret key"));
        assert_eq!(results[0].role, MessageRole::Assistant);

        Ok(())
    }
}
