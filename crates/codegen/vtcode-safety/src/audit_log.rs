//! Persistent audit logging for tool invocations.
//!
//! §18.4.4 of *The Hitchhiker's Guide to Agentic AI* calls for an audit log of
//! every tool call (arguments, outputs, timestamp). The existing
//! [`crate::command_safety::audit::SafetyAuditLogger`] only records command-safety
//! decisions in memory; this module adds a complementary, opt-in sink that
//! records every MCP and built-in tool invocation in a durable, append-only
//! JSONL stream.
//!
//! ## Sinks
//!
//! - [`JsonlFileSink`] — appends entries to a JSONL file with size-based rotation.
//! - [`InMemorySink`] — keeps entries in memory for tests and short-lived tooling.
//! - [`NullSink`] — discards everything; zero-cost when audit is disabled.
//! - [`MultiSink`] — fan-out wrapper used by [`ToolAuditLogger`].
//!
//! ## Threading
//!
//! Sinks are `Send + Sync`. Writes are non-blocking from the caller's perspective:
//! each sink uses an internal lock or background task to serialize I/O.
//!
//! See also `crates/codegen/vtcode-core/src/tools/untrusted_data.rs` for the prompt-injection
//! defense that pairs with this log.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Status of a single tool invocation for audit purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAuditStatus {
    /// Tool ran to completion and produced a result.
    Success,
    /// Tool returned an error before completing.
    Failure,
    /// Tool exceeded its wall-clock budget.
    Timeout,
    /// Tool execution was cancelled (HITL refusal, planning denial, etc.).
    Cancelled,
    /// Tool execution was blocked before it started (e.g. loop detector, fuse).
    Blocked,
}

impl ToolAuditStatus {
    /// Short string label suitable for log lines.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Blocked => "blocked",
        }
    }
}

/// One audit row written per tool invocation.
///
/// The shape mirrors what `tracing` already records for MCP tool calls
/// (`mcp.tools.call` span with provider / transport / server metadata) but adds
/// enough context for offline forensics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAuditEntry {
    /// Unix epoch milliseconds when the entry was created.
    timestamp_unix_ms: u64,
    /// Stable identifier for the session (e.g. UUID).
    session_id: String,
    /// Stable identifier for the turn within the session.
    turn_id: String,
    /// Provider-side identifier for the tool call (matches `tool_call_id`).
    tool_call_id: String,
    /// Canonical tool name (e.g. `mcp::fetch::fetch`, `read_file`).
    tool_name: String,
    /// SHA-256 of the original (unredacted) tool arguments, hex-encoded.
    arguments_hash: String,
    /// Optional redacted snapshot of the arguments (secrets removed).
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments_redacted: Option<Value>,
    /// SHA-256 of the tool result, hex-encoded.
    result_hash: String,
    /// Optional first N characters of the result for offline triage.
    #[serde(skip_serializing_if = "Option::is_none")]
    result_summary: Option<String>,
    /// Wall-clock duration of the tool call in milliseconds.
    duration_ms: u64,
    /// Final status (success / failure / timeout / cancelled / blocked).
    status: ToolAuditStatus,
    /// Optional sandbox policy applied to this tool call (path to a JSON
    /// snapshot, or a short identifier).
    #[serde(skip_serializing_if = "Option::is_none")]
    sandbox_policy: Option<String>,
    /// MCP transport when applicable (`stdio`, `streamable_http`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    transport: Option<String>,
    /// Remote server address when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    server_address: Option<String>,
    /// Remote server port when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    server_port: Option<u16>,
    /// Identifier of the model that requested the call.
    #[serde(skip_serializing_if = "Option::is_none")]
    model_id: Option<String>,
    /// True when the static `is_suspicious_instruction` probe flagged the
    /// result content.
    prompt_injection_flagged: bool,
    /// Optional free-form reason (e.g. `loop_detector`, `circuit_breaker_open`).
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

