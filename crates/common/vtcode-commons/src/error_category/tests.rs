use super::*;

// --- classify_error_message tests ---

#[test]
fn policy_violation_takes_priority_over_permission() {
    assert_eq!(classify_error_message("tool permission denied by policy"), ErrorCategory::PolicyViolation);
}

#[test]
fn rate_limit_classified_correctly() {
    assert_eq!(classify_error_message("provider returned 429 Too Many Requests"), ErrorCategory::RateLimit);
    assert_eq!(classify_error_message("rate limit exceeded"), ErrorCategory::RateLimit);
}

#[test]
fn service_unavailable_is_classified() {
    assert_eq!(classify_error_message("503 service unavailable"), ErrorCategory::ServiceUnavailable);
}

#[test]
fn follow_up_stream_failures_are_retryable() {
    // Post-tool follow-up streaming drops must not fall through to
    // generic `ExecutionError`: both shapes are retryable.
    for msg in [
        "follow-up failed: stream disconnected mid-response",
        "sse stream terminated before completion",
        "incomplete stream while reading follow-up",
        "connection closed before response completed",
        "stream reset by peer during follow-up",
        "h2 stream error received: internal error",
    ] {
        let category = classify_error_message(msg);
        assert!(category.is_retryable(), "{msg} -> {category:?} should be retryable");
    }
    // Asymmetric boundary: unrelated `stream` prose without a failure
    // marker stays non-retryable, and `downstream error` must not match
    // the h2 `stream error received` marker.
    assert_eq!(classify_error_message("upstream stream file ready"), ErrorCategory::ExecutionError);
    assert_eq!(classify_error_message("downstream error: 400 bad request"), ErrorCategory::ExecutionError);
    assert_eq!(classify_error_message("something went wrong"), ErrorCategory::ExecutionError);
}

#[test]
fn authentication_errors() {
    assert_eq!(classify_error_message("invalid api key provided"), ErrorCategory::Authentication);
    assert_eq!(classify_error_message("401 unauthorized"), ErrorCategory::Authentication);
}

#[test]
fn billing_errors_are_resource_exhausted() {
    assert_eq!(classify_error_message("you have reached your weekly usage limit"), ErrorCategory::ResourceExhausted);
    assert_eq!(classify_error_message("quota exceeded for this model"), ErrorCategory::ResourceExhausted);
}

#[test]
fn timeout_errors() {
    assert_eq!(classify_error_message("connection timeout"), ErrorCategory::Timeout);
    assert_eq!(classify_error_message("request timed out after 30s"), ErrorCategory::Timeout);
}

#[test]
fn network_errors() {
    assert_eq!(classify_error_message("connection reset by peer"), ErrorCategory::Network);
    assert_eq!(classify_error_message("dns name resolution failed"), ErrorCategory::Network);
}

#[test]
fn tool_not_found() {
    assert_eq!(classify_error_message("unknown tool: ask_questions"), ErrorCategory::ToolNotFound);
}

#[test]
fn resource_not_found() {
    assert_eq!(classify_error_message("no such file or directory: /tmp/missing"), ErrorCategory::ResourceNotFound);
    assert_eq!(
        classify_error_message("Path 'crates/codegen/vtcode-core/src/agent' does not exist"),
        ErrorCategory::ResourceNotFound
    );
}

#[test]
fn patch_format_errors_are_invalid_parameters() {
    // Patch format errors are LLM argument mistakes, not execution
    // failures. They must classify as InvalidParameters (is_llm_mistake
    // == true, no circuit breaker trip) so the model gets parameter-
    // focused recovery suggestions instead of generic "check tool docs".
    assert_eq!(
        classify_error_message("invalid patch format: missing '*** Begin Patch' marker"),
        ErrorCategory::InvalidParameters
    );
    assert_eq!(
        classify_error_message("invalid patch format: input looks like a standard unified diff (---/+++ format)"),
        ErrorCategory::InvalidParameters
    );
    assert_eq!(
        classify_error_message("invalid patch hunk on line 5: unexpected end of input"),
        ErrorCategory::InvalidParameters
    );
    assert_eq!(classify_error_message("cannot parse empty patch input"), ErrorCategory::InvalidParameters);
    assert_eq!(classify_error_message("patch does not contain any operations"), ErrorCategory::InvalidParameters);
    assert_eq!(
        classify_error_message("semantic patch anchor 'fn main' for 'src/main.rs' could not be resolved"),
        ErrorCategory::InvalidParameters
    );
}

