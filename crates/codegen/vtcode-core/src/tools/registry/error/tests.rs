use super::*;
use anyhow::anyhow;

fn classify(err: &Error) -> ToolErrorType {
    ToolErrorType::from(vtcode_commons::classify_anyhow_error(err))
}

#[test]
fn classify_error_marks_rate_limit_as_network_error() {
    let err = anyhow!("provider returned 429 Too Many Requests");
    assert!(matches!(classify(&err), ToolErrorType::NetworkError));
}

#[test]
fn classify_error_marks_service_unavailable_as_network_error() {
    let err = anyhow!("503 Service Unavailable");
    assert!(matches!(classify(&err), ToolErrorType::NetworkError));
}

#[test]
fn classify_error_marks_weekly_usage_limit_as_execution_error() {
    let err = anyhow!("you have reached your weekly usage limit");
    assert!(matches!(classify(&err), ToolErrorType::ExecutionError));
}

#[test]
fn classify_error_marks_tool_not_found() {
    let err = anyhow!("unknown tool: ask_questions");
    assert!(matches!(classify(&err), ToolErrorType::ToolNotFound));
}

#[test]
fn classify_error_marks_policy_violation_before_permission() {
    let err = anyhow!("tool permission denied by policy");
    assert!(matches!(classify(&err), ToolErrorType::PolicyViolation));
}

const ALL_TOOL_ERROR_TYPES: [ToolErrorType; 8] = [
    ToolErrorType::InvalidParameters,
    ToolErrorType::ToolNotFound,
    ToolErrorType::PermissionDenied,
    ToolErrorType::ResourceNotFound,
    ToolErrorType::NetworkError,
    ToolErrorType::Timeout,
    ToolErrorType::ExecutionError,
    ToolErrorType::PolicyViolation,
];

#[test]
fn error_type_wire_string_round_trips() {
    for error_type in ALL_TOOL_ERROR_TYPES {
        assert_eq!(parse_error_type(error_type.as_str()), error_type);
    }
}

#[test]
fn error_type_category_round_trip_is_identity() {
    // Guarantees `with_retry_decision` never changes error_type unless the
    // decision actually recategorized the error.
    for error_type in ALL_TOOL_ERROR_TYPES {
        assert_eq!(ToolErrorType::from(ErrorCategory::from(error_type)), error_type);
    }
}

#[test]
fn from_anyhow_derives_error_type_from_category() {
    let err = anyhow!("provider returned 429 Too Many Requests");
    let structured = ToolExecutionError::from_anyhow("grep_search", &err, 0, false, false, None);
    assert_eq!(structured.category, ErrorCategory::RateLimit);
    assert_eq!(structured.error_type, ToolErrorType::from(structured.category));
}

#[test]
fn missing_exec_session_classification_survives_context_and_round_trip() {
    let source = Error::new(crate::tools::exec_session::ExecSessionNotFound { session_id: "run-missing".into() })
        .context("quoted diagnostic: permission denied in vtcode.toml");
    let structured = ToolExecutionError::from_anyhow("write_stdin", &source, 0, false, false, Some("registry"));
    assert_eq!(structured.category, ErrorCategory::ResourceNotFound);
    assert!(structured.is_exec_session_not_found());
    assert!(!structured.retryable);
    assert!(!structured.circuit_breaker_impact);
    assert_eq!(structured.debug_context.as_ref().unwrap().surface.as_deref(), Some("registry"));
    assert!(structured.message.contains("reuse its output"));
    assert!(!structured.message.contains("re-run the command instead of waiting"));
    let decoded = ToolExecutionError::from_error_payload(&structured.to_json_value()).unwrap();
    assert!(decoded.is_exec_session_not_found());

    let lookalike = anyhow!("exec session 'run-missing' not found: permission denied");
    let untyped = ToolExecutionError::from_anyhow("write_stdin", &lookalike, 0, false, false, None);
    assert_eq!(untyped.category, ErrorCategory::PermissionDenied);
    assert!(!untyped.is_exec_session_not_found());
}

#[test]
fn retryable_matches_error_category_predicate() {
    for error_type in ALL_TOOL_ERROR_TYPES {
        let category = ErrorCategory::from(error_type);
        let structured = ToolExecutionError::new("grep_search".to_string(), error_type, "boom".to_string());
        assert_eq!(structured.retryable, category.is_retryable(), "retryable mismatch for {error_type:?}");
    }
}

#[test]
fn command_tool_timeouts_are_not_retryable() {
    let structured = ToolExecutionError::new(
        crate::config::constants::tools::CREATE_PTY_SESSION.to_string(),
        ToolErrorType::Timeout,
        "command timed out".to_string(),
    );
    assert!(!structured.retryable);
}

#[test]
fn tool_call_context_marks_mutating_tools_as_partial_state_possible() {
    let error =
        ToolExecutionError::new("write_file".to_string(), ToolErrorType::ExecutionError, "write failed".to_string())
            .with_tool_call_context(
                crate::config::constants::tools::WRITE_FILE,
                &serde_json::json!({"path": "note.txt", "content": "hello"}),
            );

    assert!(error.partial_state_possible);
    assert!(!error.rollback_performed);
}

#[test]
fn tool_call_context_marks_apply_patch_failures_as_rolled_back() {
    let source = Error::new(crate::tools::editing::PatchError::RolledBack {
        original: Box::new(crate::tools::editing::PatchError::SegmentNotFound {
            path: "src/lib.rs".to_string(),
            snippet: "fn main()".to_string(),
        }),
    });
    let error =
        ToolExecutionError::from_anyhow(crate::config::constants::tools::APPLY_PATCH, &source, 0, false, false, None);

    assert!(!error.partial_state_possible);
    assert!(error.rollback_performed);
}

#[test]
fn user_message_includes_retry_summary_and_wait() {
    let mut error = ToolExecutionError::new(
        crate::config::constants::tools::READ_FILE.to_string(),
        ToolErrorType::ExecutionError,
        "read failed".to_string(),
    )
    .with_attempt(2);
    error.retry_delay_ms = Some(1_500);

    let message = error.user_message();

    assert!(message.contains("Retried 1 time before failing."));
    assert!(message.contains("Recommended wait: 1.5s."));
}
