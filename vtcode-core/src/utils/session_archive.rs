use crate::config::constants::defaults;
use crate::config::{HistoryPersistence, VTCodeConfig};
use crate::llm::provider::{AssistantPhase, Message, MessageContent, MessageRole, ToolCall};
use crate::telemetry::perf::PerfSpan;
use crate::utils::dot_config::DotManager;
use crate::utils::error_log_collector::ErrorLogEntry;
use crate::utils::file_utils::{
    ensure_dir_exists, read_json_file, read_json_file_sync, write_json_file, write_json_file_sync,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const SESSION_FILE_PREFIX: &str = "session";
const SESSION_FILE_EXTENSION: &str = "json";
pub const SESSION_DIR_ENV: &str = "VT_SESSION_DIR";
pub const SESSION_MAX_FILES_ENV: &str = "VT_SESSION_MAX_FILES";
pub const SESSION_MAX_AGE_DAYS_ENV: &str = "VT_SESSION_MAX_AGE_DAYS";
pub const SESSION_MAX_SIZE_MB_ENV: &str = "VT_SESSION_MAX_SIZE_MB";
const DEFAULT_SESSION_MAX_FILES: usize = 100;
const DEFAULT_SESSION_MAX_AGE_DAYS: u64 = 14;
const DEFAULT_SESSION_MAX_SIZE_MB: u64 = 100;
const BYTES_PER_MB: u64 = 1024 * 1024;
const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy)]
struct SessionHistorySettings {
    persistence: HistoryPersistence,
    max_bytes: Option<usize>,
}

impl Default for SessionHistorySettings {
    fn default() -> Self {
        Self {
            persistence: HistoryPersistence::File,
            max_bytes: None,
        }
    }
}

static SESSION_HISTORY_SETTINGS: OnceLock<Mutex<SessionHistorySettings>> = OnceLock::new();

fn session_history_settings() -> SessionHistorySettings {
    SESSION_HISTORY_SETTINGS
        .get()
        .and_then(|settings| settings.lock().ok().map(|guard| *guard))
        .unwrap_or_default()
}

pub fn apply_session_history_config_from_vtcode(config: &VTCodeConfig) {
    let settings = SessionHistorySettings {
        persistence: config.history.persistence,
        max_bytes: config.history.max_bytes,
    };
    let cell =
        SESSION_HISTORY_SETTINGS.get_or_init(|| Mutex::new(SessionHistorySettings::default()));
    if let Ok(mut guard) = cell.lock() {
        *guard = settings;
    }
}

pub fn history_persistence_enabled() -> bool {
    matches!(
        session_history_settings().persistence,
        HistoryPersistence::File
    )
}

#[cfg(test)]
mod test_env_overrides {
    use hashbrown::HashMap;
    use std::ffi::OsString;
    use std::sync::{LazyLock, Mutex};

    static OVERRIDES: LazyLock<Mutex<HashMap<String, Option<OsString>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    pub(super) fn get(key: &str) -> Option<Option<OsString>> {
        OVERRIDES.lock().ok().and_then(|map| map.get(key).cloned())
    }

    pub(super) fn set(key: &str, value: Option<OsString>) {
        if let Ok(mut map) = OVERRIDES.lock() {
            map.insert(key.to_owned(), value);
        }
    }

    pub(super) fn clear(key: &str) {
        if let Ok(mut map) = OVERRIDES.lock() {
            map.remove(key);
        }
    }
}

fn read_env_var_os(key: &str) -> Option<std::ffi::OsString> {
    #[cfg(test)]
    if let Some(override_value) = test_env_overrides::get(key) {
        return override_value;
    }

    env::var_os(key)
}

fn read_env_var(key: &str) -> Option<String> {
    #[cfg(test)]
    if let Some(override_value) = test_env_overrides::get(key) {
        return override_value.map(|value| value.to_string_lossy().to_string());
    }

    env::var(key).ok()
}

#[cfg(test)]
fn set_test_env_override_path(key: &str, value: &Path) {
    test_env_overrides::set(key, Some(value.as_os_str().to_os_string()));
}

#[cfg(test)]
fn clear_test_env_override(key: &str) {
    test_env_overrides::clear(key);
}

#[cfg(test)]
pub(crate) fn override_sessions_dir_for_tests(path: &Path) {
    set_test_env_override_path(SESSION_DIR_ENV, path);
}

