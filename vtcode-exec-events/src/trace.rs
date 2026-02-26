//! Agent Trace specification types for AI code attribution.
//!
//! This module implements the [Agent Trace](https://agent-trace.dev/) specification v0.1.0,
//! providing vendor-neutral types for recording AI contributions alongside human authorship
//! in version-controlled codebases.
//!
//! # Overview
//!
//! Agent Trace defines how to track which code came from AI versus humans with:
//! - Line-level granularity for attribution
//! - Conversation linkage for provenance
//! - VCS integration for revision tracking
//! - Extensible metadata for vendor-specific data
//!
//! # Example
//!
//! ```rust
//! use vtcode_exec_events::trace::*;
//! use uuid::Uuid;
//! use chrono::Utc;
//!
//! let trace = TraceRecord {
//!     version: AGENT_TRACE_VERSION.to_string(),
//!     id: Uuid::new_v4(),
//!     timestamp: Utc::now(),
//!     vcs: Some(VcsInfo {
//!         vcs_type: VcsType::Git,
//!         revision: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0".to_string(),
//!     }),
//!     tool: Some(ToolInfo {
//!         name: "vtcode".to_string(),
//!         version: Some(env!("CARGO_PKG_VERSION").to_string()),
//!     }),
//!     files: vec![],
//!     metadata: None,
//! };
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

/// Current Agent Trace specification version.
pub const AGENT_TRACE_VERSION: &str = "0.1.0";

/// MIME type for Agent Trace records.
pub const AGENT_TRACE_MIME_TYPE: &str = "application/vnd.agent-trace.record+json";

// ============================================================================
// Core Types
// ============================================================================

/// A complete Agent Trace record tracking AI contributions to code.
///
/// This is the fundamental unit of Agent Trace - a snapshot of attribution
/// data for files changed in a specific revision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TraceRecord {
    /// Agent Trace specification version (e.g., "0.1.0").
    pub version: String,

    /// Unique identifier for this trace record (UUID v4).
    #[serde(with = "uuid_serde")]
    pub id: uuid::Uuid,

    /// RFC 3339 timestamp when trace was recorded.
    pub timestamp: DateTime<Utc>,

    /// Version control system information for this trace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vcs: Option<VcsInfo>,

    /// The tool that generated this trace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<ToolInfo>,

    /// Array of files with attributed ranges.
    pub files: Vec<TraceFile>,

    /// Additional metadata for implementation-specific or vendor-specific data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<TraceMetadata>,
}

impl TraceRecord {
    /// Create a new trace record with required fields.
    pub fn new() -> Self {
        Self {
            version: AGENT_TRACE_VERSION.to_string(),
            id: uuid::Uuid::new_v4(),
            timestamp: Utc::now(),
            vcs: None,
            tool: Some(ToolInfo::vtcode()),
            files: Vec::new(),
            metadata: None,
        }
    }

    /// Create a trace record for a specific git revision.
    pub fn for_git_revision(revision: impl Into<String>) -> Self {
        let mut trace = Self::new();
        trace.vcs = Some(VcsInfo::git(revision));
        trace
    }

    /// Add a file to the trace record.
    pub fn add_file(&mut self, file: TraceFile) {
        self.files.push(file);
    }

    /// Check if the trace has any attributed ranges.
    pub fn has_attributions(&self) -> bool {
        self.files
            .iter()
            .any(|f| f.conversations.iter().any(|c| !c.ranges.is_empty()))
    }
}

impl Default for TraceRecord {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// VCS Types
// ============================================================================

/// Version control system information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct VcsInfo {
    /// Version control system type.
    #[serde(rename = "type")]
    pub vcs_type: VcsType,

    /// Revision identifier (e.g., git commit SHA, jj change ID).
    pub revision: String,
}

impl VcsInfo {
    /// Create VCS info for a git repository.
    pub fn git(revision: impl Into<String>) -> Self {
        Self {
            vcs_type: VcsType::Git,
            revision: revision.into(),
        }
    }

    /// Create VCS info for a Jujutsu repository.
    pub fn jj(change_id: impl Into<String>) -> Self {
        Self {
            vcs_type: VcsType::Jj,
            revision: change_id.into(),
        }
    }
}

