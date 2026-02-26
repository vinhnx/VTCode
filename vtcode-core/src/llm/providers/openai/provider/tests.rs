use super::super::tool_serialization;
use super::*;
use crate::llm::provider::ParallelToolConfig;
use serde_json::{Value, json};

fn sample_tool() -> provider::ToolDefinition {
    provider::ToolDefinition::function(
        "search_workspace".to_owned(),
        "Search project files".to_owned(),
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            },
            "required": ["query"],
            "additionalProperties": false
        }),
    )
}

fn sample_request(model: &str) -> provider::LLMRequest {
    provider::LLMRequest {
        messages: vec![provider::Message::user("Hello".to_owned())],
        tools: Some(std::sync::Arc::new(vec![sample_tool()])),
        model: model.to_string(),
        ..Default::default()
    }
}

#[test]
fn serialize_tools_wraps_function_definition() {
    let tools = vec![sample_tool()];
    let serialized = tool_serialization::serialize_tools(&tools, models::openai::DEFAULT_MODEL)
        .expect("tools should serialize");
    let serialized_tools = serialized
        .as_array()
        .expect("serialized tools should be an array");
    assert_eq!(serialized_tools.len(), 1);

    let tool_value = serialized_tools[0]
        .as_object()
        .expect("tool should be serialized as object");
    assert_eq!(
        tool_value.get("type").and_then(Value::as_str),
        Some("function")
    );
    assert!(tool_value.contains_key("function"));
    assert_eq!(
        tool_value.get("name").and_then(Value::as_str),
        Some("search_workspace")
    );
    assert_eq!(
        tool_value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "Search project files"
    );

    let function_value = tool_value
        .get("function")
        .and_then(Value::as_object)
        .expect("function payload missing");
    assert_eq!(
        function_value.get("name").and_then(Value::as_str),
        Some("search_workspace")
    );
    assert!(function_value.contains_key("parameters"));
    assert_eq!(
        tool_value.get("parameters").and_then(Value::as_object),
        function_value.get("parameters").and_then(Value::as_object)
    );
}

#[test]
fn chat_completions_payload_uses_function_wrapper() {
    let provider =
        OpenAIProvider::with_model(String::new(), models::openai::DEFAULT_MODEL.to_string());
    let request = sample_request(models::openai::DEFAULT_MODEL);
    let payload = provider
        .convert_to_openai_format(&request)
        .expect("conversion should succeed");

    let tools = payload
        .get("tools")
        .and_then(Value::as_array)
        .expect("tools should exist on payload");
    let tool_object = tools[0].as_object().expect("tool entry should be object");
    assert!(tool_object.contains_key("function"));
    assert_eq!(
        tool_object.get("name").and_then(Value::as_str),
        Some("search_workspace")
    );
}

#[test]
fn responses_payload_uses_function_wrapper() {
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5.to_string());
    let request = sample_request(models::openai::GPT_5);
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    let _instructions = payload
        .get("instructions")
        .and_then(Value::as_str)
        .expect("instructions should be set");

    let tools = payload
        .get("tools")
        .and_then(Value::as_array)
        .expect("tools should exist on payload");
    let tool_object = tools[0].as_object().expect("tool entry should be object");
    assert!(tool_object.contains_key("function"));
    assert_eq!(
        tool_object.get("name").and_then(Value::as_str),
        Some("search_workspace")
    );
}

#[test]
fn serialize_tools_dedupes_duplicate_names() {
    let duplicate = provider::ToolDefinition::function(
        "search_workspace".to_owned(),
        "dup".to_owned(),
        json!({"type": "object"}),
    );
    let tools = vec![sample_tool(), duplicate];
    let serialized = tool_serialization::serialize_tools(&tools, models::openai::DEFAULT_MODEL)
        .expect("tools should serialize cleanly");
    let arr = serialized.as_array().expect("array");
    assert_eq!(arr.len(), 1, "duplicate names should be dropped");
}

