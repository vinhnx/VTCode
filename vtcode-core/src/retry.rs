//! Shared retry policy and classification helpers for VT Code.

use std::time::Duration;

use crate::config::constants::tools;
use crate::error::{ErrorCategory, VtCodeError};
use crate::tools::tool_intent;
use crate::tools::unified_error::UnifiedToolError;
use vtcode_commons::llm::{LLMError, LLMErrorMetadata};

/// Typed retry policy shared across runtime layers.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of total attempts, including the initial call.
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub multiplier: f64,
    pub jitter: f64,
}

impl RetryPolicy {
    pub fn new(
        max_attempts: u32,
        initial_delay: Duration,
        max_delay: Duration,
        multiplier: f64,
    ) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            initial_delay,
            max_delay,
            multiplier: multiplier.max(1.0),
            jitter: 0.0,
        }
    }

    pub fn from_retries(
        max_retries: u32,
        initial_delay: Duration,
        max_delay: Duration,
        multiplier: f64,
    ) -> Self {
        Self::new(
            max_retries.saturating_add(1),
            initial_delay,
            max_delay,
            multiplier,
        )
    }

    pub fn delay_for_attempt(&self, attempt_index: u32) -> Duration {
        let multiplier = self.multiplier.powi(attempt_index as i32);
        let base_delay = Duration::from_secs_f64(self.initial_delay.as_secs_f64() * multiplier)
            .min(self.max_delay);

        if self.jitter <= 0.0 {
            return base_delay;
        }

        let max_jitter_ms = (base_delay.as_millis() as f64 * self.jitter)
            .round()
            .clamp(0.0, u64::MAX as f64) as u64;
        if max_jitter_ms == 0 {
            return base_delay;
        }

        let offset = (u64::from(attempt_index) * 31) % (max_jitter_ms + 1);
        base_delay.saturating_add(Duration::from_millis(offset))
    }

    pub fn decision_for_category(
        &self,
        category: ErrorCategory,
        attempt_index: u32,
        retry_after: Option<Duration>,
    ) -> RetryDecision {
        let has_remaining_attempts = attempt_index.saturating_add(1) < self.max_attempts;
        if !category.is_retryable() || !has_remaining_attempts {
            return RetryDecision {
                category,
                retryable: false,
                delay: None,
                retry_after,
            };
        }

        let delay = retry_after.unwrap_or_else(|| self.delay_for_attempt(attempt_index));
        RetryDecision {
            category,
            retryable: true,
            delay: Some(delay),
            retry_after,
        }
    }

    pub fn decision_for_vtcode_error(
        &self,
        error: &VtCodeError,
        attempt_index: u32,
        tool_name: Option<&str>,
    ) -> RetryDecision {
        self.decision_for_category_with_tool(
            error.category,
            attempt_index,
            error.retry_after(),
            tool_name,
        )
    }

    pub fn decision_for_anyhow(
        &self,
        error: &anyhow::Error,
        attempt_index: u32,
        tool_name: Option<&str>,
    ) -> RetryDecision {
        if let Some(vtcode_error) = error.downcast_ref::<VtCodeError>() {
            return self.decision_for_vtcode_error(vtcode_error, attempt_index, tool_name);
        }
        if let Some(llm_error) = error.downcast_ref::<LLMError>() {
            return self.decision_for_llm_error(llm_error, attempt_index);
        }
        if let Some(tool_error) = error.downcast_ref::<UnifiedToolError>() {
            let tool_name = tool_name.or_else(|| {
                tool_error
                    .debug_context
                    .as_ref()
                    .map(|ctx| ctx.tool_name.as_str())
                    .filter(|tool_name| !tool_name.is_empty())
            });
            return self.decision_for_category_with_tool(
                tool_error.category(),
                attempt_index,
                None,
                tool_name,
            );
        }

        let category = vtcode_commons::classify_anyhow_error(error);
        self.decision_for_category_with_tool(category, attempt_index, None, tool_name)
    }

    pub fn decision_for_llm_error(&self, error: &LLMError, attempt_index: u32) -> RetryDecision {
        let retry_after = llm_metadata(error).and_then(parse_retry_after_header);
        self.decision_for_category_with_tool(
            ErrorCategory::from(error),
            attempt_index,
            retry_after,
            None,
        )
    }

    pub fn decision_for_tool_error(
        &self,
        error: &UnifiedToolError,
        attempt_index: u32,
    ) -> RetryDecision {
        let tool_name = error
            .debug_context
            .as_ref()
            .map(|ctx| ctx.tool_name.as_str())
            .filter(|tool_name| !tool_name.is_empty());
        self.decision_for_category_with_tool(error.category(), attempt_index, None, tool_name)
    }

    fn decision_for_category_with_tool(
        &self,
        category: ErrorCategory,
        attempt_index: u32,
        retry_after: Option<Duration>,
        tool_name: Option<&str>,
    ) -> RetryDecision {
        if matches!(category, ErrorCategory::Timeout) && tool_name.is_some_and(is_command_tool) {
            return RetryDecision {
                category,
                retryable: false,
                delay: None,
                retry_after,
            };
        }

        self.decision_for_category(category, attempt_index, retry_after)
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::from_retries(2, Duration::from_secs(1), Duration::from_secs(60), 2.0)
    }
}

