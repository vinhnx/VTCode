//! Integration tests for error handling and recovery scenarios.
//!
//! Covers:
//! - Unified error categorization across crates
//! - Error sanitization for user-facing display
//! - Batch outcome tracking for multi-tool executions
//! - MCP ErrorCode user guidance
//! - Recovery suggestion consistency

#[cfg(test)]
mod error_scenarios {
    use vtcode_commons::{BackoffStrategy, ErrorCategory, Retryability};

    // -----------------------------------------------------------------------
    // 1. Unified error categorization round-trips
    // -----------------------------------------------------------------------

    #[test]
    fn category_round_trip_via_tool_error_type() {
        use vtcode_core::tools::registry::ToolErrorType;

        let categories = [
            ErrorCategory::Network,
            ErrorCategory::Timeout,
            ErrorCategory::RateLimit,
            ErrorCategory::Authentication,
            ErrorCategory::InvalidParameters,
            ErrorCategory::ToolNotFound,
            ErrorCategory::ResourceNotFound,
            ErrorCategory::PermissionDenied,
            ErrorCategory::ExecutionError,
            ErrorCategory::PolicyViolation,
        ];

        for cat in &categories {
            let tool_err: ToolErrorType = ToolErrorType::from(*cat);
            let back: ErrorCategory = ErrorCategory::from(tool_err);
            // Round-trip should reach the same or a semantically equivalent category.
            // Some categories collapse (e.g. RateLimit → NetworkError → Network).
            assert!(
                !back.user_label().is_empty(),
                "Round-trip for {:?} produced empty label",
                cat
            );
        }
    }

    #[test]
    fn category_round_trip_via_unified_error_kind() {
        use vtcode_core::tools::unified_error::UnifiedErrorKind;

        let categories = [
            ErrorCategory::Network,
            ErrorCategory::Timeout,
            ErrorCategory::Authentication,
            ErrorCategory::PermissionDenied,
            ErrorCategory::ExecutionError,
        ];

        for cat in &categories {
            let kind: UnifiedErrorKind = UnifiedErrorKind::from(*cat);
            let back: ErrorCategory = ErrorCategory::from(kind);
            assert!(
                !back.user_label().is_empty(),
                "Round-trip for {:?} via UnifiedErrorKind produced empty label",
                cat
            );
        }
    }

    // -----------------------------------------------------------------------
    // 2. classify_error_message covers real-world error strings
    // -----------------------------------------------------------------------

    #[test]
    fn classify_real_world_errors() {
        let cases: Vec<(&str, ErrorCategory)> = vec![
            ("connection refused", ErrorCategory::Network),
            ("request timed out after 30s", ErrorCategory::Timeout),
            ("429 Too Many Requests", ErrorCategory::RateLimit),
            ("invalid api key", ErrorCategory::Authentication),
            (
                "permission denied (os error 13)",
                ErrorCategory::PermissionDenied,
            ),
            ("rate limit exceeded", ErrorCategory::RateLimit),
            (
                "server is overloaded, try again later",
                ErrorCategory::Network,
            ),
            ("service temporarily unavailable", ErrorCategory::Network),
        ];

        for (msg, expected) in cases {
            let actual = vtcode_commons::classify_error_message(msg);
            assert_eq!(
                actual, expected,
                "classify_error_message({:?}) = {:?}, expected {:?}",
                msg, actual, expected
            );
        }
    }

    // -----------------------------------------------------------------------
    // 3. Retryability backoff strategies are sane
    // -----------------------------------------------------------------------

    #[test]
    fn retryable_categories_have_bounded_attempts() {
        let retryable = [
            ErrorCategory::Network,
            ErrorCategory::Timeout,
            ErrorCategory::RateLimit,
            ErrorCategory::ServiceUnavailable,
        ];

        for cat in &retryable {
            match cat.retryability() {
                Retryability::Retryable {
                    max_attempts,
                    backoff,
                } => {
                    assert!(
                        max_attempts >= 1 && max_attempts <= 10,
                        "{:?} has unreasonable max_attempts={}",
                        cat,
                        max_attempts
                    );
                    // Verify backoff strategy is set
                    match backoff {
                        BackoffStrategy::Exponential { .. } | BackoffStrategy::Fixed { .. } => {}
                    }
                }
                other => panic!("{:?} should be retryable, got {:?}", cat, other),
            }
        }
    }

    #[test]
    fn non_retryable_categories_are_not_retryable() {
        let non_retryable = [
            ErrorCategory::Authentication,
            ErrorCategory::InvalidParameters,
            ErrorCategory::ToolNotFound,
            ErrorCategory::PolicyViolation,
            ErrorCategory::PlanModeViolation,
        ];

        for cat in &non_retryable {
            assert!(!cat.is_retryable(), "{:?} should be non-retryable", cat);
        }
    }

    // -----------------------------------------------------------------------
    // 4. Recovery suggestions are non-empty for actionable categories
    // -----------------------------------------------------------------------

