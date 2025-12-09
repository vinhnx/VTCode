//! Execution context for coordinating agent optimization components
//!
//! This module provides the ExecutionContext struct that integrates all optimization
//! components (LoopDetector, TokenBudgetManager, ContextOptimizer, AutonomousExecutor,
//! AgentBehaviorAnalyzer) into a cohesive framework for autonomous agent execution.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::context_optimizer::{CompactMode, ContextOptimizer};
use super::loop_detector::LoopDetector;
use super::token_budget::TokenBudgetManager;
use crate::exec::agent_optimization::AgentBehaviorAnalyzer;
use crate::tools::autonomous_executor::AutonomousExecutor;

/// Execution context that coordinates all optimization components
///
/// This struct provides a unified interface for managing agent execution with
/// integrated loop detection, token budget management, context optimization,
/// autonomous execution policy, and behavior analysis.
#[derive(Clone)]
pub struct ExecutionContext {
    /// Token budget manager for tracking context window usage
    pub token_budget: Arc<TokenBudgetManager>,

    /// Loop detector for identifying repetitive tool calls
    pub loop_detector: Arc<RwLock<LoopDetector>>,

    /// Context optimizer for managing output curation and compaction
    pub context_optimizer: Arc<RwLock<ContextOptimizer>>,

    /// Autonomous executor for determining execution policy
    pub autonomous_executor: Arc<AutonomousExecutor>,

    /// Behavior analyzer for tracking patterns and recommendations
    pub behavior_analyzer: Arc<RwLock<AgentBehaviorAnalyzer>>,

    /// Current compact mode state
    compact_mode: Arc<RwLock<CompactMode>>,
}

impl ExecutionContext {
    /// Create a new execution context with all optimization components
    ///
    /// # Arguments
    ///
    /// * `token_budget` - Token budget manager instance
    /// * `loop_detector` - Loop detector instance
    /// * `context_optimizer` - Context optimizer instance
    /// * `autonomous_executor` - Autonomous executor instance
    /// * `behavior_analyzer` - Behavior analyzer instance
    pub fn new(
        token_budget: Arc<TokenBudgetManager>,
        loop_detector: Arc<RwLock<LoopDetector>>,
        context_optimizer: Arc<RwLock<ContextOptimizer>>,
        autonomous_executor: Arc<AutonomousExecutor>,
        behavior_analyzer: Arc<RwLock<AgentBehaviorAnalyzer>>,
    ) -> Self {
        Self {
            token_budget,
            loop_detector,
            context_optimizer,
            autonomous_executor,
            behavior_analyzer,
            compact_mode: Arc::new(RwLock::new(CompactMode::Normal)),
        }
    }

    /// Get the current compact mode
    pub async fn get_compact_mode(&self) -> CompactMode {
        *self.compact_mode.read().await
    }

    /// Set the compact mode
    pub async fn set_compact_mode(&self, mode: CompactMode) {
        let mut compact_mode = self.compact_mode.write().await;
        *compact_mode = mode;
    }

    /// Update compact mode based on current token budget usage
    ///
    /// This method checks the token budget usage and automatically activates
    /// the appropriate compact mode using unified thresholds from token_constants:
    /// - Normal: < THRESHOLD_COMPACT (90%) usage
    /// - Compact: THRESHOLD_COMPACT-THRESHOLD_CHECKPOINT (90-95%) usage
    /// - Checkpoint: > THRESHOLD_CHECKPOINT (95%) usage
    pub async fn update_compact_mode_from_budget(&self) -> Result<CompactMode> {
        use crate::core::token_constants::{THRESHOLD_CHECKPOINT, THRESHOLD_COMPACT};

        let usage_ratio = self.token_budget.usage_ratio().await;

        let new_mode = if usage_ratio >= THRESHOLD_CHECKPOINT {
            CompactMode::Checkpoint
        } else if usage_ratio >= THRESHOLD_COMPACT {
            CompactMode::Compact
        } else {
            CompactMode::Normal
        };

        self.set_compact_mode(new_mode).await;
        Ok(new_mode)
    }