#[test]
fn permission_denied() {
    assert_eq!(classify_error_message("permission denied: /etc/shadow"), ErrorCategory::PermissionDenied);
}

#[test]
fn cancelled_operations() {
    assert_eq!(classify_error_message("operation cancelled by user"), ErrorCategory::Cancelled);
}

#[test]
fn planning_policy_violation() {
    assert_eq!(classify_error_message("not allowed in planning workflow"), ErrorCategory::PolicyViolation);
}

#[test]
fn sandbox_failure() {
    assert_eq!(classify_error_message("sandbox denied this operation"), ErrorCategory::SandboxFailure);
}

#[test]
fn unknown_error_is_execution_error() {
    assert_eq!(classify_error_message("something went wrong"), ErrorCategory::ExecutionError);
}

#[test]
fn invalid_parameters() {
    assert_eq!(classify_error_message("invalid argument: missing path field"), ErrorCategory::InvalidParameters);
    assert_eq!(
        classify_error_message("Failed to parse arguments for read_file handler: invalid type: boolean `false`"),
        ErrorCategory::InvalidParameters
    );
    assert_eq!(
        classify_error_message("at least one item is required for 'create'"),
        ErrorCategory::InvalidParameters
    );
    assert_eq!(
        classify_error_message("structural pattern preflight failed: pattern is not parseable as Rust syntax"),
        ErrorCategory::InvalidParameters
    );
}

// --- classify_anyhow_error: type-first recovery, context-aware fallback ---

#[test]
fn classify_anyhow_prefers_typed_llm_error_under_context() {
    // A typed `LLMError` must win over an uninformative outer context:
    // the outer message alone would classify as `ExecutionError`, but the
    // error's type is the source of truth for its recovery policy.
    let rate = anyhow::Error::new(crate::llm::LLMError::RateLimit { metadata: None }).context("compaction failed");
    assert_eq!(classify_anyhow_error(&rate), ErrorCategory::RateLimit);

    let auth =
        anyhow::Error::new(crate::llm::LLMError::Authentication { message: "bad token".to_string(), metadata: None })
            .context("request failed");
    assert_eq!(classify_anyhow_error(&auth), ErrorCategory::Authentication);
}

#[test]
fn classify_anyhow_reads_deeper_cause_chain() {
    // When the outer context is uninformative and no typed error survives,
    // the underlying cause must still be classified (`{err:#}`-style).
    let inner = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "connection reset by peer");
    let err = anyhow::Error::new(inner).context("failed to fetch resource");
    assert_eq!(classify_anyhow_error(&err), ErrorCategory::Network);
}

#[test]
fn classify_anyhow_keeps_outermost_context_priority() {
    // A deliberate, specific `.context(...)` still wins over a generic root.
    let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
    let err = anyhow::Error::new(inner).context("tool permission denied by policy");
    assert_eq!(classify_anyhow_error(&err), ErrorCategory::PolicyViolation);
}

#[test]
fn classify_anyhow_unknown_stays_execution_error() {
    let err = anyhow::anyhow!("something went wrong");
    assert_eq!(classify_anyhow_error(&err), ErrorCategory::ExecutionError);
}

// --- Retryability tests ---

