/// TUI widget for displaying model performance metrics
///
/// Shows current model usage, token counts, costs, and model switches per session.
/// Designed to be integrated into the status line or as a dedicated panel.

use crate::llm::model_optimizer::ModelOptimizer;
use std::sync::{Arc, RwLock};

/// Display format for metrics
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsDisplayFormat {
    /// Compact format: "claude-3-5 | 12.4K tokens | $0.08"
    Compact,
    /// Detailed format with more info
    Detailed,
    /// Minimal format: "claude-3-5"
    Minimal,
}

/// Statistics about model usage in current session
#[derive(Debug, Clone)]
pub struct ModelMetricsSnapshot {
    /// Currently active model
    pub current_model: Option<String>,
    /// Total tokens used this session
    pub total_tokens_used: u64,
    /// Total cost in cents this session
    pub total_cost_cents: f64,
    /// Number of model switches
    pub model_switches: u32,
    /// Models used and their counts
    pub models_used: Vec<(String, u32)>, // (model_id, request_count)
    /// Average tokens per request
    pub avg_tokens_per_request: f64,
    /// Average cost per request in cents
    pub avg_cost_per_request: f64,
}

impl Default for ModelMetricsSnapshot {
    fn default() -> Self {
        Self {
            current_model: None,
            total_tokens_used: 0,
            total_cost_cents: 0.0,
            model_switches: 0,
            models_used: Vec::new(),
            avg_tokens_per_request: 0.0,
            avg_cost_per_request: 0.0,
        }
    }
}

/// Panel for displaying model metrics in the TUI
pub struct ModelMetricsPanel {
    optimizer: Arc<RwLock<ModelOptimizer>>,
    snapshot: ModelMetricsSnapshot,
    format: MetricsDisplayFormat,
}

impl ModelMetricsPanel {
    /// Create new metrics panel from optimizer
    pub fn new(optimizer: Arc<RwLock<ModelOptimizer>>) -> Self {
        let snapshot = Self::build_snapshot(&optimizer);
        Self {
            optimizer,
            snapshot,
            format: MetricsDisplayFormat::Compact,
        }
    }

    /// Set display format
    pub fn with_format(mut self, format: MetricsDisplayFormat) -> Self {
        self.format = format;
        self
    }

    /// Update snapshot from latest metrics
    pub fn refresh(&mut self) {
        self.snapshot = Self::build_snapshot(&self.optimizer);
    }

    /// Build snapshot from optimizer state
    fn build_snapshot(optimizer: &Arc<RwLock<ModelOptimizer>>) -> ModelMetricsSnapshot {
        if let Ok(opt) = optimizer.read() {
            let all_metrics = opt.all_metrics();

            if all_metrics.is_empty() {
                return ModelMetricsSnapshot::default();
            }

            let total_tokens: u64 = all_metrics.iter()
                .map(|m| m.total_input_tokens + m.total_output_tokens)
                .sum();
            let total_cost: f64 = all_metrics.iter().map(|m| m.total_cost_cents).sum();
            let total_requests: u32 = all_metrics.iter().map(|m| m.request_count).sum();

            let mut models_used: Vec<(String, u32)> = all_metrics
                .iter()
                .map(|m| (m.model_id.clone(), m.request_count))
                .collect();
            models_used.sort_by(|a, b| b.1.cmp(&a.1));

            let current_model = models_used.first().map(|(m, _)| m.clone());
            let model_switches = (models_used.len() as u32).saturating_sub(1);

            let avg_tokens = if total_requests > 0 {
                total_tokens as f64 / total_requests as f64
            } else {
                0.0
            };

            let avg_cost = if total_requests > 0 {
                total_cost / total_requests as f64
            } else {
                0.0
            };

            ModelMetricsSnapshot {
                current_model,
                total_tokens_used: total_tokens,
                total_cost_cents: total_cost,
                model_switches,
                models_used,
                avg_tokens_per_request: avg_tokens,
                avg_cost_per_request: avg_cost,
            }
        } else {
            ModelMetricsSnapshot::default()
        }
    }

    /// Format metrics for display
    pub fn format_display(&self) -> String {
        match self.format {
            MetricsDisplayFormat::Compact => self.format_compact(),
            MetricsDisplayFormat::Detailed => self.format_detailed(),
            MetricsDisplayFormat::Minimal => self.format_minimal(),
        }
    }

    fn format_minimal(&self) -> String {
        self.snapshot
            .current_model
            .as_deref()
            .unwrap_or("no-model")
            .to_string()
    }

    fn format_compact(&self) -> String {
        if self.snapshot.current_model.is_none() {
            return String::new();
        }

        let model = self.snapshot.current_model.as_ref().unwrap();
        let tokens_display = format_tokens(self.snapshot.total_tokens_used);
        let cost_display = format_cost_cents(self.snapshot.total_cost_cents);

        format!("{} | {} | {}", model, tokens_display, cost_display)
    }

    fn format_detailed(&self) -> String {
        if self.snapshot.current_model.is_none() {
            return String::new();
        }

        let model = self.snapshot.current_model.as_ref().unwrap();
        let tokens_display = format_tokens(self.snapshot.total_tokens_used);
        let cost_display = format_cost_cents(self.snapshot.total_cost_cents);
        let avg_tokens_display = format_tokens(self.snapshot.avg_tokens_per_request as u64);
        let avg_cost_display = format_cost_cents(self.snapshot.avg_cost_per_request);

        let switches_display = if self.snapshot.model_switches > 0 {
            format!(" | {} switches", self.snapshot.model_switches)
        } else {
            String::new()
        };

        format!(
            "{} | {} total | {} avg/req | {}{} | ~{}",
            model,
            tokens_display,
            avg_tokens_display,
            cost_display,
            switches_display,
            avg_cost_display
        )
    }

