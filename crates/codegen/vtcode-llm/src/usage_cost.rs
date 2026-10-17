//! Provider-normalized token usage and shared raw/cache-aware USD estimates.

use crate::model_resolver::ModelResolver;
use crate::provider::Usage as ProviderUsage;
use vtcode_config::models::ModelPricing;

/// Returns true when `provider` reports `prompt_tokens` exclusive of
/// cache-read and cache-creation tokens.
///
/// Anthropic and Minimax (which wraps the Anthropic provider) report
/// `prompt_tokens` as the count of tokens billed at the full input rate,
/// separate from cache-read and cache-creation tokens. All other providers
/// report `prompt_tokens` as a total that already includes cached tokens, so
/// no adjustment is needed for them.
pub fn provider_reports_exclusive_input(provider: &str) -> bool {
    matches!(provider.trim().to_ascii_lowercase().as_str(), "anthropic" | "minimax")
}

/// Build a per-turn harness `Usage` sample from raw provider usage, applying
/// the provider-specific normalization documented on
/// [`provider_reports_exclusive_input`] so `input_tokens` always represents
/// the total prompt token count across every provider.
pub fn normalized_turn_usage(provider: &str, usage: &ProviderUsage) -> vtcode_exec_events::Usage {
    let totals = usage.billable_totals();
    let cached = u64::from(totals.cache_read_tokens);
    let creation = u64::from(totals.cache_creation_tokens);
    let mut input = u64::from(totals.prompt_tokens);
    if provider_reports_exclusive_input(provider) {
        input = input.saturating_add(cached).saturating_add(creation);
    }
    let output = u64::from(totals.completion_tokens);

    vtcode_exec_events::Usage {
        input_tokens: input,
        cached_input_tokens: cached,
        cache_creation_tokens: creation,
        output_tokens: output,
    }
}

/// Cache-aware and conservative session cost estimates in USD.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SessionCostEstimate {
    /// Every input token priced at the full input rate, with no cache
    /// discount applied. This is the conservative, deterministic figure used
    /// for budget enforcement.
    pub raw_usd: f64,
    /// Cache-aware estimate that discounts cache-read tokens and surcharges
    /// cache-creation tokens, for transparency in user-facing reporting.
    pub effective_usd: f64,
}

/// Base price for the bounded, standard-endpoint Decisions probe, not generation.
/// OpenAI charges $0.10 per million input tokens and no output/cache surcharge.
/// Regional and long-context premiums are outside this bounded probe route.
pub fn estimate_decisions_cost(usage: &ProviderUsage) -> SessionCostEstimate {
    let cost = f64::from(usage.prompt_tokens) * 0.10 / 1_000_000.0;
    SessionCostEstimate { raw_usd: cost, effective_usd: cost }
}

/// Accumulates independently priced turns without repricing earlier model routes.
/// Once a turn cannot be priced, a complete session total remains unknown.
#[derive(Debug, Clone)]
pub struct SessionCostAccumulator {
    total: Option<SessionCostEstimate>,
}

impl Default for SessionCostAccumulator {
    fn default() -> Self {
        Self {
            total: Some(SessionCostEstimate { raw_usd: 0.0, effective_usd: 0.0 }),
        }
    }
}

impl SessionCostAccumulator {
    pub fn record(&mut self, estimate: Option<SessionCostEstimate>) -> Option<SessionCostEstimate> {
        self.total = self.total.zip(estimate).and_then(|(total, turn)| {
            let raw_usd = total.raw_usd + turn.raw_usd;
            let effective_usd = total.effective_usd + turn.effective_usd;
            (raw_usd.is_finite() && effective_usd.is_finite()).then_some(SessionCostEstimate { raw_usd, effective_usd })
        });
        self.total
    }

    pub fn total(&self) -> Option<SessionCostEstimate> {
        self.total
    }
}

/// Resolve pricing for `provider`/`model` and estimate session costs from
/// accumulated harness usage. Returns `None` when the model cannot be
/// resolved or pricing metadata is unavailable.
pub fn estimate_session_costs(
    provider: &str,
    model: &str,
    usage: &vtcode_exec_events::Usage,
) -> Option<SessionCostEstimate> {
    let resolved = ModelResolver::resolve(Some(provider), model, &[], None)?;
    let pricing = resolved.pricing()?;
    estimate_session_costs_with_pricing(pricing, usage)
}

/// Reject a priced session budget when its selected route cannot be priced.
/// Call before any inference, including automatic compaction.
pub fn require_budget_pricing(provider: &str, model: &str, max_budget_usd: Option<f64>) -> anyhow::Result<()> {
    if let Some(maximum) = max_budget_usd {
        anyhow::ensure!(maximum.is_finite() && maximum >= 0.0, "Session USD budget must be finite and non-negative");
        anyhow::ensure!(
            estimate_session_costs(provider, model, &vtcode_exec_events::Usage::default()).is_some(),
            "Cannot enforce session USD budget for `{provider}/{model}`: complete valid pricing metadata is unavailable"
        );
    }
    Ok(())
}

