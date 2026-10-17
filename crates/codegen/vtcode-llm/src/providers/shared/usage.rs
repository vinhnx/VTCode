//! Provider cache-token counters and usage-key compatibility.

use serde_json::Value;

/// Read a u32 usage count from the first present key.
///
/// Usage blocks mix OpenAI (`prompt_tokens`) and Responses/Anthropic
/// (`input_tokens`) spellings, so callers pass the alias chain; a missing or
/// non-numeric count reads as 0. This is the canonical alias lookup — do not
/// fork the chain per provider.
pub(crate) fn usage_u32_from_keys(usage: &Value, keys: &[&str]) -> u32 {
    keys.iter()
        .find_map(|key| usage.get(*key))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0)
}

pub(crate) fn parse_cached_prompt_tokens_from_usage(
    usage_value: &Value,
    include_cached_prompt_metrics: bool,
) -> Option<u32> {
    if !include_cached_prompt_metrics {
        return None;
    }

    let cached_prompt_tokens = usage_value
        .get("input_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .or_else(|| {
            usage_value
                .get("prompt_tokens_details")
                .and_then(|details| details.get("cached_tokens"))
        })
        .or_else(|| usage_value.get("prompt_cache_hit_tokens"))
        .or_else(|| usage_value.get("cached_tokens"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());

    if let Some(cached_prompt_tokens) = cached_prompt_tokens {
        tracing::debug!(
            target = "vtcode::llm::responses::prompt_cache",
            cached_prompt_tokens,
            "Responses cached prompt token usage"
        );
    }

    cached_prompt_tokens
}

/// Parse OpenAI `cache_write_tokens` from a Responses/Chat usage payload.
///
/// GPT-5.6 and later bill cache writes at 1.25x the input rate; downstream
/// cost math consumes this via `Usage::cache_creation_tokens`.
/// Mirrors [`parse_cached_prompt_tokens_from_usage`], including the metrics
/// gate: no value is reported when cached-prompt metrics are disabled.
pub(crate) fn parse_cache_write_tokens_from_usage(
    usage_value: &Value,
    include_cached_prompt_metrics: bool,
) -> Option<u32> {
    if !include_cached_prompt_metrics {
        return None;
    }

    let cache_write_tokens = usage_value
        .get("input_tokens_details")
        .and_then(|details| details.get("cache_write_tokens"))
        .or_else(|| {
            usage_value
                .get("prompt_tokens_details")
                .and_then(|details| details.get("cache_write_tokens"))
        })
        .or_else(|| usage_value.get("prompt_cache_write_tokens"))
        .or_else(|| usage_value.get("cache_write_tokens"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());

    if let Some(cache_write_tokens) = cache_write_tokens {
        tracing::debug!(
            target = "vtcode::llm::responses::prompt_cache",
            cache_write_tokens,
            "Responses cache-write token usage"
        );
    }

    cache_write_tokens
}
