//! Tool result metadata and quality scoring
//!
//! Provides metadata about tool result quality, confidence, and usefulness.
//! This allows the agent to make informed decisions about result reliability
//! and prioritize high-quality results in context windows.

use crate::config::constants::tools;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
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
    #[allow(dead_code)]
    pub fn to_static_str(&self) -> &'static str {
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
    pub fn quality_score(&self) -> f32 {
        let weighted = (self.confidence * 0.4)
            + (self.relevance * 0.4)
            + (self.false_positive_likelihood * -0.2);
        weighted.clamp(0.0, 1.0)
    }

    /// Create metadata for a successful tool execution
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
        self.tool_metrics.extend(
            other
                .tool_metrics
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
    }
}

/// Enhanced tool result with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedToolResult {
    /// The actual tool result
    pub value: Value,

    /// Quality metadata
    pub metadata: ResultMetadata,

    /// When result was produced
    pub timestamp: u64,

    /// Tool name that produced this
    pub tool_name: String,

    /// Whether this was from cache
    #[serde(default)]
    pub from_cache: bool,
}

impl EnhancedToolResult {
    pub fn new(value: Value, metadata: ResultMetadata, tool_name: String) -> Self {
        Self {
            value,
            metadata,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            tool_name,
            from_cache: false,
        }
    }

    pub fn from_cache(value: Value, metadata: ResultMetadata, tool_name: String) -> Self {
        Self {
            value,
            metadata,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            tool_name,
            from_cache: true,
        }
    }

    /// Whether this result is useful enough to include
    pub fn is_useful(&self) -> bool {
        self.metadata.quality_score() > 0.3
    }

    /// Whether this result is high quality
    pub fn is_high_quality(&self) -> bool {
        self.metadata.quality_score() > 0.7
    }

