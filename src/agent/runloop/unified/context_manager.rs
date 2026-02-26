use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use anyhow::{Result, bail};
use vtcode_config::constants::context::{
    TOKEN_BUDGET_CRITICAL_THRESHOLD, TOKEN_BUDGET_HIGH_THRESHOLD, TOKEN_BUDGET_WARNING_THRESHOLD,
};
use vtcode_core::compaction::{CompactionConfig, compact_history};
use vtcode_core::llm::provider as uni;

use crate::agent::runloop::unified::incremental_system_prompt::{
    IncrementalSystemPrompt, PromptAssemblyMode, SystemPromptConfig, SystemPromptContext,
};

/// Parameters for building system prompts
#[derive(Clone)]
pub(crate) struct SystemPromptParams {
    pub full_auto: bool,
    pub plan_mode: bool,
    pub context_window_size: Option<usize>,
    pub active_agent_name: Option<String>,
    pub active_agent_prompt: Option<String>,
}

/// Statistics tracked incrementally to avoid re-scanning history
#[derive(Default, Clone)]
struct ContextStats {
    tool_usage_count: usize,
    error_count: usize,
    last_history_len: usize,
    /// Current prompt-side token pressure used for compaction checks.
    total_token_usage: usize,
    /// Most recent provider-reported prompt token count.
    last_prompt_tokens: usize,
    /// Most recent provider-reported total token count.
    last_total_tokens: usize,
    /// Cumulative completion tokens, retained for diagnostics only.
    completion_tokens_cumulative: usize,
    /// Whether current token usage comes from exact provider-reported usage.
    has_exact_prompt_usage: bool,
    /// Exact prompt token count from provider-native count endpoint for in-flight request.
    pending_exact_prompt_tokens: Option<usize>,
}

/// Token budget status for proactive context management
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenBudgetStatus {
    /// Below 70% - normal operation
    Normal,
    /// 70-85% - start preparing for context handoff
    Warning,
    /// 85-90% - active context management needed
    High,
    /// Above 90% - immediate action required
    Critical,
}

/// Simplified ContextManager without context trim and compaction functionality
pub(crate) struct ContextManager {
    base_system_prompt: String,
    incremental_prompt_builder: IncrementalSystemPrompt,
    /// Loaded skills for prompt injection
    loaded_skills: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    /// Incrementally tracked statistics
    cached_stats: ContextStats,
    compaction_config: CompactionConfig,
    /// Agent configuration
    agent_config: Option<vtcode_config::core::AgentConfig>,
}

impl ContextManager {
    pub(crate) fn new(
        base_system_prompt: String,
        _trim_config: (), // Removed trim config parameter
        loaded_skills: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
        agent_config: Option<vtcode_config::core::AgentConfig>,
    ) -> Self {
        Self {
            base_system_prompt,
            incremental_prompt_builder: IncrementalSystemPrompt::new(),
            loaded_skills,
            cached_stats: ContextStats::default(),
            compaction_config: CompactionConfig::default(),
            agent_config,
        }
    }

    /// Pre-request check that returns recommended action before making an LLM request.
    /// Checks session boundaries to correct runaway sessions.
    pub(crate) fn pre_request_check(
        &self,
        _history: &[uni::Message],
        context_window_size: usize,
    ) -> PreRequestAction {
        if !self.cached_stats.has_exact_prompt_usage {
            return PreRequestAction::Proceed;
        }

        let usage_ratio = if context_window_size == 0 {
            0.0
        } else {
            self.cached_stats.total_token_usage as f64 / context_window_size as f64
        };

        if usage_ratio >= self.compaction_config.trigger_threshold {
            return PreRequestAction::Compact(
                "Context window at threshold. Compacting conversation history.".to_string(),
            );
        }

        PreRequestAction::Proceed
    }

    fn update_stats(&mut self, history: &[uni::Message]) {
        let new_len = history.len();
        if new_len < self.cached_stats.last_history_len {
            // History was truncated or reset, full rescan
            self.cached_stats = ContextStats::default();
        } else if new_len == self.cached_stats.last_history_len {
            return;
        }

        // Only scan new messages
        for msg in &history[self.cached_stats.last_history_len..] {
            if msg.tool_calls.is_some() || msg.tool_call_id.is_some() {
                self.cached_stats.tool_usage_count += 1;
            }
            if msg.content.as_text().contains("error") || msg.content.as_text().contains("failed") {
                self.cached_stats.error_count += 1;
            }
        }
        self.cached_stats.last_history_len = new_len;
    }