#[test]
fn responses_tools_dedupes_apply_patch_and_function() {
    let apply_builtin = provider::ToolDefinition::apply_patch("Apply patches".to_owned());
    let apply_function = provider::ToolDefinition::function(
        "apply_patch".to_owned(),
        "alt apply".to_owned(),
        json!({"type": "object"}),
    );
    let tools = vec![apply_builtin, apply_function];
    let serialized = tool_serialization::serialize_tools_for_responses(&tools)
        .expect("responses tools should serialize");
    let arr = serialized.as_array().expect("array");
    assert_eq!(arr.len(), 1, "apply_patch should be deduped");
    let tool = arr[0].as_object().expect("object");
    assert_eq!(tool.get("type").and_then(Value::as_str), Some("function"));
    assert_eq!(
        tool.get("name").and_then(Value::as_str),
        Some("apply_patch")
    );
}

#[test]
fn responses_payload_sets_instructions_from_system_prompt() {
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5.to_string());
    let mut request = sample_request(models::openai::GPT_5);
    request.system_prompt = Some(std::sync::Arc::new(
        "You are a helpful assistant.".to_owned(),
    ));

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    let instructions = payload
        .get("instructions")
        .and_then(Value::as_str)
        .expect("instructions should be present");
    assert!(instructions.contains("You are a helpful assistant."));

    let input = payload
        .get("input")
        .and_then(Value::as_array)
        .expect("input should be serialized as array");
    assert_eq!(
        input
            .first()
            .and_then(|value| value.get("role"))
            .and_then(Value::as_str),
        Some("user")
    );
}

#[test]
fn harmony_detection_handles_common_variants() {
    assert!(OpenAIProvider::uses_harmony("gpt-oss-20b"));
    assert!(OpenAIProvider::uses_harmony("openai/gpt-oss-20b"));
    assert!(OpenAIProvider::uses_harmony("openai/gpt-oss-20b:free"));
    assert!(OpenAIProvider::uses_harmony("OPENAI/GPT-OSS-120B"));
    assert!(OpenAIProvider::uses_harmony("gpt-oss-120b@openrouter"));

    assert!(!OpenAIProvider::uses_harmony("gpt-5"));
    assert!(!OpenAIProvider::uses_harmony("gpt-oss:20b"));
}

#[test]
fn responses_payload_includes_prompt_cache_retention() {
    let mut pc = PromptCachingConfig::default();
    pc.providers.openai.prompt_cache_retention = Some("24h".to_owned());

    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        Some(models::openai::GPT_5_2.to_string()),
        None,
        Some(pc),
        None,
        None,
        None,
    );

    let request = sample_request(models::openai::GPT_5_2);
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert_eq!(
        payload
            .get("prompt_cache_retention")
            .and_then(Value::as_str),
        Some("24h")
    );
}

#[test]
fn responses_payload_includes_prompt_cache_key_for_native_openai() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        Some(models::openai::GPT_5_2.to_string()),
        None,
        None,
        None,
        None,
        None,
    );

    let mut request = sample_request(models::openai::GPT_5_2);
    request.prompt_cache_key = Some("vtcode:openai:session-123".to_string());
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert_eq!(
        payload.get("prompt_cache_key").and_then(Value::as_str),
        Some("vtcode:openai:session-123")
    );
}

#[test]
fn chat_payload_includes_prompt_cache_key_for_native_openai() {
    let provider =
        OpenAIProvider::with_model(String::new(), models::openai::DEFAULT_MODEL.to_string());

    let mut request = sample_request(models::openai::DEFAULT_MODEL);
    request.prompt_cache_key = Some("vtcode:openai:session-abc".to_string());
    let payload = provider
        .convert_to_openai_format(&request)
        .expect("conversion should succeed");

    assert_eq!(
        payload.get("prompt_cache_key").and_then(Value::as_str),
        Some("vtcode:openai:session-abc")
    );
}

