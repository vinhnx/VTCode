use crate::llm::provider::{Message, MessageRole, ToolCall};
use crate::utils::dot_config::DotManager;
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::env;
use std::fmt::Write as FmtWrite;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process;

const SESSION_FILE_PREFIX: &str = "session";
const SESSION_FILE_EXTENSION: &str = "json";
pub const SESSION_DIR_ENV: &str = "VT_SESSION_DIR";

/// Provides the directory that session archives should be written to.
pub trait SessionDirectoryResolver {
    fn ensure_sessions_dir(&self) -> Result<PathBuf>;
}

/// Default resolver that mirrors the historical vtcode behavior.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultSessionDirectoryResolver;

impl SessionDirectoryResolver for DefaultSessionDirectoryResolver {
    fn ensure_sessions_dir(&self) -> Result<PathBuf> {
        resolve_sessions_dir()
    }
}

/// Resolver that always returns a specific directory path.
#[derive(Debug, Clone)]
pub struct FixedSessionDirectoryResolver {
    root: PathBuf,
}

impl FixedSessionDirectoryResolver {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl SessionDirectoryResolver for FixedSessionDirectoryResolver {
    fn ensure_sessions_dir(&self) -> Result<PathBuf> {
        fs::create_dir_all(&self.root).with_context(|| {
            format!(
                "failed to create fixed session directory: {}",
                self.root.display()
            )
        })?;
        Ok(self.root.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionArchiveMetadata {
    pub workspace_label: String,
    pub workspace_path: String,
    pub model: String,
    pub provider: String,
    pub theme: String,
    pub reasoning_effort: String,
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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionMessage {
    pub role: MessageRole,
    pub content: String,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

impl SessionMessage {
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    pub fn with_tool_call_id(
        role: MessageRole,
        content: impl Into<String>,
        tool_call_id: Option<String>,
    ) -> Self {
        Self {
            role,
            content: content.into(),
            tool_call_id,
            tool_calls: Vec::new(),
        }
    }
}

impl From<&Message> for SessionMessage {
    fn from(message: &Message) -> Self {
        Self {
            role: message.role.clone(),
            content: message.content.clone(),
            tool_call_id: message.tool_call_id.clone(),
            tool_calls: message.tool_calls.clone().unwrap_or_default(),
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
}

#[derive(Debug, Clone)]
pub struct SessionSummaryOptions {
    pub include_transcript: bool,
    pub include_messages: bool,
    pub max_transcript_lines: Option<usize>,
    pub max_message_lines: Option<usize>,
    pub include_tool_calls: bool,
}

impl Default for SessionSummaryOptions {
    fn default() -> Self {
        Self {
            include_transcript: true,
            include_messages: true,
            max_transcript_lines: None,
            max_message_lines: None,
            include_tool_calls: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSummaryMessage {
    pub role: MessageRole,
    pub total_lines: usize,
    #[serde(default)]
    pub lines: Vec<String>,
    pub truncated: bool,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSummary {
    pub metadata: SessionArchiveMetadata,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub duration_seconds: i64,
    pub total_messages: usize,
    #[serde(default)]
    pub distinct_tools: Vec<String>,
    #[serde(default)]
    pub transcript_preview: Vec<String>,
    pub transcript_total_lines: usize,
    pub transcript_truncated: bool,
    #[serde(default)]
    pub messages: Vec<SessionSummaryMessage>,
}

#[derive(Debug, Clone, Default)]
pub struct SessionSummaryBuilder {
    options: SessionSummaryOptions,
}

impl SessionSummaryBuilder {
    pub fn new(options: SessionSummaryOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &SessionSummaryOptions {
        &self.options
    }

    pub fn summarize(&self, snapshot: &SessionSnapshot) -> SessionSummary {
        let mut distinct_tools = snapshot.distinct_tools.clone();
        distinct_tools.sort();
        distinct_tools.dedup();

        let transcript_total_lines = snapshot.transcript.len();
        let (transcript_preview, _, transcript_truncated) = if self.options.include_transcript {
            summarize_lines(
                snapshot.transcript.iter().map(|line| line.as_str()),
                self.options.max_transcript_lines,
            )
        } else {
            (Vec::new(), transcript_total_lines, false)
        };

        let messages = if self.options.include_messages {
            snapshot
                .messages
                .iter()
                .map(|message| {
                    let (lines, total_lines, truncated) =
                        summarize_lines(message.content.lines(), self.options.max_message_lines);
                    SessionSummaryMessage {
                        role: message.role.clone(),
                        total_lines,
                        lines,
                        truncated,
                        tool_call_id: message.tool_call_id.clone(),
                        tool_calls: if self.options.include_tool_calls {
                            message.tool_calls.clone()
                        } else {
                            Vec::new()
                        },
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        SessionSummary {
            metadata: snapshot.metadata.clone(),
            started_at: snapshot.started_at,
            ended_at: snapshot.ended_at,
            duration_seconds: snapshot
                .ended_at
                .signed_duration_since(snapshot.started_at)
                .num_seconds(),
            total_messages: snapshot.total_messages,
            distinct_tools,
            transcript_preview,
            transcript_total_lines,
            transcript_truncated,
            messages,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionMarkdownRendererOptions {
    pub include_transcript: bool,
    pub include_messages: bool,
    pub max_transcript_lines: Option<usize>,
    pub max_message_lines: Option<usize>,
}

impl Default for SessionMarkdownRendererOptions {
    fn default() -> Self {
        Self {
            include_transcript: true,
            include_messages: true,
            max_transcript_lines: None,
            max_message_lines: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SessionMarkdownRenderer {
    options: SessionMarkdownRendererOptions,
}

impl SessionMarkdownRenderer {
    pub fn new(options: SessionMarkdownRendererOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &SessionMarkdownRendererOptions {
        &self.options
    }

    pub fn render(&self, snapshot: &SessionSnapshot) -> String {
        let mut output = String::new();
        self.write_header(snapshot, &mut output);
        if self.options.include_transcript {
            self.write_transcript(snapshot, &mut output);
        }
        if self.options.include_messages {
            self.write_messages(snapshot, &mut output);
        }
        output
    }

    fn write_header(&self, snapshot: &SessionSnapshot, output: &mut String) {
        let metadata = &snapshot.metadata;
        let mut distinct_tools = snapshot.distinct_tools.clone();
        distinct_tools.sort();
        distinct_tools.dedup();
        let duration = snapshot.ended_at - snapshot.started_at;
        let duration_label = format_duration(duration);

        let _ = writeln!(output, "# Session: {}", metadata.workspace_label);
        let _ = writeln!(output);
        let _ = writeln!(output, "- Workspace path: `{}`", metadata.workspace_path);
        let _ = writeln!(
            output,
            "- Model: `{}` via `{}`",
            metadata.model, metadata.provider
        );
        let _ = writeln!(output, "- Theme: `{}`", metadata.theme);
        let _ = writeln!(
            output,
            "- Reasoning effort: `{}`",
            metadata.reasoning_effort
        );
        let _ = writeln!(output, "- Started: {}", snapshot.started_at.to_rfc3339());
        let _ = writeln!(output, "- Ended: {}", snapshot.ended_at.to_rfc3339());
        let _ = writeln!(output, "- Duration: {}", duration_label);
        let _ = writeln!(output, "- Total messages: {}", snapshot.total_messages);
        if distinct_tools.is_empty() {
            let _ = writeln!(output, "- Distinct tools: _none_");
        } else {
            let _ = writeln!(output, "- Distinct tools: {}", distinct_tools.join(", "));
        }
        let _ = writeln!(output);
    }

    fn write_transcript(&self, snapshot: &SessionSnapshot, output: &mut String) {
        let _ = writeln!(output, "## Transcript");
        let _ = writeln!(output);
        if snapshot.transcript.is_empty() {
            let _ = writeln!(output, "_No transcript lines were recorded._");
            let _ = writeln!(output);
            return;
        }

        let total_lines = snapshot.transcript.len();
        let display_limit = self
            .options
            .max_transcript_lines
            .map(|limit| limit.min(total_lines))
            .unwrap_or(total_lines);
        let truncated = total_lines.saturating_sub(display_limit);

        for line in snapshot.transcript.iter().take(display_limit) {
            let _ = writeln!(output, "- {}", line);
        }
        if truncated > 0 {
            let _ = writeln!(output, "- ... ({} more transcript lines)", truncated);
        }
        let _ = writeln!(output);
    }

    fn write_messages(&self, snapshot: &SessionSnapshot, output: &mut String) {
        let _ = writeln!(output, "## Messages");
        let _ = writeln!(output);
        if snapshot.messages.is_empty() {
            let _ = writeln!(output, "_No structured messages were recorded._");
            let _ = writeln!(output);
            return;
        }

        for message in &snapshot.messages {
            let role_label = describe_role(&message.role);
            let _ = writeln!(output, "### {}", role_label);

            if message.content.trim().is_empty() {
                let _ = writeln!(output, "_No content._");
            } else {
                let lines: Vec<&str> = message.content.lines().collect();
                let display_limit = self
                    .options
                    .max_message_lines
                    .map(|limit| limit.min(lines.len()))
                    .unwrap_or(lines.len());
                let truncated = lines.len().saturating_sub(display_limit);

                for line in lines.into_iter().take(display_limit) {
                    let _ = writeln!(output, "> {}", line.trim_end());
                }
                if truncated > 0 {
                    let _ = writeln!(output, "> ... ({} more lines)", truncated);
                }
            }

            if let Some(tool_call_id) = message.tool_call_id.as_deref() {
                let _ = writeln!(output, "_Primary tool call id:_ `{}`", tool_call_id);
            }

            if !message.tool_calls.is_empty() {
                let _ = writeln!(output, "_Tool calls:_");
                for call in &message.tool_calls {
                    let _ = writeln!(output, "- `{}` (`{}`)", call.function.name, call.id);
                    let arguments = call.function.arguments.trim();
                    if !arguments.is_empty() {
                        let _ = writeln!(output, "  ```json");
                        for line in arguments.lines() {
                            let _ = writeln!(output, "  {}", line);
                        }
                        let _ = writeln!(output, "  ```");
                    }
                }
            }

            let _ = writeln!(output);
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionListing {
    pub path: PathBuf,
    pub snapshot: SessionSnapshot,
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
        self.snapshot
            .messages
            .iter()
            .find(|message| message.role == role && !message.content.trim().is_empty())
            .and_then(|message| {
                message
                    .content
                    .lines()
                    .find_map(|line| {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed)
                        }
                    })
                    .map(|line| truncate_preview(line, 80))
            })
    }
}

fn describe_role(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::System => "System",
        MessageRole::User => "User",
        MessageRole::Assistant => "Assistant",
        MessageRole::Tool => "Tool",
    }
}

fn summarize_lines<I, S>(lines: I, limit: Option<usize>) -> (Vec<String>, usize, bool)
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    let collected: Vec<String> = lines
        .map(|line| line.as_ref().trim_end().to_string())
        .collect();
    let total = collected.len();
    let display_limit = limit.map(|limit| limit.min(total)).unwrap_or(total);
    let truncated = total > display_limit;
    let preview = collected.into_iter().take(display_limit).collect();
    (preview, total, truncated)
}

fn format_duration(duration: Duration) -> String {
    let mut seconds = duration.num_seconds();
    let negative = seconds.is_negative();
    if negative {
        seconds = -seconds;
    }

    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if secs > 0 || parts.is_empty() {
        parts.push(format!("{}s", secs));
    }

    let label = parts.join(" ");
    if negative {
        format!("-{}", label)
    } else {
        label
    }
}

fn generate_unique_archive_path(
    sessions_dir: &Path,
    metadata: &SessionArchiveMetadata,
    started_at: DateTime<Utc>,
) -> PathBuf {
    let sanitized_label = sanitize_component(&metadata.workspace_label);
    let timestamp = started_at.format("%Y%m%dT%H%M%SZ").to_string();
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

#[derive(Debug, Clone)]
pub struct SessionArchive {
    path: PathBuf,
    metadata: SessionArchiveMetadata,
    started_at: DateTime<Utc>,
}

impl SessionArchive {
    pub fn new(metadata: SessionArchiveMetadata) -> Result<Self> {
        Self::with_directory_resolver(metadata, &DefaultSessionDirectoryResolver)
    }

    pub fn with_directory_resolver(
        metadata: SessionArchiveMetadata,
        resolver: &impl SessionDirectoryResolver,
    ) -> Result<Self> {
        let sessions_dir = resolver.ensure_sessions_dir()?;
        let started_at = Utc::now();
        let path = generate_unique_archive_path(&sessions_dir, &metadata, started_at);

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
        };

        let payload = serde_json::to_string_pretty(&snapshot)
            .context("failed to serialize session snapshot")?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create session directory: {}", parent.display())
            })?;
        }
        fs::write(&self.path, payload)
            .with_context(|| format!("failed to write session archive: {}", self.path.display()))?;

        Ok(self.path.clone())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn into_streaming_writer(self) -> Result<SessionArchiveStreamWriter> {
        SessionArchiveStreamWriter::new(self)
    }
}

#[derive(Debug)]
pub struct SessionArchiveStreamWriter {
    file: Option<BufWriter<File>>,
    final_path: PathBuf,
    temp_path: PathBuf,
    transcript_closed: bool,
    first_transcript_entry: bool,
    first_message_entry: bool,
    total_messages: usize,
    distinct_tools: BTreeSet<String>,
}

impl SessionArchiveStreamWriter {
    fn new(archive: SessionArchive) -> Result<Self> {
        let SessionArchive {
            path: final_path,
            metadata,
            started_at,
        } = archive;
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create session directory: {}", parent.display())
            })?;
        }

        let temp_path = create_temp_path(&final_path);
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp_path)
            .with_context(|| {
                format!(
                    "failed to create temporary session archive: {}",
                    temp_path.display()
                )
            })?;
        let mut writer = BufWriter::new(file);

        write_initial_header(&mut writer, &metadata, started_at)?;

        Ok(Self {
            file: Some(writer),
            final_path,
            temp_path,
            transcript_closed: false,
            first_transcript_entry: true,
            first_message_entry: true,
            total_messages: 0,
            distinct_tools: BTreeSet::new(),
        })
    }

    pub fn append_transcript_line(&mut self, line: impl AsRef<str>) -> Result<()> {
        if self.transcript_closed {
            anyhow::bail!("transcript already finalized for streaming session")
        }

        let json =
            serde_json::to_string(line.as_ref()).context("failed to serialize transcript line")?;
        let first_entry = self.first_transcript_entry;
        let writer = self.writer()?;
        if first_entry {
            write!(writer, "\n    {}", json).context("failed to write first transcript line")?;
            self.first_transcript_entry = false;
        } else {
            write!(writer, ",\n    {}", json).context("failed to append transcript line")?;
        }
        Ok(())
    }

    pub fn finish_transcript(&mut self) -> Result<()> {
        if self.transcript_closed {
            return Ok(());
        }

        let first_entry = self.first_transcript_entry;
        let writer = self.writer()?;
        if first_entry {
            write!(writer, "],\n  \"messages\": [")
                .context("failed to finalize empty transcript array")?;
        } else {
            write!(writer, "\n  ],\n  \"messages\": [")
                .context("failed to finalize transcript array")?;
        }
        self.transcript_closed = true;
        Ok(())
    }

    pub fn append_message(&mut self, message: SessionMessage) -> Result<()> {
        self.ensure_messages_array_open()?;

        let json = serde_json::to_string_pretty(&message)
            .context("failed to serialize session message")?;
        let first_entry = self.first_message_entry;
        let writer = self.writer()?;
        if first_entry {
            write!(writer, "\n{}", indent_block(&json, 4))
                .context("failed to write first session message")?;
            self.first_message_entry = false;
        } else {
            write!(writer, ",\n{}", indent_block(&json, 4))
                .context("failed to append session message")?;
        }

        for tool_call in &message.tool_calls {
            self.distinct_tools.insert(tool_call.function.name.clone());
        }
        self.total_messages += 1;
        Ok(())
    }

    pub fn record_tool_usage(&mut self, tool: impl Into<String>) {
        self.distinct_tools.insert(tool.into());
    }

    pub fn finalize(mut self) -> Result<PathBuf> {
        self.ensure_messages_array_open()?;
        let mut writer = self
            .file
            .take()
            .context("session archive stream already finalized")?;
        if self.first_message_entry {
            write!(writer, "]").context("failed to finalize empty messages array")?;
        } else {
            write!(writer, "\n  ]").context("failed to finalize messages array")?;
        }

        let ended_at = Utc::now();
        let distinct_tools: Vec<String> = std::mem::take(&mut self.distinct_tools)
            .into_iter()
            .collect();
        let distinct_json = serde_json::to_string_pretty(&distinct_tools)
            .context("failed to serialize distinct tools")?;

        if distinct_tools.is_empty() {
            write!(
                writer,
                ",\n  \"ended_at\": \"{}\",\n  \"total_messages\": {},\n  \"distinct_tools\": []\n}}\n",
                ended_at.to_rfc3339(),
                self.total_messages
            )
            .context("failed to write session archive footer")?;
        } else {
            write!(
                writer,
                ",\n  \"ended_at\": \"{}\",\n  \"total_messages\": {},\n  \"distinct_tools\":\n{}\n}}\n",
                ended_at.to_rfc3339(),
                self.total_messages,
                indent_block(&distinct_json, 2)
            )
            .context("failed to write session archive footer")?;
        }
        writer
            .flush()
            .context("failed to flush session archive stream")?;

        drop(writer);
        fs::rename(&self.temp_path, &self.final_path).with_context(|| {
            format!(
                "failed to atomically persist session archive: {} -> {}",
                self.temp_path.display(),
                self.final_path.display()
            )
        })?;

        Ok(self.final_path.clone())
    }

    fn ensure_messages_array_open(&mut self) -> Result<()> {
        if !self.transcript_closed {
            self.finish_transcript()?;
        }
        Ok(())
    }
}

impl Drop for SessionArchiveStreamWriter {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.temp_path);
    }
}

fn create_temp_path(final_path: &Path) -> PathBuf {
    let extension = final_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!("{}.partial", ext))
        .unwrap_or_else(|| "partial".to_string());
    final_path.with_extension(extension)
}

fn write_initial_header(
    writer: &mut BufWriter<File>,
    metadata: &SessionArchiveMetadata,
    started_at: DateTime<Utc>,
) -> Result<()> {
    let metadata_json =
        serde_json::to_string_pretty(metadata).context("failed to serialize session metadata")?;

    writer
        .write_all(b"{\n  \"metadata\": ")
        .context("failed to write session metadata header")?;
    write_pretty_value(writer, &metadata_json, 2)
        .context("failed to write session metadata block")?;
    write!(
        writer,
        ",\n  \"started_at\": \"{}\",\n  \"transcript\": [",
        started_at.to_rfc3339()
    )
    .context("failed to write session archive start block")?;

    Ok(())
}

fn write_pretty_value(
    writer: &mut BufWriter<File>,
    json: &str,
    indent: usize,
) -> std::io::Result<()> {
    let indent_str = " ".repeat(indent);
    let mut lines = json.lines();
    if let Some(first_line) = lines.next() {
        writer.write_all(first_line.as_bytes())?;
        for line in lines {
            writer.write_all(b"\n")?;
            writer.write_all(indent_str.as_bytes())?;
            writer.write_all(line.as_bytes())?;
        }
    }
    Ok(())
}

fn indent_block(input: &str, spaces: usize) -> String {
    let indent = " ".repeat(spaces);
    let mut output = String::new();
    for (index, line) in input.lines().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        output.push_str(&indent);
        output.push_str(line);
    }
    output
}

impl SessionArchiveStreamWriter {
    fn writer(&mut self) -> Result<&mut BufWriter<File>> {
        self.file
            .as_mut()
            .context("session archive stream writer is finalized")
    }
}

pub fn list_recent_sessions(limit: usize) -> Result<Vec<SessionListing>> {
    list_recent_sessions_with_resolver(limit, &DefaultSessionDirectoryResolver)
}

pub fn list_recent_sessions_with_resolver(
    limit: usize,
    resolver: &impl SessionDirectoryResolver,
) -> Result<Vec<SessionListing>> {
    let sessions_dir = match resolver.ensure_sessions_dir() {
        Ok(dir) => dir,
        Err(_) => return Ok(Vec::new()),
    };

    let mut listings = Vec::new();
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

        let data = fs::read_to_string(&path)
            .with_context(|| format!("failed to read session file: {}", path.display()))?;
        let snapshot: SessionSnapshot = match serde_json::from_str(&data) {
            Ok(snapshot) => snapshot,
            Err(_) => continue,
        };
        listings.push(SessionListing { path, snapshot });
    }

    listings.sort_by(|a, b| b.snapshot.ended_at.cmp(&a.snapshot.ended_at));
    if limit > 0 && listings.len() > limit {
        listings.truncate(limit);
    }

    Ok(listings)
}

fn resolve_sessions_dir() -> Result<PathBuf> {
    if let Some(custom) = env::var_os(SESSION_DIR_ENV) {
        let path = PathBuf::from(custom);
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create custom session dir: {}", path.display()))?;
        return Ok(path);
    }

    let manager = DotManager::new().context("failed to load VTCode dot manager")?;
    manager
        .initialize()
        .context("failed to initialize VTCode dot directory structure")?;
    let dir = manager.sessions_dir();
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create session directory: {}", dir.display()))?;
    Ok(dir)
}

fn truncate_preview(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
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
        "workspace".to_string()
    } else {
        trimmed.to_string()
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
    use chrono::{Duration as ChronoDuration, TimeZone, Timelike};
    use std::time::Duration;

    struct EnvGuard {
        key: &'static str,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            unsafe {
                env::set_var(key, value);
            }
            Self { key }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn session_archive_persists_snapshot() -> Result<()> {
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
        let archive = SessionArchive::new(metadata.clone())?;
        let transcript = vec!["line one".to_string(), "line two".to_string()];
        let messages = vec![
            SessionMessage::new(MessageRole::User, "Hello world"),
            SessionMessage::new(MessageRole::Assistant, "Hi there"),
        ];
        let path = archive.finalize(
            transcript.clone(),
            4,
            vec!["tool_a".to_string()],
            messages.clone(),
        )?;

        let stored = fs::read_to_string(&path)
            .with_context(|| format!("failed to read stored session: {}", path.display()))?;
        let snapshot: SessionSnapshot =
            serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

        assert_eq!(snapshot.metadata, metadata);
        assert_eq!(snapshot.transcript, transcript);
        assert_eq!(snapshot.total_messages, 4);
        assert_eq!(snapshot.distinct_tools, vec!["tool_a".to_string()]);
        assert_eq!(snapshot.messages, messages);
        Ok(())
    }

    #[test]
    fn session_message_preserves_tool_calls_from_conversation() {
        let mut message = Message::assistant_with_tools(
            String::new(),
            vec![ToolCall::function(
                "call_1".to_string(),
                "run_command".to_string(),
                "{\"cmd\": \"ls\"}".to_string(),
            )],
        );
        message.tool_call_id = Some("call_1".to_string());

        let session_message = SessionMessage::from(&message);

        assert_eq!(session_message.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(session_message.tool_calls.len(), 1);
        let stored_call = &session_message.tool_calls[0];
        assert_eq!(stored_call.id, "call_1");
        assert_eq!(stored_call.function.name, "run_command");
        assert_eq!(stored_call.function.arguments, "{\"cmd\": \"ls\"}");
    }

    #[test]
    fn session_message_backwards_compatibility_without_tool_calls_field() -> Result<()> {
        let json = r#"{"role":"Assistant","content":"ok","tool_call_id":null}"#;
        let message: SessionMessage = serde_json::from_str(json)?;

        assert!(message.tool_calls.is_empty());
        Ok(())
    }

    #[test]
    fn streaming_writer_persists_incremental_session() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

        let metadata = SessionArchiveMetadata::new(
            "Workspace",
            "/tmp/workspace",
            "model-a",
            "provider-a",
            "dark",
            "medium",
        );
        let mut writer = SessionArchive::new(metadata.clone())?.into_streaming_writer()?;

        writer.append_transcript_line("first line")?;
        writer.append_transcript_line("second line")?;

        let user_message = SessionMessage::new(MessageRole::User, "Hello");
        writer.append_message(user_message.clone())?;

        let mut assistant_message = SessionMessage::new(MessageRole::Assistant, "Hi there");
        assistant_message.tool_calls.push(ToolCall::function(
            "call_1".to_string(),
            "run_command".to_string(),
            "{\"cmd\": \"ls\"}".to_string(),
        ));
        writer.append_message(assistant_message.clone())?;
        writer.record_tool_usage("custom_tool");

        let path = writer.finalize()?;
        let snapshot: SessionSnapshot = serde_json::from_str(&fs::read_to_string(&path)?)?;

        assert_eq!(snapshot.metadata, metadata);
        assert_eq!(snapshot.transcript, vec!["first line", "second line"]);
        assert_eq!(snapshot.total_messages, 2);
        assert_eq!(snapshot.messages, vec![user_message, assistant_message]);
        assert_eq!(snapshot.distinct_tools, vec!["custom_tool", "run_command"]);

        Ok(())
    }

    #[test]
    fn streaming_writer_handles_empty_session() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

        let metadata = SessionArchiveMetadata::new(
            "Workspace",
            "/tmp/workspace",
            "model-a",
            "provider-a",
            "dark",
            "medium",
        );

        let writer = SessionArchive::new(metadata.clone())?.into_streaming_writer()?;
        let path = writer.finalize()?;
        let snapshot: SessionSnapshot = serde_json::from_str(&fs::read_to_string(&path)?)?;

        assert_eq!(snapshot.metadata, metadata);
        assert!(snapshot.transcript.is_empty());
        assert!(snapshot.messages.is_empty());
        assert_eq!(snapshot.total_messages, 0);

        Ok(())
    }

    #[test]
    fn custom_directory_resolver_is_used() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let resolver = FixedSessionDirectoryResolver::new(temp_dir.path().join("sessions"));

        let metadata = SessionArchiveMetadata::new(
            "Workspace",
            "/tmp/ws",
            "model",
            "provider",
            "dark",
            "medium",
        );
        let archive = SessionArchive::with_directory_resolver(metadata, &resolver)?;

        assert!(archive.path().starts_with(temp_dir.path()));
        assert!(temp_dir.path().join("sessions").exists());

        Ok(())
    }

    #[test]
    fn session_archive_path_collision_adds_suffix() -> Result<()> {
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

    #[test]
    fn list_recent_sessions_orders_entries() -> Result<()> {
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
        let first_archive = SessionArchive::new(first_metadata.clone())?;
        first_archive.finalize(
            vec!["first".to_string()],
            1,
            Vec::new(),
            vec![SessionMessage::new(MessageRole::User, "First")],
        )?;

        std::thread::sleep(Duration::from_millis(10));

        let second_metadata = SessionArchiveMetadata::new(
            "Second",
            "/tmp/second",
            "model-b",
            "provider-b",
            "dark",
            "high",
        );
        let second_archive = SessionArchive::new(second_metadata.clone())?;
        second_archive.finalize(
            vec!["second".to_string()],
            2,
            vec!["tool_b".to_string()],
            vec![SessionMessage::new(MessageRole::User, "Second")],
        )?;

        let listings = list_recent_sessions(10)?;
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
        };
        let listing = SessionListing {
            path: PathBuf::from("session-workspace.json"),
            snapshot,
        };

        assert_eq!(
            listing.first_prompt_preview(),
            Some("prompt line".to_string())
        );
        let expected = super::truncate_preview(&long_response, 80);
        assert_eq!(listing.first_reply_preview(), Some(expected));
    }

    #[test]
    fn markdown_renderer_respects_options() {
        let metadata = SessionArchiveMetadata::new(
            "Workspace",
            "/tmp/workspace",
            "model-a",
            "provider-a",
            "dark",
            "medium",
        );
        let started_at = Utc
            .with_ymd_and_hms(2024, 12, 1, 9, 30, 0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();
        let ended_at = started_at + ChronoDuration::seconds(125);

        let mut assistant_message =
            SessionMessage::new(MessageRole::Assistant, "Assistant reply\nwith details");
        assistant_message.tool_call_id = Some("call_1".to_string());
        assistant_message.tool_calls.push(ToolCall::function(
            "call_1".to_string(),
            "run_command".to_string(),
            "{\"cmd\": \"ls\"}".to_string(),
        ));

        let snapshot = SessionSnapshot {
            metadata,
            started_at,
            ended_at,
            total_messages: 2,
            distinct_tools: vec!["run_command".to_string()],
            transcript: vec!["line one".to_string(), "line two".to_string()],
            messages: vec![
                SessionMessage::new(MessageRole::User, "Hello there\nadditional context"),
                assistant_message,
            ],
        };

        let options = SessionMarkdownRendererOptions {
            include_transcript: true,
            include_messages: true,
            max_transcript_lines: Some(1),
            max_message_lines: Some(1),
        };
        let renderer = SessionMarkdownRenderer::new(options);

        let markdown = renderer.render(&snapshot);

        assert!(markdown.contains("# Session: Workspace"));
        assert!(markdown.contains("- Model: `model-a` via `provider-a`"));
        assert!(markdown.contains("- ... (1 more transcript lines)"));
        assert!(markdown.contains("## Messages"));
        assert!(markdown.contains("### User"));
        assert!(markdown.contains("> Hello there"));
        assert!(markdown.contains("> ... (1 more lines)"));
        assert!(markdown.contains("### Assistant"));
        assert!(markdown.contains("_Primary tool call id:_ `call_1`"));
        assert!(markdown.contains("```json"));
        assert!(markdown.contains("\"cmd\": \"ls\""));
    }

    #[test]
    fn summary_builder_truncates_content_and_keeps_tool_calls() {
        let metadata = SessionArchiveMetadata::new(
            "Workspace",
            "/tmp/workspace",
            "model-a",
            "provider-a",
            "dark",
            "medium",
        );
        let started_at = Utc
            .with_ymd_and_hms(2025, 1, 15, 10, 5, 30)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();
        let ended_at = started_at + ChronoDuration::seconds(90);

        let mut assistant_message = SessionMessage::new(
            MessageRole::Assistant,
            "Assistant reply\nwith extra details",
        );
        assistant_message.tool_call_id = Some("call_1".to_string());
        assistant_message.tool_calls.push(ToolCall::function(
            "call_1".to_string(),
            "run_command".to_string(),
            "{\"cmd\": \"ls\"}".to_string(),
        ));

        let snapshot = SessionSnapshot {
            metadata: metadata.clone(),
            started_at,
            ended_at,
            total_messages: 2,
            distinct_tools: vec!["run_command".to_string()],
            transcript: vec![
                "line one".to_string(),
                "line two".to_string(),
                "line three".to_string(),
            ],
            messages: vec![
                SessionMessage::new(MessageRole::User, "Hello there\nadditional context"),
                assistant_message,
            ],
        };

        let options = SessionSummaryOptions {
            include_transcript: true,
            include_messages: true,
            max_transcript_lines: Some(2),
            max_message_lines: Some(1),
            include_tool_calls: true,
        };
        let builder = SessionSummaryBuilder::new(options);
        let summary = builder.summarize(&snapshot);

        assert_eq!(summary.metadata, metadata);
        assert_eq!(summary.total_messages, 2);
        assert_eq!(summary.duration_seconds, 90);
        assert_eq!(summary.transcript_preview.len(), 2);
        assert!(summary.transcript_truncated);
        assert_eq!(summary.distinct_tools, vec!["run_command".to_string()]);
        assert_eq!(summary.messages.len(), 2);
        assert!(summary.messages[0].truncated);
        assert_eq!(summary.messages[0].lines, vec!["Hello there".to_string()]);
        assert_eq!(summary.messages[1].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(summary.messages[1].tool_calls.len(), 1);
    }

    #[test]
    fn summary_builder_can_exclude_tool_calls_and_sections() {
        let metadata = SessionArchiveMetadata::new(
            "Workspace",
            "/tmp/workspace",
            "model-a",
            "provider-a",
            "dark",
            "medium",
        );
        let started_at = Utc
            .with_ymd_and_hms(2025, 1, 15, 10, 5, 30)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();
        let ended_at = started_at + ChronoDuration::seconds(15);

        let mut assistant_message = SessionMessage::new(MessageRole::Assistant, "Reply");
        assistant_message.tool_calls.push(ToolCall::function(
            "call_1".to_string(),
            "run_command".to_string(),
            "{\"cmd\": \"ls\"}".to_string(),
        ));

        let snapshot = SessionSnapshot {
            metadata,
            started_at,
            ended_at,
            total_messages: 1,
            distinct_tools: vec!["run_command".to_string()],
            transcript: vec!["only line".to_string()],
            messages: vec![assistant_message],
        };

        let options = SessionSummaryOptions {
            include_transcript: false,
            include_messages: true,
            max_transcript_lines: None,
            max_message_lines: None,
            include_tool_calls: false,
        };
        let builder = SessionSummaryBuilder::new(options);
        let summary = builder.summarize(&snapshot);

        assert!(summary.transcript_preview.is_empty());
        assert!(!summary.transcript_truncated);
        assert_eq!(summary.messages.len(), 1);
        assert!(summary.messages[0].tool_calls.is_empty());
    }
}
