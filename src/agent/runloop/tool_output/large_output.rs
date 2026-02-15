//! Large output handling - Source of Truth for PTY/Tool outputs
//!
//! When tool output exceeds a threshold, the full output is saved to a temporary
//! file which becomes the **source of truth**. The agent receives:
//! 1. A concise notification with the file path
//! 2. A preview (head + tail) for immediate context
//! 3. The ability to read the full file when needed
//!
//! This ensures:
//! - No information loss (full output preserved in file)
//! - Clean client interface (notification instead of flooding PTY)
//! - Agent can read full context from file when needed for accurate responses
//!
//! Directory structure:
//! `~/.vtcode/tmp/<session_hash>/call_<call_id>.output`
//!
//! ## Usage
//!
//! ```rust,ignore
//! // When processing large PTY output:
//! let result = spool_large_output(output, "run_pty_cmd", &config)?;
//! if let Some(spool) = result {
//!     // Send notification to client
//!     println!("{}", format_agent_notification(&spool));
//!     // Agent can later read full content via: spool.read_full_content()
//! }
//! ```

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use vtcode_core::utils::file_utils::{ensure_dir_exists_sync, read_file_with_context_sync};

/// Configuration for large output spooling
#[derive(Debug, Clone)]
pub struct LargeOutputConfig {
    /// Base directory for temporary output files (default: ~/.vtcode/tmp)
    pub base_dir: PathBuf,
    /// Size threshold (bytes) above which output is spooled to file
    pub threshold_bytes: usize,
    /// Session identifier for grouping related outputs
    pub session_id: Option<String>,
}

impl Default for LargeOutputConfig {
    fn default() -> Self {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        Self {
            base_dir: home.join(".vtcode").join("tmp"),
            threshold_bytes: 50_000, // 50KB — aligned with DEFAULT_SPOOL_THRESHOLD in streams.rs
            session_id: None,
        }
    }
}

impl LargeOutputConfig {
    /// Create a new config with custom base directory
    #[allow(dead_code)]
    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            ..Default::default()
        }
    }

    /// Set the size threshold for spooling
    pub fn with_threshold(mut self, threshold_bytes: usize) -> Self {
        self.threshold_bytes = threshold_bytes;
        self
    }

    /// Set the session ID for grouping outputs
    pub fn with_session_id(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }
}

/// Number of lines to show in preview (head)
const PREVIEW_HEAD_LINES: usize = 20;
/// Number of lines to show in preview (tail)
const PREVIEW_TAIL_LINES: usize = 10;
/// Metadata header line count to skip when reading content
#[allow(dead_code)]
const METADATA_HEADER_LINES: usize = 5;

/// Result of large output handling - This is the SOURCE OF TRUTH for the output
///
/// When tool output exceeds the threshold, the full content is saved to a file.
/// This struct provides methods to:
/// - Read the full content back
/// - Get a preview suitable for the agent
/// - Generate notifications for the client
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SpoolResult {
    /// Path where the full output was saved (source of truth)
    pub file_path: PathBuf,
    /// Size of the saved content in bytes
    pub size_bytes: usize,
    /// Total number of lines in the output
    pub line_count: usize,
    /// Tool name that produced this output
    pub tool_name: String,
    /// Whether the content was actually spooled
    pub was_spooled: bool,
}

#[allow(dead_code)]
impl SpoolResult {
    /// Read the full content from the spooled file (skips metadata header)
    ///
    /// Use this when the agent needs the complete output for analysis.
    pub fn read_full_content(&self) -> Result<String> {
        let content = read_file_with_context_sync(&self.file_path, "spooled output")?;

        // Skip the metadata header (lines before "---\n\n")
        if let Some(idx) = content.find("---\n\n") {
            Ok(content[idx + 5..].to_string())
        } else {
            Ok(content)
        }
    }

    /// Read a specific line range from the spooled file (1-indexed, inclusive)
    ///
    /// Useful for the agent to read specific sections without loading everything.
    pub fn read_lines(&self, start: usize, end: usize) -> Result<String> {
        let content = self.read_full_content()?;
        // Avoid allocating a full Vec of lines; iterate and collect only the requested range.
        if start == 0 || end == 0 || start > end {
            return Ok(String::new());
        }

        let mut out = String::new();
        let mut idx = 0usize;
        let start_idx = start.saturating_sub(1);
        let end_idx = end;
        for line in content.lines() {
            if idx >= start_idx && idx < end_idx {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(line);
            }
            idx += 1;
            if idx >= end_idx {
                break;
            }
        }

        Ok(out)
    }