    /// Update token usage from the latest LLM response.
    ///
    /// We prioritize prompt-side pressure as the compaction signal:
    /// - `prompt_tokens` when available
    /// - fallback to `total_tokens - completion_tokens`
    pub(crate) fn update_token_usage(&mut self, usage: &Option<uni::Usage>) {
        if let Some(usage) = usage {
            let prompt_tokens = usage.prompt_tokens as usize;
            let completion_tokens = usage.completion_tokens as usize;
            let total_tokens = usage.total_tokens as usize;

            self.cached_stats.completion_tokens_cumulative = self
                .cached_stats
                .completion_tokens_cumulative
                .saturating_add(completion_tokens);
            self.cached_stats.last_total_tokens = total_tokens;

            let estimated_prompt_pressure = if prompt_tokens > 0 {
                prompt_tokens
            } else if total_tokens > completion_tokens {
                total_tokens.saturating_sub(completion_tokens)
            } else {
                self.cached_stats
                    .total_token_usage
                    .saturating_add(completion_tokens)
            };

            self.cached_stats.last_prompt_tokens = estimated_prompt_pressure;
            self.cached_stats.total_token_usage = estimated_prompt_pressure;
            self.cached_stats.has_exact_prompt_usage = true;
            self.cached_stats.pending_exact_prompt_tokens = None;
        } else if let Some(exact_prompt_tokens) =
            self.cached_stats.pending_exact_prompt_tokens.take()
        {
            self.cached_stats.last_prompt_tokens = exact_prompt_tokens;
            self.cached_stats.total_token_usage = exact_prompt_tokens;
            self.cached_stats.has_exact_prompt_usage = true;
        } else {
            self.cached_stats.has_exact_prompt_usage = false;
        }
    }

    pub(crate) fn set_pending_exact_prompt_token_count(&mut self, exact_prompt_tokens: usize) {
        self.cached_stats.pending_exact_prompt_tokens = Some(exact_prompt_tokens);
    }

    /// Validate that ContextManager token tracking matches provider-reported usage
    /// Logs a warning if delta > 5% to catch tracking inconsistencies
    #[cfg(debug_assertions)]
    pub(crate) fn validate_token_tracking(&self, provider_usage: &Option<uni::Usage>) {
        if let Some(usage) = provider_usage {
            let provider_prompt = if usage.prompt_tokens > 0 {
                usage.prompt_tokens as usize
            } else {
                (usage.total_tokens as usize).saturating_sub(usage.completion_tokens as usize)
            };
            let manager_total = self.cached_stats.total_token_usage;

            if provider_prompt > 0 {
                let delta = if provider_prompt > manager_total {
                    (provider_prompt - manager_total) as f64 / provider_prompt as f64
                } else {
                    (manager_total - provider_prompt) as f64 / provider_prompt as f64
                };

                if delta > 0.05 {
                    tracing::warn!(
                        provider_prompt_tokens = provider_prompt,
                        manager_tokens = manager_total,
                        delta_percent = delta * 100.0,
                        "Prompt-token tracking divergence detected between ContextManager and provider usage"
                    );
                }
            }
        }
    }

    pub(crate) async fn compact_history_if_needed(
        &mut self,
        history: &[uni::Message],
        provider_client: &dyn uni::LLMProvider,
        model: &str,
    ) -> Result<Vec<uni::Message>> {
        let new_history =
            compact_history(provider_client, model, history, &self.compaction_config).await?;
        if new_history.len() != history.len() {
            self.cached_stats = ContextStats::default();
        }
        Ok(new_history)
    }

    /// Compute usage ratio once, avoiding repeated division
    #[inline]
    fn usage_ratio(&self, context_window_size: usize) -> f64 {
        if !self.cached_stats.has_exact_prompt_usage || context_window_size == 0 {
            0.0
        } else {
            self.cached_stats.total_token_usage as f64 / context_window_size as f64
        }
    }

