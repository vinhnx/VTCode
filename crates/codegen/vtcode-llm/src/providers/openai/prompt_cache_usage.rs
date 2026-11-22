//! OpenAI-only cache-write lifetime telemetry.
//!
//! GPT-5.6+ Responses usage reports cache writes split by requested lifetime in
//! `input_tokens_details.cache_write_tokens_details`. The two counts are a
//! breakdown of `cache_write_tokens`, not additional charges. VT Code logs them
//! for policy validation; usage accounting and cost math stay unchanged.

use serde_json::Value;

/// Reported cache-write counts by lifetime.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CacheWriteLifetimeBreakdown {
    pub ttl_30m_tokens: Option<u32>,
    pub ttl_12h_tokens: Option<u32>,
}

/// Read and log the OpenAI cache-write lifetime breakdown when metrics are enabled.
///
/// Missing, non-numeric, or oversized counts are ignored individually. This
/// never affects usage parsing or billing; callers use the existing
/// `cache_write_tokens` counter for cost accounting.
pub(crate) fn log_cache_write_lifetime_breakdown(
    usage: &Value,
    metrics_enabled: bool,
) -> Option<CacheWriteLifetimeBreakdown> {
    if !metrics_enabled {
        return None;
    }

    let details = usage.get("input_tokens_details")?.get("cache_write_tokens_details")?;
    let breakdown = CacheWriteLifetimeBreakdown {
        ttl_30m_tokens: bounded_count(details.get("ttl_30m_tokens")),
        ttl_12h_tokens: bounded_count(details.get("ttl_12h_tokens")),
    };

    if breakdown != CacheWriteLifetimeBreakdown::default() {
        tracing::debug!(
            target = "vtcode::llm::responses::prompt_cache",
            ttl_30m_tokens = breakdown.ttl_30m_tokens,
            ttl_12h_tokens = breakdown.ttl_12h_tokens,
            "OpenAI Responses cache-write lifetime breakdown"
        );
    }

    Some(breakdown)
}

fn bounded_count(value: Option<&Value>) -> Option<u32> {
    value.and_then(Value::as_u64).and_then(|count| u32::try_from(count).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_both_lifetimes() {
        let usage = json!({
            "input_tokens": 12_000,
            "input_tokens_details": {
                "cached_tokens": 4_000,
                "cache_write_tokens": 7_000,
                "cache_write_tokens_details": {"ttl_30m_tokens": 3_000, "ttl_12h_tokens": 4_000}
            }
        });

        assert_eq!(
            log_cache_write_lifetime_breakdown(&usage, true),
            Some(CacheWriteLifetimeBreakdown {
                ttl_30m_tokens: Some(3_000),
                ttl_12h_tokens: Some(4_000),
            })
        );
    }

    #[test]
    fn metrics_gate_and_malformed_counts() {
        let usage = json!({
            "input_tokens_details": {
                "cache_write_tokens_details": {
                    "ttl_30m_tokens": -1,
                    "ttl_12h_tokens": "123",
                }
            }
        });
        assert_eq!(log_cache_write_lifetime_breakdown(&usage, false), None);
        assert_eq!(log_cache_write_lifetime_breakdown(&usage, true), Some(CacheWriteLifetimeBreakdown::default()));
    }

    #[test]
    fn missing_details_are_not_an_error() {
        assert_eq!(log_cache_write_lifetime_breakdown(&json!({"usage": {}}), true), None);
        assert_eq!(log_cache_write_lifetime_breakdown(&json!({"input_tokens_details": {}}), true), None);
    }

    #[test]
    fn oversized_counts_are_ignored() {
        let usage = json!({
            "input_tokens_details": {
                "cache_write_tokens_details": {"ttl_30m_tokens": u64::from(u32::MAX) + 1}
            }
        });
        assert_eq!(
            log_cache_write_lifetime_breakdown(&usage, true),
            Some(CacheWriteLifetimeBreakdown { ttl_30m_tokens: None, ttl_12h_tokens: None })
        );
    }
}
