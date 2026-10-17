//! Allowlisted provider diagnostics; never log arbitrary response content.

use serde_json::Value;

pub(super) struct CacheDiagnostics {
    pub kind: &'static str,
    pub reason: Option<&'static str>,
    pub missed_tokens: Option<u64>,
    pub reusable_tokens: Option<u64>,
}

pub(super) fn parse(response: &Value) -> Option<CacheDiagnostics> {
    let diagnostics = response.get("prompt_cache_diagnostics")?;
    let kind = match diagnostics.get("type")?.as_str()? {
        "cache_hit" => "cache_hit",
        "cache_miss" => "cache_miss",
        "comparison_response_not_found" => "comparison_response_not_found",
        "unavailable" => "unavailable",
        _ => return None,
    };
    let mut parsed = CacheDiagnostics {
        kind,
        reason: None,
        missed_tokens: None,
        reusable_tokens: None,
    };
    if kind == "cache_miss" {
        parsed.reason = Some(match diagnostics.get("reason").and_then(Value::as_str) {
            Some("model_changed") => "model_changed",
            Some("prompt_cache_key_changed") => "prompt_cache_key_changed",
            Some("service_tier_changed") => "service_tier_changed",
            Some("tools_changed") => "tools_changed",
            Some("text_format_changed") => "text_format_changed",
            Some("reasoning_effort_changed") => "reasoning_effort_changed",
            Some("verbosity_changed") => "verbosity_changed",
            Some("context_compacted") => "context_compacted",
            Some("input_changed") => "input_changed",
            _ => "unclassified",
        });
        parsed.missed_tokens = diagnostics.get("cache_missed_tokens").and_then(Value::as_u64);
        parsed.reusable_tokens = diagnostics.get("comparison_reusable_tokens").and_then(Value::as_u64);
    }
    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_miss_counts_are_separate_from_billed_usage() {
        let response = json!({
            "usage": {"input_tokens": 12740, "input_tokens_details": {"cached_tokens": 0}},
            "prompt_cache_diagnostics": {
                "type": "cache_miss", "reason": "context_compacted",
                "cache_missed_tokens": 6003, "comparison_reusable_tokens": 7311
            }
        });
        let parsed = parse(&response).expect("supported diagnostics");
        assert_eq!(parsed.kind, "cache_miss");
        assert_eq!(parsed.reason, Some("context_compacted"));
        assert_eq!(parsed.missed_tokens, Some(6003));
        assert_eq!(parsed.reusable_tokens, Some(7311));
        assert_eq!(response.pointer("/usage/input_tokens"), Some(&json!(12740)));
    }

    #[test]
    fn diagnostic_statuses_do_not_imply_usage_hits_or_generation_failure() {
        for kind in ["cache_hit", "comparison_response_not_found", "unavailable"] {
            let parsed = parse(&json!({"prompt_cache_diagnostics": {
                "type": kind, "reason": "tools_changed", "cache_missed_tokens": 42
            }}))
            .expect("status accepted");
            assert_eq!(parsed.kind, kind);
            assert!(parsed.reason.is_none());
            assert!(parsed.missed_tokens.is_none());
        }
        assert!(parse(&json!({})).is_none());
        assert!(parse(&json!({"prompt_cache_diagnostics": {"type": "secret\nforged log"}})).is_none());
    }

    #[test]
    fn untrusted_diagnostics_are_bounded_and_malformed_counts_are_ignored() {
        let parsed = parse(&json!({"prompt_cache_diagnostics": {
            "type": "cache_miss", "reason": "secret\nforged log".repeat(10000),
            "cache_missed_tokens": -1, "comparison_reusable_tokens": "123"
        }}))
        .expect("miss accepted");
        assert_eq!(parsed.reason, Some("unclassified"));
        assert!(parsed.missed_tokens.is_none());
        assert!(parsed.reusable_tokens.is_none());
    }
}