#[cfg(test)]
pub(crate) fn clear_sessions_dir_override_for_tests() {
    clear_test_env_override(SESSION_DIR_ENV);
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionArchiveMetadata {
    pub workspace_label: String,
    pub workspace_path: String,
    pub model: String,
    pub provider: String,
    pub theme: String,
    pub reasoning_effort: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug_log_path: Option<String>,
    /// Names of skills loaded in this session
    #[serde(default)]
    pub loaded_skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_lineage_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_mode: Option<SessionForkMode>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionForkMode {
    FullCopy,
    Summarized,
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
            session_mode: None,
            debug_log_path: None,
            loaded_skills: Vec::new(),
            prompt_cache_lineage_id: None,
            external_thread_id: None,
            parent_session_id: None,
            fork_mode: None,
        }
    }

    /// Set loaded skills for this session
    pub fn with_loaded_skills(mut self, skills: Vec<String>) -> Self {
        self.loaded_skills = skills;
        self
    }

    /// Set debug log path associated with this archive.
    pub fn with_debug_log_path(mut self, path: Option<String>) -> Self {
        self.debug_log_path = path;
        self
    }

    pub fn with_prompt_cache_lineage_id(mut self, lineage_id: impl Into<String>) -> Self {
        self.prompt_cache_lineage_id = Some(lineage_id.into());
        self
    }

    pub fn with_external_thread_id(mut self, thread_id: impl Into<String>) -> Self {
        self.external_thread_id = Some(thread_id.into());
        self
    }

    pub fn ensure_prompt_cache_lineage_id(mut self) -> Self {
        if self.prompt_cache_lineage_id.is_none() {
            self.prompt_cache_lineage_id = Some(format!("lineage-{}", Uuid::new_v4()));
        }
        self
    }

    pub fn with_parent_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.parent_session_id = Some(session_id.into());
        self
    }

    pub fn with_fork_mode(mut self, fork_mode: SessionForkMode) -> Self {
        self.fork_mode = Some(fork_mode);
        self
    }

    fn fork_seed(&self) -> Self {
        Self {
            workspace_label: self.workspace_label.clone(),
            workspace_path: self.workspace_path.clone(),
            model: self.model.clone(),
            provider: self.provider.clone(),
            theme: self.theme.clone(),
            reasoning_effort: self.reasoning_effort.clone(),
            session_mode: self.session_mode.clone(),
            debug_log_path: self.debug_log_path.clone(),
            loaded_skills: self.loaded_skills.clone(),
            prompt_cache_lineage_id: self.prompt_cache_lineage_id.clone(),
            external_thread_id: self.external_thread_id.clone(),
            parent_session_id: None,
            fork_mode: None,
        }
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
    pub phase: Option<AssistantPhase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_tool: Option<String>,
}

impl Eq for SessionMessage {}

impl SessionMessage {
    fn base(role: MessageRole, content: MessageContent) -> Self {
        Self {
            role,
            content,
            reasoning: None,
            reasoning_details: None,
            tool_calls: None,
            tool_call_id: None,
            phase: None,
            origin_tool: None,
        }
    }

    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self::base(role, MessageContent::Text(content.into()))
    }

    pub fn with_content(role: MessageRole, content: MessageContent) -> Self {
        Self::base(role, content)
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
        let mut message = Self::base(role, content);
        message.tool_call_id = tool_call_id;
        message
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
            phase: message.phase,
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
            phase: message.phase,
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

fn normalize_workspace_for_match(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };

    crate::utils::path::normalize_path(&absolute)
}

pub fn session_workspace_path(listing: &SessionListing) -> Option<PathBuf> {
    let raw = listing.snapshot.metadata.workspace_path.trim();
    if raw.is_empty() {
        None
    } else {
        Some(PathBuf::from(raw))
    }
}

pub fn session_listing_matches_workspace(listing: &SessionListing, workspace: &Path) -> bool {
    let Some(session_workspace) = session_workspace_path(listing) else {
        return false;
    };

    normalize_workspace_for_match(&session_workspace) == normalize_workspace_for_match(workspace)
}

fn generate_unique_archive_path(
    sessions_dir: &Path,
    metadata: &SessionArchiveMetadata,
    started_at: DateTime<Utc>,
    custom_suffix: Option<&str>,
) -> PathBuf {
    generate_unique_archive_path_for_label(
        sessions_dir,
        &metadata.workspace_label,
        started_at,
        custom_suffix,
    )
}

fn generate_unique_archive_path_for_label(
    sessions_dir: &Path,
    workspace_label: &str,
    started_at: DateTime<Utc>,
    custom_suffix: Option<&str>,
) -> PathBuf {
    if custom_suffix.is_some() {
        return sessions_dir.join(archive_file_name_for_label(
            workspace_label,
            started_at,
            custom_suffix,
            None,
        ));
    }

    let mut attempt = 0u32;
    loop {
        let candidate = sessions_dir.join(archive_file_name_for_label(
            workspace_label,
            started_at,
            None,
            Some(attempt),
        ));
        if !candidate.exists() {
            return candidate;
        }
        attempt = attempt.wrapping_add(1);
    }
}