/// Estimate session costs from an already-resolved [`ModelPricing`].
///
/// `effective_usd` can exceed `raw_usd` early in a session when
/// cache-creation tokens (billed at a premium) dominate the accumulated
/// usage. `raw_usd` remains the enforcement figure so budget behavior stays
/// deterministic and discount-free.
pub fn estimate_session_costs_with_pricing(
    pricing: ModelPricing,
    usage: &vtcode_exec_events::Usage,
) -> Option<SessionCostEstimate> {
    let input_rate = pricing.input?;
    let output_rate = pricing.output?;
    if [pricing.input, pricing.output, pricing.cache_read, pricing.cache_write]
        .into_iter()
        .flatten()
        .any(|rate| !rate.is_finite() || rate < 0.0)
    {
        return None;
    }

    let input_tokens = usage.input_tokens as f64;
    let output_tokens = usage.output_tokens as f64;
    let cached_tokens = usage.cached_input_tokens as f64;
    let creation_tokens = usage.cache_creation_tokens as f64;

    let raw_usd = input_tokens * input_rate + output_tokens * output_rate;

    // Heuristic fallbacks when a model's catalog entry does not specify
    // dedicated cache rates: cache reads are assumed to cost roughly 10% of
    // the input rate, and cache writes roughly 125% of the input rate
    // (2x for Anthropic 1h extended TTL).
    let read_rate = pricing.cache_read.unwrap_or(input_rate * 0.10);
    let write_rate = pricing.cache_write.unwrap_or(input_rate * DEFAULT_CACHE_WRITE_MULTIPLIER);

    let uncached_tokens = usage
        .input_tokens
        .saturating_sub(usage.cached_input_tokens)
        .saturating_sub(usage.cache_creation_tokens) as f64;

    let effective_usd = uncached_tokens * input_rate
        + cached_tokens * read_rate
        + creation_tokens * write_rate
        + output_tokens * output_rate;

    (raw_usd.is_finite() && effective_usd.is_finite()).then_some(SessionCostEstimate { raw_usd, effective_usd })
}

/// Default cache-write multiplier versus uncached input (5m TTL).
pub const DEFAULT_CACHE_WRITE_MULTIPLIER: f64 = 1.25;

/// Anthropic 1h extended-TTL cache writes cost 2x base input.
pub const EXTENDED_TTL_CACHE_WRITE_MULTIPLIER: f64 = 2.0;

/// Effective cache-write rate. `extended_ttl` selects the 2x 1h multiplier
/// when the catalog does not declare an explicit write rate.
#[must_use]
pub fn cache_write_rate(input_rate: f64, configured: Option<f64>, extended_ttl: bool) -> f64 {
    if let Some(rate) = configured {
        return rate;
    }
    let multiplier = if extended_ttl {
        EXTENDED_TTL_CACHE_WRITE_MULTIPLIER
    } else {
        DEFAULT_CACHE_WRITE_MULTIPLIER
    };
    input_rate * multiplier
}

