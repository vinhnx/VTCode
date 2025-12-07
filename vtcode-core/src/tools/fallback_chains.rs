//! Fallback chain definitions and execution
//!
//! Provides structured fallback chains that automatically try alternative tools
//! when a primary tool fails or returns low-quality results.

use crate::config::constants::tools;
use crate::tools::result_metadata::{EnhancedToolResult, ResultMetadata};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};

/// A single step in a fallback chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackStep {
    /// Tool name to execute
    pub tool: String,

    /// Minimum confidence required to accept this result
    pub min_confidence: f32,

    /// Maximum time to wait for execution (ms)
    pub timeout_ms: u64,

    /// Whether to stop chain if this succeeds
    pub terminal: bool,
}

impl FallbackStep {
    pub fn new(tool: &str) -> Self {
        Self {
            tool: tool.to_string(),
            min_confidence: 0.5,
            timeout_ms: 10000,
            terminal: true,
        }
    }

    pub fn min_confidence(mut self, confidence: f32) -> Self {
        self.min_confidence = confidence.clamp(0.0, 1.0);
        self
    }

    pub fn timeout_ms(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    pub fn non_terminal(mut self) -> Self {
        self.terminal = false;
        self
    }
}

/// Condition to abort a fallback chain
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AbortCondition {
    /// Stop after N failures
    MaxFailures { count: usize },

    /// Stop if execution time exceeded
    TimeoutMs { timeout_ms: u64 },

    /// Stop if we have sufficient quality results
    SufficientResults { min_count: usize, min_quality: f32 },
}

/// A fallback chain definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackChain {
    /// Chain identifier
    pub name: String,

    /// Primary tool to try first
    pub primary: FallbackStep,

    /// Fallback tools to try in order
    pub fallbacks: Vec<FallbackStep>,

    /// Conditions to abort the chain
    pub abort_conditions: Vec<AbortCondition>,
}

impl FallbackChain {
    pub fn new(name: &str, primary: &str) -> Self {
        Self {
            name: name.to_string(),
            primary: FallbackStep::new(primary),
            fallbacks: vec![],
            abort_conditions: vec![],
        }
    }

    pub fn with_fallback(mut self, step: FallbackStep) -> Self {
        self.fallbacks.push(step);
        self
    }

    pub fn with_abort(mut self, condition: AbortCondition) -> Self {
        self.abort_conditions.push(condition);
        self
    }

    /// Default file search chain: grep → ripgrep → find
    pub fn file_search() -> Self {
        Self {
            name: "file_search".to_string(),
            primary: FallbackStep::new(tools::GREP_FILE).min_confidence(0.7),
            fallbacks: vec![
                FallbackStep::new("ripgrep")
                    .min_confidence(0.65)
                    .non_terminal(),
                FallbackStep::new("find").min_confidence(0.5),
            ],
            abort_conditions: vec![
                AbortCondition::MaxFailures { count: 3 },
                AbortCondition::SufficientResults {
                    min_count: 5,
                    min_quality: 0.75,
                },
            ],
        }
    }

    /// Code parsing chain: tree-sitter → regex → grep
    pub fn code_parsing() -> Self {
        Self {
            name: "code_parsing".to_string(),
            primary: FallbackStep::new("tree_sitter_query").min_confidence(0.8),
            fallbacks: vec![
                FallbackStep::new("regex_parse")
                    .min_confidence(0.6)
                    .non_terminal(),
                FallbackStep::new(tools::GREP_FILE).min_confidence(0.4),
            ],
            abort_conditions: vec![AbortCondition::MaxFailures { count: 2 }],
        }
    }

    /// Command execution chain: run_pty → shell → bash
    pub fn command_execution() -> Self {
        Self {
            name: "command_execution".to_string(),
            primary: FallbackStep::new("run_pty").min_confidence(0.8),
            fallbacks: vec![FallbackStep::new("shell").min_confidence(0.7)],
            abort_conditions: vec![AbortCondition::MaxFailures { count: 1 }],
        }
    }

    /// Get all tools in order (primary + fallbacks)
    pub fn all_tools(&self) -> Vec<&str> {
        let mut tools = vec![self.primary.tool.as_str()];
        for fallback in &self.fallbacks {
            tools.push(fallback.tool.as_str());
        }
        tools
    }
}

