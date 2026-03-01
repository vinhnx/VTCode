use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

/// Aggregates telemetry data for the agent session.
#[derive(Debug, Clone, Default)]
pub struct TelemetryManager {
    stats: Arc<Mutex<TelemetryStats>>,
    start_time: Option<Instant>,
    dropped_metric_updates: Arc<AtomicU64>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelUsageStats {
    pub api_time: Duration,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TelemetryStats {
    pub total_turns: usize,
    pub total_tool_calls: usize,
    pub total_tokens: usize, // Placeholder if we get token usage
    pub tool_counts: HashMap<String, usize>,
    pub tool_errors: HashMap<String, usize>,
    pub session_duration: Duration,
    pub api_time_spent: Duration,
    pub model_usage: HashMap<String, ModelUsageStats>,
    pub dropped_metric_updates: u64,
}

impl TelemetryManager {
    pub fn new() -> Self {
        Self {
            stats: Arc::new(Mutex::new(TelemetryStats::default())),
            start_time: Some(Instant::now()),
            dropped_metric_updates: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn record_turn(&self) {
        self.with_stats_mut_non_blocking(|stats| {
            stats.total_turns += 1;
            if let Some(start) = self.start_time {
                stats.session_duration = start.elapsed();
            }
        });
    }

    pub fn record_tool_usage(&self, tool: &str, success: bool) {
        self.with_stats_mut_non_blocking(|stats| {
            stats.total_tool_calls += 1;
            if let Some(count) = stats.tool_counts.get_mut(tool) {
                *count += 1;
            } else {
                stats.tool_counts.insert(tool.to_owned(), 1);
            }
            if !success {
                if let Some(count) = stats.tool_errors.get_mut(tool) {
                    *count += 1;
                } else {
                    stats.tool_errors.insert(tool.to_owned(), 1);
                }
            }
        });
    }

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

#[cfg(test)]
mod tests {
    use super::TelemetryManager;
    use std::time::Duration;

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
        assert_eq!(snapshot.dropped_metric_updates, 0);
    }
}