#[test]
fn retryable_categories() {
    assert!(ErrorCategory::Network.is_retryable());
    assert!(ErrorCategory::Timeout.is_retryable());
    assert!(ErrorCategory::RateLimit.is_retryable());
    assert!(ErrorCategory::ServiceUnavailable.is_retryable());
    assert!(ErrorCategory::CircuitOpen.is_retryable());
}

#[test]
fn non_retryable_categories() {
    assert!(!ErrorCategory::Authentication.is_retryable());
    assert!(!ErrorCategory::InvalidParameters.is_retryable());
    assert!(!ErrorCategory::PolicyViolation.is_retryable());
    assert!(!ErrorCategory::ResourceExhausted.is_retryable());
    assert!(!ErrorCategory::Cancelled.is_retryable());
}

#[test]
fn permanent_error_detection() {
    assert!(ErrorCategory::Authentication.is_permanent());
    assert!(ErrorCategory::PolicyViolation.is_permanent());
    assert!(!ErrorCategory::Network.is_permanent());
    assert!(!ErrorCategory::Timeout.is_permanent());
}

#[test]
fn llm_mistake_detection() {
    assert!(ErrorCategory::InvalidParameters.is_llm_mistake());
    assert!(!ErrorCategory::Network.is_llm_mistake());
    assert!(!ErrorCategory::Timeout.is_llm_mistake());
}

// --- LLM error conversion ---

#[test]
fn llm_error_authentication_converts() {
    let err = crate::llm::LLMError::Authentication { message: "bad key".to_string(), metadata: None };
    assert_eq!(ErrorCategory::from(&err), ErrorCategory::Authentication);
}

#[test]
fn llm_error_rate_limit_converts() {
    let err = crate::llm::LLMError::RateLimit { metadata: None };
    assert_eq!(ErrorCategory::from(&err), ErrorCategory::RateLimit);
}

#[test]
fn llm_error_quota_exhaustion_converts() {
    let err = crate::llm::LLMError::RateLimit {
        metadata: Some(crate::llm::LLMErrorMetadata::new(
            "openai",
            Some(429),
            Some("insufficient_quota".to_string()),
            None,
            None,
            None,
            Some("quota exceeded".to_string()),
        )),
    };

    assert_eq!(ErrorCategory::from(&err), ErrorCategory::ResourceExhausted);
}

#[test]
fn llm_error_network_converts() {
    let err = crate::llm::LLMError::Network {
        message: "connection refused".to_string(),
        metadata: None,
    };
    assert_eq!(ErrorCategory::from(&err), ErrorCategory::Network);
}

#[test]
fn llm_error_provider_with_status_code() {
    use crate::llm::LLMErrorMetadata;
    let err = crate::llm::LLMError::Provider {
        message: "error".to_string(),
        metadata: Some(LLMErrorMetadata::new("openai", Some(503), None, None, None, None, None)),
    };
    assert_eq!(ErrorCategory::from(&err), ErrorCategory::ServiceUnavailable);
}

#[test]
fn minimax_invalid_response_is_service_unavailable() {
    assert_eq!(
        classify_error_message("Invalid response from MiniMax: missing choices"),
        ErrorCategory::ServiceUnavailable
    );
    assert_eq!(
        classify_error_message("Invalid response format: missing message"),
        ErrorCategory::ServiceUnavailable
    );
}

// --- is_retryable_llm_error_message ---

#[test]
fn retryable_llm_messages() {
    assert!(is_retryable_llm_error_message("429 too many requests"));
    assert!(is_retryable_llm_error_message("500 internal server error"));
    assert!(is_retryable_llm_error_message("connection timeout"));
    assert!(is_retryable_llm_error_message("network error"));
}

#[test]
fn non_retryable_llm_messages() {
    assert!(!is_retryable_llm_error_message("invalid api key"));
    assert!(!is_retryable_llm_error_message("weekly usage limit reached"));
    assert!(!is_retryable_llm_error_message("permission denied"));
}