    /// Get token budget status and guidance together (single computation)
    /// Uses thresholds from Anthropic context window documentation:
    /// - 70%: Warning - prepare for handoff
    /// - 85%: High - active management needed
    /// - 90%: Critical - immediate action required
    pub(crate) fn get_token_budget_status_and_guidance(
        &self,
        context_window_size: usize,
    ) -> (TokenBudgetStatus, &'static str) {
        let usage_ratio = self.usage_ratio(context_window_size);

        if usage_ratio >= TOKEN_BUDGET_CRITICAL_THRESHOLD {
            (
                TokenBudgetStatus::Critical,
                "CRITICAL: Update artifacts (task.md/docs) and consider starting a new session.",
            )
        } else if usage_ratio >= TOKEN_BUDGET_HIGH_THRESHOLD {
            (
                TokenBudgetStatus::High,
                "HIGH: Start summarizing key findings and preparing for context handoff.",
            )
        } else if usage_ratio >= TOKEN_BUDGET_WARNING_THRESHOLD {
            (
                TokenBudgetStatus::Warning,
                "WARNING: Consider updating progress docs to preserve important context.",
            )
        } else {
            (TokenBudgetStatus::Normal, "")
        }
    }

    /// Get guidance message based on token budget status
    /// Returns actionable guidance for context management
    pub(crate) fn get_token_budget_guidance(&self, context_window_size: usize) -> &'static str {
        self.get_token_budget_status_and_guidance(context_window_size)
            .1
    }

    /// Get current token budget status based on usage ratio
    #[allow(dead_code)]
    pub(crate) fn get_token_budget_status(&self, context_window_size: usize) -> TokenBudgetStatus {
        self.get_token_budget_status_and_guidance(context_window_size)
            .0
    }

    /// Get current token usage
    #[allow(dead_code)]
    pub(crate) fn current_token_usage(&self) -> usize {
        self.cached_stats.total_token_usage
    }

    pub(crate) fn current_exact_token_usage(&self) -> Option<usize> {
        if self.cached_stats.has_exact_prompt_usage {
            Some(self.cached_stats.total_token_usage)
        } else {
            None
        }
    }

    pub(crate) async fn build_system_prompt(
        &mut self,
        attempt_history: &[uni::Message],
        retry_attempts: usize,
        params: SystemPromptParams,
    ) -> Result<String> {
        if self.base_system_prompt.trim().is_empty() {
            bail!("Base system prompt is empty; cannot build prompt");
        }

        // Update statistics incrementally
        self.update_stats(attempt_history);

        // Create configuration with pre-computed hash (avoids cloning base_prompt)
        let config = SystemPromptConfig::new(
            &self.base_system_prompt,
            retry_attempts > 0,
            false,
            3, // This could be configurable
            PromptAssemblyMode::BaseIncludesInstructions,
        );

        // Determine if model supports context awareness (Claude 4.5+)
        let supports_context_awareness = params.context_window_size.is_some();

        // Get token budget guidance if context awareness is supported
        let token_budget_guidance = if supports_context_awareness {
            self.get_token_budget_guidance(params.context_window_size.unwrap_or(0))
        } else {
            ""
        };

        // Compute token usage ratio from ContextManager's cached stats (single source of truth)
        let token_usage_ratio = if let Some(context_size) = params.context_window_size {
            self.usage_ratio(context_size)
        } else {
            0.0
        };

        let context = SystemPromptContext {
            conversation_length: attempt_history.len(),
            tool_usage_count: self.cached_stats.tool_usage_count,
            error_count: self.cached_stats.error_count,
            token_usage_ratio,
            full_auto: params.full_auto,
            plan_mode: params.plan_mode,
            active_agent_name: params.active_agent_name.unwrap_or("coder".to_string()),
            active_agent_prompt: params.active_agent_prompt,
            discovered_skills: self.loaded_skills.read().await.values().cloned().collect(),
            context_window_size: params.context_window_size,
            current_token_usage: if supports_context_awareness {
                self.current_exact_token_usage()
            } else {
                None
            },
            supports_context_awareness,
            token_budget_guidance,
        };

        // Use incremental builder to avoid redundant cloning and processing
        let system_prompt = self
            .incremental_prompt_builder
            .get_system_prompt(
                &self.base_system_prompt,
                config.hash(),
                context.hash(),
                retry_attempts,
                config.prompt_assembly_mode,
                &context,
                self.agent_config.as_ref(),
            )
            .await;

        Ok(system_prompt)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum PreRequestAction {
    /// Normal operation, proceed with request
    Proceed,
    /// Proceed but inject a warning/reminder to the agent
    Warn(String),
    /// Stop execution and force user intervention or summary
    Stop(String),
    /// Compact history before proceeding
    Compact(String),
}

#[cfg(test)]
#[path = "context_manager_tests.rs"]
mod tests;