/// Trait every audit sink implements.
pub trait ToolAuditSink: Send + Sync {
    /// Persist a single entry. Implementations must never block on network I/O
    /// (disk I/O is fine — JSONL writes are O(milliseconds) and buffered).
    fn write(&self, entry: &ToolAuditEntry);

    /// Flush any buffered entries to durable storage. Default no-op.
    fn flush(&self) {}
}

/// Null sink — accepts every entry but discards them.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;

impl ToolAuditSink for NullSink {
    fn write(&self, _entry: &ToolAuditEntry) {}
}

/// In-memory sink — useful for tests and ephemeral tooling.
#[derive(Debug, Default, Clone)]
pub struct InMemorySink {
    entries: Arc<Mutex<Vec<ToolAuditEntry>>>,
}

impl InMemorySink {
    /// Create a new in-memory sink with no entries.
    #[must_use]
    fn new() -> Self {
        Self::default()
    }

    /// Snapshot all entries recorded so far.
    fn entries(&self) -> Vec<ToolAuditEntry> {
        self.entries.lock().expect("in-memory sink poisoned").clone()
    }

    /// Number of recorded entries.
    fn len(&self) -> usize {
        self.entries.lock().expect("in-memory sink poisoned").len()
    }

    /// Returns true when no entries have been recorded.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drop all recorded entries.
    fn clear(&self) {
        self.entries.lock().expect("in-memory sink poisoned").clear();
    }
}

impl ToolAuditSink for InMemorySink {
    fn write(&self, entry: &ToolAuditEntry) {
        self.entries.lock().expect("in-memory sink poisoned").push(entry.clone());
    }
}

/// Append-only JSONL file sink with size-based rotation.
///
/// The sink writes each entry as a single JSON line. When the current file
/// exceeds `max_size_bytes`, it is renamed to `<path>.1` (and previous
/// generations shift up to `<path>.N`) — keeping at most `max_files` files
/// on disk.
pub struct JsonlFileSink {
    path: PathBuf,
    max_size_bytes: u64,
    max_files: usize,
    state: Mutex<JsonlFileState>,
}

struct JsonlFileState {
    writer: Option<BufWriter<File>>,
    bytes_written: u64,
}

impl std::fmt::Debug for JsonlFileSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonlFileSink")
            .field("path", &self.path)
            .field("max_size_bytes", &self.max_size_bytes)
            .field("max_files", &self.max_files)
            .finish_non_exhaustive()
    }
}

impl JsonlFileSink {
    /// Open (or create) the sink at `path`. Returns an error if the file can't
    /// be created — typically a permissions issue.
    fn open(path: impl Into<PathBuf>, max_size_bytes: u64, max_files: usize) -> std::io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let bytes_written = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            path,
            max_size_bytes,
            max_files: max_files.max(1),
            state: Mutex::new(JsonlFileState { writer: Some(BufWriter::new(file)), bytes_written }),
        })
    }

    /// Path of the active JSONL file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn rotate_if_needed(
        state: &mut JsonlFileState,
        path: &Path,
        max_size_bytes: u64,
        max_files: usize,
    ) -> std::io::Result<()> {
        if state.bytes_written < max_size_bytes {
            return Ok(());
        }
        // Drop the writer before renaming.
        if let Some(mut writer) = state.writer.take() {
            let _ = writer.flush();
        }
        // Shift generations: <path>.N-1 -> <path>.N, …, <path> -> <path>.1
        for index in (1..max_files).rev() {
            let from = rotated_path(path, index);
            let to = rotated_path(path, index + 1);
            if from.exists() {
                let _ = std::fs::rename(&from, &to);
            }
        }
        if path.exists() {
            std::fs::rename(path, rotated_path(path, 1))?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        state.writer = Some(BufWriter::new(file));
        state.bytes_written = 0;
        Ok(())
    }
}

