use anyhow::{Context, Result};
use hashbrown::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};
use vtcode_config::constants::tools;

/// Aggregates telemetry data for the agent session.
#[derive(Debug, Clone, Default)]
pub struct TelemetryManager {
    stats: Arc<Mutex<TelemetryStats>>,
    start_time: Option<Instant>,
    dropped_metric_updates: Arc<AtomicU64>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelUsageStats {
    /// Cumulative API call duration for this model.
    pub api_time: Duration,
    /// Total prompt tokens consumed.
    pub prompt_tokens: u64,
    /// Total completion tokens generated.
    pub completion_tokens: u64,
    /// Total cached prompt tokens reused.
    pub cached_prompt_tokens: u64,
    /// Total tokens read from the prompt cache.
    pub cache_read_tokens: u64,
    /// Total tokens written into the prompt cache.
    pub cache_creation_tokens: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TelemetryStats {
    /// Total agent turns in the session.
    pub total_turns: usize,
    /// Total tool invocations across all tools.
    pub total_tool_calls: usize,
    /// Total tokens consumed (placeholder for future token tracking).
    pub total_tokens: usize,
    /// Per-tool invocation counts.
    pub tool_counts: HashMap<String, usize>,
    /// Per-tool error counts.
    pub tool_errors: HashMap<String, usize>,
    /// Total elapsed session duration.
    pub session_duration: Duration,
    /// Cumulative time spent waiting for LLM API responses.
    pub api_time_spent: Duration,
    /// Per-model usage breakdown.
    pub model_usage: HashMap<String, ModelUsageStats>,
    /// Number of metric updates dropped due to lock contention.
    pub dropped_metric_updates: u64,
}

impl TelemetryManager {
    /// Create a new telemetry manager and start the session timer.
    pub fn new() -> Self {
        Self {
            stats: Arc::new(Mutex::new(TelemetryStats::default())),
            start_time: Some(Instant::now()),
            dropped_metric_updates: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Record that a new agent turn has started.
    pub fn record_turn(&self) {
        self.with_stats_mut_non_blocking(|stats| {
            stats.total_turns += 1;
            if let Some(start) = self.start_time {
                stats.session_duration = start.elapsed();
            }
        });
    }

    /// Record a tool invocation, incrementing error count if it failed.
    pub fn record_tool_usage(&self, tool: &str, success: bool) {
        let tool = public_tool_telemetry_label(tool);
        self.with_stats_mut_non_blocking(|stats| {
            stats.total_tool_calls += 1;
            if let Some(count) = stats.tool_counts.get_mut(&tool) {
                *count += 1;
            } else {
                stats.tool_counts.insert(tool.clone(), 1);
            }
            if !success {
                if let Some(count) = stats.tool_errors.get_mut(&tool) {
                    *count += 1;
                } else {
                    stats.tool_errors.insert(tool, 1);
                }
            }
        });
    }

    /// Record an LLM API request with its duration and optional token usage.
    pub fn record_llm_request(
        &self,
        model: &str,
        duration: Duration,
        usage: Option<&crate::llm::provider::Usage>,
    ) {
        self.with_stats_mut_non_blocking(|stats| {
            stats.api_time_spent = stats.api_time_spent.saturating_add(duration);
            let model_stats = if let Some(existing) = stats.model_usage.get_mut(model) {
                existing
            } else {
                stats.model_usage.entry(model.to_owned()).or_default()
            };
            model_stats.api_time = model_stats.api_time.saturating_add(duration);

            if let Some(usage) = usage {
                model_stats.prompt_tokens = model_stats
                    .prompt_tokens
                    .saturating_add(usage.prompt_tokens as u64);
                model_stats.completion_tokens = model_stats
                    .completion_tokens
                    .saturating_add(usage.completion_tokens as u64);
                model_stats.cached_prompt_tokens = model_stats
                    .cached_prompt_tokens
                    .saturating_add(usage.cached_prompt_tokens.unwrap_or(0) as u64);
                model_stats.cache_read_tokens = model_stats
                    .cache_read_tokens
                    .saturating_add(usage.cache_read_tokens_or_fallback() as u64);
                model_stats.cache_creation_tokens = model_stats
                    .cache_creation_tokens
                    .saturating_add(usage.cache_creation_tokens_or_zero() as u64);
            }
        });
    }

    fn with_stats_mut_non_blocking<F>(&self, update: F)
    where
        F: FnOnce(&mut TelemetryStats),
    {
        match self.stats.try_lock() {
            Ok(mut stats) => update(&mut stats),
            Err(TryLockError::WouldBlock) | Err(TryLockError::Poisoned(_)) => {
                self.dropped_metric_updates.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Take a snapshot of the current telemetry statistics.
    pub fn get_snapshot(&self) -> Result<TelemetryStats> {
        let stats = self
            .stats
            .lock()
            .map_err(|err| anyhow::anyhow!("telemetry stats lock poisoned: {err}"))
            .context("Failed to read telemetry snapshot")?;
        let mut snapshot = stats.clone();
        snapshot.dropped_metric_updates = self.dropped_metric_updates.load(Ordering::Relaxed);
        Ok(snapshot)
    }
}

fn public_tool_telemetry_label(tool: &str) -> String {
    match tool {
        tools::UNIFIED_EXEC => tools::EXEC_COMMAND.to_string(),
        tools::UNIFIED_SEARCH => tools::CODE_SEARCH.to_string(),
        tools::UNIFIED_FILE => "file_operation".to_string(),
        _ => tool.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::TelemetryManager;
    use std::time::Duration;
    use vtcode_config::constants::tools;

    #[test]
    fn records_llm_request_usage_per_model() {
        let telemetry = TelemetryManager::new();
        telemetry.record_llm_request(
            "gpt-5",
            Duration::from_secs(30),
            Some(&crate::llm::provider::Usage {
                prompt_tokens: 100,
                completion_tokens: 200,
                total_tokens: 300,
                cached_prompt_tokens: Some(50),
                cache_creation_tokens: None,
                cache_read_tokens: None,
                iterations: None,
            }),
        );
        telemetry.record_llm_request("gpt-5", Duration::from_secs(10), None);

        let snapshot = telemetry.get_snapshot().expect("snapshot");
        assert_eq!(snapshot.api_time_spent, Duration::from_secs(40));
        let model = snapshot.model_usage.get("gpt-5").expect("model usage");
        assert_eq!(model.api_time, Duration::from_secs(40));
        assert_eq!(model.prompt_tokens, 100);
        assert_eq!(model.completion_tokens, 200);
        assert_eq!(model.cached_prompt_tokens, 50);
        assert_eq!(model.cache_read_tokens, 50);
        assert_eq!(model.cache_creation_tokens, 0);
        assert_eq!(snapshot.dropped_metric_updates, 0);
    }

    #[test]
    fn records_cache_read_fallback_and_creation_tokens() {
        let telemetry = TelemetryManager::new();
        telemetry.record_llm_request(
            "gpt-5",
            Duration::from_secs(5),
            Some(&crate::llm::provider::Usage {
                prompt_tokens: 500,
                completion_tokens: 100,
                total_tokens: 600,
                cached_prompt_tokens: Some(320),
                cache_creation_tokens: Some(80),
                cache_read_tokens: None,
                iterations: None,
            }),
        );

        let snapshot = telemetry.get_snapshot().expect("snapshot");
        let model = snapshot.model_usage.get("gpt-5").expect("model usage");
        assert_eq!(model.cache_read_tokens, 320);
        assert_eq!(model.cache_creation_tokens, 80);
    }

    #[test]
    fn code_search_telemetry_label_remains_canonical() {
        let telemetry = TelemetryManager::new();
        telemetry.record_tool_usage(tools::UNIFIED_EXEC, true);
        telemetry.record_tool_usage(tools::UNIFIED_SEARCH, false);
        telemetry.record_tool_usage(tools::CODE_SEARCH, true);
        telemetry.record_tool_usage(tools::UNIFIED_FILE, true);

        let snapshot = telemetry.get_snapshot().expect("snapshot");
        assert_eq!(snapshot.total_tool_calls, 4);
        assert_eq!(snapshot.tool_counts.get("exec_command"), Some(&1));
        assert_eq!(snapshot.tool_counts.get("code_search"), Some(&2));
        assert_eq!(snapshot.tool_counts.get("file_operation"), Some(&1));
        assert_eq!(snapshot.tool_errors.get("code_search"), Some(&1));
        assert!(
            !snapshot
                .tool_counts
                .keys()
                .any(|label| label.contains("unified_"))
        );
        assert!(
            !snapshot
                .tool_errors
                .keys()
                .any(|label| label.contains("unified_"))
        );
    }
}
