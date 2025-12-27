//! Execution context for coordinating agent optimization components
//!
//! This module provides the ExecutionContext struct that integrates all optimization
//! components (LoopDetector, ContextOptimizer, AutonomousExecutor,
//! AgentBehaviorAnalyzer) into a cohesive framework for autonomous agent execution.

use std::sync::Arc;
use tokio::sync::RwLock;

use super::context_optimizer::ContextOptimizer;
use super::loop_detector::LoopDetector;
use crate::exec::agent_optimization::AgentBehaviorAnalyzer;
use crate::tools::autonomous_executor::AutonomousExecutor;

/// Execution context that coordinates all optimization components
///
/// This struct provides a unified interface for managing agent execution with
/// integrated loop detection, context optimization, autonomous execution policy,
/// and behavior analysis.
#[derive(Clone)]
pub struct ExecutionContext {
    /// Loop detector for identifying repetitive tool calls
    pub loop_detector: Arc<RwLock<LoopDetector>>,

    /// Context optimizer for managing output curation and compaction
    pub context_optimizer: Arc<RwLock<ContextOptimizer>>,

    /// Autonomous executor for determining execution policy
    pub autonomous_executor: Arc<AutonomousExecutor>,

    /// Behavior analyzer for tracking patterns and recommendations
    pub behavior_analyzer: Arc<RwLock<AgentBehaviorAnalyzer>>,
}

impl ExecutionContext {
    /// Create a new execution context with all optimization components
    ///
    /// # Arguments
    ///
    /// * `loop_detector` - Loop detector instance
    /// * `context_optimizer` - Context optimizer instance
    /// * `autonomous_executor` - Autonomous executor instance
    /// * `behavior_analyzer` - Behavior analyzer instance
    pub fn new(
        loop_detector: Arc<RwLock<LoopDetector>>,
        context_optimizer: Arc<RwLock<ContextOptimizer>>,
        autonomous_executor: Arc<AutonomousExecutor>,
        behavior_analyzer: Arc<RwLock<AgentBehaviorAnalyzer>>,
    ) -> Self {
        Self {
            loop_detector,
            context_optimizer,
            autonomous_executor,
            behavior_analyzer,
        }
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

        report.push_str("=== Execution Context Status ===\n\n");

        // Loop detection status
        let detector = self.loop_detector.read().await;
        report.push_str(&format!(
            "Loop Detection: {} tools tracked\n",
            detector.get_tracked_tool_count()
        ));

        report
    }
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self::new(
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
        assert!(report.contains("Loop Detection"));
    }
}