    /// Get raw snapshot for programmatic use
    pub fn snapshot(&self) -> &ModelMetricsSnapshot {
        &self.snapshot
    }

    /// Get cost trend (percentage change between first and last request)
    pub fn cost_trend(&self) -> Option<f64> {
        if self.snapshot.models_used.is_empty() {
            return None;
        }

        if let Ok(opt) = self.optimizer.read() {
            let all_metrics = opt.all_metrics();
            if all_metrics.len() >= 2 {
                // Simple trend: compare average cost of first half vs second half
                let mid = all_metrics.len() / 2;
                let first_half_avg: f64 = all_metrics[..mid]
                    .iter()
                    .map(|m| m.avg_cost_cents)
                    .sum::<f64>()
                    / mid as f64;
                let second_half_avg: f64 = all_metrics[mid..]
                    .iter()
                    .map(|m| m.avg_cost_cents)
                    .sum::<f64>()
                    / (all_metrics.len() - mid) as f64;

                if first_half_avg > 0.0 {
                    return Some(((second_half_avg - first_half_avg) / first_half_avg) * 100.0);
                }
            }
        }

        None
    }

    /// Check if metrics indicate potential optimization opportunity
    pub fn has_optimization_opportunity(&self) -> bool {
        if self.snapshot.models_used.len() > 1 {
            return true; // Multiple models might indicate room for consolidation
        }

        if let Some(trend) = self.cost_trend() {
            if trend > 20.0 {
                return true; // Cost increasing significantly
            }
        }

        self.snapshot.total_cost_cents > 100.0 && self.snapshot.model_switches == 0
            // High cost with no optimization attempts
    }
}

/// Format token count for display
fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

/// Format cost in cents for display
fn format_cost_cents(cents: f64) -> String {
    if cents > 100.0 {
        format!("${:.2}", cents / 100.0)
    } else if cents >= 1.0 {
        format!("{:.2}¢", cents)
    } else if cents > 0.0 {
        format!("{:.1}m¢", cents)
    } else {
        "$0.00".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(1_500), "1.5K");
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }

    #[test]
    fn test_format_cost_cents() {
        assert_eq!(format_cost_cents(0.0), "$0.00");
        assert_eq!(format_cost_cents(0.5), "0.5m¢");
        assert_eq!(format_cost_cents(5.0), "5.00¢");
        assert_eq!(format_cost_cents(150.0), "$1.50");
    }

    #[test]
    fn test_metrics_panel_creation() {
        let optimizer = Arc::new(RwLock::new(ModelOptimizer::new()));
        let panel = ModelMetricsPanel::new(optimizer);

        assert_eq!(panel.snapshot().current_model, None);
        assert_eq!(panel.snapshot().total_tokens_used, 0);
    }

    #[test]
    fn test_metrics_panel_with_format() {
        let optimizer = Arc::new(RwLock::new(ModelOptimizer::new()));
        let panel = ModelMetricsPanel::new(optimizer).with_format(MetricsDisplayFormat::Detailed);

        assert_eq!(panel.format, MetricsDisplayFormat::Detailed);
    }

    #[test]
    fn test_minimal_format_with_no_model() {
        let optimizer = Arc::new(RwLock::new(ModelOptimizer::new()));
        let panel = ModelMetricsPanel::new(optimizer).with_format(MetricsDisplayFormat::Minimal);

        assert_eq!(panel.format_display(), "no-model");
    }

    #[test]
    fn test_compact_format_with_data() {
        let optimizer = Arc::new(RwLock::new(ModelOptimizer::new()));
        {
            let mut opt = optimizer.write().unwrap();
            opt.record_model_usage("gpt-4", "openai", 1000, 500, 12.34, 8192);
        }

        let mut panel = ModelMetricsPanel::new(optimizer);
        panel.refresh();

        let display = panel.format_display();
        assert!(display.contains("gpt-4"));
        assert!(display.contains("1.5K")); // 1500 tokens
        assert!(display.contains("12.34")); // cost
    }

    #[test]
    fn test_multiple_model_tracking() {
        let optimizer = Arc::new(RwLock::new(ModelOptimizer::new()));
        {
            let mut opt = optimizer.write().unwrap();
            opt.record_model_usage("gpt-4", "openai", 1000, 500, 12.34, 8192);
            opt.record_model_usage("claude-3", "anthropic", 800, 400, 5.00, 100000);
            opt.record_model_usage("gpt-4", "openai", 1000, 500, 12.34, 8192);
        }

        let mut panel = ModelMetricsPanel::new(optimizer);
        panel.refresh();

        assert_eq!(panel.snapshot().total_tokens_used, 3300);
        assert_eq!(panel.snapshot().total_cost_cents, 29.68);
        assert_eq!(panel.snapshot().model_switches, 1);
    }

    #[test]
    fn test_optimization_opportunity_detection() {
        let optimizer = Arc::new(RwLock::new(ModelOptimizer::new()));
        {
            let mut opt = optimizer.write().unwrap();
            opt.record_model_usage("gpt-4", "openai", 5000, 2000, 100.0, 8192);
            opt.record_model_usage("claude-3", "anthropic", 4000, 1600, 50.0, 100000);
        }

        let mut panel = ModelMetricsPanel::new(optimizer);
        panel.refresh();

        // Should detect opportunity due to multiple models
        assert!(panel.has_optimization_opportunity());
    }
}
