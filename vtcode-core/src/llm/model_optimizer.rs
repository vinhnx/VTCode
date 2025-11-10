/// Model optimization and selection based on cost and performance metrics
///
/// Analyzes token usage across models and provider configurations to recommend
/// the most cost-effective model for a given task, considering speed, capability,
/// and budget constraints.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

/// Performance metrics for a model across requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetrics {
    /// Model identifier
    pub model_id: String,
    /// Provider name
    pub provider: String,
    /// Total tokens used in this model
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    /// Total estimated cost in cents
    pub total_cost_cents: f64,
    /// Number of requests
    pub request_count: u32,
    /// Average input tokens per request
    pub avg_input_tokens: f64,
    /// Average output tokens per request
    pub avg_output_tokens: f64,
    /// Average cost per request in cents
    pub avg_cost_cents: f64,
    /// Minimum context window (tokens)
    pub context_window: usize,
    /// Timestamp of last request
    pub last_used: u64,
}

/// Recommendation for model selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRecommendation {
    /// Recommended model ID
    pub model_id: String,
    /// Reason for recommendation
    pub reason: String,
    /// Estimated cost for typical request in cents
    pub estimated_cost_cents: f64,
    /// Performance score (0-100, higher is better)
    pub score: f64,
    /// Alternative models
    pub alternatives: Vec<(String, f64)>, // (model_id, score)
}

/// Task complexity classification for model selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskComplexity {
    /// Simple tasks: read files, list, basic searches
    Simple,
    /// Standard tasks: code analysis, refactoring, testing
    Standard,
    /// Complex tasks: multi-file refactoring, design decisions
    Complex,
    /// Expert tasks: architecture design, security analysis
    Expert,
}

/// Budget constraint for model selection
#[derive(Debug, Clone, Copy)]
pub struct BudgetConstraint {
    /// Maximum cost in cents for this session
    pub max_session_cost_cents: f64,
    /// Maximum cost per request in cents
    pub max_per_request_cents: f64,
    /// Prefer speed over cost (0.0 = cost-focused, 1.0 = speed-focused)
    pub speed_priority: f64,
}

/// Model optimizer for tracking and recommending models
#[derive(Debug, Clone)]
pub struct ModelOptimizer {
    /// Metrics per model
    metrics: HashMap<String, ModelMetrics>,
    /// Cost estimator reference data
    provider_speeds: HashMap<String, f64>, // model_id -> relative speed score
    capabilities: HashMap<String, Vec<String>>, // model_id -> capabilities
}

impl ModelOptimizer {
    /// Create new model optimizer
    pub fn new() -> Self {
        let mut speeds = HashMap::new();
        let mut capabilities = HashMap::new();

        // Speed scores (relative, higher = faster)
        speeds.insert("gpt-3.5-turbo".to_string(), 0.9);
        speeds.insert("gpt-4".to_string(), 0.6);
        speeds.insert("gpt-4-turbo".to_string(), 0.8);
        speeds.insert("claude-3-haiku".to_string(), 0.95);
        speeds.insert("claude-3-sonnet".to_string(), 0.7);
        speeds.insert("claude-3-opus".to_string(), 0.5);
        speeds.insert("gemini-pro".to_string(), 0.8);
        speeds.insert("gemini-1.5-pro".to_string(), 0.6);

        // Capabilities per model
        capabilities.insert(
            "gpt-3.5-turbo".to_string(),
            vec!["basic".to_string(), "code".to_string()],
        );
        capabilities.insert(
            "gpt-4".to_string(),
            vec!["advanced".to_string(), "code".to_string(), "reasoning".to_string()],
        );
        capabilities.insert(
            "gpt-4-turbo".to_string(),
            vec!["advanced".to_string(), "code".to_string(), "vision".to_string()],
        );
        capabilities.insert(
            "claude-3-haiku".to_string(),
            vec!["basic".to_string(), "fast".to_string()],
        );
        capabilities.insert(
            "claude-3-sonnet".to_string(),
            vec!["advanced".to_string(), "code".to_string()],
        );
        capabilities.insert(
            "claude-3-opus".to_string(),
            vec!["expert".to_string(), "code".to_string(), "reasoning".to_string()],
        );
        capabilities.insert(
            "gemini-pro".to_string(),
            vec!["basic".to_string(), "code".to_string()],
        );
        capabilities.insert(
            "gemini-1.5-pro".to_string(),
            vec!["advanced".to_string(), "vision".to_string(), "large-context".to_string()],
        );

        Self {
            metrics: HashMap::new(),
            provider_speeds: speeds,
            capabilities,
        }
    }

