/// Cost estimation for LLM API calls across different providers
///
/// Provides accurate cost calculations for OpenAI, Anthropic, and Google Gemini,
/// enabling cost-aware model selection and provider recommendations.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Pricing information for a specific model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    /// Model identifier
    pub model_id: String,
    /// Provider name (openai, anthropic, google)
    pub provider: String,
    /// Cost per 1M input tokens (in cents)
    pub input_cost_per_1m: f64,
    /// Cost per 1M output tokens (in cents)
    pub output_cost_per_1m: f64,
    /// Minimum cost per request (in cents, defaults to 0)
    pub minimum_cost: f64,
}

/// Estimated cost for a single request
#[derive(Debug, Clone)]
pub struct EstimatedCost {
    /// Model used
    pub model_id: String,
    /// Provider name
    pub provider: String,
    /// Estimated input tokens
    pub input_tokens: usize,
    /// Estimated output tokens
    pub output_tokens: usize,
    /// Total cost in cents (USD)
    pub total_cents: f64,
    /// Cost breakdown: (input_cost, output_cost)
    pub breakdown: (f64, f64),
}

/// Comparison of costs across models
#[derive(Debug, Clone)]
pub struct CostComparison {
    /// Primary estimate (recommended)
    pub primary: EstimatedCost,
    /// Alternative cheaper options
    pub alternatives: Vec<EstimatedCost>,
    /// Estimated savings using cheapest option (in cents)
    pub savings_cents: f64,
    /// Percentage savings
    pub savings_percent: f64,
}

/// Cost estimator for multiple providers
#[derive(Clone)]
pub struct CostEstimator {
    pricing: HashMap<String, ModelPricing>,
}

impl CostEstimator {
    /// Create a new cost estimator with default provider pricing
    pub fn new() -> Self {
        let mut pricing = HashMap::new();

        // OpenAI models (as of 2024)
        pricing.insert(
            "gpt-4-turbo".to_string(),
            ModelPricing {
                model_id: "gpt-4-turbo".to_string(),
                provider: "openai".to_string(),
                input_cost_per_1m: 1000.0, // $0.01 per 1K tokens
                output_cost_per_1m: 3000.0, // $0.03 per 1K tokens
                minimum_cost: 0.0,
            },
        );

        pricing.insert(
            "gpt-4".to_string(),
            ModelPricing {
                model_id: "gpt-4".to_string(),
                provider: "openai".to_string(),
                input_cost_per_1m: 3000.0, // $0.03 per 1K tokens
                output_cost_per_1m: 6000.0, // $0.06 per 1K tokens
                minimum_cost: 0.0,
            },
        );

        pricing.insert(
            "gpt-3.5-turbo".to_string(),
            ModelPricing {
                model_id: "gpt-3.5-turbo".to_string(),
                provider: "openai".to_string(),
                input_cost_per_1m: 50.0, // $0.0005 per 1K tokens
                output_cost_per_1m: 150.0, // $0.0015 per 1K tokens
                minimum_cost: 0.0,
            },
        );

        // Anthropic models
        pricing.insert(
            "claude-3-opus".to_string(),
            ModelPricing {
                model_id: "claude-3-opus".to_string(),
                provider: "anthropic".to_string(),
                input_cost_per_1m: 1500.0, // $0.015 per 1K tokens
                output_cost_per_1m: 7500.0, // $0.075 per 1K tokens
                minimum_cost: 0.0,
            },
        );

        pricing.insert(
            "claude-3-sonnet".to_string(),
            ModelPricing {
                model_id: "claude-3-sonnet".to_string(),
                provider: "anthropic".to_string(),
                input_cost_per_1m: 300.0, // $0.003 per 1K tokens
                output_cost_per_1m: 1500.0, // $0.015 per 1K tokens
                minimum_cost: 0.0,
            },
        );

        pricing.insert(
            "claude-3-haiku".to_string(),
            ModelPricing {
                model_id: "claude-3-haiku".to_string(),
                provider: "anthropic".to_string(),
                input_cost_per_1m: 80.0, // $0.0008 per 1K tokens
                output_cost_per_1m: 240.0, // $0.0024 per 1K tokens
                minimum_cost: 0.0,
            },
        );

        // Google Gemini models
        pricing.insert(
            "gemini-1.5-pro".to_string(),
            ModelPricing {
                model_id: "gemini-1.5-pro".to_string(),
                provider: "google".to_string(),
                input_cost_per_1m: 350.0, // $0.0035 per 1K tokens
                output_cost_per_1m: 1050.0, // $0.0105 per 1K tokens
                minimum_cost: 0.0,
            },
        );

        pricing.insert(
            "gemini-pro".to_string(),
            ModelPricing {
                model_id: "gemini-pro".to_string(),
                provider: "google".to_string(),
                input_cost_per_1m: 0.0, // Free tier available
                output_cost_per_1m: 0.0,
                minimum_cost: 0.0,
            },
        );

        Self { pricing }
    }

