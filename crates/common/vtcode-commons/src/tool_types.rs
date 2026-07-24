//! Shared runtime types for the VT Code tool system.
//!
//! This module provides types shared between the LLM and tools subsystems,
//! breaking the circular dependency that would otherwise exist between them.
//!
//! # Overview
//!
//! The key types here are:
//! - [`CompactStr`] - stack-allocated string for short tool names
//! - [`EnhancedToolResult`] - tool result with quality metadata
//! - [`ResultMetadata`] - quality/confidence scoring for tool results
//! - [`tool_names`] - tool name constants used across subsystems

// ---------------------------------------------------------------------------
// CompactStr type alias
// ---------------------------------------------------------------------------

/// Compact inline string -- stack-allocated for strings up to 24 bytes.
/// Drop-in replacement for `String` with zero heap allocation for short strings.
pub type CompactStr = compact_str::CompactString;

// ---------------------------------------------------------------------------
// Tool name constants (canonical names used across subsystems)
// ---------------------------------------------------------------------------

/// Canonical tool name constants used by both LLM and tools subsystems.
/// These match the values defined in `vtcode-config::constants::tools`.
pub mod tool_names {
    /// Advanced bounded source search tool
    pub const CODE_SEARCH: &str = "code_search";
    /// Shell command execution tool
    pub const EXEC_COMMAND: &str = "exec_command";
}

/// Use direct tool name without alias resolution.
/// Alias resolution is now handled by the tool registry inventory
/// which maintains a mapping of aliases to canonical tool names.
pub const fn canonical_tool_name(name: &str) -> &str {
    name
}

// ---------------------------------------------------------------------------
// Operational constants shared across subsystems
// ---------------------------------------------------------------------------

/// Standard error patterns used for error detection across tools
pub const ERROR_DETECTION_PATTERNS: &[&str] = &[
    "error",
    "failed",
    "exception",
    "permission denied",
    "not found",
    "no such file",
    "cannot",
    "could not",
    "panic",
    "crash",
    "unhandled",
    "fatal",
    "timeout",
    "connection refused",
    "access denied",
    "stack trace",
    "traceback",
    "abort",
    "terminate",
];

/// Network-related error patterns for more specific error detection
pub const NETWORK_ERROR_PATTERNS: &[&str] = &["connection", "timeout", "network", "http", "ssl", "tls", "dns", "proxy"];

/// Default capacity hints for common collections
pub const DEFAULT_VEC_CAPACITY: usize = 32;
pub const DEFAULT_HASHMAP_CAPACITY: usize = 16;
pub const DEFAULT_STRING_CAPACITY: usize = 256;

/// Context optimization constants following AGENTS.md guidelines
pub const MAX_SEARCH_RESULTS: usize = 5;
pub const MAX_LIST_ITEMS_SUMMARY: usize = 5;
pub const OVERFLOW_INDICATOR_PREFIX: &str = "[+]";
pub const OVERFLOW_INDICATOR_SUFFIX: &str = "more items]";

/// Common tool operation limits
pub const MAX_FILE_SIZE_FOR_PROCESSING: usize = 100 * 1024 * 1024; // 100MB
pub const MAX_CONTEXT_LINES: usize = 20;
pub const MAX_OUTPUT_TOKENS: usize = 4000;

/// Reusable empty JSON object schema `{"type": "object"}` for tool parameter definitions.
/// Used by tools that accept no parameters or only optional parameters.
pub fn empty_object_schema() -> Value {
    serde_json::json!({"type": "object"})
}

// ---------------------------------------------------------------------------
// Tool result metadata types
// ---------------------------------------------------------------------------

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::time::SystemTime;

/// Result completeness level
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResultCompleteness {
    /// Full result with no truncation
    Complete,
    /// Partial result (more data exists but not shown)
    Partial,
    /// Result truncated due to size limits
    Truncated,
    /// Empty result (no matches)
    Empty,
}

impl ResultCompleteness {
    /// Deprecated: prefer using the `Display` impl; `ToString` is derived from Display.
    fn to_static_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Truncated => "truncated",
            Self::Empty => "empty",
        }
    }
}

impl fmt::Display for ResultCompleteness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_static_str())
    }
}

/// Quality metadata for tool results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultMetadata {
    /// Confidence that result is correct (0.0-1.0)
    #[serde(default = "default_confidence")]
    pub confidence: f32,

    /// Relevance to current task (0.0-1.0)
    #[serde(default = "default_relevance")]
    pub relevance: f32,

    /// Result completeness level
    pub completeness: ResultCompleteness,

    /// Count of matches/results
    #[serde(default)]
    pub result_count: usize,

    /// Likelihood of false positives (0.0-1.0)
    #[serde(default)]
    pub false_positive_likelihood: f32,

    /// Detected content types (code, docs, config, binary, etc.)
    #[serde(default)]
    pub content_types: Vec<String>,

    /// Tool-specific metrics (lines matched, execution time, etc.)
    #[serde(default)]
    pub tool_metrics: HashMap<String, Value>,
}