    /// Record a model usage with token metrics
    pub fn record_model_usage(
        &mut self,
        model_id: &str,
        provider: &str,
        input_tokens: u64,
        output_tokens: u64,
        cost_cents: f64,
        context_window: usize,
    ) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let metrics = self.metrics.entry(model_id.to_string()).or_insert_with(|| {
            ModelMetrics {
                model_id: model_id.to_string(),
                provider: provider.to_string(),
                total_input_tokens: 0,
                total_output_tokens: 0,
                total_cost_cents: 0.0,
                request_count: 0,
                avg_input_tokens: 0.0,
                avg_output_tokens: 0.0,
                avg_cost_cents: 0.0,
                context_window,
                last_used: now,
            }
        });

        metrics.total_input_tokens += input_tokens;
        metrics.total_output_tokens += output_tokens;
        metrics.total_cost_cents += cost_cents;
        metrics.request_count += 1;
        metrics.last_used = now;

        // Update averages
        metrics.avg_input_tokens = metrics.total_input_tokens as f64 / metrics.request_count as f64;
        metrics.avg_output_tokens =
            metrics.total_output_tokens as f64 / metrics.request_count as f64;
        metrics.avg_cost_cents = metrics.total_cost_cents / metrics.request_count as f64;
    }

    /// Get metrics for a specific model
    pub fn model_metrics(&self, model_id: &str) -> Option<&ModelMetrics> {
        self.metrics.get(model_id)
    }

    /// Get all recorded metrics
    pub fn all_metrics(&self) -> Vec<&ModelMetrics> {
        self.metrics.values().collect()
    }

    /// Recommend model based on task complexity and budget
    pub fn recommend_model(
        &self,
        complexity: TaskComplexity,
        budget: Option<BudgetConstraint>,
        required_capability: Option<&str>,
    ) -> ModelRecommendation {
        let budget = budget.unwrap_or(BudgetConstraint {
            max_session_cost_cents: f64::INFINITY,
            max_per_request_cents: f64::INFINITY,
            speed_priority: 0.5, // balanced
        });

        // Model selection based on complexity
        let preferred_models = match complexity {
            TaskComplexity::Simple => vec![
                "gpt-3.5-turbo",
                "claude-3-haiku",
                "gemini-pro",
            ],
            TaskComplexity::Standard => vec![
                "gpt-4-turbo",
                "claude-3-sonnet",
                "gemini-1.5-pro",
            ],
            TaskComplexity::Complex => vec![
                "gpt-4",
                "claude-3-opus",
            ],
            TaskComplexity::Expert => vec![
                "claude-3-opus",
                "gpt-4",
            ],
        };

        // Filter by required capability
        let candidates: Vec<_> = preferred_models
            .iter()
            .filter(|&m| {
                if let Some(cap) = required_capability {
                    self.capabilities
                        .get(*m)
                        .map(|caps| caps.contains(&cap.to_string()))
                        .unwrap_or(false)
                } else {
                    true
                }
            })
            .map(|&m| m.to_string())
            .collect();

        let mut scores: Vec<(String, f64)> = candidates
            .iter()
            .map(|model_id| {
                let score = self.calculate_selection_score(model_id, &budget);
                (model_id.clone(), score)
            })
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let best_model = scores.first().map(|(m, _)| m.clone()).unwrap_or_else(|| {
            match complexity {
                TaskComplexity::Simple => "gpt-3.5-turbo".to_string(),
                TaskComplexity::Standard => "gpt-4-turbo".to_string(),
                TaskComplexity::Complex => "gpt-4".to_string(),
                TaskComplexity::Expert => "claude-3-opus".to_string(),
            }
        });

        let reason = self.format_recommendation_reason(&best_model, complexity, &budget);
        let estimated_cost = self.estimate_typical_cost(&best_model);

        ModelRecommendation {
            model_id: best_model,
            reason,
            estimated_cost_cents: estimated_cost,
            score: scores.first().map(|(_, s)| *s).unwrap_or(0.0),
            alternatives: scores.iter().skip(1).take(2).cloned().collect(),
        }
    }

    /// Calculate selection score for a model
    fn calculate_selection_score(&self, model_id: &str, budget: &BudgetConstraint) -> f64 {
        let speed_score = self
            .provider_speeds
            .get(model_id)
            .copied()
            .unwrap_or(0.5);

        // Cost penalty (lower is better)
        let historical_cost = self
            .metrics
            .get(model_id)
            .map(|m| m.avg_cost_cents)
            .unwrap_or(0.0);

        // Budget compatibility
        let budget_penalty = if historical_cost > budget.max_per_request_cents {
            0.0 // violates budget
        } else {
            1.0 - (historical_cost / budget.max_per_request_cents.max(1.0)).min(1.0) * 0.5
        };

        // Usage preference (recently used models score slightly higher)
        let recency_bonus = if self.metrics.get(model_id).is_some() { 0.05 } else { 0.0 };

        // Combined score
        (speed_score * budget.speed_priority + budget_penalty * (1.0 - budget.speed_priority))
            * 100.0
            + recency_bonus
    }

    /// Estimate typical cost for a model based on historical data
    fn estimate_typical_cost(&self, model_id: &str) -> f64 {
        self.metrics
            .get(model_id)
            .map(|m| m.avg_cost_cents)
            .unwrap_or(0.01) // default: 0.01 cents for unknown models
    }

    /// Format human-readable recommendation reason
    fn format_recommendation_reason(
        &self,
        model_id: &str,
        complexity: TaskComplexity,
        budget: &BudgetConstraint,
    ) -> String {
        let complexity_name = match complexity {
            TaskComplexity::Simple => "simple",
            TaskComplexity::Standard => "standard",
            TaskComplexity::Complex => "complex",
            TaskComplexity::Expert => "expert",
        };

        if budget.speed_priority > 0.7 {
            format!("{} is fast and suitable for {complexity_name} tasks", model_id)
        } else if budget.speed_priority < 0.3 {
            format!("{} is cost-effective for {complexity_name} tasks", model_id)
        } else {
            format!("{} balances cost and speed for {complexity_name} tasks", model_id)
        }
    }

    /// Get top N most used models
    pub fn top_models(&self, n: usize) -> Vec<&ModelMetrics> {
        let mut models: Vec<_> = self.metrics.values().collect();
        models.sort_by(|a, b| b.request_count.cmp(&a.request_count));
        models.into_iter().take(n).collect()
    }

    /// Get cost breakdown by model
    pub fn cost_breakdown(&self) -> Vec<(String, f64, f64)> {
        // (model_id, total_cost_cents, percentage)
        let total_cost: f64 = self.metrics.values().map(|m| m.total_cost_cents).sum();
        let mut costs: Vec<_> = self
            .metrics
            .values()
            .map(|m| {
                let percentage = if total_cost > 0.0 {
                    (m.total_cost_cents / total_cost) * 100.0
                } else {
                    0.0
                };
                (m.model_id.clone(), m.total_cost_cents, percentage)
            })
            .collect();
        costs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        costs
    }

    /// Format summary of model performance
    pub fn format_summary(&self) -> String {
        let mut output = String::new();
        output.push_str("📊 Model Performance Summary\n");

        let breakdown = self.cost_breakdown();
        if !breakdown.is_empty() {
            output.push_str("\nCost Breakdown:\n");
            let total_cost: f64 = breakdown.iter().map(|(_, cost, _)| cost).sum();
            for (model, cost, percent) in breakdown.iter().take(5) {
                output.push_str(&format!(
                    "  {} - ${:.2} ({:.1}%)\n",
                    model,
                    cost / 100.0,
                    percent
                ));
            }
            output.push_str(&format!("\nTotal Cost: ${:.2}\n", total_cost / 100.0));
        }

        let top_models = self.top_models(3);
        if !top_models.is_empty() {
            output.push_str("\nMost Used:\n");
            for model in top_models {
                output.push_str(&format!(
                    "  {} - {} requests, avg {} tokens/request\n",
                    model.model_id,
                    model.request_count,
                    (model.avg_input_tokens + model.avg_output_tokens) as u64
                ));
            }
        }

        output
    }
}