fn archive_file_name_for_label(
    workspace_label: &str,
    started_at: DateTime<Utc>,
    custom_suffix: Option<&str>,
    attempt: Option<u32>,
) -> String {
    let sanitized_label = sanitize_component(workspace_label);
    let timestamp = started_at.format("%Y%m%dT%H%M%SZ").to_string();

    if let Some(suffix) = custom_suffix {
        return format!(
            "{}-{}-{}-{}.{}",
            SESSION_FILE_PREFIX,
            sanitized_label,
            timestamp,
            sanitize_component(suffix),
            SESSION_FILE_EXTENSION
        );
    }

    let micros = started_at.timestamp_subsec_micros();
    let pid = process::id();
    let attempt_suffix = match attempt.unwrap_or_default() {
        0 => String::new(),
        value => format!("-{:02}", value),
    };
    format!(
        "{}-{}-{}_{:06}-{:05}{}.{}",
        SESSION_FILE_PREFIX,
        sanitized_label,
        timestamp,
        micros,
        pid,
        attempt_suffix,
        SESSION_FILE_EXTENSION
    )
}

pub fn generate_session_archive_identifier(
    workspace_label: &str,
    custom_suffix: Option<String>,
) -> String {
    let file_name = archive_file_name_for_label(
        workspace_label,
        Utc::now(),
        custom_suffix.as_deref(),
        Some(0),
    );
    Path::new(&file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "session-{}-{}",
                sanitize_component(workspace_label),
                process::id()
            )
        })
}

fn session_identifier_from_archive_path(path: &Path) -> Result<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow::anyhow!("failed to derive session identifier from archive path"))
}

fn is_valid_session_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

fn validate_session_identifier(session_identifier: &str) -> Result<()> {
    if is_valid_session_identifier(session_identifier) {
        return Ok(());
    }

    Err(anyhow::anyhow!(
        "Invalid session identifier '{}': only ASCII letters, digits, '-' and '_' are allowed",
        session_identifier
    ))
}

fn session_archive_path_for_identifier(
    sessions_dir: &Path,
    session_identifier: &str,
) -> Result<PathBuf> {
    validate_session_identifier(session_identifier)?;
    Ok(sessions_dir.join(format!("{}.{}", session_identifier, SESSION_FILE_EXTENSION)))
}

fn reserve_new_session_archive_path(
    sessions_dir: &Path,
    session_identifier: &str,
) -> Result<PathBuf> {
    let path = session_archive_path_for_identifier(sessions_dir, session_identifier)?;
    if path.exists() {
        return Err(anyhow::anyhow!(
            "Session archive identifier '{}' already exists",
            session_identifier
        ));
    }

    Ok(path)
}

async fn resolve_sessions_dir_for_archive_writes() -> Result<PathBuf> {
    let sessions_dir = resolve_sessions_dir().await?;
    apply_session_retention_best_effort(&sessions_dir);
    Ok(sessions_dir)
}

/// Reserve a unique session archive identifier for the current process.
///
/// The returned identifier is the JSON file stem (without `.json`) and can be reused
/// to create an archive and pair external artifacts (for example debug logs).
pub async fn reserve_session_archive_identifier(
    workspace_label: &str,
    custom_suffix: Option<String>,
) -> Result<String> {
    let sessions_dir = resolve_sessions_dir_for_archive_writes().await?;
    let started_at = Utc::now();
    let path = generate_unique_archive_path_for_label(
        &sessions_dir,
        workspace_label,
        started_at,
        custom_suffix.as_deref(),
    );
    session_identifier_from_archive_path(&path)
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
    }

    clean_transcript_lines(&transcript)
}

