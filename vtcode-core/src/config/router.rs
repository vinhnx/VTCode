use serde::{Deserialize, Serialize};

/// Budget awareness for routing decisions
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

/// Router configuration for dynamic model/engine selection
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
}

impl Default for RouterConfig {
    fn default() -> Self {
        use crate::config::constants::models;
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
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_enabled() -> bool {
    true
}