/// Result of classifying a failure for retry handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryDecision {
    pub category: ErrorCategory,
    pub retryable: bool,
    pub delay: Option<Duration>,
    pub retry_after: Option<Duration>,
}

fn llm_metadata(error: &LLMError) -> Option<&LLMErrorMetadata> {
    match error {
        LLMError::Authentication { metadata, .. }
        | LLMError::RateLimit { metadata }
        | LLMError::InvalidRequest { metadata, .. }
        | LLMError::Network { metadata, .. }
        | LLMError::Provider { metadata, .. } => metadata.as_deref(),
    }
}

fn parse_retry_after_header(metadata: &LLMErrorMetadata) -> Option<Duration> {
    let raw = metadata.retry_after.as_deref()?.trim();
    if let Ok(seconds) = raw.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    if let Ok(seconds) = raw.parse::<f64>() {
        return Some(Duration::from_secs_f64(seconds.max(0.0)));
    }
    None
}

pub fn decision_for_vtcode_error(
    error: &VtCodeError,
    attempt_index: u32,
    tool_name: Option<&str>,
    policy_override: Option<&RetryPolicy>,
) -> RetryDecision {
    let owned_policy;
    let policy = if let Some(policy) = policy_override {
        policy
    } else {
        owned_policy = RetryPolicy::default();
        &owned_policy
    };
    policy.decision_for_vtcode_error(error, attempt_index, tool_name)
}

pub fn decision_for_anyhow_error(
    error: &anyhow::Error,
    attempt_index: u32,
    tool_name: Option<&str>,
    policy_override: Option<&RetryPolicy>,
) -> RetryDecision {
    let owned_policy;
    let policy = if let Some(policy) = policy_override {
        policy
    } else {
        owned_policy = RetryPolicy::default();
        &owned_policy
    };
    policy.decision_for_anyhow(error, attempt_index, tool_name)
}