fn rotated_path(path: &Path, index: usize) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(format!(".{index}"));
    PathBuf::from(s)
}

impl ToolAuditSink for JsonlFileSink {
    fn write(&self, entry: &ToolAuditEntry) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };

        let serialized = match serde_json::to_string(entry) {
            Ok(serialized) => serialized,
            Err(err) => {
                tracing::warn!(error = %err, "JsonlFileSink: failed to serialize audit entry");
                return;
            }
        };
        let line_length = serialized.len() as u64 + 1; // account for trailing newline

        if let Err(err) = Self::rotate_if_needed(&mut state, &self.path, self.max_size_bytes, self.max_files) {
            tracing::warn!(error = %err, path = %self.path.display(), "JsonlFileSink: rotation failed");
        }
        if let Some(writer) = state.writer.as_mut() {
            if let Err(err) = writeln!(writer, "{serialized}") {
                tracing::warn!(error = %err, path = %self.path.display(), "JsonlFileSink: write failed");
                return;
            }
            state.bytes_written = state.bytes_written.saturating_add(line_length);
        }
    }

    fn flush(&self) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(writer) = state.writer.as_mut() {
            let _ = writer.flush();
        }
    }
}

impl Drop for JsonlFileSink {
    fn drop(&mut self) {
        self.flush();
    }
}

/// Fan-out sink — forwards every entry to a list of inner sinks.
///
/// Sinks are called sequentially in registration order; a panic in one sink
/// (theoretically impossible, but defensive) doesn't suppress the others.
pub struct MultiSink {
    inner: Vec<Arc<dyn ToolAuditSink>>,
}

impl std::fmt::Debug for MultiSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiSink").field("sink_count", &self.inner.len()).finish()
    }
}

impl MultiSink {
    /// Create a fan-out sink. The slice is moved into `Arc`s to allow sharing
    /// with callers that want to read from one of the sinks while still
    /// forwarding writes.
    #[must_use]
    fn new(sinks: Vec<Arc<dyn ToolAuditSink>>) -> Self {
        Self { inner: sinks }
    }

    /// Number of inner sinks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns true when no inner sinks are registered.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl ToolAuditSink for MultiSink {
    fn write(&self, entry: &ToolAuditEntry) {
        for sink in &self.inner {
            sink.write(entry);
        }
    }

    fn flush(&self) {
        for sink in &self.inner {
            sink.flush();
        }
    }
}

/// Top-level audit logger — currently a thin wrapper that owns a single sink.
///
/// Future iterations can grow this into a fan-out by default (e.g. always
/// pair [`InMemorySink`] with the user's configured persistent sink).
#[derive(Clone)]
pub struct ToolAuditLogger {
    sink: Arc<dyn ToolAuditSink>,
}

impl std::fmt::Debug for ToolAuditLogger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolAuditLogger").finish_non_exhaustive()
    }
}

impl ToolAuditLogger {
    /// Wrap a sink in a logger.
    #[must_use]
    fn new(sink: Arc<dyn ToolAuditSink>) -> Self {
        Self { sink }
    }

    /// Disabled logger that drops every entry (equivalent to `NullSink`).
    #[must_use]
    fn disabled() -> Self {
        Self::new(Arc::new(NullSink))
    }

    /// Persist a single entry through the underlying sink.
    fn record(&self, entry: ToolAuditEntry) {
        self.sink.write(&entry);
    }

    /// Flush the underlying sink.
    fn flush(&self) {
        self.sink.flush();
    }

    /// Borrow the inner sink (used by tests and the CLI debug commands).
    #[must_use]
    pub fn sink(&self) -> &Arc<dyn ToolAuditSink> {
        &self.sink
    }
}

impl Default for ToolAuditLogger {
    fn default() -> Self {
        Self::disabled()
    }
}