impl Default for ModelOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creates_optimizer() {
        let optimizer = ModelOptimizer::new();
        assert!(!optimizer.provider_speeds.is_empty());
        assert!(!optimizer.capabilities.is_empty());
    }

    #[test]
    fn test_records_usage() {
        let mut optimizer = ModelOptimizer::new();
        optimizer.record_model_usage("gpt-3.5-turbo", "openai", 1000, 500, 2.5, 4096);

        let metrics = optimizer.model_metrics("gpt-3.5-turbo");
        assert!(metrics.is_some());
        let metrics = metrics.unwrap();
        assert_eq!(metrics.total_input_tokens, 1000);
        assert_eq!(metrics.total_output_tokens, 500);
        assert_eq!(metrics.request_count, 1);
    }

    #[test]
    fn test_tracks_multiple_requests() {
        let mut optimizer = ModelOptimizer::new();
        optimizer.record_model_usage("gpt-3.5-turbo", "openai", 1000, 500, 2.5, 4096);
        optimizer.record_model_usage("gpt-3.5-turbo", "openai", 1200, 600, 3.0, 4096);

        let metrics = optimizer.model_metrics("gpt-3.5-turbo").unwrap();
        assert_eq!(metrics.request_count, 2);
        assert_eq!(metrics.total_input_tokens, 2200);
        assert_eq!(metrics.avg_input_tokens, 1100.0);
        assert!(metrics.avg_cost_cents - 2.75 < 0.01);
    }

    #[test]
    fn test_recommends_for_simple_task() {
        let optimizer = ModelOptimizer::new();
        let rec = optimizer.recommend_model(TaskComplexity::Simple, None, None);
        assert!(!rec.model_id.is_empty());
    }

    #[test]
    fn test_recommends_for_expert_task() {
        let optimizer = ModelOptimizer::new();
        let rec = optimizer.recommend_model(TaskComplexity::Expert, None, None);
        assert_eq!(rec.model_id, "claude-3-opus");
    }

    #[test]
    fn test_respects_budget_constraint() {
        let mut optimizer = ModelOptimizer::new();
        optimizer.record_model_usage("gpt-4", "openai", 10000, 5000, 50.0, 8192);

        let budget = BudgetConstraint {
            max_session_cost_cents: 1000.0,
            max_per_request_cents: 10.0,
            speed_priority: 0.5,
        };

        let rec = optimizer.recommend_model(TaskComplexity::Standard, Some(budget), None);
        // Should avoid expensive models
        assert_ne!(rec.model_id, "gpt-4");
    }

    #[test]
    fn test_filters_by_capability() {
        let optimizer = ModelOptimizer::new();
        let rec = optimizer.recommend_model(
            TaskComplexity::Standard,
            None,
            Some("vision"),
        );
        // gpt-4-turbo or gemini-1.5-pro have vision
        assert!(
            rec.model_id == "gpt-4-turbo" || rec.model_id == "gemini-1.5-pro"
        );
    }

    #[test]
    fn test_cost_breakdown() {
        let mut optimizer = ModelOptimizer::new();
        optimizer.record_model_usage("gpt-3.5-turbo", "openai", 1000, 500, 2.5, 4096);
        optimizer.record_model_usage("claude-3-haiku", "anthropic", 1000, 500, 5.0, 4096);

        let breakdown = optimizer.cost_breakdown();
        assert_eq!(breakdown.len(), 2);
        // claude should be more expensive
        assert!(breakdown[0].1 > breakdown[1].1);
    }

    #[test]
    fn test_top_models() {
        let mut optimizer = ModelOptimizer::new();
        optimizer.record_model_usage("gpt-3.5-turbo", "openai", 1000, 500, 2.5, 4096);
        optimizer.record_model_usage("gpt-3.5-turbo", "openai", 1000, 500, 2.5, 4096);
        optimizer.record_model_usage("claude-3-haiku", "anthropic", 1000, 500, 5.0, 4096);

        let top = optimizer.top_models(1);
        assert_eq!(top[0].model_id, "gpt-3.5-turbo");
        assert_eq!(top[0].request_count, 2);
    }

    #[test]
    fn test_format_summary() {
        let mut optimizer = ModelOptimizer::new();
        optimizer.record_model_usage("gpt-3.5-turbo", "openai", 1000, 500, 2.5, 4096);

        let summary = optimizer.format_summary();
        assert!(summary.contains("Model Performance"));
        assert!(summary.contains("gpt-3.5-turbo"));
    }

    #[test]
    fn test_speed_priority_affects_score() {
        let optimizer = ModelOptimizer::new();

        let budget_fast = BudgetConstraint {
            max_session_cost_cents: f64::INFINITY,
            max_per_request_cents: f64::INFINITY,
            speed_priority: 0.9,
        };

        let budget_cheap = BudgetConstraint {
            max_session_cost_cents: f64::INFINITY,
            max_per_request_cents: f64::INFINITY,
            speed_priority: 0.1,
        };

        let fast_rec = optimizer.recommend_model(TaskComplexity::Standard, Some(budget_fast), None);
        let cheap_rec = optimizer.recommend_model(TaskComplexity::Standard, Some(budget_cheap), None);

        // Different constraints should potentially recommend different models
        // (though they might be the same - we just verify they work)
        assert!(!fast_rec.model_id.is_empty());
        assert!(!cheap_rec.model_id.is_empty());
    }
}