/// Result of executing a fallback chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackChainResult {
    /// Chain that was executed
    pub chain_name: String,

    /// All results collected (in order of execution)
    pub results: Vec<EnhancedToolResult>,

    /// Which tool succeeded (if any)
    pub successful_tool: Option<String>,

    /// Total execution time (ms)
    pub execution_time_ms: u64,

    /// Number of attempts made
    pub attempts: usize,

    /// Why the chain stopped
    pub stop_reason: ChainStopReason,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChainStopReason {
    /// Primary tool succeeded
    PrimarySuccess,
    /// Fallback tool succeeded
    FallbackSuccess,
    /// Got sufficient results
    SufficientResults,
    /// Hit abort condition
    AbortCondition,
    /// All tools exhausted
    AllToolsExhausted,
    /// Timeout exceeded
    Timeout,
}

impl FallbackChainResult {
    /// Whether the chain execution was successful
    pub fn is_successful(&self) -> bool {
        matches!(
            self.stop_reason,
            ChainStopReason::PrimarySuccess | ChainStopReason::FallbackSuccess
        )
    }

    /// Get the best result (highest quality)
    pub fn best_result(&self) -> Option<&EnhancedToolResult> {
        self.results.iter().max_by(|a, b| {
            a.metadata
                .quality_score()
                .partial_cmp(&b.metadata.quality_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Merge all results into a single value
    pub fn merged_value(&self) -> Value {
        if self.results.is_empty() {
            return Value::Null;
        }

        if self.results.len() == 1 {
            return self.results[0].value.clone();
        }

        // Merge multiple results
        Value::Array(self.results.iter().map(|r| &r.value).cloned().collect())
    }
}

/// Executes fallback chains
pub struct FallbackChainExecutor;

impl FallbackChainExecutor {
    /// Execute a fallback chain
    pub async fn execute<F>(chain: &FallbackChain, executor: F) -> FallbackChainResult
    where
        F: Fn(&str) -> futures::future::BoxFuture<'static, anyhow::Result<(Value, ResultMetadata)>>,
    {
        let start = Instant::now();
        let mut results = vec![];
        let mut attempts = 0;
        let mut stop_reason = ChainStopReason::AllToolsExhausted;

        // Try primary tool
        attempts += 1;
        match execute_tool_step(&chain.primary, &executor, start).await {
            Some((value, metadata)) => {
                let result =
                    EnhancedToolResult::new(value, metadata.clone(), chain.primary.tool.clone());

                if metadata.confidence >= chain.primary.min_confidence {
                    results.push(result);
                    stop_reason = ChainStopReason::PrimarySuccess;
                    let tool_name = chain.primary.tool.clone();
                    return FallbackChainResult {
                        chain_name: chain.name.clone(),
                        results,
                        successful_tool: Some(tool_name),
                        execution_time_ms: start.elapsed().as_millis() as u64,
                        attempts,
                        stop_reason,
                    };
                }

                results.push(result);

                // Check abort conditions
                if should_abort_chain(&chain.abort_conditions, attempts, &results, start) {
                    stop_reason = ChainStopReason::AbortCondition;
                    return FallbackChainResult {
                        chain_name: chain.name.clone(),
                        results,
                        successful_tool: None,
                        execution_time_ms: start.elapsed().as_millis() as u64,
                        attempts,
                        stop_reason,
                    };
                }
            }
            None => {
                // Primary failed
                if should_abort_chain(&chain.abort_conditions, attempts, &results, start) {
                    stop_reason = ChainStopReason::AbortCondition;
                    return FallbackChainResult {
                        chain_name: chain.name.clone(),
                        results,
                        successful_tool: None,
                        execution_time_ms: start.elapsed().as_millis() as u64,
                        attempts,
                        stop_reason,
                    };
                }
            }
        }

        // Try fallbacks
        for fallback in &chain.fallbacks {
            attempts += 1;

            // Check timeout
            if start.elapsed().as_millis() as u64
                > chain
                    .abort_conditions
                    .iter()
                    .find_map(|c| {
                        if let AbortCondition::TimeoutMs { timeout_ms } = c {
                            Some(*timeout_ms)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(30000)
            {
                stop_reason = ChainStopReason::Timeout;
                break;
            }

            match execute_tool_step(fallback, &executor, start).await {
                Some((value, metadata)) => {
                    let result =
                        EnhancedToolResult::new(value, metadata.clone(), fallback.tool.clone());

                    if metadata.confidence >= fallback.min_confidence {
                        results.push(result);
                        stop_reason = ChainStopReason::FallbackSuccess;

                        if fallback.terminal {
                            break;
                        }
                    } else {
                        results.push(result);
                    }

                    // Check abort conditions
                    if should_abort_chain(&chain.abort_conditions, attempts, &results, start) {
                        stop_reason = ChainStopReason::SufficientResults;
                        break;
                    }
                }
                None => {
                    // Tool failed, continue to next fallback
                    if should_abort_chain(&chain.abort_conditions, attempts, &results, start) {
                        stop_reason = ChainStopReason::AbortCondition;
                        break;
                    }
                }
            }
        }

        FallbackChainResult {
            chain_name: chain.name.clone(),
            results: results.clone(),
            successful_tool: results.iter().find_map(|r| {
                if r.metadata.confidence >= 0.7 {
                    Some(r.tool_name.clone())
                } else {
                    None
                }
            }),
            execution_time_ms: start.elapsed().as_millis() as u64,
            attempts,
            stop_reason,
        }
    }
}

/// Execute a single fallback step
async fn execute_tool_step<F>(
    step: &FallbackStep,
    executor: &F,
    start: Instant,
) -> Option<(Value, ResultMetadata)>
where
    F: Fn(&str) -> futures::future::BoxFuture<'static, anyhow::Result<(Value, ResultMetadata)>>,
{
    // Check timeout
    let timeout = Duration::from_millis(step.timeout_ms);
    if start.elapsed() > timeout {
        return None;
    }

    match executor(&step.tool).await {
        Ok((value, metadata)) => Some((value, metadata)),
        Err(_) => None,
    }
}

/// Check if chain should abort
fn should_abort_chain(
    conditions: &[AbortCondition],
    attempts: usize,
    results: &[EnhancedToolResult],
    start: Instant,
) -> bool {
    for condition in conditions {
        match condition {
            AbortCondition::MaxFailures { count } => {
                if attempts >= *count {
                    return true;
                }
            }
            AbortCondition::TimeoutMs { timeout_ms } => {
                if start.elapsed().as_millis() as u64 > *timeout_ms {
                    return true;
                }
            }
            AbortCondition::SufficientResults {
                min_count,
                min_quality,
            } => {
                let qualified_results = results
                    .iter()
                    .filter(|r| r.metadata.quality_score() >= *min_quality)
                    .count();

                if qualified_results >= *min_count {
                    return true;
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_step_builder() {
        let step = FallbackStep::new("grep")
            .min_confidence(0.8)
            .timeout_ms(5000)
            .non_terminal();

        assert_eq!(step.tool, "grep");
        assert_eq!(step.min_confidence, 0.8);
        assert_eq!(step.timeout_ms, 5000);
        assert!(!step.terminal);
    }

    #[test]
    fn test_fallback_chain_file_search() {
        let chain = FallbackChain::file_search();
        assert_eq!(chain.primary.tool, "grep_file");
        assert_eq!(chain.fallbacks.len(), 2);
    }

    #[test]
    fn test_fallback_chain_all_tools() {
        let chain = FallbackChain::file_search();
        let tools = chain.all_tools();
        assert_eq!(tools.len(), 3); // primary + 2 fallbacks
    }

    #[test]
    fn test_fallback_chain_result_best() {
        let mut result1 = EnhancedToolResult::new(
            Value::String("result1".to_string()),
            ResultMetadata::success(0.5, 0.5),
            "tool1".to_string(),
        );

        let mut result2 = EnhancedToolResult::new(
            Value::String("result2".to_string()),
            ResultMetadata::success(0.9, 0.9),
            "tool2".to_string(),
        );

        let chain_result = FallbackChainResult {
            chain_name: "test".to_string(),
            results: vec![result1, result2.clone()],
            successful_tool: Some("tool2".to_string()),
            execution_time_ms: 1000,
            attempts: 2,
            stop_reason: ChainStopReason::FallbackSuccess,
        };

        let best = chain_result.best_result();
        assert!(best.is_some());
        assert_eq!(best.unwrap().tool_name, "tool2");
    }

    #[test]
    fn test_abort_condition_max_failures() {
        let condition = AbortCondition::MaxFailures { count: 3 };
        let results = vec![];
        let start = Instant::now();

        assert!(!should_abort_chain(
            &[condition.clone()],
            2,
            &results,
            start
        ));
        assert!(should_abort_chain(&[condition], 3, &results, start));
    }

    #[test]
    fn test_abort_condition_sufficient_results() {
        let condition = AbortCondition::SufficientResults {
            min_count: 2,
            min_quality: 0.7,
        };

        let result1 = EnhancedToolResult::new(
            Value::Null,
            ResultMetadata::success(0.8, 0.8),
            "tool".to_string(),
        );

        let result2 = EnhancedToolResult::new(
            Value::Null,
            ResultMetadata::success(0.75, 0.75),
            "tool".to_string(),
        );

        let results = vec![result1, result2];
        let start = Instant::now();

        assert!(should_abort_chain(&[condition], 1, &results, start));
    }
}
