//! Domain-specific retry adapters for VT Code.
//!
//! The retry *policy math* (attempt budgets, backoff, jitter, category-based
//! decisions) lives in [`vtcode_commons::retry`]; this module re-exports the
//! canonical [`RetryPolicy`] / [`RetryDecision`] and layers the workspace's
//! domain knowledge on top via [`RetryPolicyCoreExt`]: typed error downcasts
//! (`VtCodeError`, `LLMError`, tool errors), LLM `Retry-After` extraction,
//! and the tool-aware command-timeout rule.

use std::future::Future;
use std::result::Result as StdResult;
use std::time::Duration;

use crate::error::{ErrorCategory, VtCodeError};
use crate::retry_after::retry_after_from_llm_metadata;
use crate::tools::registry::ToolExecutionError;
use crate::tools::tool_intent::is_command_tool;
use crate::tools::unified_error::UnifiedToolError;
use vtcode_commons::llm::LLMError;
use vtcode_commons::misconfiguration::detect_misconfiguration;

pub use vtcode_commons::retry::{RetryDecision, RetryPolicy};

/// Domain-aware retry decisions layered over the shared [`RetryPolicy`].
///
/// Import this trait to call the `decision_for_*` / `step_for_*` methods on
/// a policy; the policy struct itself is defined in `vtcode-commons`.
pub trait RetryPolicyCoreExt {
    fn decision_for_vtcode_error(
        &self,
        error: &VtCodeError,
        attempt_index: u32,
        tool_name: Option<&str>,
    ) -> RetryDecision;

    fn decision_for_anyhow(&self, error: &anyhow::Error, attempt_index: u32, tool_name: Option<&str>) -> RetryDecision;

    fn decision_for_llm_error(&self, error: &LLMError, attempt_index: u32) -> RetryDecision;

    fn decision_for_tool_error(&self, error: &UnifiedToolError, attempt_index: u32) -> RetryDecision;

    fn decision_for_tool_execution_error(&self, error: &ToolExecutionError, attempt_index: u32) -> RetryDecision;

    /// Classify a `VtCodeError` failure into a typed [`RetryStep`].
    ///
    /// This consolidates the "decision -> sleep or give-up" branching used by
    /// agent-level retry loops, removing the brittle
    /// `decision.delay.expect("retryable decisions need delay")` calls and
    /// guaranteeing a delay is always available for retryable steps.
    fn step_for_vtcode_error(&self, error: VtCodeError, attempt_index: u32, tool_name: Option<&str>) -> RetryStep;

    fn apply_to_tool_execution_error(
        &self,
        error: ToolExecutionError,
        attempt_index: u32,
        tool_name: Option<&str>,
    ) -> ToolExecutionError;
}

impl RetryPolicyCoreExt for RetryPolicy {
    fn decision_for_vtcode_error(
        &self,
        error: &VtCodeError,
        attempt_index: u32,
        tool_name: Option<&str>,
    ) -> RetryDecision {
        // Misconfiguration-first: never retry user-fixable config failures.
        if error.is_misconfiguration() {
            return misconfiguration_decision(error.category, error.retry_after());
        }
        decision_for_category_with_tool(self, error.category, attempt_index, error.retry_after(), tool_name)
    }

    fn decision_for_anyhow(&self, error: &anyhow::Error, attempt_index: u32, tool_name: Option<&str>) -> RetryDecision {
        if let Some(vtcode_error) = error.downcast_ref::<VtCodeError>() {
            return self.decision_for_vtcode_error(vtcode_error, attempt_index, tool_name);
        }
        if let Some(llm_error) = error.downcast_ref::<LLMError>() {
            return self.decision_for_llm_error(llm_error, attempt_index);
        }
        if let Some(tool_error) = error.downcast_ref::<UnifiedToolError>() {
            let category = tool_error.category();
            if detect_misconfiguration(category, &unified_tool_text(tool_error)).is_some() {
                return misconfiguration_decision(category, None);
            }
            // Preserve fallback tool name for command-timeout rule.
            let effective_tool = tool_name.or_else(|| {
                tool_error
                    .debug_context
                    .as_ref()
                    .map(|ctx| ctx.tool_name.as_str())
                    .filter(|name| !name.is_empty())
            });
            return decision_for_category_with_tool(self, category, attempt_index, None, effective_tool);
        }

        let category = vtcode_commons::classify_anyhow_error(error);
        let message = format!("{error:#}");
        if detect_misconfiguration(category, &message).is_some() {
            return misconfiguration_decision(category, None);
        }
        decision_for_category_with_tool(self, category, attempt_index, None, tool_name)
    }