    /// Convert to a message-friendly format
    pub fn to_summary(&self) -> String {
        let quality = (self.metadata.quality_score() * 100.0) as u32;
        match self.metadata.completeness {
            ResultCompleteness::Complete => {
                format!(
                    "{} found {} results (confidence: {}%)",
                    self.tool_name, self.metadata.result_count, quality
                )
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
                format!(
                    "{} found results (truncated due to size, confidence: {}%)",
                    self.tool_name, quality
                )
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

/// Scorer for grep results
pub struct GrepScorer;

impl ResultScorer for GrepScorer {
    fn score(&self, result: &Value) -> ResultMetadata {
        let mut metadata = ResultMetadata::default();
        metadata.content_types.push("code".to_string());

        match result {
            Value::Object(map) => {
                // Count matches
                if let Some(matches) = map.get("matches")
                    && let Some(count) = matches.as_array()
                {
                    metadata.result_count = count.len();

                    // High confidence if specific matches
                    metadata.confidence = if count.len() > 5 {
                        0.85
                    } else if !count.is_empty() {
                        0.80
                    } else {
                        1.0 // High confidence in "no matches"
                    };

                    metadata.relevance = 0.75; // Grep results are usually relevant

                    metadata.completeness = if count.len() < 1000 {
                        ResultCompleteness::Complete
                    } else {
                        ResultCompleteness::Partial
                    };

                    // Lower false positive chance for specific patterns
                    metadata.false_positive_likelihood = 0.05;
                }

                // Track line count
                if let Some(lines) = map.get("line_count")
                    && let Some(n) = lines.as_u64()
                {
                    metadata
                        .tool_metrics
                        .insert("line_count".to_string(), Value::Number(n.into()));
                }
            }
            Value::Array(arr) => {
                metadata.result_count = arr.len();
                metadata.confidence = if arr.is_empty() { 1.0 } else { 0.80 };
                metadata.relevance = 0.75;
            }
            _ => {
                metadata = ResultMetadata::error();
            }
        }

        metadata
    }

    fn tool_name(&self) -> &str {
        tools::GREP_FILE
    }
}

/// Scorer for file finding results
pub struct FindScorer;

impl ResultScorer for FindScorer {
    fn score(&self, result: &Value) -> ResultMetadata {
        let mut metadata = ResultMetadata::default();
        metadata.content_types.push("filesystem".to_string());

        match result {
            Value::Object(map) => {
                if let Some(files) = map.get("files")
                    && let Some(file_arr) = files.as_array()
                {
                    metadata.result_count = file_arr.len();
                    metadata.confidence = if file_arr.is_empty() {
                        1.0 // High confidence in "no files"
                    } else {
                        0.90 // Very high confidence in file paths
                    };
                    metadata.relevance = 0.80;
                    metadata.completeness = ResultCompleteness::Complete;
                }
            }
            Value::Array(arr) => {
                metadata.result_count = arr.len();
                metadata.confidence = 0.90;
                metadata.relevance = 0.80;
            }
            _ => {
                metadata = ResultMetadata::error();
            }
        }

        metadata
    }

    fn tool_name(&self) -> &str {
        "find"
    }
}

/// Scorer for shell command results
pub struct ShellScorer;

impl ResultScorer for ShellScorer {
    fn score(&self, result: &Value) -> ResultMetadata {
        let mut metadata = ResultMetadata::default();

        match result {
            Value::Object(map) => {
                // Check for exit code
                let exit_code = map.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(-1);

                // Success means high confidence
                if exit_code == 0 {
                    metadata.confidence = 0.85;
                } else {
                    metadata.confidence = 0.20;
                    metadata.completeness = ResultCompleteness::Empty;
                }

                if let Some(output) = map.get("stdout")
                    && let Some(s) = output.as_str()
                {
                    metadata.result_count = s.lines().count();
                    metadata.relevance = 0.70;
                }
            }
            _ => {
                metadata = ResultMetadata::error();
            }
        }

        metadata
    }

    fn tool_name(&self) -> &str {
        "shell"
    }
}

/// Registry for result scorers
pub struct ScorerRegistry {
    scorers: HashMap<String, Box<dyn ResultScorer>>,
}

impl ScorerRegistry {
    pub fn new() -> Self {
        let mut scorers: HashMap<String, Box<dyn ResultScorer>> = HashMap::new();
        scorers.insert(
            tools::GREP_FILE.to_string(),
            Box::new(GrepScorer) as Box<dyn ResultScorer>,
        );
        scorers.insert(
            "find".to_string(),
            Box::new(FindScorer) as Box<dyn ResultScorer>,
        );
        scorers.insert(
            "shell".to_string(),
            Box::new(ShellScorer) as Box<dyn ResultScorer>,
        );

        Self { scorers }
    }

    /// Register a custom scorer
    pub fn register(&mut self, scorer: Box<dyn ResultScorer>) {
        self.scorers.insert(scorer.tool_name().to_string(), scorer);
    }

    /// Score a tool result
    pub fn score(&self, tool_name: &str, result: &Value) -> ResultMetadata {
        if let Some(scorer) = self.scorers.get(tool_name) {
            scorer.score(result)
        } else {
            // Default scoring for unknown tools
            match result {
                Value::Null => ResultMetadata::empty(),
                Value::Object(_) => ResultMetadata::success(0.6, 0.6),
                Value::Array(arr) => {
                    let mut meta = ResultMetadata::success(0.6, 0.6);
                    meta.result_count = arr.len();
                    meta
                }
                _ => ResultMetadata::success(0.5, 0.5),
            }
        }
    }
}

impl Default for ScorerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_result_completeness() {
        assert_eq!(ResultCompleteness::Complete.to_string(), "complete");
        assert_eq!(ResultCompleteness::Partial.to_string(), "partial");
        assert_eq!(ResultCompleteness::Empty.to_string(), "empty");
    }

    #[test]
    fn test_quality_score() {
        let meta = ResultMetadata {
            confidence: 0.8,
            relevance: 0.8,
            false_positive_likelihood: 0.1,
            ..Default::default()
        };

        let score = meta.quality_score();
        assert!(score > 0.6 && score < 0.8);
    }

    #[test]
    fn test_enhanced_result_is_useful() {
        let result = EnhancedToolResult::new(
            json!({"matches": []}),
            ResultMetadata::success(0.8, 0.8),
            tools::GREP_FILE.to_string(),
        );

        assert!(result.is_useful());
        assert!(result.is_high_quality());
    }

    #[test]
    fn test_grep_scorer() {
        let scorer = GrepScorer;
        let result = json!({
            "matches": ["line1", "line2", "line3"],
            "line_count": 100
        });

        let meta = scorer.score(&result);
        assert_eq!(meta.result_count, 3);
        assert!(meta.confidence > 0.7);
    }

    #[test]
    fn test_scorer_registry() {
        let registry = ScorerRegistry::new();
        let result = json!({"files": ["a.txt", "b.txt"]});

        let meta = registry.score("find", &result);
        assert_eq!(meta.result_count, 2);
    }
}