fn clean_transcript_lines(lines: &[String]) -> Vec<String> {
    let mut cleaned = Vec::new();
    let mut seen_tool_blocks: HashMap<String, (usize, usize, String)> = HashMap::new();
    let mut index = 0usize;

    while index < lines.len() {
        let line = lines[index].trim_end();

        if should_reset_tool_dedupe_scope(line) {
            seen_tool_blocks.clear();
        }

        if let Some(replacement) = normalize_recovery_line(line) {
            push_clean_transcript_line(&mut cleaned, replacement);
            index += 1;
            continue;
        }

        if should_drop_transcript_line(line) {
            index += 1;
            continue;
        }

        if line.trim_start().starts_with("• ") {
            let (summary, next_index) = summarize_tool_block(lines, index);
            let signature = normalized_transcript_key(&summary);

            if let Some((first_index, repeats, original_line)) =
                seen_tool_blocks.get_mut(&signature)
            {
                *repeats += 1;
                if let Some(existing) = cleaned.get_mut(*first_index) {
                    *existing = format_repeated_summary(original_line, *repeats);
                }
            } else {
                let insertion_index = cleaned.len();
                push_clean_transcript_line(&mut cleaned, summary);
                if cleaned.len() > insertion_index {
                    seen_tool_blocks.insert(
                        signature,
                        (insertion_index, 1, cleaned[insertion_index].clone()),
                    );
                }
            }
            index = next_index;
            continue;
        }

        push_clean_transcript_line(&mut cleaned, line.to_string());
        index += 1;
    }

    while cleaned.last().is_some_and(|line: &String| line.is_empty()) {
        cleaned.pop();
    }

    cleaned
}

fn should_reset_tool_dedupe_scope(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty()
        && !line.starts_with(' ')
        && !trimmed.starts_with("• ")
        && !trimmed.starts_with("[!]")
}

fn should_drop_transcript_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("Latest tool output:")
        || trimmed.starts_with("Latest user request:")
        || trimmed.starts_with("Tool output 1:")
        || trimmed.starts_with("Structured result with fields:")
        || trimmed.starts_with("Reuse the latest tool outputs already collected in this turn")
        || trimmed.starts_with("Interrupt received. Stopping task...")
}

fn normalize_recovery_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with("[!] Turn balancer:")
        || trimmed.starts_with("[!] Navigation Loop:")
        || trimmed.starts_with("[!] Navigation loop:")
    {
        return Some("Repeated low-signal tool churn triggered recovery.".to_string());
    }

    if trimmed.contains("I couldn't produce a final synthesis because the model returned no answer on the recovery pass.")
    {
        return Some("Recovery pass failed to produce a final synthesis.".to_string());
    }

    None
}

fn summarize_tool_block(lines: &[String], start: usize) -> (String, usize) {
    let header = lines[start].trim().to_string();
    let mut command_continuations = Vec::new();
    let mut metadata = Vec::new();
    let mut metadata_seen = HashSet::new();
    let mut index = start + 1;

    while index < lines.len() {
        let raw = lines[index].trim_end();
        let trimmed = raw.trim_start();
        if trimmed.starts_with("• ") || trimmed.starts_with("[!]") || !is_tool_detail_line(trimmed)
        {
            break;
        }

        if let Some(continuation) = trimmed.strip_prefix("│ ") {
            let continuation = continuation.trim();
            if !continuation.is_empty() {
                command_continuations.push(continuation.to_string());
            }
        } else if let Some(extra) = summarize_tool_detail(trimmed)
            && metadata_seen.insert(extra.clone())
        {
            metadata.push(extra);
        }

        index += 1;
    }

    let mut summary = header;
    if !command_continuations.is_empty() {
        summary.push(' ');
        summary.push_str(&command_continuations.join(" "));
    }
    if !metadata.is_empty() {
        summary.push_str(" [");
        summary.push_str(&metadata.join(", "));
        summary.push(']');
    }

    (collapse_whitespace(&summary), index)
}

fn is_tool_detail_line(line: &str) -> bool {
    line.starts_with("│ ")
        || line.starts_with("└ ")
        || line.starts_with("✓ ")
        || line.starts_with("✗ ")
        || line.starts_with("… +")
        || line.starts_with("Large output was spooled")
        || line == "(no output)"
}