fn default_confidence() -> f32 {
    0.5
}

fn default_relevance() -> f32 {
    0.5
}

impl Default for ResultMetadata {
    fn default() -> Self {
        Self {
            confidence: 0.5,
            relevance: 0.5,
            completeness: ResultCompleteness::Complete,
            result_count: 0,
            false_positive_likelihood: 0.1,
            content_types: vec![],
            tool_metrics: HashMap::new(),
        }
    }
}

impl ResultMetadata {
    /// Overall quality score (0.0-1.0)
    #[inline]
    pub fn quality_score(&self) -> f32 {
        let weighted = (self.confidence * 0.4) + (self.relevance * 0.4) + (self.false_positive_likelihood * -0.2);
        weighted.clamp(0.0, 1.0)
    }

    /// Create metadata for a successful tool execution
    #[inline]
    pub fn success(confidence: f32, relevance: f32) -> Self {
        Self {
            confidence: confidence.clamp(0.0, 1.0),
            relevance: relevance.clamp(0.0, 1.0),
            completeness: ResultCompleteness::Complete,
            result_count: 1,
            false_positive_likelihood: 0.05,
            ..Default::default()
        }
    }

    /// Create metadata for empty results
    #[inline]
    pub fn empty() -> Self {
        Self {
            completeness: ResultCompleteness::Empty,
            result_count: 0,
            confidence: 1.0, // High confidence in "no results"
            ..Default::default()
        }
    }

    /// Create metadata for error/inconclusive results
    pub fn error() -> Self {
        Self {
            confidence: 0.2,
            completeness: ResultCompleteness::Empty,
            ..Default::default()
        }
    }

    /// Merge with another metadata (for combining results)
    pub fn merge(&mut self, other: &ResultMetadata) {
        self.result_count += other.result_count;
        self.confidence = (self.confidence + other.confidence) / 2.0;
        self.relevance = (self.relevance + other.relevance) / 2.0;

        // Merge content types
        for ct in &other.content_types {
            if !self.content_types.contains(ct) {
                self.content_types.push(ct.clone());
            }
        }

        // Merge tool metrics - use extend to avoid double clone
        self.tool_metrics
            .extend(other.tool_metrics.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
}

/// Enhanced tool result with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedToolResult {
    /// The actual tool result
    value: Value,

    /// Quality metadata
    pub metadata: ResultMetadata,

    /// When result was produced
    timestamp: u64,

    /// Tool name that produced this
    tool_name: CompactStr,

    /// Whether this was from cache
    #[serde(default)]
    from_cache: bool,
}

impl EnhancedToolResult {
    pub fn new(value: Value, metadata: ResultMetadata, tool_name: impl Into<CompactStr>) -> Self {
        Self {
            value,
            metadata,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            tool_name: tool_name.into(),
            from_cache: false,
        }
    }

    pub fn from_cache(value: Value, metadata: ResultMetadata, tool_name: impl Into<CompactStr>) -> Self {
        Self {
            value,
            metadata,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            tool_name: tool_name.into(),
            from_cache: true,
        }
    }

    /// Whether this result is useful enough to include
    #[inline]
    pub fn is_useful(&self) -> bool {
        self.metadata.quality_score() > 0.3
    }

    /// Whether this result is high quality
    #[inline]
    pub fn is_high_quality(&self) -> bool {
        self.metadata.quality_score() > 0.7
    }

    /// Convert to a message-friendly format
    #[allow(clippy::cast_sign_loss)] // quality_score is always 0.0-1.0
    pub fn to_summary(&self) -> String {
        let quality = ((self.metadata.quality_score() * 100.0).round().max(0.0) as u32).min(100);
        match self.metadata.completeness {
            ResultCompleteness::Complete => {
                format!("{} found {} results (confidence: {}%)", self.tool_name, self.metadata.result_count, quality)
            }
            ResultCompleteness::Partial => {
                format!(
                    "{} found {} results (truncated, confidence: {}%)",
                    self.tool_name, self.metadata.result_count, quality
                )
            }
            ResultCompleteness::Empty => {
                format!("{} found no results", self.tool_name)
            }
            ResultCompleteness::Truncated => {
                format!("{} found results (truncated due to size, confidence: {}%)", self.tool_name, quality)
            }
        }
    }
}

/// Trait for scoring tool results
pub trait ResultScorer {
    /// Score a tool result and return metadata
    fn score(&self, result: &Value) -> ResultMetadata;

    /// Tool name this scorer handles
    fn tool_name(&self) -> &str;
}