/// Supported version control system types.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum VcsType {
    /// Git version control.
    Git,
    /// Jujutsu (jj) version control.
    Jj,
    /// Mercurial version control.
    Hg,
    /// Subversion.
    Svn,
}

// ============================================================================
// Tool Types
// ============================================================================

/// Information about the tool that generated the trace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ToolInfo {
    /// Name of the tool.
    pub name: String,

    /// Version of the tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl ToolInfo {
    /// Create tool info for VT Code.
    pub fn vtcode() -> Self {
        Self {
            name: "vtcode".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }
    }

    /// Create custom tool info.
    pub fn new(name: impl Into<String>, version: Option<String>) -> Self {
        Self {
            name: name.into(),
            version,
        }
    }
}

// ============================================================================
// File Attribution Types
// ============================================================================

/// A file with attributed conversation ranges.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TraceFile {
    /// Relative file path from repository root.
    pub path: String,

    /// Array of conversations that contributed to this file.
    pub conversations: Vec<TraceConversation>,
}

impl TraceFile {
    /// Create a new trace file entry.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            conversations: Vec::new(),
        }
    }

    /// Add a conversation to the file.
    pub fn add_conversation(&mut self, conversation: TraceConversation) {
        self.conversations.push(conversation);
    }

    /// Create a file with a single AI-attributed conversation.
    pub fn with_ai_ranges(
        path: impl Into<String>,
        model_id: impl Into<String>,
        ranges: Vec<TraceRange>,
    ) -> Self {
        let mut file = Self::new(path);
        file.add_conversation(TraceConversation {
            url: None,
            contributor: Some(Contributor::ai(model_id)),
            ranges,
            related: None,
        });
        file
    }
}

/// A conversation that contributed code to a file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TraceConversation {
    /// URL to look up the conversation that produced this code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// The contributor for ranges in this conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contributor: Option<Contributor>,

    /// Array of line ranges produced by this conversation.
    pub ranges: Vec<TraceRange>,

    /// Other related resources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related: Option<Vec<RelatedResource>>,
}

impl TraceConversation {
    /// Create a conversation with AI contributor.
    pub fn ai(model_id: impl Into<String>, ranges: Vec<TraceRange>) -> Self {
        Self {
            url: None,
            contributor: Some(Contributor::ai(model_id)),
            ranges,
            related: None,
        }
    }

    /// Create a conversation with session URL.
    pub fn with_session_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }
}

/// A related resource linked to a conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct RelatedResource {
    /// Type of the related resource.
    #[serde(rename = "type")]
    pub resource_type: String,

    /// URL of the related resource.
    pub url: String,
}

impl RelatedResource {
    /// Create a session resource link.
    pub fn session(url: impl Into<String>) -> Self {
        Self {
            resource_type: "session".to_string(),
            url: url.into(),
        }
    }

    /// Create a prompt resource link.
    pub fn prompt(url: impl Into<String>) -> Self {
        Self {
            resource_type: "prompt".to_string(),
            url: url.into(),
        }
    }
}

// ============================================================================
// Range Attribution Types
// ============================================================================

/// A range of lines with attribution information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TraceRange {
    /// Start line number (1-indexed, inclusive).
    pub start_line: u32,

    /// End line number (1-indexed, inclusive).
    pub end_line: u32,

    /// Hash of attributed content for position-independent tracking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,

    /// Override contributor for this specific range (e.g., for agent handoffs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contributor: Option<Contributor>,
}

impl TraceRange {
    /// Create a new range.
    pub fn new(start_line: u32, end_line: u32) -> Self {
        Self {
            start_line,
            end_line,
            content_hash: None,
            contributor: None,
        }
    }

    /// Create a range for a single line.
    pub fn single_line(line: u32) -> Self {
        Self::new(line, line)
    }

    /// Add a content hash to the range.
    pub fn with_hash(mut self, hash: impl Into<String>) -> Self {
        self.content_hash = Some(hash.into());
        self
    }

    /// Compute and set content hash from content using MurmurHash3.
    pub fn with_content_hash(mut self, content: &str) -> Self {
        let hash = compute_content_hash(content);
        self.content_hash = Some(hash);
        self
    }
}

// ============================================================================
// Contributor Types
// ============================================================================

/// The contributor that produced a code contribution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct Contributor {
    /// Type of contributor.
    #[serde(rename = "type")]
    pub contributor_type: ContributorType,

    /// Model identifier following models.dev convention (e.g., "anthropic/claude-opus-4-5-20251101").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

