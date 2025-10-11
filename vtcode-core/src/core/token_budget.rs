//! Token budget management for context engineering
//!
//! This module implements token counting and budget tracking to manage
//! the attention budget of LLMs. Following Anthropic's context engineering
//! principles, it helps prevent context rot by tracking token usage and
//! triggering compaction when thresholds are exceeded.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tiktoken_rs::get_bpe_from_model;
use tokio::sync::RwLock;
use tracing::debug;

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
        if max_tokens == 0 {
            return 0.0;
        }
        (self.total_tokens as f64 / max_tokens as f64) * 100.0
    }

    /// Check if compaction is needed
    pub fn needs_compaction(&self, max_tokens: usize, threshold: f64) -> bool {
        let usage = self.total_tokens as f64 / max_tokens as f64;
        usage >= threshold
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

/// Token budget manager
pub struct TokenBudgetManager {
    config: Arc<RwLock<TokenBudgetConfig>>,
    stats: Arc<RwLock<TokenUsageStats>>,
    component_tokens: Arc<RwLock<HashMap<String, usize>>>,
    tokenizer_cache: Arc<RwLock<Option<tiktoken_rs::CoreBPE>>>,
}

impl TokenBudgetManager {
    /// Create a new token budget manager
    pub fn new(config: TokenBudgetConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            stats: Arc::new(RwLock::new(TokenUsageStats::new())),
            component_tokens: Arc::new(RwLock::new(HashMap::new())),
            tokenizer_cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Initialize or update tokenizer for the current model
    async fn ensure_tokenizer(&self) -> Result<()> {
        let mut cache = self.tokenizer_cache.write().await;
        if cache.is_none() {
            let config = self.config.read().await;
            let bpe = get_bpe_from_model(&config.model)
                .with_context(|| format!("Failed to get tokenizer for model: {}", config.model))?;
            *cache = Some(bpe);
        }
        Ok(())
    }

    /// Count tokens in text
    pub async fn count_tokens(&self, text: &str) -> Result<usize> {
        self.ensure_tokenizer().await?;
        let cache = self.tokenizer_cache.read().await;
        let bpe = cache
            .as_ref()
            .ok_or_else(|| anyhow!("Tokenizer not initialized"))?;
        Ok(bpe.encode_with_special_tokens(text).len())
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

        let usage_pct = stats.usage_percentage(config.max_context_tokens);
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

        if usage_pct >= config.compaction_threshold * 100.0 {
            report.push_str("\nALERT: Compaction threshold exceeded");
        } else if usage_pct >= config.warning_threshold * 100.0 {
            report.push_str("\nWARNING: Approaching token limit");
        }

        report
    }
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
}