#[test]
fn responses_payload_omits_prompt_cache_key_for_non_native_openai_base_url() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        Some(models::openai::GPT_5_2.to_string()),
        Some("https://example.local/v1".to_string()),
        None,
        None,
        None,
        None,
    );

    let mut request = sample_request(models::openai::GPT_5_2);
    request.prompt_cache_key = Some("vtcode:openai:session-xyz".to_string());
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert!(payload.get("prompt_cache_key").is_none());
}

#[test]
fn responses_payload_excludes_prompt_cache_retention_when_not_set() {
    let pc = PromptCachingConfig::default(); // default is Some("24h"); ram: to simulate none, set to None
    let mut pc = pc;
    pc.providers.openai.prompt_cache_retention = None;
    let provider = OpenAIProvider::from_config(
        Some("key".to_string()),
        Some(models::openai::GPT_5_2.to_string()),
        None,
        Some(pc),
        None,
        None,
        None,
    );

    let mut request = sample_request(models::openai::GPT_5_2);
    request.stream = true;
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert!(payload.get("prompt_cache_retention").is_none());
}

#[test]
fn responses_payload_includes_prompt_cache_retention_streaming() {
    let mut pc = PromptCachingConfig::default();
    pc.providers.openai.prompt_cache_retention = Some("12h".to_owned());

    let provider = OpenAIProvider::from_config(
        Some("key".to_string()),
        Some(models::openai::GPT_5_2.to_string()),
        None,
        Some(pc),
        None,
        None,
        None,
    );

    let mut request = sample_request(models::openai::GPT_5_2);
    request.stream = true;
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert_eq!(
        payload
            .get("prompt_cache_retention")
            .and_then(Value::as_str),
        Some("12h")
    );
}

#[test]
fn responses_payload_excludes_retention_for_non_responses_model() {
    let mut pc = PromptCachingConfig::default();
    pc.providers.openai.prompt_cache_retention = Some("9999s".to_string());

    let provider = OpenAIProvider::from_config(
        Some("key".to_string()),
        Some(models::openai::GPT_OSS_20B.to_string()),
        None,
        Some(pc),
        None,
        None,
        None,
    );

    let request = sample_request(models::openai::GPT_OSS_20B);
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert!(payload.get("prompt_cache_retention").is_none());
}

#[test]
fn provider_from_config_respects_prompt_cache_retention() {
    let mut pc = PromptCachingConfig::default();
    pc.providers.openai.prompt_cache_retention = Some("72h".to_owned());
    let provider = OpenAIProvider::from_config(
        Some("key".to_string()),
        Some(models::openai::GPT_5_2.to_string()),
        None,
        Some(pc.clone()),
        None,
        None,
        None,
    );

    assert_eq!(
        provider.prompt_cache_settings.prompt_cache_retention,
        Some("72h".to_owned())
    );
}

#[test]
fn test_parse_harmony_tool_name() {
    assert_eq!(
        OpenAIProvider::parse_harmony_tool_name("repo_browser.list_files"),
        "list_files"
    );
    assert_eq!(
        OpenAIProvider::parse_harmony_tool_name("container.exec"),
        "run_pty_cmd"
    );
    assert_eq!(
        OpenAIProvider::parse_harmony_tool_name("unknown.tool"),
        "tool"
    );
    // Direct tool names (not harmony namespaces) pass through
    // Alias resolution happens in canonical_tool_name()
    assert_eq!(OpenAIProvider::parse_harmony_tool_name("exec"), "exec");
    assert_eq!(
        OpenAIProvider::parse_harmony_tool_name("exec_pty_cmd"),
        "exec_pty_cmd"
    );
    assert_eq!(
        OpenAIProvider::parse_harmony_tool_name("exec_code"),
        "exec_code"
    );
    assert_eq!(OpenAIProvider::parse_harmony_tool_name("simple"), "simple");
}