impl Contributor {
    /// Create an AI contributor with model ID.
    pub fn ai(model_id: impl Into<String>) -> Self {
        Self {
            contributor_type: ContributorType::Ai,
            model_id: Some(model_id.into()),
        }
    }

    /// Create a human contributor.
    pub fn human() -> Self {
        Self {
            contributor_type: ContributorType::Human,
            model_id: None,
        }
    }

    /// Create a mixed contributor (human-edited AI or AI-edited human).
    pub fn mixed() -> Self {
        Self {
            contributor_type: ContributorType::Mixed,
            model_id: None,
        }
    }

    /// Create an unknown contributor.
    pub fn unknown() -> Self {
        Self {
            contributor_type: ContributorType::Unknown,
            model_id: None,
        }
    }
}

/// Type of contributor for code attribution.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum ContributorType {
    /// Code authored directly by a human developer.
    Human,
    /// Code generated by AI.
    Ai,
    /// Human-edited AI output or AI-edited human code.
    Mixed,
    /// Origin cannot be determined.
    Unknown,
}

// ============================================================================
// Metadata Types
// ============================================================================

/// Additional metadata for trace records.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TraceMetadata {
    /// Confidence score for the attribution (0.0 - 1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,

    /// Post-processing tools applied to the code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_processing_tools: Option<Vec<String>>,

    /// VT Code specific metadata.
    #[serde(rename = "dev.vtcode", skip_serializing_if = "Option::is_none")]
    pub vtcode: Option<VtCodeMetadata>,

    /// Additional vendor-specific data.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// VT Code specific metadata in traces.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct VtCodeMetadata {
    /// Session ID that produced this trace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Turn number within the session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_number: Option<u32>,

    /// Workspace path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,

    /// Provider name (anthropic, openai, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Hash algorithm for content hashes.
#[derive(Debug, Clone, Copy, Default)]
pub enum HashAlgorithm {
    /// MurmurHash3 (recommended by Agent Trace spec for cross-tool compatibility).
    #[default]
    MurmurHash3,
    /// FNV-1a (simple and fast fallback).
    Fnv1a,
}

/// Compute a content hash using the default algorithm (MurmurHash3).
///
/// MurmurHash3 is recommended by the Agent Trace spec for cross-tool compatibility.
pub fn compute_content_hash(content: &str) -> String {
    compute_content_hash_with(content, HashAlgorithm::default())
}

/// Compute a content hash using the specified algorithm.
pub fn compute_content_hash_with(content: &str, algorithm: HashAlgorithm) -> String {
    match algorithm {
        HashAlgorithm::MurmurHash3 => {
            // MurmurHash3 x86_32 implementation
            let hash = murmur3_32(content.as_bytes(), 0);
            format!("murmur3:{hash:08x}")
        }
        HashAlgorithm::Fnv1a => {
            const FNV_OFFSET: u64 = 14695981039346656037;
            const FNV_PRIME: u64 = 1099511628211;
            let mut hash = FNV_OFFSET;
            for byte in content.bytes() {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(FNV_PRIME);
            }
            format!("fnv1a:{hash:016x}")
        }
    }
}

/// MurmurHash3 x86_32 implementation.
fn murmur3_32(data: &[u8], seed: u32) -> u32 {
    const C1: u32 = 0xcc9e2d51;
    const C2: u32 = 0x1b873593;
    const R1: u32 = 15;
    const R2: u32 = 13;
    const M: u32 = 5;
    const N: u32 = 0xe6546b64;

    let mut hash = seed;
    let len = data.len();
    let chunks = len / 4;

    // Process 4-byte chunks
    for i in 0..chunks {
        let idx = i * 4;
        let mut k = u32::from_le_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]);
        k = k.wrapping_mul(C1);
        k = k.rotate_left(R1);
        k = k.wrapping_mul(C2);
        hash ^= k;
        hash = hash.rotate_left(R2);
        hash = hash.wrapping_mul(M).wrapping_add(N);
    }

    // Process remaining bytes
    let tail = &data[chunks * 4..];
    let mut k1: u32 = 0;
    match tail.len() {
        3 => {
            k1 ^= (tail[2] as u32) << 16;
            k1 ^= (tail[1] as u32) << 8;
            k1 ^= tail[0] as u32;
        }
        2 => {
            k1 ^= (tail[1] as u32) << 8;
            k1 ^= tail[0] as u32;
        }
        1 => {
            k1 ^= tail[0] as u32;
        }
        _ => {}
    }
    if !tail.is_empty() {
        k1 = k1.wrapping_mul(C1);
        k1 = k1.rotate_left(R1);
        k1 = k1.wrapping_mul(C2);
        hash ^= k1;
    }

    // Finalization
    hash ^= len as u32;
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x85ebca6b);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(0xc2b2ae35);
    hash ^= hash >> 16;

    hash
}

