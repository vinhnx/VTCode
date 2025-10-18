//! Token budget management for context engineering
//!
//! This module implements token counting and budget tracking to manage
//! the attention budget of LLMs. Following Anthropic's context engineering
//! principles, it helps prevent context rot by tracking token usage and
//! triggering compaction when thresholds are exceeded.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokenizers::Tokenizer;
use tokio::sync::RwLock;
use tokio::task;
use tracing::{debug, warn};

/// Token budget configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBudgetConfig {
    /// Maximum tokens allowed in context window
    pub max_context_tokens: usize,
    /// Threshold percentage to trigger warnings (0.0-1.0)
    pub warning_threshold: f64,
    /// Threshold percentage to trigger compaction (0.0-1.0)
    pub compaction_threshold: f64,
    /// Model name for tokenizer selection
    pub model: String,
    /// Optional override for tokenizer identifier or local path
    pub tokenizer_id: Option<String>,
    /// Enable detailed token tracking
    pub detailed_tracking: bool,
}

impl Default for TokenBudgetConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: 128_000,
            warning_threshold: 0.75,
            compaction_threshold: 0.85,
            model: "gpt-4".to_string(),
            tokenizer_id: None,
            detailed_tracking: false,
        }
    }
}

impl TokenBudgetConfig {
    /// Create config for specific model
    pub fn for_model(model: &str, max_tokens: usize) -> Self {
        Self {
            max_context_tokens: max_tokens,
            model: model.to_string(),
            tokenizer_id: None,
            ..Default::default()
        }
    }
}

/// Token usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageStats {
    pub total_tokens: usize,
    pub system_prompt_tokens: usize,
    pub user_messages_tokens: usize,
    pub assistant_messages_tokens: usize,
    pub tool_results_tokens: usize,
    pub decision_ledger_tokens: usize,
    pub timestamp: u64,
}