fn summarize_tool_detail(line: &str) -> Option<String> {
    let path = line
        .strip_prefix("└ Path:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("path {value}"));
    if path.is_some() {
        return path;
    }

    let pattern = line
        .strip_prefix("└ Pattern:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("pattern {value}"));
    if pattern.is_some() {
        return pattern;
    }

    let filter = line
        .strip_prefix("└ Filter:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("filter {value}"));
    if filter.is_some() {
        return filter;
    }

    let glob = line
        .strip_prefix("└ Glob:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("glob {value}"));
    if glob.is_some() {
        return glob;
    }

    if let Some(status) = line.strip_prefix("✗ ") {
        let status = status.trim();
        if !status.is_empty() {
            return Some(status.to_string());
        }
    }

    None
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalized_transcript_key(text: &str) -> String {
    collapse_whitespace(text).to_ascii_lowercase()
}

fn format_repeated_summary(line: &str, repeats: usize) -> String {
    if repeats <= 1 {
        return line.to_string();
    }
    format!("{line} (repeated x{repeats})")
}

fn push_clean_transcript_line(target: &mut Vec<String>, line: String) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        if target.last().is_none_or(|last| !last.is_empty()) {
            target.push(String::new());
        }
        return;
    }

    if target
        .last()
        .is_some_and(|last| normalized_transcript_key(last) == normalized_transcript_key(trimmed))
    {
        return;
    }

    target.push(line);
}

fn normalize_session_tool_name(name: &str) -> String {
    crate::tools::tool_intent::canonical_unified_exec_tool_name(name)
        .unwrap_or(name)
        .to_string()
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
    fn from_path(
        path: PathBuf,
        metadata: SessionArchiveMetadata,
        started_at: DateTime<Utc>,
    ) -> Self {
        Self {
            path,
            metadata,
            started_at,
            progress_throttle: Arc::new(Mutex::new(ProgressThrottle::new())),
        }
    }

    fn build_snapshot(
        &self,
        total_messages: usize,
        distinct_tools: Vec<String>,
        transcript: Vec<String>,
        messages: Vec<SessionMessage>,
        progress: Option<SessionProgress>,
    ) -> SessionSnapshot {
        use crate::utils::error_log_collector::drain_error_logs;

        SessionSnapshot {
            metadata: self.metadata.clone(),
            started_at: self.started_at,
            ended_at: Utc::now(),
            total_messages,
            distinct_tools,
            transcript,
            messages,
            progress,
            error_logs: drain_error_logs(),
        }
    }

    fn build_final_snapshot(
        &self,
        transcript: Vec<String>,
        total_messages: usize,
        distinct_tools: Vec<String>,
        messages: Vec<SessionMessage>,
    ) -> SessionSnapshot {
        self.build_snapshot(
            total_messages,
            normalize_distinct_tools_for_summary(&distinct_tools),
            clean_transcript_lines(&transcript),
            messages,
            None,
        )
    }

    fn build_progress_snapshot(&self, args: SessionProgressArgs) -> SessionSnapshot {
        let SessionProgressArgs {
            total_messages,
            distinct_tools,
            recent_messages,
            turn_number,
            token_usage,
            max_context_tokens,
            loaded_skills,
        } = args;

        let transcript = progress_transcript_from_recent_messages(&recent_messages);
        let distinct_tools = normalize_distinct_tools_for_summary(&distinct_tools);
        let tool_summaries = distinct_tools.clone();
        let messages = recent_messages.clone();

        self.build_snapshot(
            total_messages,
            distinct_tools,
            transcript,
            messages,
            Some(SessionProgress {
                turn_number,
                recent_messages,
                tool_summaries,
                token_usage,
                max_context_tokens,
                loaded_skills: loaded_skills.unwrap_or_default(),
            }),
        )
    }

    pub async fn new(
        metadata: SessionArchiveMetadata,
        custom_suffix: Option<String>,
    ) -> Result<Self> {
        let sessions_dir = resolve_sessions_dir_for_archive_writes().await?;
        let started_at = Utc::now();
        let path = generate_unique_archive_path(
            &sessions_dir,
            &metadata,
            started_at,
            custom_suffix.as_deref(),
        );

        Ok(Self::from_path(path, metadata, started_at))
    }

    /// Create a session archive using an explicitly reserved session identifier.
    pub async fn new_with_identifier(
        metadata: SessionArchiveMetadata,
        session_identifier: String,
    ) -> Result<Self> {
        let sessions_dir = resolve_sessions_dir_for_archive_writes().await?;
        let path = reserve_new_session_archive_path(&sessions_dir, &session_identifier)?;

        Ok(Self::from_path(path, metadata, Utc::now()))
    }

    /// Reopen an existing archive file so follow-up runs can overwrite the snapshot in place.
    pub fn resume_from_listing(listing: &SessionListing, metadata: SessionArchiveMetadata) -> Self {
        Self::from_path(listing.path.clone(), metadata, listing.snapshot.started_at)
    }

    pub fn finalize(
        &self,
        transcript: Vec<String>,
        total_messages: usize,
        distinct_tools: Vec<String>,
        messages: Vec<SessionMessage>,
    ) -> Result<PathBuf> {
        let snapshot =
            self.build_final_snapshot(transcript, total_messages, distinct_tools, messages);

        let path = self.write_snapshot(snapshot)?;
        if let Some(parent) = path.parent() {
            apply_session_retention_best_effort(parent);
        }
        Ok(path)
    }

    pub fn persist_progress(&self, args: SessionProgressArgs) -> Result<PathBuf> {
        let mut perf = PerfSpan::new("vtcode.perf.session_progress_write_ms");
        perf.tag("mode", "sync");

        let snapshot = self.build_progress_snapshot(args);

        self.write_snapshot(snapshot)
    }

    pub async fn persist_progress_async(&self, args: SessionProgressArgs) -> Result<PathBuf> {
        let mut perf = PerfSpan::new("vtcode.perf.session_progress_write_ms");
        perf.tag("mode", "async");

        if !self.should_persist_progress(args.turn_number)? {
            return Ok(self.path.clone());
        }

        let snapshot = self.build_progress_snapshot(args);

        self.write_snapshot_async(snapshot).await
    }

    fn write_snapshot(&self, snapshot: SessionSnapshot) -> Result<PathBuf> {
        let Some(snapshot) = prepare_snapshot_for_write(snapshot)? else {
            return Ok(self.path.clone());
        };

        write_json_file_sync(&self.path, &snapshot)?;
        Ok(self.path.clone())
    }

    async fn write_snapshot_async(&self, snapshot: SessionSnapshot) -> Result<PathBuf> {
        let Some(snapshot) = prepare_snapshot_for_write(snapshot)? else {
            return Ok(self.path.clone());
        };

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
        create_fork_archive(source_snapshot, custom_suffix, None).await
    }
}

async fn create_fork_archive(
    source_snapshot: &SessionSnapshot,
    custom_suffix: Option<String>,
    explicit_identifier: Option<String>,
) -> Result<SessionArchive> {
    let sessions_dir = resolve_sessions_dir_for_archive_writes().await?;
    let started_at = Utc::now();

    let forked_metadata = source_snapshot.metadata.fork_seed();

    let path = if let Some(session_identifier) = explicit_identifier {
        reserve_new_session_archive_path(&sessions_dir, &session_identifier)?
    } else {
        generate_unique_archive_path(
            &sessions_dir,
            &forked_metadata,
            started_at,
            custom_suffix.as_deref(),
        )
    };

    Ok(SessionArchive::from_path(path, forked_metadata, started_at))
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

    let path = match session_archive_path_for_identifier(&sessions_dir, identifier) {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    if !path.exists() {
        return Ok(None);
    }

    let snapshot: SessionSnapshot = read_json_file_sync(&path)?;
    Ok(Some(SessionListing { path, snapshot }))
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
    if query.trim().is_empty() || max_results == 0 {
        return Ok(Vec::new());
    }

    let listings = list_recent_sessions(session_limit).await?;
    let matcher = RegexBuilder::new(&regex::escape(query))
        .case_insensitive(true)
        .build()
        .context("failed to compile session search query")?;
    let mut results = Vec::new();

    for listing in listings {
        for (idx, msg) in listing.snapshot.messages.iter().enumerate() {
            let content = match &msg.content {
                MessageContent::Text(t) => t.as_str(),
                MessageContent::Parts(_) => continue,
            };

            if let Some(matched) = matcher.find(content) {
                let snippet = search_result_snippet(content, matched.start(), matched.end());

                results.push(SearchResult {
                    session_id: listing.identifier(),
                    session_path: listing.path.clone(),
                    timestamp: listing.snapshot.started_at,
                    message_index: idx,
                    role: msg.role.clone(),
                    content_snippet: snippet,
                    score: 1.0,
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

fn search_result_snippet(content: &str, match_start: usize, match_end: usize) -> String {
    const CONTEXT_BYTES: usize = 50;

    let start = floor_char_boundary(content, match_start.saturating_sub(CONTEXT_BYTES));
    let end = ceil_char_boundary(content, (match_end + CONTEXT_BYTES).min(content.len()));

    let mut snippet = String::new();
    if start > 0 {
        snippet.push_str("...");
    }
    snippet.push_str(&content[start..end].replace('\n', " "));
    if end < content.len() {
        snippet.push_str("...");
    }
    snippet
}

fn floor_char_boundary(content: &str, mut index: usize) -> usize {
    while index > 0 && !content.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(content: &str, mut index: usize) -> usize {
    while index < content.len() && !content.is_char_boundary(index) {
        index += 1;
    }
    index
}

async fn resolve_sessions_dir() -> Result<PathBuf> {
    if let Some(custom) = read_env_var_os(SESSION_DIR_ENV) {
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

impl SessionRetentionLimits {
    fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            max_files: parse_env_value(SESSION_MAX_FILES_ENV).unwrap_or(defaults.max_files),
            max_age_days: parse_env_value(SESSION_MAX_AGE_DAYS_ENV)
                .unwrap_or(defaults.max_age_days),
            max_total_size_bytes: parse_env_value::<u64>(SESSION_MAX_SIZE_MB_ENV)
                .map(|value| value.saturating_mul(BYTES_PER_MB))
                .unwrap_or(defaults.max_total_size_bytes),
        }
    }
}

fn session_retention_limits() -> SessionRetentionLimits {
    SessionRetentionLimits::from_env()
}

fn parse_env_value<T>(key: &str) -> Option<T>
where
    T: FromStr,
{
    read_env_var(key)?.trim().parse().ok()
}

fn apply_session_retention_best_effort(sessions_dir: &Path) {
    if let Err(err) = apply_session_retention(sessions_dir) {
        tracing::warn!(
            sessions_dir = %sessions_dir.display(),
            error = %err,
            "Failed to prune session archives"
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
                tracing::warn!(
                    sessions_dir = %sessions_dir.display(),
                    error = %err,
                    "Failed to read a session archive entry"
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
                tracing::warn!(
                    path = %path.display(),
                    error = %err,
                    "Failed to read session archive metadata"
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
            tracing::warn!(
                path = %entry.path.display(),
                error = %err,
                "Failed to remove session archive"
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

fn compact_snapshot_to_max_bytes(
    mut snapshot: SessionSnapshot,
    max_bytes: usize,
) -> Result<SessionSnapshot> {
    if max_bytes == 0 {
        minimize_snapshot_payload(&mut snapshot);
        return Ok(snapshot);
    }

    while serde_json::to_vec(&snapshot)?.len() > max_bytes {
        if trim_oldest_snapshot_entries(&mut snapshot) {
            continue;
        }
        if strip_snapshot_overhead(&mut snapshot) {
            continue;
        }
        if shrink_snapshot_strings(&mut snapshot) {
            continue;
        }
        break;
    }

    if serde_json::to_vec(&snapshot)?.len() > max_bytes {
        minimize_snapshot_payload(&mut snapshot);
        let _ = shrink_snapshot_strings(&mut snapshot);
    }

    Ok(snapshot)
}

fn prepare_snapshot_for_write(snapshot: SessionSnapshot) -> Result<Option<SessionSnapshot>> {
    if !history_persistence_enabled() {
        return Ok(None);
    }

    let max_bytes = session_history_settings().max_bytes;
    let snapshot = match max_bytes {
        Some(max_bytes) => compact_snapshot_to_max_bytes(snapshot, max_bytes)?,
        None => snapshot,
    };

    Ok(Some(snapshot))
}

fn minimize_snapshot_payload(snapshot: &mut SessionSnapshot) {
    snapshot.messages.clear();
    snapshot.transcript.clear();
    snapshot.distinct_tools.clear();
    snapshot.error_logs.clear();
    if let Some(progress) = snapshot.progress.as_mut() {
        progress.recent_messages.clear();
        progress.tool_summaries.clear();
        progress.token_usage = None;
        progress.max_context_tokens = None;
        progress.loaded_skills.clear();
    }
}

fn trim_oldest_snapshot_entries(snapshot: &mut SessionSnapshot) -> bool {
    let mut changed = false;

    if snapshot.messages.len() > 1 {
        snapshot.messages.remove(0);
        changed = true;
    }

    if snapshot.transcript.len() > 1 {
        snapshot.transcript.remove(0);
        changed = true;
    }

    if let Some(progress) = snapshot.progress.as_mut()
        && progress.recent_messages.len() > 1
    {
        progress.recent_messages.remove(0);
        changed = true;
    }

    changed
}

fn strip_snapshot_overhead(snapshot: &mut SessionSnapshot) -> bool {
    let mut changed = false;

    if !snapshot.transcript.is_empty() {
        snapshot.transcript.clear();
        changed = true;
    }
    if !snapshot.distinct_tools.is_empty() {
        snapshot.distinct_tools.clear();
        changed = true;
    }
    if !snapshot.error_logs.is_empty() {
        snapshot.error_logs.clear();
        changed = true;
    }

    if let Some(progress) = snapshot.progress.as_mut() {
        if !progress.tool_summaries.is_empty() {
            progress.tool_summaries.clear();
            changed = true;
        }
        if progress.token_usage.take().is_some() {
            changed = true;
        }
        if progress.max_context_tokens.take().is_some() {
            changed = true;
        }
        if !progress.loaded_skills.is_empty() {
            progress.loaded_skills.clear();
            changed = true;
        }
    }

    changed
}

fn shrink_snapshot_strings(snapshot: &mut SessionSnapshot) -> bool {
    let mut changed = shrink_snapshot_metadata(&mut snapshot.metadata);

    for transcript in &mut snapshot.transcript {
        changed |= shrink_string(transcript);
    }

    for message in &mut snapshot.messages {
        changed |= shrink_session_message(message);
    }

    for error_log in &mut snapshot.error_logs {
        changed |= shrink_string(&mut error_log.message);
    }

    if let Some(progress) = snapshot.progress.as_mut() {
        for message in &mut progress.recent_messages {
            changed |= shrink_session_message(message);
        }
        if let Some(token_usage) = progress.token_usage.as_mut() {
            changed |= shrink_string(token_usage);
        }
    }

    changed
}

fn shrink_session_message(message: &mut SessionMessage) -> bool {
    let mut changed = false;
    changed |= shrink_message_content(&mut message.content);

    if let Some(reasoning) = message.reasoning.as_mut() {
        changed |= shrink_string(reasoning);
    }
    if let Some(reasoning_details) = message.reasoning_details.as_mut()
        && !reasoning_details.is_empty()
    {
        reasoning_details.clear();
        changed = true;
    }
    if let Some(tool_call_id) = message.tool_call_id.as_mut() {
        changed |= shrink_string(tool_call_id);
    }
    if let Some(origin_tool) = message.origin_tool.as_mut() {
        changed |= shrink_string(origin_tool);
    }
    if let Some(tool_calls) = message.tool_calls.as_mut() {
        for tool_call in tool_calls {
            changed |= shrink_string(&mut tool_call.id);
            changed |= shrink_string(&mut tool_call.call_type);
            if let Some(function) = tool_call.function.as_mut() {
                changed |= shrink_string(&mut function.name);
                changed |= shrink_string(&mut function.arguments);
            }
            if let Some(text) = tool_call.text.as_mut() {
                changed |= shrink_string(text);
            }
            if let Some(thought_signature) = tool_call.thought_signature.as_mut() {
                changed |= shrink_string(thought_signature);
            }
        }
    }

    changed
}

fn shrink_snapshot_metadata(metadata: &mut SessionArchiveMetadata) -> bool {
    let mut changed = false;

    changed |= shrink_string(&mut metadata.workspace_label);
    changed |= shrink_string(&mut metadata.workspace_path);
    changed |= shrink_string(&mut metadata.model);
    changed |= shrink_string(&mut metadata.provider);
    changed |= shrink_string(&mut metadata.theme);
    changed |= shrink_string(&mut metadata.reasoning_effort);
    changed |= shrink_optional_string(&mut metadata.session_mode);
    changed |= shrink_optional_string(&mut metadata.debug_log_path);
    changed |= shrink_optional_string(&mut metadata.prompt_cache_lineage_id);
    changed |= shrink_optional_string(&mut metadata.external_thread_id);
    changed |= shrink_optional_string(&mut metadata.parent_session_id);

    for skill in &mut metadata.loaded_skills {
        changed |= shrink_string(skill);
    }

    changed
}

fn shrink_message_content(content: &mut MessageContent) -> bool {
    match content {
        MessageContent::Text(text) => shrink_string(text),
        MessageContent::Parts(parts) => {
            let mut changed = false;
            for part in parts {
                changed |= match part {
                    crate::llm::provider::ContentPart::Text { text } => shrink_string(text),
                    crate::llm::provider::ContentPart::Image {
                        data, mime_type, ..
                    } => shrink_string(data) | shrink_string(mime_type),
                    crate::llm::provider::ContentPart::File {
                        filename,
                        file_id,
                        file_data,
                        file_url,
                        ..
                    } => {
                        shrink_optional_string(filename)
                            | shrink_optional_string(file_id)
                            | shrink_optional_string(file_data)
                            | shrink_optional_string(file_url)
                    }
                };
            }
            changed
        }
    }
}

fn shrink_optional_string(value: &mut Option<String>) -> bool {
    value.as_mut().is_some_and(shrink_string)
}

fn shrink_string(value: &mut String) -> bool {
    const MIN_RETAINED_CHARS: usize = 8;
    const TRUNCATION_MARKER: &str = "...";

    if value.len() <= MIN_RETAINED_CHARS + TRUNCATION_MARKER.len() {
        return false;
    }

    let keep_len = (value.len() / 2).max(MIN_RETAINED_CHARS);
    let prefix_len = keep_len.saturating_sub(TRUNCATION_MARKER.len());
    value.truncate(prefix_len);
    value.push_str(TRUNCATION_MARKER);
    true
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
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some(ext)
            if ext.eq_ignore_ascii_case(SESSION_FILE_EXTENSION)
                || ext.eq_ignore_ascii_case("jsonl")
                || ext.eq_ignore_ascii_case("log")
    )
}

#[cfg(test)]
#[path = "session_archive_tests.rs"]
mod tests;