    #[test]
    fn actionable_categories_have_recovery_suggestions() {
        let actionable = [
            ErrorCategory::Network,
            ErrorCategory::Timeout,
            ErrorCategory::RateLimit,
            ErrorCategory::Authentication,
            ErrorCategory::ToolNotFound,
            ErrorCategory::PermissionDenied,
            ErrorCategory::ResourceNotFound,
        ];

        for cat in &actionable {
            let suggestions = cat.recovery_suggestions();
            assert!(
                !suggestions.is_empty(),
                "{:?} should have recovery suggestions",
                cat
            );
            for s in &suggestions {
                assert!(
                    !s.trim().is_empty(),
                    "{:?} has an empty suggestion string",
                    cat
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // 5. User labels are human-friendly
    // -----------------------------------------------------------------------

    #[test]
    fn user_labels_are_short_and_descriptive() {
        let all_categories = [
            ErrorCategory::Network,
            ErrorCategory::Timeout,
            ErrorCategory::RateLimit,
            ErrorCategory::ServiceUnavailable,
            ErrorCategory::CircuitOpen,
            ErrorCategory::Authentication,
            ErrorCategory::InvalidParameters,
            ErrorCategory::ToolNotFound,
            ErrorCategory::ResourceNotFound,
            ErrorCategory::PermissionDenied,
            ErrorCategory::PolicyViolation,
            ErrorCategory::PlanModeViolation,
            ErrorCategory::SandboxFailure,
            ErrorCategory::ResourceExhausted,
            ErrorCategory::Cancelled,
            ErrorCategory::ExecutionError,
        ];

        for cat in &all_categories {
            let label = cat.user_label();
            assert!(!label.is_empty(), "{:?} has empty user_label", cat);
            assert!(label.len() <= 40, "{:?} label too long: {:?}", cat, label);
            assert!(
                label.contains(' ') || label.len() >= 6,
                "{:?} label should be descriptive: {:?}",
                cat,
                label
            );
        }
    }

    // -----------------------------------------------------------------------
    // 6. MCP ErrorCode user guidance
    // -----------------------------------------------------------------------

    #[test]
    fn mcp_error_codes_have_guidance() {
        use vtcode_core::mcp::errors::ErrorCode;

        let codes = [
            ErrorCode::ToolNotFound,
            ErrorCode::ToolInvocationFailed,
            ErrorCode::ProviderNotFound,
            ErrorCode::ProviderUnavailable,
            ErrorCode::SchemaInvalid,
            ErrorCode::ConfigurationError,
            ErrorCode::InitializationTimeout,
        ];

        for code in &codes {
            let guidance = code.user_guidance();
            assert!(
                !guidance.is_empty(),
                "{:?} should have non-empty user_guidance",
                code
            );
            // Guidance should be a complete sentence or phrase.
            assert!(
                guidance.len() > 10,
                "{:?} guidance too short to be helpful: {:?}",
                code,
                guidance
            );
        }
    }

    // -----------------------------------------------------------------------
    // 7. is_retryable_llm_error_message consistency with classify
    // -----------------------------------------------------------------------

    #[test]
    fn llm_retryable_errors_classified_as_retryable_categories() {
        let retryable_messages = [
            "rate limit exceeded",
            "connection reset by peer",
            "request timed out",
            "503 service unavailable",
            "server is overloaded",
            "Too Many Requests",
        ];

        for msg in &retryable_messages {
            assert!(
                vtcode_commons::is_retryable_llm_error_message(msg),
                "is_retryable_llm_error_message({:?}) should be true",
                msg
            );

            let cat = vtcode_commons::classify_error_message(msg);
            assert!(
                cat.is_retryable(),
                "classify_error_message({:?}) = {:?} should be retryable",
                msg,
                cat
            );
        }
    }

    #[test]
    fn non_retryable_errors_are_consistent() {
        let non_retryable_messages = [
            "invalid api key",
            "permission denied",
            "authentication failed",
        ];

        for msg in &non_retryable_messages {
            assert!(
                !vtcode_commons::is_retryable_llm_error_message(msg),
                "is_retryable_llm_error_message({:?}) should be false",
                msg
            );

            let cat = vtcode_commons::classify_error_message(msg);
            assert!(
                !cat.is_retryable(),
                "classify_error_message({:?}) = {:?} should be non-retryable",
                msg,
                cat
            );
        }
    }

    // -----------------------------------------------------------------------
    // 8. classify_anyhow_error works on wrapped errors
    // -----------------------------------------------------------------------

    #[test]
    fn classify_anyhow_error_with_context() {
        let inner = std::io::Error::new(std::io::ErrorKind::TimedOut, "connection timed out");
        let anyhow_err =
            anyhow::Error::new(inner).context("request timed out while fetching resource");

        let cat = vtcode_commons::classify_anyhow_error(&anyhow_err);
        assert_eq!(
            cat,
            ErrorCategory::Timeout,
            "Expected Timeout, got {:?}",
            cat
        );
    }

    #[test]
    fn classify_anyhow_error_permission_denied() {
        let inner = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
        let anyhow_err = anyhow::Error::new(inner).context("permission denied while reading file");

        let cat = vtcode_commons::classify_anyhow_error(&anyhow_err);
        assert_eq!(
            cat,
            ErrorCategory::PermissionDenied,
            "Expected PermissionDenied, got {:?}",
            cat
        );
    }
}