#[test]
fn test_parse_harmony_tool_call_from_text() {
    let text = r#"to=repo_browser.list_files {"path":"", "recursive":"true"}"#;
    let result = OpenAIProvider::parse_harmony_tool_call_from_text(text);
    assert!(result.is_some());

    let (tool_name, args) = result.unwrap();
    assert_eq!(tool_name, "list_files");
    assert_eq!(args["path"], serde_json::json!(""));
    assert_eq!(args["recursive"], serde_json::json!("true"));
}

#[test]
fn test_parse_harmony_tool_call_from_text_container_exec() {
    let text = r#"to=container.exec {"cmd":["ls", "-la"]}"#;
    let result = OpenAIProvider::parse_harmony_tool_call_from_text(text);
    assert!(result.is_some());

    let (tool_name, args) = result.unwrap();
    assert_eq!(tool_name, "run_pty_cmd");
    assert_eq!(args["cmd"], serde_json::json!(["ls", "-la"]));
}

#[test]
fn chat_completions_uses_max_completion_tokens_field() {
    let provider =
        OpenAIProvider::with_model(String::new(), models::openai::DEFAULT_MODEL.to_string());
    let request = sample_request(models::openai::DEFAULT_MODEL);

    let payload = provider
        .convert_to_openai_format(&request)
        .expect("conversion should succeed");

    let max_tokens_value = payload
        .get(MAX_COMPLETION_TOKENS_FIELD)
        .and_then(Value::as_u64)
        .expect("max completion tokens should be set");
    assert_eq!(max_tokens_value, 512);
    assert!(payload.get("max_tokens").is_none());
}

#[test]
fn chat_completions_applies_temperature_independent_of_max_tokens() {
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5.to_string());
    let mut request = sample_request(models::openai::GPT_5);
    request.temperature = Some(0.4);

    let payload = provider
        .convert_to_openai_format(&request)
        .expect("conversion should succeed");

    assert!(payload.get(MAX_COMPLETION_TOKENS_FIELD).is_none());
    let temperature_value = payload
        .get("temperature")
        .and_then(Value::as_f64)
        .expect("temperature should be present");
    assert!((temperature_value - 0.4).abs() < 1e-6);
}

#[test]
fn responses_payload_omits_parallel_tool_config_when_not_supported() {
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5.to_string());
    let mut request = sample_request(models::openai::GPT_5);
    request.parallel_tool_calls = Some(true);
    request.parallel_tool_config = Some(Box::new(ParallelToolConfig {
        disable_parallel_tool_use: true,
        max_parallel_tools: Some(2),
        encourage_parallel: false,
    }));

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert_eq!(payload.get("parallel_tool_calls"), Some(&Value::Bool(true)));
    assert!(
        payload.get("parallel_tool_config").is_none(),
        "OpenAI payload should not include parallel_tool_config"
    );
}

mod streaming_tests {
    use super::*;

    #[test]
    fn test_gpt5_models_disable_streaming() {
        // Test that GPT-5 models return false for supports_streaming
        let test_models = [
            models::openai::GPT_5,
            models::openai::GPT_5_MINI,
            models::openai::GPT_5_NANO,
        ];

        for &model in &test_models {
            let provider = OpenAIProvider::with_model("test-key".to_owned(), model.to_owned());
            assert_eq!(
                provider.supports_streaming(),
                false,
                "Model {} should not support streaming",
                model
            );
        }
    }
}

mod caching_tests {
    use super::*;
    use crate::config::core::PromptCachingConfig;
    use serde_json::json;