/// Convert a model string to models.dev convention format.
///
/// # Example
/// ```rust
/// use vtcode_exec_events::trace::normalize_model_id;
///
/// assert_eq!(
///     normalize_model_id("claude-3-opus-20240229", "anthropic"),
///     "anthropic/claude-3-opus-20240229"
/// );
/// ```
pub fn normalize_model_id(model: &str, provider: &str) -> String {
    if model.contains('/') {
        model.to_string()
    } else {
        format!("{provider}/{model}")
    }
}

// ============================================================================
// Serialization Helpers
// ============================================================================

mod uuid_serde {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use uuid::Uuid;

    pub fn serialize<S>(uuid: &Uuid, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&uuid.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Uuid, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Uuid::parse_str(&s).map_err(serde::de::Error::custom)
    }
}

// ============================================================================
// Builder Pattern
// ============================================================================

/// Builder for constructing trace records incrementally.
#[derive(Debug, Default)]
pub struct TraceRecordBuilder {
    vcs: Option<VcsInfo>,
    tool: Option<ToolInfo>,
    files: Vec<TraceFile>,
    metadata: Option<TraceMetadata>,
}

impl TraceRecordBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set VCS information.
    pub fn vcs(mut self, vcs: VcsInfo) -> Self {
        self.vcs = Some(vcs);
        self
    }

    /// Set git revision.
    pub fn git_revision(mut self, revision: impl Into<String>) -> Self {
        self.vcs = Some(VcsInfo::git(revision));
        self
    }

    /// Set tool information.
    pub fn tool(mut self, tool: ToolInfo) -> Self {
        self.tool = Some(tool);
        self
    }

    /// Add a file.
    pub fn file(mut self, file: TraceFile) -> Self {
        self.files.push(file);
        self
    }

    /// Set metadata.
    pub fn metadata(mut self, metadata: TraceMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Build the trace record.
    pub fn build(self) -> TraceRecord {
        TraceRecord {
            version: AGENT_TRACE_VERSION.to_string(),
            id: uuid::Uuid::new_v4(),
            timestamp: Utc::now(),
            vcs: self.vcs,
            tool: self.tool.or_else(|| Some(ToolInfo::vtcode())),
            files: self.files,
            metadata: self.metadata,
        }
    }
}

// ============================================================================
// Conversion from TurnDiffTracker
// ============================================================================

/// Information needed to create a trace from file changes.
#[derive(Debug, Clone)]
pub struct TraceContext {
    /// Git revision (commit SHA).
    pub revision: Option<String>,
    /// Session ID for conversation URL.
    pub session_id: Option<String>,
    /// Model ID in provider/model format.
    pub model_id: String,
    /// Provider name.
    pub provider: String,
    /// Turn number.
    pub turn_number: Option<u32>,
    /// Workspace path for resolving relative paths.
    pub workspace_path: Option<PathBuf>,
}

impl TraceContext {
    /// Create a new trace context.
    pub fn new(model_id: impl Into<String>, provider: impl Into<String>) -> Self {
        Self {
            revision: None,
            session_id: None,
            model_id: model_id.into(),
            provider: provider.into(),
            turn_number: None,
            workspace_path: None,
        }
    }

    /// Set the git revision.
    pub fn with_revision(mut self, revision: impl Into<String>) -> Self {
        self.revision = Some(revision.into());
        self
    }

    /// Set the session ID.
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Set the turn number.
    pub fn with_turn_number(mut self, turn: u32) -> Self {
        self.turn_number = Some(turn);
        self
    }

