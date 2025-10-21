use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// Budget awareness for routing decisions
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ResourceBudget {
    /// Max tokens per request (soft cap)
    #[serde(default)]
    pub max_tokens: Option<usize>,
    /// Maximum parallel tool calls allowed
    #[serde(default)]
    pub max_parallel_tools: Option<usize>,
    /// Max latency target in milliseconds (advisory)
    #[serde(default)]
    pub latency_ms_target: Option<u64>,
}

/// Map from a complexity label to a model identifier
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ComplexityModelMap {
    /// Simple, quick tasks
    #[serde(default)]
    pub simple: String,
    /// Standard single-turn tasks
    #[serde(default)]
    pub standard: String,
    /// Complex, multi-step reasoning
    #[serde(default)]
    pub complex: String,
    /// Code-generation heavy tasks (diffs, patches)
    #[serde(default)]
    pub codegen_heavy: String,
    /// Retrieval/search heavy tasks
    #[serde(default)]
    pub retrieval_heavy: String,
}

/// Tunable thresholds for heuristic task classification
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HeuristicSettings {
    /// Maximum characters treated as a "simple" request
    #[serde(default = "default_short_request_max_chars")]
    pub short_request_max_chars: usize,
    /// Minimum characters before we assume a complex request
    #[serde(default = "default_long_request_min_chars")]
    pub long_request_min_chars: usize,
    /// Indicators that the request contains code or patch operations
    #[serde(default = "default_code_patch_markers")]
    pub code_patch_markers: Vec<String>,
    /// Indicators that the request is retrieval or search heavy
    #[serde(default = "default_retrieval_markers")]
    pub retrieval_markers: Vec<String>,
    /// Indicators that the request is complex or multi-step
    #[serde(default = "default_complex_markers")]
    pub complex_markers: Vec<String>,
}

impl Default for HeuristicSettings {
    fn default() -> Self {
        Self {
            short_request_max_chars: default_short_request_max_chars(),
            long_request_min_chars: default_long_request_min_chars(),
            code_patch_markers: default_code_patch_markers(),
            retrieval_markers: default_retrieval_markers(),
            complex_markers: default_complex_markers(),
        }
    }
}

impl HeuristicSettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.long_request_min_chars > self.short_request_max_chars,
            "Router heuristic long_request_min_chars must be greater than short_request_max_chars"
        );

        ensure!(
            self.code_patch_markers
                .iter()
                .all(|marker| !marker.trim().is_empty()),
            "Router heuristic code_patch_markers must not contain empty entries"
        );
        ensure!(
            self.retrieval_markers
                .iter()
                .all(|marker| !marker.trim().is_empty()),
            "Router heuristic retrieval_markers must not contain empty entries"
        );
        ensure!(
            self.complex_markers
                .iter()
                .all(|marker| !marker.trim().is_empty()),
            "Router heuristic complex_markers must not contain empty entries"
        );

        Ok(())
    }
}

/// Router configuration for dynamic model/engine selection
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RouterConfig {
    /// Enable router decisions for chat/ask commands
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Use heuristics to classify complexity (no extra LLM call)
    #[serde(default = "default_true")]
    pub heuristic_classification: bool,
    /// Optional: allow an LLM-based router step
    #[serde(default)]
    pub llm_router_model: String,
    /// Model mapping per complexity class
    #[serde(default)]
    pub models: ComplexityModelMap,
    /// Budgets used to guide generation parameters per class
    #[serde(default)]
    pub budgets: std::collections::HashMap<String, ResourceBudget>,
    /// Heuristic classification configuration
    #[serde(default)]
    pub heuristics: HeuristicSettings,
}

impl Default for RouterConfig {
    fn default() -> Self {
        use crate::constants::models;
        Self {
            enabled: true,
            heuristic_classification: true,
            llm_router_model: String::new(),
            models: ComplexityModelMap {
                simple: models::google::GEMINI_2_5_FLASH_PREVIEW.to_string(),
                standard: models::google::GEMINI_2_5_FLASH_PREVIEW.to_string(),
                complex: models::google::GEMINI_2_5_PRO.to_string(),
                codegen_heavy: models::google::GEMINI_2_5_PRO.to_string(),
                retrieval_heavy: models::google::GEMINI_2_5_PRO.to_string(),
            },
            budgets: Default::default(),
            heuristics: HeuristicSettings::default(),
        }
    }
}

impl RouterConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        self.heuristics
            .validate()
            .context("Invalid router heuristics")?;

        ensure!(
            !self.models.simple.trim().is_empty(),
            "Router models.simple must not be empty"
        );
        ensure!(
            !self.models.standard.trim().is_empty(),
            "Router models.standard must not be empty"
        );
        ensure!(
            !self.models.complex.trim().is_empty(),
            "Router models.complex must not be empty"
        );
        ensure!(
            !self.models.codegen_heavy.trim().is_empty(),
            "Router models.codegen_heavy must not be empty"
        );
        ensure!(
            !self.models.retrieval_heavy.trim().is_empty(),
            "Router models.retrieval_heavy must not be empty"
        );

        Ok(())
    }
}

fn default_true() -> bool {
    true
}
fn default_enabled() -> bool {
    true
}

fn default_short_request_max_chars() -> usize {
    120
}

fn default_long_request_min_chars() -> usize {
    1200
}

fn default_code_patch_markers() -> Vec<String> {
    vec![
        "```".to_string(),
        "diff --git".to_string(),
        "apply_patch".to_string(),
        "unified diff".to_string(),
        "patch".to_string(),
        "edit_file".to_string(),
        "create_file".to_string(),
    ]
}

fn default_retrieval_markers() -> Vec<String> {
    vec![
        "search".to_string(),
        "web".to_string(),
        "google".to_string(),
        "docs".to_string(),
        "cite".to_string(),
        "source".to_string(),
        "up-to-date".to_string(),
    ]
}

fn default_complex_markers() -> Vec<String> {
    vec![
        "plan".to_string(),
        "multi-step".to_string(),
        "decompose".to_string(),
        "orchestrate".to_string(),
        "architecture".to_string(),
        "benchmark".to_string(),
        "implement end-to-end".to_string(),
        "design api".to_string(),
        "refactor module".to_string(),
        "evaluate".to_string(),
        "tests suite".to_string(),
    ]
}