    fn decision_for_llm_error(&self, error: &LLMError, attempt_index: u32) -> RetryDecision {
        let retry_after = error.metadata().and_then(retry_after_from_llm_metadata);
        let category = ErrorCategory::from(error);
        if vtcode_commons::detect_misconfiguration_in_llm_error(error).is_some() {
            return misconfiguration_decision(category, retry_after);
        }
        decision_for_category_with_tool(self, category, attempt_index, retry_after, None)
    }

    fn decision_for_tool_error(&self, error: &UnifiedToolError, attempt_index: u32) -> RetryDecision {
        let category = error.category();
        if detect_misconfiguration(category, &unified_tool_text(error)).is_some() {
            return misconfiguration_decision(category, None);
        }
        let tool_name = error
            .debug_context
            .as_ref()
            .map(|ctx| ctx.tool_name.as_str())
            .filter(|tool_name| !tool_name.is_empty());
        decision_for_category_with_tool(self, category, attempt_index, None, tool_name)
    }

    fn decision_for_tool_execution_error(&self, error: &ToolExecutionError, attempt_index: u32) -> RetryDecision {
        if error.patch_failure.is_some() {
            return RetryDecision {
                category: error.category,
                retryable: false,
                delay: None,
                retry_after: None,
            };
        }
        if detect_misconfiguration(error.category, &tool_execution_text(error)).is_some() {
            return misconfiguration_decision(error.category, error.retry_after());
        }
        decision_for_category_with_tool(
            self,
            error.category,
            attempt_index,
            error.retry_after(),
            Some(error.tool_name.as_str()),
        )
    }

    fn step_for_vtcode_error(&self, error: VtCodeError, attempt_index: u32, tool_name: Option<&str>) -> RetryStep {
        // Attach config guidance first so the surfaced error guides the user
        // before any retry/backoff decision.
        let error = error.with_misconfiguration_guidance();
        let decision = self.decision_for_vtcode_error(&error, attempt_index, tool_name);
        if decision.retryable {
            let delay = decision.delay.unwrap_or_else(|| self.delay_for_attempt(attempt_index));
            RetryStep::Backoff { delay, decision, error }
        } else {
            RetryStep::GiveUp { decision, error }
        }
    }

    fn apply_to_tool_execution_error(
        &self,
        error: ToolExecutionError,
        attempt_index: u32,
        tool_name: Option<&str>,
    ) -> ToolExecutionError {
        if error.patch_failure.is_some() {
            return error;
        }
        if let Some(guidance) = detect_misconfiguration(error.category, &tool_execution_text(&error)) {
            let decision = misconfiguration_decision(error.category, error.retry_after());
            let mut guided = error.with_retry_decision(decision);
            // Prepend config guidance so the user sees settings/config first.
            let mut suggestions = vec![std::borrow::Cow::Owned(guidance.user_message())];
            suggestions.append(&mut guided.recovery_suggestions);
            guided.recovery_suggestions = suggestions;
            guided.retryable = false;
            guided.is_recoverable = false;
            guided.circuit_breaker_impact = false;
            return guided;
        }
        let decision = decision_for_category_with_tool(
            self,
            error.category,
            attempt_index,
            error.retry_after(),
            tool_name.or(Some(error.tool_name.as_str())),
        );
        error.with_retry_decision(decision)
    }
}

fn decision_for_category_with_tool(
    policy: &RetryPolicy,
    category: ErrorCategory,
    attempt_index: u32,
    retry_after: Option<Duration>,
    tool_name: Option<&str>,
) -> RetryDecision {
    if is_non_retryable_command_timeout(category, tool_name) {
        return RetryDecision {
            category,
            retryable: false,
            delay: None,
            retry_after,
        };
    }

    policy.decision_for_category(category, attempt_index, retry_after)
}

pub(crate) fn category_was_retryable(error: &VtCodeError) -> bool {
    error.category.is_retryable() && !error.is_misconfiguration()
}

/// Fail-closed decision for user misconfiguration: never retry, preserve
/// any `Retry-After` hint for diagnostics but force immediate `GiveUp`.
fn misconfiguration_decision(category: ErrorCategory, retry_after: Option<Duration>) -> RetryDecision {
    RetryDecision {
        category,
        retryable: false,
        delay: None,
        retry_after,
    }
}

/// The single tool-aware retry rule: command tool timeouts are never
/// retryable because the underlying process may still be running, so a
/// retry can contend for locks or duplicate side effects.
pub(crate) fn is_non_retryable_command_timeout(category: ErrorCategory, tool_name: Option<&str>) -> bool {
    matches!(category, ErrorCategory::Timeout) && tool_name.is_some_and(is_command_tool)
}