    /// Register custom pricing for a model
    pub fn register_model(&mut self, pricing: ModelPricing) {
        self.pricing.insert(pricing.model_id.clone(), pricing);
    }

    /// Estimate cost for a single model
    pub fn estimate_cost(
        &self,
        model_id: &str,
        input_tokens: usize,
        output_tokens: usize,
    ) -> Option<EstimatedCost> {
        self.pricing.get(model_id).map(|pricing| {
            let input_cost = (input_tokens as f64 / 1_000_000.0) * pricing.input_cost_per_1m;
            let output_cost = (output_tokens as f64 / 1_000_000.0) * pricing.output_cost_per_1m;
            let total_cents = (input_cost + output_cost).max(pricing.minimum_cost);

            EstimatedCost {
                model_id: model_id.to_string(),
                provider: pricing.provider.clone(),
                input_tokens,
                output_tokens,
                total_cents,
                breakdown: (input_cost, output_cost),
            }
        })
    }

    /// Compare costs across multiple models
    pub fn compare_costs(
        &self,
        primary_model: &str,
        input_tokens: usize,
        output_tokens: usize,
        alternative_providers: &[&str],
    ) -> Option<CostComparison> {
        let primary = self.estimate_cost(primary_model, input_tokens, output_tokens)?;

        let mut alternatives: Vec<_> = alternative_providers
            .iter()
            .filter_map(|&model| self.estimate_cost(model, input_tokens, output_tokens))
            .collect();

        alternatives.sort_by(|a, b| a.total_cents.partial_cmp(&b.total_cents).unwrap());

        let cheapest_cost = alternatives.first().map(|c| c.total_cents).unwrap_or(primary.total_cents);
        let savings_cents = (primary.total_cents - cheapest_cost).max(0.0);
        let savings_percent = if primary.total_cents > 0.0 {
            (savings_cents / primary.total_cents) * 100.0
        } else {
            0.0
        };

        Some(CostComparison {
            primary,
            alternatives,
            savings_cents,
            savings_percent,
        })
    }

    /// Get all models for a provider
    pub fn models_for_provider(&self, provider: &str) -> Vec<&ModelPricing> {
        self.pricing
            .values()
            .filter(|p| p.provider == provider)
            .collect()
    }

    /// Find cheapest model across all providers
    pub fn cheapest_model(
        &self,
        input_tokens: usize,
        output_tokens: usize,
    ) -> Option<EstimatedCost> {
        self.pricing
            .keys()
            .filter_map(|model_id| self.estimate_cost(model_id, input_tokens, output_tokens))
            .min_by(|a, b| a.total_cents.partial_cmp(&b.total_cents).unwrap())
    }

    /// Format cost for display
    pub fn format_cost_dollars(cents: f64) -> String {
        if cents < 1.0 {
            format!("${:.4}", cents / 100.0)
        } else if cents < 100.0 {
            format!("${:.2}", cents / 100.0)
        } else {
            format!("${:.1}", cents / 100.0)
        }
    }