    /// Get a preview with head and tail lines for immediate context
    ///
    /// Returns a string suitable for including in the agent's response,
    /// with clear markers showing what was truncated.
    pub fn get_preview(&self) -> Result<String> {
        let content = self.read_full_content()?;
        // Avoid allocating all lines; stream to collect only head and a bounded tail buffer.
        use std::collections::VecDeque;

        let mut total = 0usize;
        let mut head_lines: Vec<&str> = Vec::with_capacity(PREVIEW_HEAD_LINES);

        let mut tail_buf: VecDeque<(&str, usize)> = VecDeque::with_capacity(PREVIEW_TAIL_LINES);
        let _tail_tokens_acc = 0usize; // reserved for future token-aware logic

        for line in content.lines() {
            // head capture
            if head_lines.len() < PREVIEW_HEAD_LINES {
                head_lines.push(line);
            }

            // tail rolling buffer
            // store (line, estimated_len) so we can pop_front efficiently
            tail_buf.push_back((line, line.len()));
            if tail_buf.len() > PREVIEW_TAIL_LINES {
                tail_buf.pop_front();
            }

            total += 1;
        }

        if total <= PREVIEW_HEAD_LINES + PREVIEW_TAIL_LINES {
            return Ok(content);
        }

        let hidden = total - PREVIEW_HEAD_LINES - PREVIEW_TAIL_LINES;

        let head_join = head_lines.join("\n");
        let tail_join = tail_buf
            .iter()
            .map(|(l, _)| *l)
            .collect::<Vec<&str>>()
            .join("\n");

        Ok(format!(
            "{}\n\n[... {} lines omitted - full output in: {} ...]\n\n{}",
            head_join,
            hidden,
            self.file_path.display(),
            tail_join
        ))
    }

    /// Generate a structured response for the agent
    ///
    /// This is the recommended format for tool results when output was spooled.
    /// It gives the agent everything it needs: preview, file path, and size info.
    pub fn to_agent_response(&self) -> Result<String> {
        let preview = self.get_preview()?;

        Ok(format!(
            r#"Output saved to file (source of truth): {}

Size: {} bytes ({} lines)
Tool: {}

--- Preview (first {} + last {} lines) ---
{}
--- End Preview ---

To read full content, use: read_file({{"path":"{}","offset_lines":1,"limit":{}}})
To read specific lines, use: read_file({{"path":"{}","offset_lines":<start>,"limit":<line_count>}})"#,
            self.file_path.display(),
            self.size_bytes,
            self.line_count,
            self.tool_name,
            PREVIEW_HEAD_LINES,
            PREVIEW_TAIL_LINES,
            preview,
            self.file_path.display(),
            self.line_count,
            self.file_path.display(),
        ))
    }
}

/// Generate a unique hash for the session directory
pub(super) fn generate_session_hash(session_id: Option<&str>) -> String {
    let mut hasher = Sha256::new();

    // Include session ID if provided
    if let Some(id) = session_id {
        hasher.update(id.as_bytes());
    }

    // Include timestamp for uniqueness
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    hasher.update(timestamp.to_le_bytes());

    // Include process ID for additional uniqueness
    hasher.update(std::process::id().to_le_bytes());

    let result = hasher.finalize();
    // Convert to hex string manually
    result.iter().fold(String::new(), |mut output, b| {
        let _ = std::fmt::write(&mut output, format_args!("{:02x}", b));
        output
    })
}

/// Generate a unique call ID
fn generate_call_id() -> String {
    let mut hasher = Sha256::new();

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    hasher.update(timestamp.to_le_bytes());

    // Add some randomness using process info and address
    let random_val = std::process::id() as u64 ^ timestamp as u64;
    hasher.update(random_val.to_le_bytes());

    let result = hasher.finalize();
    // Use first 12 bytes (24 hex chars) for a shorter but still unique ID
    result[..12].iter().fold(String::new(), |mut output, b| {
        let _ = std::fmt::write(&mut output, format_args!("{:02x}", b));
        output
    })
}