fn unified_tool_text(error: &UnifiedToolError) -> String {
    let mut text = error.user_message.clone();
    if let Some(source) = error.source.as_ref() {
        text.push('\n');
        text.push_str(&format!("{source:#}"));
    }
    text
}

fn tool_execution_text(error: &ToolExecutionError) -> String {
    let mut text = error.message.clone();
    if let Some(original) = error.original_error.as_deref() {
        text.push('\n');
        text.push_str(original);
    }
    text
}

/// Typed step produced by [`RetryPolicyCoreExt::step_for_vtcode_error`].
///
/// Callers match on this instead of re-deriving the
/// `if decision.retryable { sleep(decision.delay.expect(...)) }` pattern.
#[derive(Debug)]
pub enum RetryStep {
    /// Wait `delay` then retry; `error` is the failure being retried.
    Backoff {
        delay: Duration,
        decision: RetryDecision,
        error: VtCodeError,
    },
    /// Give up immediately and surface `error`.
    GiveUp {
        decision: RetryDecision,
        error: VtCodeError,
    },
}

/// Lifecycle event emitted by [`run_with_retry`] so callers can attach
/// logging, metrics, or stats updates without re-implementing the loop.
///
/// `category_was_retryable` is provided alongside `GiveUp` and `Backoff`
/// because call sites frequently need to distinguish "non-retryable
/// category" from "retryable category, budget exhausted" — the distinction
/// is otherwise invisible once `step_for_vtcode_error` collapses both
/// into `GiveUp`.
#[derive(Debug)]
pub enum RetryEvent<'a> {
    /// An attempt is about to start. `attempt` is 0-indexed.
    AttemptStart { attempt: u32, max_attempts: u32 },
    /// The operation succeeded on the given attempt.
    Success { attempt: u32 },
    /// The policy decided to give up and surface `error` immediately.
    GiveUp {
        attempt: u32,
        error: &'a VtCodeError,
        decision: &'a RetryDecision,
        category_was_retryable: bool,
    },
    /// The policy decided to back off and retry. The driver will sleep
    /// for `delay` before invoking the operation again.
    Backoff {
        attempt: u32,
        error: &'a VtCodeError,
        decision: &'a RetryDecision,
        delay: Duration,
        category_was_retryable: bool,
    },
    /// All attempts were exhausted. `last_error` is the most recent
    /// error captured from a `Backoff` step.
    Exhausted { last_error: Option<&'a VtCodeError> },
}

