use crate::core::token_budget::TokenUsageStats;
use crate::llm::provider::{Message, MessageContent, MessageRole};
use crate::utils::dot_config::DotManager;
use crate::utils::error_messages::*;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

const SESSION_FILE_PREFIX: &str = "session";
const SESSION_FILE_EXTENSION: &str = "json";
pub const SESSION_DIR_ENV: &str = "VT_SESSION_DIR";

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionMessage {
    pub role: MessageRole,
    pub content: MessageContent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

impl SessionMessage {
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: MessageContent::Text(content.into()),
            reasoning: None,
            tool_call_id: None,
        }
    }

    pub fn with_content(role: MessageRole, content: MessageContent) -> Self {
        Self {
            role,
            content,
            reasoning: None,
            tool_call_id: None,
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
            tool_call_id,
        }
    }
}

impl From<&Message> for SessionMessage {
    fn from(message: &Message) -> Self {
        Self {
            role: message.role.clone(),
            content: message.content.clone(),
            reasoning: message.reasoning.clone(),
            tool_call_id: message.tool_call_id.clone(),
        }
    }
}

impl From<&SessionMessage> for Message {
    fn from(message: &SessionMessage) -> Self {
        Self {
            role: message.role.clone(),
            content: message.content.clone(),
            reasoning: message.reasoning.clone(),
            reasoning_details: None,
            tool_calls: None,
            tool_call_id: message.tool_call_id.clone(),
            origin_tool: None,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SessionProgress {
    pub turn_number: usize,
    #[serde(default)]
    pub recent_messages: Vec<SessionMessage>,
    #[serde(default)]
    pub tool_summaries: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsageStats>,
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
    pub token_usage: Option<TokenUsageStats>,
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

#[derive(Debug, Clone)]
pub struct SessionArchive {
    path: PathBuf,
    metadata: SessionArchiveMetadata,
    started_at: DateTime<Utc>,
}

impl SessionArchive {
    pub async fn new(metadata: SessionArchiveMetadata, custom_suffix: Option<String>) -> Result<Self> {
        let sessions_dir = resolve_sessions_dir().await?;
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
        })
    }

    pub fn finalize(
        &self,
        transcript: Vec<String>,
        total_messages: usize,
        distinct_tools: Vec<String>,
        messages: Vec<SessionMessage>,
    ) -> Result<PathBuf> {
        let snapshot = SessionSnapshot {
            metadata: self.metadata.clone(),
            started_at: self.started_at,
            ended_at: Utc::now(),
            total_messages,
            distinct_tools,
            transcript,
            messages,
            progress: None,
        };

        self.write_snapshot(snapshot)
    }

    pub fn persist_progress(&self, args: SessionProgressArgs) -> Result<PathBuf> {
        let tool_summaries = args.distinct_tools.clone();
        let snapshot = SessionSnapshot {
            metadata: self.metadata.clone(),
            started_at: self.started_at,
            ended_at: Utc::now(),
            total_messages: args.total_messages,
            distinct_tools: args.distinct_tools,
            transcript: Vec::new(),
            messages: args.recent_messages.clone(),
            progress: Some(SessionProgress {
                turn_number: args.turn_number,
                recent_messages: args.recent_messages,
                tool_summaries,
                token_usage: args.token_usage,
                max_context_tokens: args.max_context_tokens,
                loaded_skills: args.loaded_skills.unwrap_or_default(),
            }),
        };

        self.write_snapshot(snapshot)
    }

    fn write_snapshot(&self, snapshot: SessionSnapshot) -> Result<PathBuf> {
        let payload = serde_json::to_string_pretty(&snapshot).context(ERR_SERIALIZE_STATE)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("{}: {}", ERR_CREATE_SESSION_DIR, parent.display()))?;
        }
        fs::write(&self.path, payload)
            .with_context(|| format!("{}: {}", ERR_WRITE_SESSION, self.path.display()))?;

        Ok(self.path.clone())
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
                match tokio::fs::read_to_string(&path).await {
                    Ok(data) => match serde_json::from_str::<SessionSnapshot>(&data) {
                        Ok(snapshot) => Some(SessionListing { path, snapshot }),
                        Err(_) => None, // Invalid JSON, skip
                    },
                    Err(_) => None, // Failed to read, skip
                }
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

        let data = fs::read_to_string(&path)
            .with_context(|| format!("failed to read session file: {}", path.display()))?;
        let snapshot: SessionSnapshot = serde_json::from_str(&data)
            .with_context(|| format!("failed to parse session archive: {}", path.display()))?;
        return Ok(Some(SessionListing { path, snapshot }));
    }

    Ok(None)
}

async fn resolve_sessions_dir() -> Result<PathBuf> {
    if let Some(custom) = env::var_os(SESSION_DIR_ENV) {
        let path = PathBuf::from(custom);
        tokio::fs::create_dir_all(&path)
            .await
            .with_context(|| format!("failed to create custom session dir: {}", path.display()))?;
        return Ok(path);
    }

    let manager = DotManager::new().context("failed to load VT Code dot manager")?;
    manager
        .initialize()
        .await
        .context("failed to initialize VT Code dot directory structure")?;
    let dir = manager.sessions_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("failed to create session directory: {}", dir.display()))?;
    Ok(dir)
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
    use crate::core::token_budget::TokenUsageStats;
    use crate::llm::provider::ContentPart;
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
        let archive = SessionArchive::new(metadata.clone()).await?;
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
        let archive = SessionArchive::new(metadata).await?;
        let recent = vec![SessionMessage::new(MessageRole::Assistant, "recent")];
        let usage = TokenUsageStats {
            total_tokens: 10,
            ..TokenUsageStats::new()
        };

        let path = archive.persist_progress(SessionProgressArgs {
            total_messages: 1,
            distinct_tools: vec!["tool_a".to_owned()],
            recent_messages: recent.clone(),
            turn_number: 2,
            token_usage: Some(usage.clone()),
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
        assert_eq!(progress.token_usage, Some(usage));
        assert_eq!(progress.tool_summaries, vec!["tool_a".to_string()]);
        assert_eq!(progress.max_context_tokens, Some(128));
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
        let archive = SessionArchive::new(metadata.clone()).await?;
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

        let first_path = generate_unique_archive_path(temp_dir.path(), &metadata, started_at);
        fs::write(&first_path, "{}").context("failed to create sentinel file")?;

        let second_path = generate_unique_archive_path(temp_dir.path(), &metadata, started_at);

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

        let path = generate_unique_archive_path(temp_dir.path(), &metadata, started_at);
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
        let first_archive = SessionArchive::new(first_metadata.clone()).await?;
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
        let second_archive = SessionArchive::new(second_metadata.clone()).await?;
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
}
