//! Tool timeout policies and adaptive tuning.
//!
//! This module contains timeout management for tool executions,
//! including category-based timeouts and adaptive timeout adjustments
//! based on historical latency data.

use std::collections::VecDeque;
use std::time::Duration;

use crate::config::TimeoutsConfig;

/// Categories of tools with different timeout requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolTimeoutCategory {
    /// Standard tool execution.
    Default,
    /// PTY-based interactive commands (longer timeouts).
    Pty,
    /// MCP tool execution (moderate timeouts).
    Mcp,
}

impl ToolTimeoutCategory {
    /// Human-readable label for the category.
    pub fn label(&self) -> &'static str {
        match self {
            ToolTimeoutCategory::Default => "standard",
            ToolTimeoutCategory::Pty => "PTY",
            ToolTimeoutCategory::Mcp => "MCP",
        }
    }
}

/// Policy for tool execution timeouts per category.
#[derive(Debug, Clone)]
pub struct ToolTimeoutPolicy {
    default_ceiling: Option<Duration>,
    pty_ceiling: Option<Duration>,
    mcp_ceiling: Option<Duration>,
    warning_fraction: f32,
}

impl Default for ToolTimeoutPolicy {
    fn default() -> Self {
        Self {
            default_ceiling: Some(Duration::from_secs(180)),
            pty_ceiling: Some(Duration::from_secs(300)),
            mcp_ceiling: Some(Duration::from_secs(120)),
            warning_fraction: 0.8,
        }
    }
}

impl ToolTimeoutPolicy {
    /// Create a timeout policy from configuration.
    pub fn from_config(config: &TimeoutsConfig) -> Self {
        Self {
            default_ceiling: config.ceiling_duration(config.default_ceiling_seconds),
            pty_ceiling: config.ceiling_duration(config.pty_ceiling_seconds),
            mcp_ceiling: config.ceiling_duration(config.mcp_ceiling_seconds),
            warning_fraction: config.warning_threshold_fraction().clamp(0.0, 0.99),
        }
    }

    /// Validate a single ceiling duration against bounds.
    #[inline]
    fn validate_ceiling(ceiling: Option<Duration>, name: &str) -> anyhow::Result<()> {
        if let Some(ceiling) = ceiling {
            if ceiling < Duration::from_secs(1) {
                anyhow::bail!(
                    "{} must be at least 1 second (got {}s)",
                    name,
                    ceiling.as_secs()
                );
            }
            if ceiling > Duration::from_secs(3600) {
                anyhow::bail!(
                    "{} must not exceed 3600 seconds/1 hour (got {}s)",
                    name,
                    ceiling.as_secs()
                );
            }
        }
        Ok(())
    }

    /// Validate the timeout policy configuration.
    ///
    /// Ensures that:
    /// - Ceiling values are within reasonable bounds (1s - 3600s)
    /// - Warning fraction is between 0.0 and 1.0
    /// - No ceiling is configured as 0 seconds
    pub fn validate(&self) -> anyhow::Result<()> {
        Self::validate_ceiling(self.default_ceiling, "default_ceiling_seconds")?;
        Self::validate_ceiling(self.pty_ceiling, "pty_ceiling_seconds")?;
        Self::validate_ceiling(self.mcp_ceiling, "mcp_ceiling_seconds")?;

        // Validate warning fraction
        if self.warning_fraction <= 0.0 {
            anyhow::bail!(
                "warning_threshold_percent must be greater than 0 (got {})",
                self.warning_fraction * 100.0
            );
        }
        if self.warning_fraction >= 1.0 {
            anyhow::bail!(
                "warning_threshold_percent must be less than 100 (got {})",
                self.warning_fraction * 100.0
            );
        }

        Ok(())
    }

    /// Get the ceiling timeout for a given category.
    pub fn ceiling_for(&self, category: ToolTimeoutCategory) -> Option<Duration> {
        match category {
            ToolTimeoutCategory::Default => self.default_ceiling,
            ToolTimeoutCategory::Pty => self.pty_ceiling.or(self.default_ceiling),
            ToolTimeoutCategory::Mcp => self.mcp_ceiling.or(self.default_ceiling),
        }
    }

    /// Get the warning threshold fraction.
    pub fn warning_fraction(&self) -> f32 {
        self.warning_fraction
    }
}

/// Tracks latency samples for adaptive timeout calculation.
#[derive(Debug, Clone, Default)]
pub struct ToolLatencyStats {
    pub(super) samples: VecDeque<Duration>,
    pub(super) max_samples: usize,
}

impl ToolLatencyStats {
    /// Create a new latency tracker with a maximum sample count.
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    /// Record a new latency sample.
    pub fn record(&mut self, duration: Duration) {
        if self.samples.len() >= self.max_samples {
            self.samples.pop_front();
        }
        self.samples.push_back(duration);
    }

    /// Calculate the percentile latency from recorded samples.
    pub fn percentile(&self, pct: f64) -> Option<Duration> {
        if self.samples.is_empty() {
            return None;
        }
        let mut sorted: Vec<Duration> = self.samples.iter().copied().collect();
        sorted.sort_unstable();
        let idx =
            ((pct.clamp(0.0, 1.0)) * (sorted.len().saturating_sub(1) as f64)).round() as usize;
        sorted.get(idx).copied()
    }
}

/// Tuning parameters for adaptive timeout adjustment.
#[derive(Debug, Clone, Copy)]
pub struct AdaptiveTimeoutTuning {
    /// Ratio to decay timeout toward ceiling on success.
    pub decay_ratio: f64,
    /// Number of consecutive successes before decaying.
    pub success_streak: u32,
    /// Minimum floor for adaptive timeout in milliseconds.
    pub min_floor_ms: u64,
}

impl Default for AdaptiveTimeoutTuning {
    fn default() -> Self {
        Self {
            decay_ratio: 0.875,  // relax toward ceiling by 12.5%
            success_streak: 5,   // decay after 5 consecutive successes
            min_floor_ms: 1_000, // never clamp below 1s
        }
    }
}

impl AdaptiveTimeoutTuning {
    /// Create adaptive tuning parameters from configuration.
    pub fn from_config(timeouts: &TimeoutsConfig) -> Self {
        Self {
            decay_ratio: timeouts.adaptive_decay_ratio,
            success_streak: timeouts.adaptive_success_streak,
            min_floor_ms: timeouts.adaptive_min_floor_ms,
        }
    }
}