    /// Set the workspace path.
    pub fn with_workspace_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.workspace_path = Some(path.into());
        self
    }

    /// Get the normalized model ID.
    pub fn normalized_model_id(&self) -> String {
        normalize_model_id(&self.model_id, &self.provider)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_record_creation() {
        let trace = TraceRecord::new();
        assert_eq!(trace.version, AGENT_TRACE_VERSION);
        assert!(trace.tool.is_some());
        assert!(trace.files.is_empty());
    }

    #[test]
    fn test_trace_record_for_git() {
        let trace = TraceRecord::for_git_revision("abc123");
        assert!(trace.vcs.is_some());
        let vcs = trace.vcs.as_ref().expect("trace.vcs is None");
        assert_eq!(vcs.vcs_type, VcsType::Git);
        assert_eq!(vcs.revision, "abc123");
    }

    #[test]
    fn test_contributor_types() {
        let ai = Contributor::ai("anthropic/claude-opus-4");
        assert_eq!(ai.contributor_type, ContributorType::Ai);
        assert_eq!(ai.model_id, Some("anthropic/claude-opus-4".to_string()));

        let human = Contributor::human();
        assert_eq!(human.contributor_type, ContributorType::Human);
        assert!(human.model_id.is_none());
    }

    #[test]
    fn test_trace_range() {
        let range = TraceRange::new(10, 25);
        assert_eq!(range.start_line, 10);
        assert_eq!(range.end_line, 25);

        let range_with_hash = range.with_content_hash("hello world");
        assert!(range_with_hash.content_hash.is_some());
        // Default is MurmurHash3 per Agent Trace spec
        assert!(
            range_with_hash
                .content_hash
                .unwrap()
                .starts_with("murmur3:")
        );
    }

    #[test]
    fn test_hash_algorithms() {
        let murmur = compute_content_hash_with("hello world", HashAlgorithm::MurmurHash3);
        assert!(murmur.starts_with("murmur3:"));

        let fnv = compute_content_hash_with("hello world", HashAlgorithm::Fnv1a);
        assert!(fnv.starts_with("fnv1a:"));

        // Default should be MurmurHash3
        let default_hash = compute_content_hash("hello world");
        assert_eq!(default_hash, murmur);
    }

    #[test]
    fn test_trace_file_builder() {
        let file = TraceFile::with_ai_ranges(
            "src/main.rs",
            "anthropic/claude-opus-4",
            vec![TraceRange::new(1, 50)],
        );
        assert_eq!(file.path, "src/main.rs");
        assert_eq!(file.conversations.len(), 1);
    }

    #[test]
    fn test_normalize_model_id() {
        assert_eq!(
            normalize_model_id("claude-3-opus", "anthropic"),
            "anthropic/claude-3-opus"
        );
        assert_eq!(
            normalize_model_id("anthropic/claude-3-opus", "anthropic"),
            "anthropic/claude-3-opus"
        );
    }

    #[test]
    fn test_trace_record_builder() {
        let trace = TraceRecordBuilder::new()
            .git_revision("abc123def456")
            .file(TraceFile::with_ai_ranges(
                "src/lib.rs",
                "openai/gpt-5",
                vec![TraceRange::new(10, 20)],
            ))
            .build();

        assert!(trace.vcs.is_some());
        assert_eq!(trace.files.len(), 1);
        assert!(trace.has_attributions());
    }

    #[test]
    fn test_trace_serialization() {
        let trace = TraceRecord::for_git_revision("abc123");
        let json = serde_json::to_string_pretty(&trace).expect("Failed to serialize trace to JSON");
        assert!(json.contains("\"version\": \"0.1.0\""));
        assert!(json.contains("abc123"));

        let restored: TraceRecord =
            serde_json::from_str(&json).expect("Failed to deserialize trace from JSON");
        assert_eq!(restored.version, trace.version);
    }

    #[test]
    fn test_content_hash_consistency() {
        // MurmurHash3 (default)
        let hash1 = compute_content_hash("hello world");
        let hash2 = compute_content_hash("hello world");
        assert_eq!(hash1, hash2);

        let hash3 = compute_content_hash("hello world!");
        assert_ne!(hash1, hash3);

        // FNV-1a
        let fnv1 = compute_content_hash_with("test", HashAlgorithm::Fnv1a);
        let fnv2 = compute_content_hash_with("test", HashAlgorithm::Fnv1a);
        assert_eq!(fnv1, fnv2);
    }
}