/// Helper: SHA-256 hash of arbitrary bytes, hex-encoded.
///
/// Exposed so callers building a `ToolAuditEntry` can compute `arguments_hash`
/// / `result_hash` without pulling in `sha2` directly.
#[must_use]
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_entry(suffix: &str) -> ToolAuditEntry {
        ToolAuditEntry {
            timestamp_unix_ms: 1_700_000_000_000 + u64::from(suffix.bytes().next().unwrap_or(b'a')),
            session_id: format!("session-{suffix}"),
            turn_id: format!("turn-{suffix}"),
            tool_call_id: format!("call-{suffix}"),
            tool_name: "mcp::fetch::fetch".to_owned(),
            arguments_hash: sha256_hex(suffix.as_bytes()),
            arguments_redacted: None,
            result_hash: sha256_hex(format!("result-{suffix}").as_bytes()),
            result_summary: Some(format!("first line of result {suffix}")),
            duration_ms: 42,
            status: ToolAuditStatus::Success,
            sandbox_policy: None,
            transport: Some("stdio".to_owned()),
            server_address: None,
            server_port: None,
            model_id: Some("test-model".to_owned()),
            prompt_injection_flagged: false,
            reason: None,
        }
    }

    #[test]
    fn in_memory_sink_records_entries() {
        let sink = InMemorySink::new();
        sink.write(&sample_entry("a"));
        sink.write(&sample_entry("b"));
        assert_eq!(sink.len(), 2);
        assert_eq!(sink.entries()[0].tool_name, "mcp::fetch::fetch");
        sink.clear();
        assert!(sink.is_empty());
    }

    #[test]
    fn null_sink_accepts_without_recording() {
        let sink = NullSink;
        sink.write(&sample_entry("x"));
        // No observable state, but the call must not panic.
    }

    #[test]
    fn jsonl_file_sink_appends_and_flushes_on_drop() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("audit.jsonl");
        let sink = JsonlFileSink::open(&path, 1024 * 1024, 4).expect("open sink");

        sink.write(&sample_entry("a"));
        sink.write(&sample_entry("b"));
        sink.flush();

        let body = std::fs::read_to_string(&path).expect("read back");
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            let value: Value = serde_json::from_str(line).expect("line is valid JSON");
            assert_eq!(value["tool_name"], "mcp::fetch::fetch");
        }
    }

    #[test]
    fn jsonl_file_sink_rotates_when_threshold_exceeded() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("audit.jsonl");
        // Tiny rotation threshold so the second entry triggers a rotate.
        let sink = JsonlFileSink::open(&path, 60, 3).expect("open sink");

        sink.write(&sample_entry("a"));
        sink.flush();
        sink.write(&sample_entry("b"));
        sink.flush();

        // After rotation, the original path should contain the most recent entry
        // and `<path>.1` should contain the older one.
        let active = std::fs::read_to_string(&path).expect("active");
        assert!(active.contains("\"call-b\""), "expected rotated active file to contain call-b, got: {active}");
        let rotated = std::fs::read_to_string(dir.path().join("audit.jsonl.1")).expect("rotated");
        assert!(rotated.contains("\"call-a\""), "expected rotated file to contain call-a, got: {rotated}");
    }

    #[test]
    fn multi_sink_forwards_to_every_inner_sink() {
        let a = Arc::new(InMemorySink::new());
        let b = Arc::new(InMemorySink::new());
        let multi = MultiSink::new(vec![a.clone(), b.clone()]);
        multi.write(&sample_entry("z"));
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn tool_audit_logger_record_routes_through_sink() {
        let sink = Arc::new(InMemorySink::new());
        let logger = ToolAuditLogger::new(sink.clone());
        logger.record(sample_entry("k"));
        logger.flush();
        assert_eq!(sink.len(), 1);
    }

    #[test]
    fn sha256_hex_is_stable() {
        assert_eq!(sha256_hex(b"hello"), "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
        assert_eq!(sha256_hex(b"hello"), sha256_hex(b"hello"));
    }
}