/// Drive a retry loop per `policy`, invoking `on_event` for each
/// lifecycle event. Returns the first successful result, or the final
/// error if the policy gives up or all attempts are exhausted.
///
/// `state` is reborrowed mutably and passed to each callback on every
/// invocation, so callers can use it to thread a `&mut self` (or any
/// other mutable context) through the loop without resorting to
/// `RefCell` or split borrows. The `operation` callback must return a
/// boxed future so the helper can `await` it without tying its lifetime
/// to the closure's own borrow of `state`.
///
/// The returned future is `Send` so it can be passed to `tokio::spawn`.
///
/// `synthesize_exhausted_error` is invoked only in the (degenerate)
/// case where the loop completes without ever recording a `Backoff`
/// error. The closure receives the `&RetryPolicy` so it can attach
/// site-specific context (e.g. `policy.max_attempts`) without having
/// to capture the policy in its environment — which is exactly the
/// pattern that conflicts with `&mut state`.
#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub async fn run_with_retry<T, E, S, F, OnEvent, Synthesize>(
    policy: &RetryPolicy,
    state: &mut S,
    mut on_event: OnEvent,
    mut operation: F,
    synthesize_exhausted_error: Synthesize,
) -> crate::error::Result<T>
where
    F: for<'a> FnMut(&'a mut S) -> std::pin::Pin<Box<dyn Future<Output = StdResult<T, E>> + Send + 'a>>,
    E: Into<VtCodeError>,
    OnEvent: FnMut(&mut S, RetryEvent<'_>),
    Synthesize: FnOnce(&RetryPolicy) -> VtCodeError,
{
    use tokio::time::sleep;

    let mut last_error: Option<VtCodeError> = None;
    for attempt in 0..policy.max_attempts {
        on_event(state, RetryEvent::AttemptStart { attempt, max_attempts: policy.max_attempts });
        match operation(state).await {
            Ok(value) => {
                on_event(state, RetryEvent::Success { attempt });
                return Ok(value);
            }
            Err(err) => {
                let err: VtCodeError = err.into();
                let category_was_retryable = category_was_retryable(&err);
                let step = policy.step_for_vtcode_error(err, attempt, None);
                match step {
                    RetryStep::GiveUp { decision, error } => {
                        on_event(
                            state,
                            RetryEvent::GiveUp {
                                attempt,
                                error: &error,
                                decision: &decision,
                                category_was_retryable,
                            },
                        );
                        return Err(error);
                    }
                    RetryStep::Backoff { delay, decision, error } => {
                        on_event(
                            state,
                            RetryEvent::Backoff {
                                attempt,
                                error: &error,
                                decision: &decision,
                                delay,
                                category_was_retryable,
                            },
                        );
                        last_error = Some(error);
                        sleep(delay).await;
                    }
                }
            }
        }
    }
    let final_error = last_error.unwrap_or_else(|| synthesize_exhausted_error(policy));
    on_event(state, RetryEvent::Exhausted { last_error: Some(&final_error) });
    Err(final_error)
}

pub fn decision_for_vtcode_error(
    error: &VtCodeError,
    attempt_index: u32,
    tool_name: Option<&str>,
    policy_override: Option<&RetryPolicy>,
) -> RetryDecision {
    let policy = policy_override.unwrap_or(&RetryPolicy::DEFAULT);
    policy.decision_for_vtcode_error(error, attempt_index, tool_name)
}

pub fn decision_for_anyhow_error(
    error: &anyhow::Error,
    attempt_index: u32,
    tool_name: Option<&str>,
    policy_override: Option<&RetryPolicy>,
) -> RetryDecision {
    let policy = policy_override.unwrap_or(&RetryPolicy::DEFAULT);
    policy.decision_for_anyhow(error, attempt_index, tool_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::tools;
    use crate::error::{ErrorCode, VtCodeError};
    use vtcode_commons::llm::LLMErrorMetadata;

    #[test]
    fn non_retryable_categories_stop_immediately() {
        let policy = RetryPolicy::from_retries(2, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        let err = VtCodeError::security(ErrorCode::PermissionDenied, "blocked by policy");

        let decision = policy.decision_for_vtcode_error(&err, 0, None);
        assert_eq!(decision.category, ErrorCategory::PolicyViolation);
        assert!(!decision.retryable);
        assert!(decision.delay.is_none());
    }

    #[test]
    fn retry_after_header_overrides_backoff_delay() {
        let policy = RetryPolicy::from_retries(3, Duration::from_secs(1), Duration::from_secs(8), 2.0);
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
        let policy = RetryPolicy::from_retries(3, Duration::from_secs(1), Duration::from_secs(8), 2.0);
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
        let policy = RetryPolicy::from_retries(1, Duration::from_secs(1), Duration::from_secs(8), 2.0);

        let decision = policy.decision_for_anyhow(&anyhow::anyhow!("HTTP 503 Service Unavailable"), 0, None);
        assert_eq!(decision.category, ErrorCategory::ServiceUnavailable);
        assert!(decision.retryable);
        assert_eq!(decision.delay, Some(Duration::from_secs(1)));
    }

    #[test]
    fn anyhow_prefers_typed_llm_errors() {
        let policy = RetryPolicy::from_retries(3, Duration::from_secs(1), Duration::from_secs(8), 2.0);
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
            assert!(is_command_tool(alias), "expected {alias} to be a command tool");
        }
    }

    #[test]
    fn typed_tool_timeout_for_command_tools_is_not_retryable() {
        let policy = RetryPolicy::from_retries(2, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        let err = UnifiedToolError::new(crate::tools::unified_error::UnifiedErrorKind::Timeout, "timed out")
            .with_tool_name(tools::RUN_PTY_CMD);

        let decision = policy.decision_for_tool_error(&err, 0);
        assert_eq!(decision.category, ErrorCategory::Timeout);
        assert!(!decision.retryable);
    }

    #[test]
    fn anyhow_typed_tool_timeout_uses_fallback_tool_name() {
        let policy = RetryPolicy::from_retries(2, Duration::from_secs(1), Duration::from_secs(8), 2.0);
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
        let policy = RetryPolicy::from_retries(2, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        let err = VtCodeError::new(ErrorCategory::Timeout, ErrorCode::Timeout, "timed out");

        let decision = policy.decision_for_vtcode_error(&err, 0, Some(tools::RUN_PTY_CMD));
        assert_eq!(decision.category, ErrorCategory::Timeout);
        assert!(!decision.retryable);
    }

    #[tokio::test]
    async fn run_with_retry_returns_first_success() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};
        let policy = RetryPolicy::from_retries(3, Duration::from_millis(0), Duration::from_millis(1), 2.0);
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_for_op = attempts.clone();
        let result: crate::error::Result<String> = run_with_retry(
            &policy,
            &mut (),
            |_: &mut (), _| {},
            |_| {
                let attempts = attempts_for_op.clone();
                Box::pin(async move {
                    let n = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                    if n < 2 {
                        Err(VtCodeError::network(ErrorCode::ConnectionFailed, "transient"))
                    } else {
                        Ok("ok".to_string())
                    }
                })
            },
            |_: &RetryPolicy| VtCodeError::execution(ErrorCode::ToolExecutionFailed, "exhausted"),
        )
        .await;
        assert_eq!(result.unwrap(), "ok");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn run_with_retry_surfaces_give_up_immediately() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};
        let policy = RetryPolicy::from_retries(5, Duration::from_millis(0), Duration::from_millis(1), 2.0);
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_for_op = attempts.clone();
        let result: crate::error::Result<String> = run_with_retry(
            &policy,
            &mut (),
            |_: &mut (), _| {},
            |_| {
                let attempts = attempts_for_op.clone();
                Box::pin(async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    Err::<String, _>(VtCodeError::input(ErrorCode::InvalidArgument, "bad input"))
                })
            },
            |_: &RetryPolicy| VtCodeError::execution(ErrorCode::ToolExecutionFailed, "exhausted"),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1, "GiveUp should short-circuit retries");
    }

    #[test]
    fn misconfiguration_fails_fast_with_guidance() {
        let policy = RetryPolicy::from_retries(5, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        let err = VtCodeError::new(
            ErrorCategory::Authentication,
            ErrorCode::AuthenticationFailed,
            "Authentication failed: invalid api key",
        );

        let decision = policy.decision_for_vtcode_error(&err, 0, None);
        assert!(!decision.retryable);
        assert!(decision.delay.is_none());

        let step = policy.step_for_vtcode_error(err, 0, None);
        match step {
            RetryStep::GiveUp { error, .. } => {
                assert!(error.message.contains("Check settings/config first"));
                assert!(error.message.contains("before retrying"));
            }
            RetryStep::Backoff { .. } => panic!("misconfiguration must not back off"),
        }
    }

    #[test]
    fn misconfiguration_model_error_does_not_retry() {
        let policy = RetryPolicy::from_retries(5, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        let err = VtCodeError::input(ErrorCode::InvalidArgument, "unknown model 'gpt-99' in agent.model");

        assert!(err.is_misconfiguration());
        let decision = policy.decision_for_vtcode_error(&err, 0, None);
        assert!(!decision.retryable);
    }

    #[test]
    fn transient_errors_still_retry_without_guidance() {
        let policy = RetryPolicy::from_retries(3, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        let err = VtCodeError::network(ErrorCode::ConnectionFailed, "connection reset by peer");

        assert!(!err.is_misconfiguration());
        let decision = policy.decision_for_vtcode_error(&err, 0, None);
        assert!(decision.retryable);
    }

    #[test]
    fn misconfiguration_does_not_count_as_retryable_category() {
        let err = VtCodeError::network(ErrorCode::ConnectionFailed, "invalid provider_overrides configuration");

        assert!(err.is_misconfiguration());
        assert!(!category_was_retryable(&err));
    }

    #[test]
    fn llm_auth_error_is_misconfiguration() {
        let policy = RetryPolicy::from_retries(3, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        let err = LLMError::Authentication {
            message: "invalid api key".to_string(),
            metadata: None,
        };

        let decision = policy.decision_for_llm_error(&err, 0);
        assert!(!decision.retryable);
        assert_eq!(decision.category, ErrorCategory::Authentication);
    }

    #[test]
    fn tool_execution_misconfiguration_prepends_guidance() {
        use crate::tools::registry::ToolErrorType;

        let policy = RetryPolicy::from_retries(3, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        let err = ToolExecutionError::new(
            "test_tool".to_string(),
            ToolErrorType::ExecutionError,
            "unknown model 'foo' in agent.model".to_string(),
        );

        let guided = policy.apply_to_tool_execution_error(err, 0, None);
        assert!(!guided.retryable);
        assert!(!guided.is_recoverable);
        assert!(!guided.circuit_breaker_impact);
        assert!(
            guided
                .recovery_suggestions
                .first()
                .is_some_and(|s| s.contains("Check settings/config first"))
        );
    }
}