#[test]
fn context_capacity_markers_are_specific() {
    assert!(is_context_capacity_message("invalid request: maximum context length is 114688 tokens"));
    assert!(is_context_capacity_message("input token count exceeds the maximum number of tokens allowed"));
    assert!(!is_context_capacity_message("invalid request: context field is missing"));
}

#[test]
fn context_capacity_markers_cover_gateway_variants() {
    // Near-budget follow-up failures from merge-gateway / Anthropic /
    // OpenAI routes must enter the compacting recovery path instead of
    // falling through to generic `ExecutionError`.
    for msg in [
        "input tokens exceed context limit (956758 > 1000000)",
        "prompt tokens exceed the model context window",
        "context limit exceeded: too many input tokens",
        "request exceeds context size for anthropic/claude-sonnet-5",
        "token limit exceeded for prompt",
        "prompt too large for context window",
    ] {
        assert!(is_context_capacity_message(msg), "{msg} should be a capacity signal");
    }
    // Negative guard: ordinary prose must stay out of recovery (the
    // field-missing case is covered in `context_capacity_markers_are_specific`).
    assert!(!is_context_capacity_message("upstream stream file ready"));
    // "context limited" contains "context limit" as a substring; it must
    // not enter capacity recovery.
    assert!(!is_context_capacity_message("context limited to 5 tools"));
}

// --- Recovery suggestions ---

#[test]
fn recovery_suggestions_non_empty() {
    for cat in [
        ErrorCategory::Network,
        ErrorCategory::Timeout,
        ErrorCategory::RateLimit,
        ErrorCategory::Authentication,
        ErrorCategory::InvalidParameters,
        ErrorCategory::ToolNotFound,
        ErrorCategory::ResourceNotFound,
        ErrorCategory::PermissionDenied,
        ErrorCategory::PolicyViolation,
        ErrorCategory::ExecutionError,
    ] {
        assert!(!cat.recovery_suggestions().is_empty(), "Missing recovery suggestions for {cat:?}");
    }
}

// --- User label ---

#[test]
fn user_labels_are_non_empty() {
    assert!(!ErrorCategory::Network.user_label().is_empty());
    assert!(!ErrorCategory::ExecutionError.user_label().is_empty());
}

// --- auth_recovery_guidance ---

#[test]
fn auth_recovery_guidance_no_credential_mentions_secret_add() {
    let guidance = ErrorCategory::Authentication.auth_recovery_guidance("StepFun", "stepfun", false, false);
    assert_eq!(guidance.len(), 1);
    assert_eq!(
        guidance[0],
        "Authentication failed for StepFun. Run /secret add stepfun to store your API key in secure storage (OS keyring or encrypted file)."
    );
}

#[test]
fn auth_recovery_guidance_credential_stored_mentions_overwrite() {
    let guidance = ErrorCategory::Authentication.auth_recovery_guidance("StepFun", "stepfun", false, true);
    assert_eq!(guidance.len(), 1);
    assert_eq!(
        guidance[0],
        "Authentication failed for StepFun. The stored API key was rejected — run /secret add stepfun to replace it with a valid key."
    );
}

#[test]
fn auth_recovery_guidance_managed_auth_provider_mentions_login() {
    let guidance = ErrorCategory::Authentication.auth_recovery_guidance("GitHub Copilot", "copilot", true, false);
    assert_eq!(guidance.len(), 1);
    assert_eq!(guidance[0], "Authentication failed for GitHub Copilot. Run /login copilot to re-authenticate.");
}

#[test]
fn auth_recovery_guidance_non_auth_category_returns_empty() {
    assert!(
        ErrorCategory::Network
            .auth_recovery_guidance("OpenAI", "openai", false, false)
            .is_empty()
    );
    assert!(
        ErrorCategory::Timeout
            .auth_recovery_guidance("OpenAI", "openai", false, false)
            .is_empty()
    );
}

// --- Display ---

#[test]
fn display_matches_user_label() {
    assert_eq!(format!("{}", ErrorCategory::RateLimit), ErrorCategory::RateLimit.user_label());
}