    /// Check if loop detection should block execution
    ///
    /// # Arguments
    ///
    /// * `tool_name` - Name of the tool being called
    ///
    /// # Returns
    ///
    /// Returns true if the tool should be blocked due to loop detection
    pub async fn should_block_for_loop(&self, tool_name: &str) -> bool {
        let detector = self.loop_detector.read().await;
        detector.is_hard_limit_exceeded(tool_name)
    }

    /// Record a tool call for loop detection
    ///
    /// # Arguments
    ///
    /// * `tool_name` - Name of the tool being called
    /// * `args` - Tool arguments as JSON value
    ///
    /// # Returns
    ///
    /// Returns Some(warning_message) if a loop is detected, None otherwise
    pub async fn record_tool_call(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Option<String> {
        let mut detector = self.loop_detector.write().await;
        detector.record_call(tool_name, args)
    }

    /// Reset loop detection for a specific tool after successful progress
    ///
    /// # Arguments
    ///
    /// * `tool_name` - Name of the tool to reset
    pub async fn reset_tool_loop_detection(&self, tool_name: &str) {
        let mut detector = self.loop_detector.write().await;
        detector.reset_tool(tool_name);
    }

    /// Get tool execution success rate from behavior analyzer
    ///
    /// # Arguments
    ///
    /// * `tool_name` - Name of the tool
    ///
    /// # Returns
    ///
    /// Returns the success rate as a ratio (0.0-1.0)
    /// Note: This is calculated from recorded usage and failure data
    pub async fn get_tool_success_rate(&self, tool_name: &str) -> f64 {
        let analyzer = self.behavior_analyzer.read().await;
        analyzer.tool_success_rate(tool_name)
    }

    /// Record tool execution result in behavior analyzer
    ///
    /// # Arguments
    ///
    /// * `tool_name` - Name of the tool
    /// * `success` - Whether the execution was successful
    pub async fn record_tool_execution(&self, tool_name: &str, success: bool) {
        let mut analyzer = self.behavior_analyzer.write().await;
        if success {
            analyzer.record_tool_usage(tool_name);
        } else {
            analyzer.record_tool_failure(tool_name, "execution_failed");
        }
    }

    /// Check if a warning should be shown for a tool based on failure rate
    ///
    /// # Arguments
    ///
    /// * `tool_name` - Name of the tool
    ///
    /// # Returns
    ///
    /// Returns Some(warning_message) if a warning should be shown, None otherwise
    pub async fn should_warn_for_tool(&self, tool_name: &str) -> Option<String> {
        let analyzer = self.behavior_analyzer.read().await;
        analyzer.should_warn(tool_name)
    }

    /// Get recovery action for a known error pattern
    ///
    /// # Arguments
    ///
    /// * `error_type` - Type of error encountered
    ///
    /// # Returns
    ///
    /// Returns Some(recovery_action) if a known pattern exists, None otherwise
    pub async fn get_recovery_action(&self, error_type: &str) -> Option<String> {
        let analyzer = self.behavior_analyzer.read().await;
        analyzer.get_recovery_action(error_type)
    }

    /// Generate a comprehensive status report
    ///
    /// # Returns
    ///
    /// Returns a formatted string with status information from all components
    pub async fn generate_status_report(&self) -> String {
        let mut report = String::new();

        // Token budget status
        report.push_str("=== Execution Context Status ===\n\n");
        report.push_str(&self.token_budget.generate_report().await);
        report.push_str("\n\n");

        // Compact mode status
        let compact_mode = self.get_compact_mode().await;
        report.push_str(&format!("Compact Mode: {:?}\n", compact_mode));

        // Loop detection status
        let detector = self.loop_detector.read().await;
        report.push_str(&format!(
            "\nLoop Detection: {} tools tracked\n",
            detector.get_tracked_tool_count()
        ));

        report
    }
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self::new(
            Arc::new(TokenBudgetManager::default()),
            Arc::new(RwLock::new(LoopDetector::new())),
            Arc::new(RwLock::new(ContextOptimizer::new())),
            Arc::new(AutonomousExecutor::new()),
            Arc::new(RwLock::new(AgentBehaviorAnalyzer::new())),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::token_budget::TokenBudgetConfig;

    #[tokio::test]
    async fn test_execution_context_creation() {
        let context = ExecutionContext::default();

        // Verify initial state
        assert_eq!(context.get_compact_mode().await, CompactMode::Normal);
        assert_eq!(context.token_budget.usage_ratio().await, 0.0);
    }

    #[tokio::test]
    async fn test_compact_mode_updates() {
        let context = ExecutionContext::default();

        // Test manual mode setting
        context.set_compact_mode(CompactMode::Compact).await;
        assert_eq!(context.get_compact_mode().await, CompactMode::Compact);

        context.set_compact_mode(CompactMode::Checkpoint).await;
        assert_eq!(context.get_compact_mode().await, CompactMode::Checkpoint);
    }

    #[tokio::test]
    async fn test_compact_mode_from_budget() {
        let config = TokenBudgetConfig {
            max_context_tokens: 1000,
            ..Default::default()
        };
        let token_budget = Arc::new(TokenBudgetManager::new(config));

        let context = ExecutionContext::new(
            token_budget.clone(),
            Arc::new(RwLock::new(LoopDetector::new())),
            Arc::new(RwLock::new(ContextOptimizer::new())),
            Arc::new(AutonomousExecutor::new()),
            Arc::new(RwLock::new(AgentBehaviorAnalyzer::new())),
        );

        // Initially should be Normal
        let mode = context.update_compact_mode_from_budget().await.unwrap();
        assert_eq!(mode, CompactMode::Normal);

        // Add tokens to reach 92% (should trigger Compact)
        use crate::core::token_budget::ContextComponent;
        token_budget
            .record_tokens_for_component(ContextComponent::UserMessage, 920, None)
            .await;

        let mode = context.update_compact_mode_from_budget().await.unwrap();
        assert_eq!(mode, CompactMode::Compact);

        // Add more tokens to reach 96% (should trigger Checkpoint)
        token_budget
            .record_tokens_for_component(ContextComponent::UserMessage, 40, None)
            .await;

        let mode = context.update_compact_mode_from_budget().await.unwrap();
        assert_eq!(mode, CompactMode::Checkpoint);
    }

    #[tokio::test]
    async fn test_loop_detection_integration() {
        let context = ExecutionContext::default();

        let tool_name = "test_tool";
        let args = serde_json::json!({"param": "value"});

        // First few calls should not trigger warning
        assert!(context.record_tool_call(tool_name, &args).await.is_none());
        assert!(context.record_tool_call(tool_name, &args).await.is_none());
        assert!(context.record_tool_call(tool_name, &args).await.is_none());

        // Should not be blocked yet
        assert!(!context.should_block_for_loop(tool_name).await);

        // Reset should clear the counter
        context.reset_tool_loop_detection(tool_name).await;
        assert!(context.record_tool_call(tool_name, &args).await.is_none());
    }

    #[tokio::test]
    async fn test_behavior_tracking() {
        let context = ExecutionContext::default();

        let tool_name = "test_tool";

        // Record some executions
        context.record_tool_execution(tool_name, true).await;
        context.record_tool_execution(tool_name, true).await;
        context.record_tool_execution(tool_name, false).await;

        // Success rate should be 2/3 ≈ 0.67
        let success_rate = context.get_tool_success_rate(tool_name).await;
        assert!(success_rate > 0.6 && success_rate < 0.7);
    }

    #[tokio::test]
    async fn test_status_report_generation() {
        let context = ExecutionContext::default();

        let report = context.generate_status_report().await;

        // Report should contain key sections
        assert!(report.contains("Execution Context Status"));
        assert!(report.contains("Token Budget Report"));
        assert!(report.contains("Compact Mode"));
        assert!(report.contains("Loop Detection"));
    }
}
