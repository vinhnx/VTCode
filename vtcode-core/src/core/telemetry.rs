use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Aggregates telemetry data for the agent session.
#[derive(Debug, Clone, Default)]
pub struct TelemetryManager {
    stats: Arc<Mutex<TelemetryStats>>,
    start_time: Option<Instant>,
}

#[derive(Debug, Clone, Default)]
pub struct TelemetryStats {
    pub total_turns: usize,
    pub total_tool_calls: usize,
    pub total_tokens: usize, // Placeholder if we get token usage
    pub tool_counts: HashMap<String, usize>,
    pub tool_errors: HashMap<String, usize>,
    pub session_duration: Duration,
}

impl TelemetryManager {
    pub fn new() -> Self {
        Self {
            stats: Arc::new(Mutex::new(TelemetryStats::default())),
            start_time: Some(Instant::now()),
        }
    }

    pub fn record_turn(&self) {
        if let Ok(mut stats) = self.stats.lock() {
            stats.total_turns += 1;
            if let Some(start) = self.start_time {
                stats.session_duration = start.elapsed();
            }
        }
    }

    pub fn record_tool_usage(&self, tool: &str, success: bool) {
        if let Ok(mut stats) = self.stats.lock() {
            stats.total_tool_calls += 1;
            *stats.tool_counts.entry(tool.to_string()).or_insert(0) += 1;
            if !success {
                *stats.tool_errors.entry(tool.to_string()).or_insert(0) += 1;
            }
        }
    }

    pub fn get_snapshot(&self) -> Result<TelemetryStats> {
        let stats = self
            .stats
            .lock()
            .map_err(|err| anyhow::anyhow!("telemetry stats lock poisoned: {err}"))
            .context("Failed to read telemetry snapshot")?;
        Ok(stats.clone())
    }
}