    #[test]
    fn test_openai_prompt_cache_retention() {
        // Setup configuration with retention
        let mut config = PromptCachingConfig::default();
        config.enabled = true;
        config.providers.openai.enabled = true;
        config.providers.openai.prompt_cache_retention = Some("24h".to_string());

        // Initialize provider
        let provider = OpenAIProvider::from_config(
            Some("key".into()),
            None,
            None,
            Some(config),
            None,
            None,
            None,
        );

        // Create a dummy request for a Responses API model
        // Must use an exact model name from RESPONSES_API_MODELS
        let request = provider::LLMRequest {
            messages: vec![provider::Message::user("Hello".to_string())],
            model: crate::config::constants::models::openai::GPT_5_2_CODEX.to_string(),
            ..Default::default()
        };

        // We need to access private method `convert_to_openai_responses_format`
        // OR we can test `convert_to_openai_format` if it calls it, but `convert_to_openai_format`
        // is for Chat Completions. The Responses API conversion is private.
        // However, since we are inside the module (submodule), we can access private methods of parent if we import them?
        // No, `mod caching_tests` is a child module. Parent private items are visible to child modules
        // in Rust 2018+ if we use `super::`.

        // Let's verify visibility. `convert_to_openai_responses_format` is private `fn`.
        // Child modules can verify it.

        let json_result = provider.convert_to_openai_responses_format(&request);

        assert!(json_result.is_ok());
        let json = json_result.unwrap();

        // Verify the field is present
        assert_eq!(json["prompt_cache_retention"], json!("24h"));
    }

    #[test]
    fn test_openai_prompt_cache_retention_skipped_for_chat_api() {
        // Setup configuration with retention
        let mut config = PromptCachingConfig::default();
        config.enabled = true;
        config.providers.openai.enabled = true;
        config.providers.openai.prompt_cache_retention = Some("24h".to_string());

        let provider = OpenAIProvider::from_config(
            Some("key".into()),
            None,
            None,
            Some(config),
            None,
            None,
            None,
        );

        // Standard GPT-4o model (Chat Completions API)
        let request = provider::LLMRequest {
            messages: vec![provider::Message::user("Hello".to_string())],
            model: "gpt-5".to_string(),
            ..Default::default()
        };

        // This uses the standard chat format conversion
        let json_result = provider.convert_to_openai_format(&request);
        assert!(json_result.is_ok());
        let json = json_result.unwrap();

        // Should NOT have prompt_cache_retention
        assert!(json.get("prompt_cache_retention").is_none());
    }
}

mod exact_count_tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn exact_count_uses_openai_input_tokens_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/responses/input_tokens"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "input_tokens": 321
            })))
            .mount(&server)
            .await;

        let provider = OpenAIProvider::new_with_client(
            "test-key".to_owned(),
            models::openai::GPT_5_2.to_owned(),
            reqwest::Client::new(),
            format!("{}/v1", server.uri()),
            crate::config::TimeoutsConfig::default(),
        );
        let request = sample_request(models::openai::GPT_5_2);

        let count = <OpenAIProvider as provider::LLMProvider>::count_prompt_tokens_exact(
            &provider, &request,
        )
        .await
        .expect("count should succeed");

        assert_eq!(count, Some(321));
    }

    #[tokio::test]
    async fn exact_count_accepts_usage_input_tokens_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/responses/input_tokens"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "usage": {"input_tokens": 77}
            })))
            .mount(&server)
            .await;

        let provider = OpenAIProvider::new_with_client(
            "test-key".to_owned(),
            models::openai::GPT_5_2.to_owned(),
            reqwest::Client::new(),
            format!("{}/v1", server.uri()),
            crate::config::TimeoutsConfig::default(),
        );
        let request = sample_request(models::openai::GPT_5_2);

        let count = <OpenAIProvider as provider::LLMProvider>::count_prompt_tokens_exact(
            &provider, &request,
        )
        .await
        .expect("count should succeed");

        assert_eq!(count, Some(77));
    }

    #[tokio::test]
    async fn exact_count_returns_none_for_non_native_openai_base_url() {
        let provider = OpenAIProvider::from_config(
            Some("key".to_owned()),
            Some(models::openai::GPT_5_2.to_owned()),
            Some("https://example.local/v1".to_owned()),
            None,
            None,
            None,
            None,
        );
        let request = sample_request(models::openai::GPT_5_2);

        let count = <OpenAIProvider as provider::LLMProvider>::count_prompt_tokens_exact(
            &provider, &request,
        )
        .await
        .expect("count should succeed");

        assert_eq!(count, None);
    }
}
