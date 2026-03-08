use std::time::Duration;

use vtcode_core::RetryPolicy;
use vtcode_core::llm::provider::{LLMError, LLMErrorMetadata};

#[test]
fn retry_policy_uses_retry_after_and_blocks_quota_exhaustion() {
    let policy = RetryPolicy::from_retries(2, Duration::from_secs(1), Duration::from_secs(4), 2.0);

    let rate_limited = LLMError::RateLimit {
        metadata: Some(LLMErrorMetadata::new(
            "Anthropic",
            Some(429),
            Some("rate_limit_error".to_string()),
            None,
            None,
            Some("5".to_string()),
            Some("try again later".to_string()),
        )),
    };
    let limited_decision = policy.decision_for_llm_error(&rate_limited, 0);
    assert!(limited_decision.retryable);
    assert_eq!(limited_decision.retry_after, Some(Duration::from_secs(5)));
    assert_eq!(limited_decision.delay, Some(Duration::from_secs(5)));

    let quota_exhausted = LLMError::RateLimit {
        metadata: Some(LLMErrorMetadata::new(
            "OpenAI",
            Some(429),
            Some("insufficient_quota".to_string()),
            None,
            None,
            None,
            Some("quota exceeded".to_string()),
        )),
    };
    let quota_decision = policy.decision_for_llm_error(&quota_exhausted, 0);
    assert!(!quota_decision.retryable);
}