/// Prompt volume that counts toward provider TPM-style rate limits. Cached
/// and cache-write tokens are cheaper but still consume provider capacity.
#[must_use]
pub fn prompt_tokens_for_rate_limit(usage: &vtcode_exec_events::Usage) -> u64 {
    let uncached = usage
        .input_tokens
        .saturating_sub(usage.cached_input_tokens)
        .saturating_sub(usage.cache_creation_tokens);
    uncached
        .saturating_add(usage.cached_input_tokens)
        .saturating_add(usage.cache_creation_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_normalization_matches_across_three_provider_families() {
        let pricing = ModelPricing {
            input: Some(0.01),
            output: Some(0.02),
            cache_read: Some(0.001),
            cache_write: Some(0.0125),
        };
        for provider in ["openai", "anthropic", "gemini"] {
            let usage = ProviderUsage {
                prompt_tokens: if provider == "anthropic" { 150 } else { 1000 },
                completion_tokens: 100,
                total_tokens: 1100,
                cached_prompt_tokens: None,
                cache_read_tokens: Some(800),
                cache_creation_tokens: Some(50),
                iterations: None,
            };
            let normalized = normalized_turn_usage(provider, &usage);
            let cost = estimate_session_costs_with_pricing(pricing, &normalized).expect("priced");
            assert!((cost.raw_usd - 12.0).abs() < 1e-12, "{provider}");
            assert!((cost.effective_usd - 4.925).abs() < 1e-12, "{provider}");
        }
    }

    #[test]
    fn anthropic_compaction_iterations_are_included_in_normalized_usage() {
        let usage = ProviderUsage {
            // These top-level values describe only the final message pass.
            prompt_tokens: 10,
            completion_tokens: 1,
            total_tokens: 11,
            cached_prompt_tokens: None,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            iterations: Some(vec![
                serde_json::json!({
                    "type": "compaction",
                    "input_tokens": 50,
                    "output_tokens": 5,
                }),
                serde_json::json!({
                    "type": "message",
                    "input_tokens": 10,
                    "output_tokens": 2,
                }),
            ]),
        };

        let normalized = normalized_turn_usage("anthropic", &usage);
        assert_eq!(normalized.input_tokens, 60);
        assert_eq!(normalized.output_tokens, 7);
    }

    #[test]
    fn astra_pricing_is_resolved_per_route_without_inference() {
        let usage = vtcode_exec_events::Usage {
            input_tokens: 1000,
            output_tokens: 100,
            cached_input_tokens: 800,
            cache_creation_tokens: 0,
        };
        for (provider, model, priced) in [
            ("openai", "gpt-6-astra", true),
            ("openrouter", "openai/gpt-6-astra", true),
            ("merge-gateway", "openai/gpt-6-astra", false),
        ] {
            assert_eq!(estimate_session_costs(provider, model, &usage).is_some(), priced, "{provider}");
            assert_eq!(require_budget_pricing(provider, model, Some(1.0)).is_ok(), priced, "{provider}");
        }
    }

    #[test]
    fn switching_to_a_cheaper_model_never_reprices_previous_spend() {
        let usage = vtcode_exec_events::Usage { input_tokens: 100, ..Default::default() };
        let mut session = SessionCostAccumulator::default();
        for (rate, expected) in [(0.10, 10.0), (0.001, 10.1), (0.10, 20.1)] {
            let pricing = ModelPricing {
                input: Some(rate),
                output: Some(rate),
                cache_read: None,
                cache_write: None,
            };
            let total = session
                .record(estimate_session_costs_with_pricing(pricing, &usage))
                .expect("priced");
            assert!((total.raw_usd - expected).abs() < 1e-12);
            assert!((total.effective_usd - expected).abs() < 1e-12);
        }
    }

    #[test]
    fn an_unpriced_turn_keeps_session_cost_unknown() {
        let mut session = SessionCostAccumulator::default();
        assert!(session.record(None).is_none());
        assert!(
            session
                .record(Some(SessionCostEstimate { raw_usd: 1.0, effective_usd: 1.0 }))
                .is_none()
        );
    }

    #[test]
    fn missing_pricing_requires_removing_the_budget_explicitly() {
        assert!(require_budget_pricing("openai", "unknown-dynamic-model", Some(1.0)).is_err());
        assert!(require_budget_pricing("openai", "unknown-dynamic-model", None).is_ok());
    }

    #[test]
    fn invalid_pricing_cannot_bypass_budget_enforcement() {
        for invalid in [f64::NAN, f64::INFINITY, -1.0] {
            let pricing = ModelPricing {
                input: Some(invalid),
                output: Some(0.01),
                cache_read: None,
                cache_write: None,
            };
            assert!(estimate_session_costs_with_pricing(pricing, &vtcode_exec_events::Usage::default()).is_none());
        }
    }

    #[test]
    fn overflowing_estimates_are_treated_as_unpriced() {
        let pricing = ModelPricing {
            input: Some(f64::MAX),
            output: Some(f64::MAX),
            cache_read: None,
            cache_write: None,
        };
        let usage = vtcode_exec_events::Usage {
            input_tokens: u64::MAX,
            output_tokens: u64::MAX,
            ..Default::default()
        };
        assert!(estimate_session_costs_with_pricing(pricing, &usage).is_none());
    }

    #[test]
    fn cache_write_rate_uses_extended_ttl_multiplier() {
        #[allow(clippy::float_cmp, reason = "exact constants under test")]
        {
            assert_eq!(cache_write_rate(1.0, None, false), DEFAULT_CACHE_WRITE_MULTIPLIER);
            assert_eq!(cache_write_rate(1.0, None, true), EXTENDED_TTL_CACHE_WRITE_MULTIPLIER);
            // Explicit catalog rates win over the heuristic multipliers.
            assert_eq!(cache_write_rate(1.0, Some(0.5), true), 0.5);
        }
    }

    #[test]
    fn prompt_tokens_for_rate_limit_count_cache_traffic() {
        let usage = vtcode_exec_events::Usage {
            input_tokens: 1000,
            cached_input_tokens: 400,
            cache_creation_tokens: 200,
            output_tokens: 50,
        };
        // Uncached (400) + cached (400) + creation (200) = 1000.
        assert_eq!(prompt_tokens_for_rate_limit(&usage), 1000);
    }
}