/// Spool large output to a temporary file if it exceeds the threshold
///
/// Returns `Ok(Some(result))` if output was spooled, `Ok(None)` if below threshold
pub fn spool_large_output(
    content: &str,
    tool_name: &str,
    config: &LargeOutputConfig,
) -> Result<Option<SpoolResult>> {
    // Check if content exceeds threshold
    if content.len() < config.threshold_bytes {
        return Ok(None);
    }

    // Generate session directory hash
    let session_hash = generate_session_hash(config.session_id.as_deref());
    let session_dir = config.base_dir.join(&session_hash);

    // Create session directory
    ensure_dir_exists_sync(&session_dir).with_context(|| {
        format!(
            "Failed to create output spool directory: {}",
            session_dir.display()
        )
    })?;

    // Generate unique call ID
    let call_id = generate_call_id();
    let filename = format!("call_{}.output", call_id);
    let file_path = session_dir.join(&filename);

    // Write content to file
    let mut file = fs::File::create(&file_path)
        .with_context(|| format!("Failed to create spool file: {}", file_path.display()))?;

    // Write metadata header
    let metadata = format!(
        "# VT Code Tool Output\n# Tool: {}\n# Timestamp: {}\n# Size: {} bytes\n---\n\n",
        tool_name,
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
        content.len()
    );
    file.write_all(metadata.as_bytes())
        .with_context(|| format!("Failed to write metadata to: {}", file_path.display()))?;

    // Write actual content
    file.write_all(content.as_bytes())
        .with_context(|| format!("Failed to write content to: {}", file_path.display()))?;

    // Count lines for metadata
    let line_count = content.lines().count();

    Ok(Some(SpoolResult {
        file_path,
        size_bytes: content.len(),
        line_count,
        tool_name: tool_name.to_string(),
        was_spooled: true,
    }))
}

/// Format a notification message for spooled output (client display)
///
/// Example output:
/// ```text
/// │ Output too long and was saved to:                                    │
/// │ /Users/user/.vtcode/tmp/40490821eec37be65d00bb1d9e60f6f4d2aa9753e... │
/// │ call_b557fe1443144e71a2c00a34.output                                  │
/// ```
#[allow(dead_code)]
pub fn format_spool_notification(result: &SpoolResult) -> String {
    let path_str = result.file_path.display().to_string();

    // Format with box drawing characters for visual appeal
    let mut lines = Vec::new();
    lines.push(format!(
        "│ Output too long ({} bytes) and was saved to:",
        result.size_bytes
    ));

    // Split long paths across multiple lines if needed
    if path_str.len() > 70 {
        // Find a good split point (at path separator)
        if let Some(idx) = path_str.rfind('/') {
            let (dir, file) = path_str.split_at(idx + 1);
            lines.push(format!("│ {}", dir));
            lines.push(format!("│ {}", file));
        } else {
            lines.push(format!("│ {}", path_str));
        }
    } else {
        lines.push(format!("│ {}", path_str));
    }

    lines.join("\n")
}

/// Format a compact notification for inline display
#[allow(dead_code)]
pub fn format_compact_notification(result: &SpoolResult) -> String {
    format!(
        "[Output saved: {} ({} bytes)]",
        result.file_path.display(),
        result.size_bytes
    )
}

/// Clean up old spool directories older than the specified duration
#[allow(dead_code)]
pub fn cleanup_old_spool_dirs(base_dir: &PathBuf, max_age_hours: u64) -> Result<usize> {
    let mut cleaned = 0;
    let max_age = std::time::Duration::from_secs(max_age_hours * 3600);
    let now = SystemTime::now();

    if !base_dir.exists() {
        return Ok(0);
    }

    for entry in fs::read_dir(base_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };

        let Ok(modified) = metadata.modified() else {
            continue;
        };

        let Ok(age) = now.duration_since(modified) else {
            continue;
        };

        if age > max_age && fs::remove_dir_all(&path).is_ok() {
            cleaned += 1;
        }
    }

    Ok(cleaned)
}