    /// Format cost comparison for display
    pub fn format_comparison(comparison: &CostComparison) -> String {
        let mut output = String::new();
        output.push_str("💰 Cost Comparison\n");
        output.push_str(&format!(
            "  Primary: {} - {} tokens → {}\n",
            comparison.primary.model_id,
            comparison.primary.input_tokens + comparison.primary.output_tokens,
            Self::format_cost_dollars(comparison.primary.total_cents)
        ));

        if !comparison.alternatives.is_empty() {
            output.push_str("\n  Cheaper alternatives:\n");
            for (i, alt) in comparison.alternatives.iter().take(3).enumerate() {
                output.push_str(&format!(
                    "    {}. {} - {}\n",
                    i + 1,
                    alt.model_id,
                    Self::format_cost_dollars(alt.total_cents)
                ));
            }

            if comparison.savings_percent > 0.0 {
                output.push_str(&format!(
                    "\n  💡 Save {:.0}% ({}) by using {}\n",
                    comparison.savings_percent,
                    Self::format_cost_dollars(comparison.savings_cents),
                    comparison
                        .alternatives
                        .first()
                        .map(|c| c.model_id.as_str())
                        .unwrap_or("cheapest model")
                ));
            }
        }

        output
    }
}

impl Default for CostEstimator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creates_estimator() {
        let estimator = CostEstimator::new();
        assert!(!estimator.pricing.is_empty());
    }

    #[test]
    fn test_estimates_single_model() {
        let estimator = CostEstimator::new();
        let cost = estimator.estimate_cost("gpt-3.5-turbo", 1000, 500);

        assert!(cost.is_some());
        let cost = cost.unwrap();
        assert_eq!(cost.input_tokens, 1000);
        assert_eq!(cost.output_tokens, 500);
        assert!(cost.total_cents > 0.0);
    }

    #[test]
    fn test_compares_models() {
        let estimator = CostEstimator::new();
        let comparison = estimator.compare_costs(
            "gpt-4",
            10000,
            5000,
            &["gpt-3.5-turbo", "claude-3-haiku", "gemini-pro"],
        );

        assert!(comparison.is_some());
        let comparison = comparison.unwrap();
        assert!(comparison.alternatives.len() > 0);
        assert!(comparison.savings_cents >= 0.0);
    }

    #[test]
    fn test_finds_cheapest_model() {
        let estimator = CostEstimator::new();
        let cheapest = estimator.cheapest_model(10000, 5000);

        assert!(cheapest.is_some());
        // Gemini Pro should be cheapest (free tier)
        let cheapest = cheapest.unwrap();
        assert_eq!(cheapest.total_cents, 0.0);
    }

    #[test]
    fn test_registers_custom_pricing() {
        let mut estimator = CostEstimator::new();
        estimator.register_model(ModelPricing {
            model_id: "custom-model".to_string(),
            provider: "custom".to_string(),
            input_cost_per_1m: 100,
            output_cost_per_1m: 200,
            minimum_cost: 0.0,
        });

        let cost = estimator.estimate_cost("custom-model", 1000, 500);
        assert!(cost.is_some());
    }

    #[test]
    fn test_models_for_provider() {
        let estimator = CostEstimator::new();
        let openai_models = estimator.models_for_provider("openai");
        assert!(openai_models.len() >= 2);
    }

    #[test]
    fn test_formats_cost() {
        assert_eq!(CostEstimator::format_cost_dollars(5.0), "$0.0500");
        assert_eq!(CostEstimator::format_cost_dollars(150.0), "$1.50");
        assert_eq!(CostEstimator::format_cost_dollars(15000.0), "$150.0");
    }

    #[test]
    fn test_handles_zero_tokens() {
        let estimator = CostEstimator::new();
        let cost = estimator.estimate_cost("gpt-3.5-turbo", 0, 0);

        assert!(cost.is_some());
        let cost = cost.unwrap();
        assert_eq!(cost.total_cents, 0.0);
    }

    #[test]
    fn test_unknown_model_returns_none() {
        let estimator = CostEstimator::new();
        let cost = estimator.estimate_cost("unknown-model", 1000, 500);

        assert!(cost.is_none());
    }
}
