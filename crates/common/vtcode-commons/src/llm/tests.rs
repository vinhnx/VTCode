use super::{LLMError, LLMErrorMetadata, ToolCall};
use serde_json::json;

#[test]
fn parsed_arguments_accepts_trailing_characters() {
    let call = ToolCall::function(
        "call_read".to_string(),
        "exec_command".to_string(),
        r#"{"path":"src/main.rs"} trailing text"#.to_string(),
    );

    let parsed = call.parsed_arguments().expect("arguments with trailing text should recover");
    assert_eq!(parsed, json!({"path":"src/main.rs"}));
}

#[test]
fn parsed_arguments_accepts_code_fenced_json() {
    let call = ToolCall::function(
        "call_read".to_string(),
        "exec_command".to_string(),
        "```json\n{\"path\":\"src/lib.rs\",\"limit\":25}\n```".to_string(),
    );

    let parsed = call.parsed_arguments().expect("code-fenced arguments should recover");
    assert_eq!(parsed, json!({"path":"src/lib.rs","limit":25}));
}

#[test]
fn parsed_arguments_recovers_truncated_json_missing_closing_brace() {
    let call = ToolCall::function(
        "call_search".to_string(),
        "code_search".to_string(),
        r#"{"query":"context","path":".","file_types":["rust"],"result_types":["definition"],"max_results":20"#
            .to_string(),
    );

    let parsed = call
        .parsed_arguments()
        .expect("truncated JSON missing closing brace should recover");
    assert_eq!(
        parsed,
        json!({
            "query": "context",
            "path": ".",
            "file_types": ["rust"],
            "result_types": ["definition"],
            "max_results": 20
        })
    );
}

#[test]
fn parsed_arguments_rejects_incomplete_json() {
    let call = ToolCall::function(
        "call_read".to_string(),
        "exec_command".to_string(),
        r#"{"path":"src/main.rs","limit""#.to_string(),
    );

    assert!(call.parsed_arguments().is_err());
}

#[test]
fn llm_error_debug_and_json_redact_provider_secrets() {
    let secret = concat!("sk-", "test1234567890abcdefghij");
    let error = LLMError::Provider {
        message: format!("response body api_key={secret} bearer Bearer abcdefghijklmnop"),
        metadata: Some(LLMErrorMetadata::new(
            "OpenAI",
            Some(401),
            Some("invalid_api_key".to_owned()),
            Some("req-123".to_owned()),
            None,
            None,
            Some(format!("{}={}", "AWS_SECRET_ACCESS_KEY", "cloud-secret-value")),
        )),
    };

    let debug = format!("{error:?}");
    let json = serde_json::to_string(&error).expect("LLM errors should serialize");

    assert!(!debug.contains(secret));
    assert!(!debug.contains("cloud-secret-value"));
    assert!(!json.contains(secret));
    assert!(!json.contains("cloud-secret-value"));
    assert!(json.contains("req-123"));
    assert!(json.contains("401"));
}

#[test]
fn parsed_arguments_recovers_truncated_minimax_markup() {
    let call = ToolCall::function(
        "call_search".to_string(),
        "code_search".to_string(),
        "{\"query\":\"persistent_memory\",\"file_types\":[\"rust\"],\"result_types\":[\"text\"],\"max_results\":20,\"path\":\"crates/codegen/vtcode-core/src</parameter>\n<</invoke>\n</minimax:tool_call>".to_string(),
    );

    let parsed = call.parsed_arguments().expect("minimax markup spillover should recover");
    assert_eq!(
        parsed,
        json!({
            "query": "persistent_memory",
            "path": "crates/codegen/vtcode-core/src",
            "file_types": ["rust"],
            "result_types": ["text"],
            "max_results": 20
        })
    );
}

#[test]
fn function_call_serializes_optional_namespace() {
    let call = ToolCall::function_with_namespace(
        "call_read".to_string(),
        Some("workspace".to_string()),
        "exec_command".to_string(),
        r#"{"path":"src/main.rs"}"#.to_string(),
    );

    let json = serde_json::to_value(&call).expect("tool call should serialize");
    assert_eq!(json["function"]["namespace"], "workspace");
    assert_eq!(json["function"]["name"], "exec_command");
}

#[test]
fn custom_tool_call_exposes_raw_execution_arguments() {
    let patch = "*** Begin Patch\n*** End Patch\n".to_string();
    let call = ToolCall::custom("call_patch".to_string(), "apply_patch".to_string(), patch.clone());

    assert!(call.is_custom());
    assert_eq!(call.tool_name(), Some("apply_patch"));
    assert_eq!(call.raw_input(), Some(patch.as_str()));
    assert_eq!(call.execution_arguments().expect("custom arguments"), json!(patch));
    assert!(call.parsed_arguments().is_err(), "custom tool payload should stay freeform rather than JSON");
}

#[test]
fn metadata_accessor_covers_every_variant() {
    // Every `LLMError` variant exposes its metadata through one accessor so
    // callers never re-spell the five-variant match.
    let meta = LLMErrorMetadata::new(
        "OpenAI",
        Some(429),
        Some("rate_limit_error".to_string()),
        None,
        None,
        Some("3".to_string()),
        Some("too many requests".to_string()),
    );

    assert!(
        LLMError::Authentication {
            message: "a".to_string(),
            metadata: Some(meta.clone())
        }
        .metadata()
        .is_some()
    );
    assert!(LLMError::RateLimit { metadata: Some(meta.clone()) }.metadata().is_some());
    assert!(
        LLMError::InvalidRequest {
            message: "b".to_string(),
            metadata: Some(meta.clone())
        }
        .metadata()
        .is_some()
    );
    assert!(
        LLMError::Network {
            message: "c".to_string(),
            metadata: Some(meta.clone())
        }
        .metadata()
        .is_some()
    );
    assert!(
        LLMError::Provider { message: "d".to_string(), metadata: Some(meta) }
            .metadata()
            .is_some()
    );

    // Absent metadata returns `None` rather than panicking.
    assert!(LLMError::RateLimit { metadata: None }.metadata().is_none());
    assert!(
        LLMError::Network { message: "no meta".to_string(), metadata: None }
            .metadata()
            .is_none()
    );
}