pub fn is_command_tool(tool_name: &str) -> bool {
    tool_name == tools::CREATE_PTY_SESSION
        || tool_name == tools::SEND_PTY_INPUT
        || tool_intent::canonical_unified_exec_tool_name(tool_name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{ErrorCode, VtCodeError};

    #[test]
    fn non_retryable_categories_stop_immediately() {
        let policy =
            RetryPolicy::from_retries(2, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        let err = VtCodeError::security(ErrorCode::PermissionDenied, "blocked by policy");

        let decision = policy.decision_for_vtcode_error(&err, 0, None);
        assert_eq!(decision.category, ErrorCategory::PolicyViolation);
        assert!(!decision.retryable);
        assert!(decision.delay.is_none());
    }

    #[test]
    fn retry_after_header_overrides_backoff_delay() {
        let policy =
            RetryPolicy::from_retries(3, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        let err = LLMError::RateLimit {
            metadata: Some(LLMErrorMetadata::new(
                "Anthropic",
                Some(429),
                Some("rate_limit_error".to_string()),
                None,
                None,
                Some("7".to_string()),
                Some("too many requests".to_string()),
            )),
        };

        let decision = policy.decision_for_llm_error(&err, 0);
        assert!(decision.retryable);
        assert_eq!(decision.retry_after, Some(Duration::from_secs(7)));
        assert_eq!(decision.delay, Some(Duration::from_secs(7)));
    }

    #[test]
    fn quota_exhaustion_is_not_retryable() {
        let policy =
            RetryPolicy::from_retries(3, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        let err = LLMError::RateLimit {
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

        let decision = policy.decision_for_llm_error(&err, 0);
        assert_eq!(decision.category, ErrorCategory::ResourceExhausted);
        assert!(!decision.retryable);
    }

    #[test]
    fn anyhow_fallback_uses_shared_classifier() {
        let policy =
            RetryPolicy::from_retries(1, Duration::from_secs(1), Duration::from_secs(8), 2.0);

        let decision =
            policy.decision_for_anyhow(&anyhow::anyhow!("HTTP 503 Service Unavailable"), 0, None);
        assert_eq!(decision.category, ErrorCategory::Network);
        assert!(decision.retryable);
        assert_eq!(decision.delay, Some(Duration::from_secs(1)));
    }

    #[test]
    fn anyhow_prefers_typed_llm_errors() {
        let policy =
            RetryPolicy::from_retries(3, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        let err = anyhow::Error::new(LLMError::RateLimit {
            metadata: Some(LLMErrorMetadata::new(
                "Anthropic",
                Some(429),
                Some("rate_limit_error".to_string()),
                None,
                None,
                Some("9".to_string()),
                Some("too many requests".to_string()),
            )),
        });

        let decision = policy.decision_for_anyhow(&err, 0, None);
        assert!(decision.retryable);
        assert_eq!(decision.retry_after, Some(Duration::from_secs(9)));
        assert_eq!(decision.delay, Some(Duration::from_secs(9)));
    }

    #[test]
    fn canonical_exec_aliases_are_command_tools() {
        for alias in [
            tools::RUN_PTY_CMD,
            tools::EXEC_COMMAND,
            tools::WRITE_STDIN,
            tools::UNIFIED_EXEC,
            "shell",
            "bash",
            "container.exec",
        ] {
            assert!(
                is_command_tool(alias),
                "expected {alias} to be a command tool"
            );
        }
    }

    #[test]
    fn typed_tool_timeout_for_command_tools_is_not_retryable() {
        let policy =
            RetryPolicy::from_retries(2, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        let err = UnifiedToolError::new(
            crate::tools::unified_error::UnifiedErrorKind::Timeout,
            "timed out",
        )
        .with_tool_name(tools::RUN_PTY_CMD);

        let decision = policy.decision_for_tool_error(&err, 0);
        assert_eq!(decision.category, ErrorCategory::Timeout);
        assert!(!decision.retryable);
    }

    #[test]
    fn anyhow_typed_tool_timeout_uses_fallback_tool_name() {
        let policy =
            RetryPolicy::from_retries(2, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        let err = anyhow::Error::new(UnifiedToolError::new(
            crate::tools::unified_error::UnifiedErrorKind::Timeout,
            "timed out",
        ));

        let decision = policy.decision_for_anyhow(&err, 0, Some(tools::RUN_PTY_CMD));
        assert_eq!(decision.category, ErrorCategory::Timeout);
        assert!(!decision.retryable);
    }

    #[test]
    fn command_timeouts_do_not_retry() {
        let policy =
            RetryPolicy::from_retries(2, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        let err = VtCodeError::new(ErrorCategory::Timeout, ErrorCode::Timeout, "timed out");

        let decision = policy.decision_for_vtcode_error(&err, 0, Some(tools::RUN_PTY_CMD));
        assert_eq!(decision.category, ErrorCategory::Timeout);
        assert!(!decision.retryable);
    }
}