impl TokenUsageStats {
    pub fn new() -> Self {
        Self {
            total_tokens: 0,
            system_prompt_tokens: 0,
            user_messages_tokens: 0,
            assistant_messages_tokens: 0,
            tool_results_tokens: 0,
            decision_ledger_tokens: 0,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Calculate percentage of max context used
    pub fn usage_percentage(&self, max_tokens: usize) -> f64 {
        self.usage_ratio(max_tokens) * 100.0
    }

    /// Calculate ratio (0.0-1.0) of max context used
    pub fn usage_ratio(&self, max_tokens: usize) -> f64 {
        if max_tokens == 0 || self.total_tokens == 0 {
            return 0.0;
        }
        self.total_tokens as f64 / max_tokens as f64
    }

    /// Check if compaction is needed
    pub fn needs_compaction(&self, max_tokens: usize, threshold: f64) -> bool {
        self.usage_ratio(max_tokens) >= threshold
    }
}

impl Default for TokenUsageStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Component types for detailed tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContextComponent {
    SystemPrompt,
    UserMessage,
    AssistantMessage,
    ToolResult,
    DecisionLedger,
    ProjectGuidelines,
    FileContent,
}

#[derive(Clone)]
enum TokenCounter {
    HuggingFace(Arc<Tokenizer>),
    Approximate,
}

impl TokenCounter {
    fn huggingface(tokenizer: Tokenizer) -> Self {
        Self::HuggingFace(Arc::new(tokenizer))
    }

    fn count_tokens(&self, text: &str) -> Result<usize> {
        if text.is_empty() {
            return Ok(0);
        }

        match self {
            TokenCounter::HuggingFace(tokenizer) => {
                let encoding = tokenizer
                    .encode(text, true)
                    .map_err(|err| anyhow!("Tokenizer encode failed: {err}"))?;
                Ok(encoding.len())
            }
            TokenCounter::Approximate => Ok(approximate_token_count(text)),
        }
    }
}

enum TokenizerSpec {
    LocalFile(PathBuf),
    Pretrained {
        id: String,
        revision: Option<String>,
    },
}

/// Token budget manager
pub struct TokenBudgetManager {
    config: Arc<RwLock<TokenBudgetConfig>>,
    stats: Arc<RwLock<TokenUsageStats>>,
    component_tokens: Arc<RwLock<HashMap<String, usize>>>,
    tokenizer_cache: Arc<RwLock<Option<TokenCounter>>>,
}

impl TokenBudgetManager {
    /// Create a new token budget manager
    pub fn new(mut config: TokenBudgetConfig) -> Self {
        config.tokenizer_id = normalize_optional_string(config.tokenizer_id);

        Self {
            config: Arc::new(RwLock::new(config)),
            stats: Arc::new(RwLock::new(TokenUsageStats::new())),
            component_tokens: Arc::new(RwLock::new(HashMap::new())),
            tokenizer_cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Ensure a tokenizer (or fallback counter) is available for the configured model
    async fn token_counter(&self) -> Result<TokenCounter> {
        if let Some(counter) = self.tokenizer_cache.read().await.clone() {
            return Ok(counter);
        }

        let (model, tokenizer_id) = {
            let config = self.config.read().await;
            (config.model.clone(), config.tokenizer_id.clone())
        };
        let model_for_log = model.clone();
        let tokenizer_for_log = tokenizer_id.clone();

        let load_result =
            task::spawn_blocking(move || load_tokenizer_for_model(&model, tokenizer_id.as_deref()))
                .await
                .context("Tokenizer loading task failed")?;

        let counter = match load_result {
            Ok(tokenizer) => {
                debug!(
                    model = %model_for_log,
                    tokenizer = tokenizer_for_log
                        .as_deref()
                        .unwrap_or("<model-default>"),
                    "Initialized Hugging Face tokenizer",
                );
                TokenCounter::huggingface(tokenizer)
            }
            Err(error) => {
                warn!(
                    model = %model_for_log,
                    tokenizer = tokenizer_for_log
                        .as_deref()
                        .unwrap_or("<model-default>"),
                    error = %error,
                    "Falling back to heuristic token counter",
                );
                TokenCounter::Approximate
            }
        };

        let mut cache = self.tokenizer_cache.write().await;
        *cache = Some(counter.clone());
        Ok(counter)
    }

    /// Count tokens in text
    pub async fn count_tokens(&self, text: &str) -> Result<usize> {
        let counter = self.token_counter().await?;
        counter.count_tokens(text)
    }

    /// Update the token budget configuration at runtime.
    /// Resets the cached tokenizer when model-specific values change.
    pub async fn update_config(&self, mut new_config: TokenBudgetConfig) {
        new_config.tokenizer_id = normalize_optional_string(new_config.tokenizer_id);

        let mut config_guard = self.config.write().await;
        let model_changed = config_guard.model != new_config.model
            || config_guard.tokenizer_id != new_config.tokenizer_id
            || config_guard.max_context_tokens != new_config.max_context_tokens;

        *config_guard = new_config;
        drop(config_guard);

        if model_changed {
            let mut cache = self.tokenizer_cache.write().await;
            *cache = None;
        }
    }

    /// Count tokens with component tracking
    pub async fn count_tokens_for_component(
        &self,
        text: &str,
        component: ContextComponent,
        component_id: Option<&str>,
    ) -> Result<usize> {
        let token_count = self.count_tokens(text).await?;

        self.record_tokens_for_component(component, token_count, component_id)
            .await;

        Ok(token_count)
    }

    /// Record token usage for a component using a provided token count.
    pub async fn record_tokens_for_component(
        &self,
        component: ContextComponent,
        tokens: usize,
        component_id: Option<&str>,
    ) {
        if tokens == 0 {
            return;
        }

        let detailed_tracking = {
            let config = self.config.read().await;
            config.detailed_tracking
        };

        if detailed_tracking {
            let key = if let Some(id) = component_id {
                format!("{:?}:{}", component, id)
            } else {
                format!("{:?}", component)
            };
            let mut components = self.component_tokens.write().await;
            *components.entry(key).or_insert(0) += tokens;
        }

        let mut stats = self.stats.write().await;
        stats.total_tokens += tokens;

        match component {
            ContextComponent::SystemPrompt => stats.system_prompt_tokens += tokens,
            ContextComponent::UserMessage => stats.user_messages_tokens += tokens,
            ContextComponent::AssistantMessage => stats.assistant_messages_tokens += tokens,
            ContextComponent::ToolResult => stats.tool_results_tokens += tokens,
            ContextComponent::DecisionLedger => stats.decision_ledger_tokens += tokens,
            _ => {}
        }

        stats.timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }

    /// Get current usage statistics
    pub async fn get_stats(&self) -> TokenUsageStats {
        self.stats.read().await.clone()
    }

    /// Get component-level token breakdown
    pub async fn get_component_breakdown(&self) -> HashMap<String, usize> {
        self.component_tokens.read().await.clone()
    }

    /// Check if warning threshold is exceeded
    pub async fn is_warning_threshold_exceeded(&self) -> bool {
        let stats = self.stats.read().await;
        let config = self.config.read().await;
        stats.needs_compaction(config.max_context_tokens, config.warning_threshold)
    }

    /// Check if compaction threshold is exceeded
    pub async fn is_compaction_threshold_exceeded(&self) -> bool {
        let stats = self.stats.read().await;
        let config = self.config.read().await;
        stats.needs_compaction(config.max_context_tokens, config.compaction_threshold)
    }

    /// Get current usage percentage
    pub async fn usage_percentage(&self) -> f64 {
        let stats = self.stats.read().await;
        let config = self.config.read().await;
        stats.usage_percentage(config.max_context_tokens)
    }

    /// Get current usage ratio (0.0-1.0)
    pub async fn usage_ratio(&self) -> f64 {
        let stats = self.stats.read().await;
        let config = self.config.read().await;
        stats.usage_ratio(config.max_context_tokens)
    }

    /// Get remaining tokens in budget
    pub async fn remaining_tokens(&self) -> usize {
        let stats = self.stats.read().await;
        let config = self.config.read().await;
        config.max_context_tokens.saturating_sub(stats.total_tokens)
    }

    /// Reset token counts (e.g., after compaction)
    pub async fn reset(&self) {
        let mut stats = self.stats.write().await;
        *stats = TokenUsageStats::new();
        let mut components = self.component_tokens.write().await;
        components.clear();
        debug!("Token budget reset");
    }

    /// Deduct tokens (after compaction/removal)
    pub async fn deduct_tokens(&self, component: ContextComponent, tokens: usize) {
        let mut stats = self.stats.write().await;
        stats.total_tokens = stats.total_tokens.saturating_sub(tokens);

        match component {
            ContextComponent::SystemPrompt => {
                stats.system_prompt_tokens = stats.system_prompt_tokens.saturating_sub(tokens)
            }
            ContextComponent::UserMessage => {
                stats.user_messages_tokens = stats.user_messages_tokens.saturating_sub(tokens)
            }
            ContextComponent::AssistantMessage => {
                stats.assistant_messages_tokens =
                    stats.assistant_messages_tokens.saturating_sub(tokens)
            }
            ContextComponent::ToolResult => {
                stats.tool_results_tokens = stats.tool_results_tokens.saturating_sub(tokens)
            }
            ContextComponent::DecisionLedger => {
                stats.decision_ledger_tokens = stats.decision_ledger_tokens.saturating_sub(tokens)
            }
            _ => {}
        }

        debug!("Deducted {} tokens from {:?}", tokens, component);
    }

    /// Generate a budget report
    pub async fn generate_report(&self) -> String {
        let stats = self.stats.read().await;
        let config = self.config.read().await;
        let components = self.component_tokens.read().await;

        let usage_ratio = stats.usage_ratio(config.max_context_tokens);
        let usage_pct = usage_ratio * 100.0;
        let remaining = config.max_context_tokens.saturating_sub(stats.total_tokens);

        let mut report = format!(
            "Token Budget Report\n\
             ==================\n\
             Total Tokens: {}/{} ({:.1}%)\n\
             Remaining: {} tokens\n\n\
             Breakdown by Category:\n\
             - System Prompt: {} tokens\n\
             - User Messages: {} tokens\n\
             - Assistant Messages: {} tokens\n\
             - Tool Results: {} tokens\n\
             - Decision Ledger: {} tokens\n",
            stats.total_tokens,
            config.max_context_tokens,
            usage_pct,
            remaining,
            stats.system_prompt_tokens,
            stats.user_messages_tokens,
            stats.assistant_messages_tokens,
            stats.tool_results_tokens,
            stats.decision_ledger_tokens
        );

        if config.detailed_tracking && !components.is_empty() {
            report.push_str("\nDetailed Component Tracking:\n");
            let mut sorted: Vec<_> = components.iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(a.1));
            for (component, tokens) in sorted.iter().take(10) {
                report.push_str(&format!("  - {}: {} tokens\n", component, tokens));
            }
        }

        if usage_ratio >= config.compaction_threshold {
            report.push_str("\nALERT: Compaction threshold exceeded");
        } else if usage_ratio >= config.warning_threshold {
            report.push_str("\nWARNING: Approaching token limit");
        }

        report
    }
}

fn approximate_token_count(text: &str) -> usize {
    if text.trim().is_empty() {
        return 0;
    }

    let whitespace_tokens = text.split_whitespace().count();
    let char_estimate = (text.chars().count() as f64 / 4.0).ceil() as usize;

    whitespace_tokens.max(char_estimate).max(1)
}

fn load_tokenizer_for_model(model: &str, tokenizer_id: Option<&str>) -> Result<Tokenizer> {
    if let Some(identifier) = tokenizer_id {
        if let Some(spec) = resolve_tokenizer_spec(identifier) {
            return load_tokenizer_from_spec(&spec);
        }
    }

    if let Some(spec) = resolve_tokenizer_spec(model) {
        return load_tokenizer_from_spec(&spec);
    }

    Err(anyhow!(
        "No tokenizer mapping available for model '{}'",
        model
    ))
}

fn load_tokenizer_from_spec(spec: &TokenizerSpec) -> Result<Tokenizer> {
    match spec {
        TokenizerSpec::LocalFile(path) => Tokenizer::from_file(path)
            .map_err(|err| anyhow!("Failed to load tokenizer from {}: {err}", path.display())),
        TokenizerSpec::Pretrained { id, revision } => {
            if let Some(rev) = revision {
                warn!(
                    "Tokenizer revision override '{}' is not supported; using default revision for '{}'",
                    rev, id
                );
            }
            Tokenizer::from_pretrained(id, None)
                .map_err(|err| anyhow!("Failed to load tokenizer '{id}': {err}"))
        }
    }
}

fn resolve_tokenizer_spec(identifier: &str) -> Option<TokenizerSpec> {
    let trimmed = identifier.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(path) = resolve_local_tokenizer_path(trimmed) {
        return Some(TokenizerSpec::LocalFile(path));
    }

    if trimmed.contains('/') {
        return Some(TokenizerSpec::Pretrained {
            id: trimmed.to_string(),
            revision: None,
        });
    }

    Some(TokenizerSpec::Pretrained {
        id: map_model_to_pretrained(trimmed).to_string(),
        revision: None,
    })
}

fn resolve_local_tokenizer_path(identifier: &str) -> Option<PathBuf> {
    let direct_path = Path::new(identifier);
    if direct_path.exists() {
        return Some(direct_path.to_path_buf());
    }

    let mut resource_path = PathBuf::from("resources/tokenizers");
    if identifier.ends_with(".json") {
        resource_path.push(identifier);
    } else {
        resource_path.push(format!("{identifier}.json"));
    }

    if resource_path.exists() {
        return Some(resource_path);
    }

    None
}

fn map_model_to_pretrained(model: &str) -> &'static str {
    let normalized = model.to_ascii_lowercase();

    if normalized.contains("gpt-4o") || normalized.contains("gpt-5") {
        "openai-community/gpt-4o-mini-tokenizer"
    } else if normalized.contains("gpt") {
        "openai-community/gpt2"
    } else if normalized.contains("gemini") {
        "google/gemma-2b"
    } else if normalized.contains("claude") {
        "Xenova/claude-3-haiku-20240307"
    } else if normalized.contains("glm") {
        "THUDM/chatglm3-6b"
    } else if normalized.contains("qwen") {
        "Qwen/Qwen1.5-7B-Chat"
    } else {
        "openai-community/gpt2"
    }
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|inner| {
        let trimmed = inner.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

impl Default for TokenBudgetManager {
    fn default() -> Self {
        Self::new(TokenBudgetConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_token_counting() {
        let config = TokenBudgetConfig::default();
        let manager = TokenBudgetManager::new(config);

        let text = "Hello, world!";
        let count = manager.count_tokens(text).await.unwrap();
        assert!(count > 0);
    }

    #[tokio::test]
    async fn test_component_tracking() {
        let mut config = TokenBudgetConfig::default();
        config.detailed_tracking = true;
        let manager = TokenBudgetManager::new(config);

        let text = "This is a test message";
        let count = manager
            .count_tokens_for_component(text, ContextComponent::UserMessage, Some("msg1"))
            .await
            .unwrap();

        assert!(count > 0);

        let stats = manager.get_stats().await;
        assert_eq!(stats.user_messages_tokens, count);
    }

    #[tokio::test]
    async fn test_threshold_detection() {
        let mut config = TokenBudgetConfig::default();
        config.max_context_tokens = 100;
        config.compaction_threshold = 0.8;
        let manager = TokenBudgetManager::new(config);

        // Add enough tokens to exceed threshold
        let text = "word ".repeat(25); // Should be > 80 tokens
        manager
            .count_tokens_for_component(&text, ContextComponent::UserMessage, None)
            .await
            .unwrap();

        assert!(manager.is_compaction_threshold_exceeded().await);
    }

    #[tokio::test]
    async fn test_token_deduction() {
        let manager = TokenBudgetManager::new(TokenBudgetConfig::default());

        let text = "Hello, world!";
        let count = manager
            .count_tokens_for_component(text, ContextComponent::ToolResult, None)
            .await
            .unwrap();

        let initial_total = manager.get_stats().await.total_tokens;

        manager
            .deduct_tokens(ContextComponent::ToolResult, count)
            .await;

        let after_total = manager.get_stats().await.total_tokens;
        assert_eq!(after_total, initial_total - count);
    }

    #[tokio::test]
    async fn test_usage_ratio_updates_with_config_changes() {
        let mut config = TokenBudgetConfig::default();
        config.max_context_tokens = 100;
        let manager = TokenBudgetManager::new(config);

        manager
            .record_tokens_for_component(ContextComponent::SystemPrompt, 20, None)
            .await;

        let initial_ratio = manager.usage_ratio().await;
        assert!((initial_ratio - 0.2).abs() < f64::EPSILON);

        let mut new_config = TokenBudgetConfig::for_model("gpt-4", 200);
        new_config.warning_threshold = 0.6;
        new_config.compaction_threshold = 0.8;
        manager.update_config(new_config).await;

        let updated_ratio = manager.usage_ratio().await;
        assert!((updated_ratio - 0.1).abs() < f64::EPSILON);
    }
}
