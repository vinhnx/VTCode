use super::super::tool_serialization;
use super::*;
use crate::config::TimeoutsConfig;
use crate::config::core::{
    OpenAIHostedShellConfig, OpenAIHostedShellEnvironment, OpenAIHostedSkill, OpenAIServiceTier,
};
use crate::llm::provider::ParallelToolConfig;
use futures::StreamExt;
use serde_json::{Value, json};
use std::sync::Mutex;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
        tools: Some(Arc::new(vec![sample_tool()])),
        model: model.to_string(),
        ..Default::default()
    }
}

fn shell_tool() -> provider::ToolDefinition {
    provider::ToolDefinition::function(
        "shell".to_owned(),
        "Execute a shell command and return its output.".to_owned(),
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"}
            },
            "required": ["command"],
            "additionalProperties": false
        }),
    )
}

fn shell_request(model: &str) -> provider::LLMRequest {
    provider::LLMRequest {
        messages: vec![provider::Message::user("Run pwd".to_owned())],
        tools: Some(Arc::new(vec![shell_tool()])),
        model: model.to_string(),
        ..Default::default()
    }
}

fn priority_openai_config() -> OpenAIConfig {
    OpenAIConfig {
        service_tier: Some(OpenAIServiceTier::Priority),
        ..Default::default()
    }
}

fn hosted_shell_openai_config() -> OpenAIConfig {
    OpenAIConfig {
        hosted_shell: OpenAIHostedShellConfig {
            enabled: true,
            environment: OpenAIHostedShellEnvironment::ContainerAuto,
            container_id: None,
            file_ids: vec!["file_123".to_string()],
            skills: vec![OpenAIHostedSkill::SkillReference {
                skill_id: "skill_123".to_string(),
                version: "latest".to_string(),
            }],
        },
        ..Default::default()
    }
}

fn test_provider(base_url: &str, model: &str) -> OpenAIProvider {
    let http_client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test client should build");
    OpenAIProvider::new_with_client(
        "test-key".to_string(),
        model.to_string(),
        http_client,
        base_url.to_string(),
        TimeoutsConfig::default(),
    )
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

    let tools = payload
        .get("tools")
        .and_then(Value::as_array)
        .expect("tools should exist on payload");
    let tool_object = tools[0].as_object().expect("tool entry should be object");
    assert_eq!(
        tool_object.get("type").and_then(Value::as_str),
        Some("function")
    );
    assert_eq!(
        tool_object.get("name").and_then(Value::as_str),
        Some("search_workspace")
    );
    assert!(tool_object.contains_key("parameters"));
}

#[test]
fn responses_payload_uses_hosted_shell_when_enabled() {
    let provider = OpenAIProvider::from_config(
        Some(String::new()),
        Some(models::openai::GPT_5.to_string()),
        Some("https://api.openai.com/v1".to_string()),
        None,
        None,
        None,
        Some(hosted_shell_openai_config()),
        None,
    );
    let request = shell_request(models::openai::GPT_5);

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    let tool_object = payload["tools"][0]
        .as_object()
        .expect("tool entry should be object");
    assert_eq!(tool_object.get("type").and_then(Value::as_str), Some("shell"));
    assert_eq!(
        tool_object["environment"]["type"].as_str(),
        Some("container_auto")
    );
    assert_eq!(
        tool_object["environment"]["network_policy"]["type"].as_str(),
        Some("disabled")
    );
    assert_eq!(
        tool_object["environment"]["file_ids"][0].as_str(),
        Some("file_123")
    );
    assert_eq!(
        tool_object["environment"]["skills"][0]["type"].as_str(),
        Some("skill_reference")
    );
    let output_types = payload["output_types"]
        .as_array()
        .expect("output types should be present");
    assert!(output_types.iter().any(|value| value.as_str() == Some("shell_call")));
}

#[test]
fn responses_payload_uses_container_reference_when_configured() {
    let provider = OpenAIProvider::from_config(
        Some(String::new()),
        Some(models::openai::GPT_5.to_string()),
        Some("https://api.openai.com/v1".to_string()),
        None,
        None,
        None,
        Some(OpenAIConfig {
            hosted_shell: OpenAIHostedShellConfig {
                enabled: true,
                environment: OpenAIHostedShellEnvironment::ContainerReference,
                container_id: Some("cntr_123".to_string()),
                file_ids: vec!["file_ignored".to_string()],
                skills: vec![OpenAIHostedSkill::SkillReference {
                    skill_id: "skill_ignored".to_string(),
                    version: "latest".to_string(),
                }],
            },
            ..Default::default()
        }),
        None,
    );
    let request = shell_request(models::openai::GPT_5);

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    let tool_object = payload["tools"][0]
        .as_object()
        .expect("tool entry should be object");
    assert_eq!(
        tool_object["environment"]["type"].as_str(),
        Some("container_reference")
    );
    assert_eq!(
        tool_object["environment"]["container_id"].as_str(),
        Some("cntr_123")
    );
    assert!(tool_object["environment"].get("file_ids").is_none());
    assert!(tool_object["environment"].get("skills").is_none());
}

#[test]
fn non_native_openai_base_url_keeps_local_shell_tool() {
    let provider = OpenAIProvider::from_config(
        Some(String::new()),
        Some(models::openai::GPT_5.to_string()),
        Some("https://example.com/v1".to_string()),
        None,
        None,
        None,
        Some(hosted_shell_openai_config()),
        None,
    );
    let request = shell_request(models::openai::GPT_5);

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    let tool_object = payload["tools"][0]
        .as_object()
        .expect("tool entry should be object");
    assert_eq!(
        tool_object.get("type").and_then(Value::as_str),
        Some("function")
    );
    assert_eq!(tool_object.get("name").and_then(Value::as_str), Some("shell"));
    let output_types = payload["output_types"]
        .as_array()
        .expect("output types should be present");
    assert!(
        output_types
            .iter()
            .all(|value| value.as_str() != Some("shell_call"))
    );
}

#[test]
fn missing_container_reference_id_keeps_local_shell_tool() {
    let provider = OpenAIProvider::from_config(
        Some(String::new()),
        Some(models::openai::GPT_5.to_string()),
        Some("https://api.openai.com/v1".to_string()),
        None,
        None,
        None,
        Some(OpenAIConfig {
            hosted_shell: OpenAIHostedShellConfig {
                enabled: true,
                environment: OpenAIHostedShellEnvironment::ContainerReference,
                container_id: Some("   ".to_string()),
                file_ids: Vec::new(),
                skills: Vec::new(),
            },
            ..Default::default()
        }),
        None,
    );
    let request = shell_request(models::openai::GPT_5);

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    let tool_object = payload["tools"][0]
        .as_object()
        .expect("tool entry should be object");
    assert_eq!(
        tool_object.get("type").and_then(Value::as_str),
        Some("function")
    );
    assert_eq!(tool_object.get("name").and_then(Value::as_str), Some("shell"));
}

#[test]
fn responses_payload_passes_context_management() {
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5.to_string());
    let mut request = sample_request(models::openai::GPT_5);
    request.context_management = Some(json!([{
        "type": "compaction",
        "compact_threshold": 200000
    }]));

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    let management = payload
        .get("context_management")
        .and_then(Value::as_array)
        .expect("context_management should be present");
    assert_eq!(management.len(), 1);
    assert_eq!(
        management[0].get("type").and_then(Value::as_str),
        Some("compaction")
    );
}

#[test]
fn supports_responses_compaction_tracks_responses_api_availability() {
    let openai = OpenAIProvider::with_model(String::new(), models::openai::GPT_5.to_string());
    assert!(openai.supports_responses_compaction(models::openai::GPT_5));

    let xai = OpenAIProvider::from_config(
        Some(String::new()),
        Some(models::openai::GPT_5.to_string()),
        Some("https://api.x.ai/v1".to_string()),
        None,
        None,
        None,
        None,
        None,
    );
    assert!(!xai.supports_responses_compaction(models::openai::GPT_5));
}

#[test]
fn responses_payload_serializes_user_input_file_by_id() {
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5.to_string());
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user_with_parts(vec![
            provider::ContentPart::text("Summarize this file".to_string()),
            provider::ContentPart::file_from_id("file-abc123".to_string()),
        ])],
        model: models::openai::GPT_5.to_string(),
        ..Default::default()
    };

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    let input = payload
        .get("input")
        .and_then(Value::as_array)
        .expect("responses input should exist");
    let content = input[0]
        .get("content")
        .and_then(Value::as_array)
        .expect("user content should be an array");

    assert!(
        content.iter().any(
            |part| part.get("type").and_then(Value::as_str) == Some("input_file")
                && part.get("file_id").and_then(Value::as_str) == Some("file-abc123")
        ),
        "expected input_file part with file_id"
    );
}

#[test]
fn chat_payload_rejects_file_url_content_parts() {
    let provider =
        OpenAIProvider::with_model(String::new(), models::openai::DEFAULT_MODEL.to_string());
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user_with_parts(vec![
            provider::ContentPart::file_from_url("https://example.com/doc.pdf".to_string()),
        ])],
        model: models::openai::DEFAULT_MODEL.to_string(),
        ..Default::default()
    };

    let error = provider
        .convert_to_openai_format(&request)
        .expect_err("chat payload should reject file_url");
    match error {
        provider::LLMError::InvalidRequest { message, .. } => {
            assert!(message.contains("does not support file_url"));
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
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
    let serialized = tool_serialization::serialize_tools_for_responses(&tools, None)
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
fn responses_payload_serializes_hosted_tool_search_and_deferred_function() {
    let hosted_search = provider::ToolDefinition::hosted_tool_search();
    let deferred = provider::ToolDefinition::function(
        "search_docs".to_owned(),
        "Search internal docs".to_owned(),
        json!({
            "type": "object",
            "properties": { "query": { "type": "string" } },
            "required": ["query"],
            "additionalProperties": false
        }),
    )
    .with_defer_loading(true);

    let tools = vec![hosted_search, deferred];
    let payload = tool_serialization::serialize_tools_for_responses(&tools, None)
        .expect("tools should serialize for responses");
    let arr = payload.as_array().expect("tool array");

    assert!(
        arr.iter()
            .any(|tool| tool.get("type").and_then(Value::as_str) == Some("tool_search"))
    );

    let deferred_tool = arr
        .iter()
        .find(|tool| tool.get("name").and_then(Value::as_str) == Some("search_docs"))
        .expect("deferred function should be present");
    assert_eq!(deferred_tool["defer_loading"], json!(true));
}

#[test]
fn responses_payload_serializes_hosted_web_search_tool() {
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5.to_string());
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user(
            "Find the latest VT Code news".to_owned(),
        )],
        tools: Some(Arc::new(vec![provider::ToolDefinition::web_search(
            json!({
                "search_context_size": "medium"
            }),
        )])),
        model: models::openai::GPT_5.to_string(),
        ..Default::default()
    };

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    let tools = payload
        .get("tools")
        .and_then(Value::as_array)
        .expect("tools should exist on payload");
    assert_eq!(tools.len(), 1);
    assert_eq!(
        tools[0].get("type").and_then(Value::as_str),
        Some("web_search")
    );
    assert_eq!(
        tools[0].get("search_context_size").and_then(Value::as_str),
        Some("medium")
    );
}

#[test]
fn responses_payload_serializes_file_search_tool() {
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5.to_string());
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user(
            "Search the docs vector store".to_owned(),
        )],
        tools: Some(Arc::new(vec![provider::ToolDefinition::file_search(
            json!({
                "vector_store_ids": ["vs_docs"]
            }),
        )])),
        model: models::openai::GPT_5.to_string(),
        ..Default::default()
    };

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    let tools = payload
        .get("tools")
        .and_then(Value::as_array)
        .expect("tools should exist on payload");
    assert_eq!(tools.len(), 1);
    assert_eq!(
        tools[0].get("type").and_then(Value::as_str),
        Some("file_search")
    );
    assert_eq!(
        tools[0]
            .get("vector_store_ids")
            .and_then(Value::as_array)
            .and_then(|ids| ids.first())
            .and_then(Value::as_str),
        Some("vs_docs")
    );
}

#[test]
fn responses_payload_keeps_distinct_remote_mcp_tools() {
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5.to_string());
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user("Use both MCP servers".to_owned())],
        tools: Some(Arc::new(vec![
            provider::ToolDefinition::mcp(json!({
                "server_label": "dmcp",
                "server_url": "https://dmcp-server.deno.dev/sse",
                "require_approval": "never"
            })),
            provider::ToolDefinition::mcp(json!({
                "server_label": "docs",
                "server_url": "https://docs.example/sse",
                "require_approval": "never"
            })),
        ])),
        model: models::openai::GPT_5.to_string(),
        ..Default::default()
    };

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    let tools = payload
        .get("tools")
        .and_then(Value::as_array)
        .expect("tools should exist on payload");
    assert_eq!(tools.len(), 2);
    assert!(
        tools
            .iter()
            .any(|tool| tool.get("server_label").and_then(Value::as_str) == Some("dmcp"))
    );
    assert!(
        tools
            .iter()
            .any(|tool| tool.get("server_label").and_then(Value::as_str) == Some("docs"))
    );
}

#[test]
fn chat_payload_serializes_deferred_function_for_tool_search() {
    let deferred = provider::ToolDefinition::function(
        "search_docs".to_owned(),
        "Search internal docs".to_owned(),
        json!({
            "type": "object",
            "properties": { "query": { "type": "string" } },
            "required": ["query"],
            "additionalProperties": false
        }),
    )
    .with_defer_loading(true);

    let payload = tool_serialization::serialize_tools(&[deferred], models::openai::GPT_5_4)
        .expect("tools should serialize");
    let arr = payload.as_array().expect("tool array");
    assert_eq!(arr[0]["defer_loading"], json!(true));
}

#[test]
fn responses_payload_sets_instructions_from_system_prompt() {
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5.to_string());
    let mut request = sample_request(models::openai::GPT_5);
    request.system_prompt = Some(Arc::new("You are a helpful assistant.".to_owned()));

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
fn responses_payload_includes_previous_response_and_optional_fields() {
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5.to_string());
    let mut request = sample_request(models::openai::GPT_5);
    request.previous_response_id = Some("resp_previous_123".to_string());
    request.response_store = Some(false);
    request.responses_include = Some(vec![
        "reasoning.encrypted_content".to_string(),
        "output_text.annotations".to_string(),
    ]);

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert_eq!(
        payload.get("previous_response_id").and_then(Value::as_str),
        Some("resp_previous_123")
    );
    assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));

    let include = payload
        .get("include")
        .and_then(Value::as_array)
        .expect("include should be present");
    assert_eq!(include.len(), 2);
    assert_eq!(
        include.first().and_then(Value::as_str),
        Some("reasoning.encrypted_content")
    );
}

#[test]
fn responses_payload_includes_assistant_phase_for_native_openai() {
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5_4.to_string());
    let request = provider::LLMRequest {
        messages: vec![
            provider::Message::user("Start".to_owned()),
            provider::Message::assistant("Checking prerequisites".to_owned())
                .with_phase(Some(provider::AssistantPhase::Commentary)),
            provider::Message::assistant("Done".to_owned())
                .with_phase(Some(provider::AssistantPhase::FinalAnswer)),
        ],
        model: models::openai::GPT_5_4.to_string(),
        ..Default::default()
    };

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    let input = payload
        .get("input")
        .and_then(Value::as_array)
        .expect("input should exist");

    assert_eq!(
        input[1].get("phase").and_then(Value::as_str),
        Some("commentary")
    );
    assert_eq!(
        input[2].get("phase").and_then(Value::as_str),
        Some("final_answer")
    );
}

#[test]
fn responses_payload_omits_phase_for_non_assistant_items() {
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5_4.to_string());
    let request = provider::LLMRequest {
        messages: vec![
            provider::Message::user("Start".to_owned())
                .with_phase(Some(provider::AssistantPhase::Commentary)),
            provider::Message::assistant_with_tools(
                "Looking up data".to_owned(),
                vec![provider::ToolCall::function(
                    "call_1".to_string(),
                    "search_workspace".to_string(),
                    r#"{"query":"phase"}"#.to_string(),
                )],
            )
            .with_phase(Some(provider::AssistantPhase::Commentary)),
            provider::Message::tool_response("call_1".to_string(), "{\"ok\":true}".to_string())
                .with_phase(Some(provider::AssistantPhase::FinalAnswer)),
        ],
        model: models::openai::GPT_5_4.to_string(),
        ..Default::default()
    };

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    let input = payload
        .get("input")
        .and_then(Value::as_array)
        .expect("input should exist");

    assert!(
        input[0].get("phase").is_none(),
        "user items should omit phase"
    );
    assert_eq!(
        input[1].get("phase").and_then(Value::as_str),
        Some("commentary")
    );
    assert!(
        input[2].get("phase").is_none(),
        "tool items should omit phase"
    );
}

#[test]
fn responses_payload_omits_assistant_phase_for_non_native_openai_endpoints() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        Some(models::openai::GPT_5_4.to_string()),
        Some("https://example.local/v1".to_string()),
        None,
        None,
        None,
        None,
        None,
    );
    let request = provider::LLMRequest {
        messages: vec![
            provider::Message::user("Start".to_owned()),
            provider::Message::assistant("Checking prerequisites".to_owned())
                .with_phase(Some(provider::AssistantPhase::Commentary)),
        ],
        model: models::openai::GPT_5_4.to_string(),
        ..Default::default()
    };

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    let input = payload
        .get("input")
        .and_then(Value::as_array)
        .expect("input should exist");

    assert!(input[1].get("phase").is_none());
}

#[test]
fn responses_payload_uses_provider_level_responses_options() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        Some(models::openai::O4_MINI.to_string()),
        None,
        None,
        None,
        None,
        Some(OpenAIConfig {
            responses_store: Some(false),
            responses_include: vec![
                " reasoning.encrypted_content ".to_string(),
                "".to_string(),
                "output_text.annotations".to_string(),
            ],
            ..Default::default()
        }),
        None,
    );
    let request = sample_request(models::openai::O4_MINI);

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
    assert_eq!(
        payload
            .get("include")
            .and_then(Value::as_array)
            .expect("include should be present")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>(),
        vec!["reasoning.encrypted_content", "output_text.annotations"]
    );
}

#[test]
fn responses_payload_uses_provider_level_service_tier_for_native_openai() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        Some(models::openai::GPT_5_2.to_string()),
        None,
        None,
        None,
        None,
        Some(priority_openai_config()),
        None,
    );
    let request = sample_request(models::openai::GPT_5_2);

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert_eq!(
        payload.get("service_tier").and_then(Value::as_str),
        Some("priority")
    );
}

#[test]
fn responses_payload_omits_service_tier_for_models_without_service_tier_support() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        Some(models::openai::GPT_OSS_20B.to_string()),
        None,
        None,
        None,
        None,
        Some(priority_openai_config()),
        None,
    );
    let request = sample_request(models::openai::GPT_OSS_20B);

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert!(payload.get("service_tier").is_none());
}

#[test]
fn supported_models_include_o_series_reasoning_models() {
    let provider = OpenAIProvider::new("key".to_owned());
    let supported = provider.supported_models();

    assert!(supported.contains(&models::openai::O3.to_string()));
    assert!(supported.contains(&models::openai::O4_MINI.to_string()));
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
fn chat_payload_uses_provider_level_service_tier_for_native_openai() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        Some(models::openai::DEFAULT_MODEL.to_string()),
        None,
        None,
        None,
        None,
        Some(priority_openai_config()),
        None,
    );

    let payload = provider
        .convert_to_openai_format(&sample_request(models::openai::DEFAULT_MODEL))
        .expect("conversion should succeed");

    assert_eq!(
        payload.get("service_tier").and_then(Value::as_str),
        Some("priority")
    );
}

#[test]
fn chat_payload_omits_service_tier_for_models_without_service_tier_support() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        Some(models::openai::GPT_OSS_20B.to_string()),
        None,
        None,
        None,
        None,
        Some(priority_openai_config()),
        None,
    );

    let payload = provider
        .convert_to_openai_format(&sample_request(models::openai::GPT_OSS_20B))
        .expect("conversion should succeed");

    assert!(payload.get("service_tier").is_none());
}

#[test]
fn chat_payload_omits_assistant_phase_metadata() {
    let provider =
        OpenAIProvider::with_model(String::new(), models::openai::DEFAULT_MODEL.to_string());
    let request = provider::LLMRequest {
        messages: vec![
            provider::Message::user("Start".to_owned()),
            provider::Message::assistant("Working".to_owned())
                .with_phase(Some(provider::AssistantPhase::Commentary)),
        ],
        model: models::openai::DEFAULT_MODEL.to_string(),
        ..Default::default()
    };

    let payload = provider
        .convert_to_openai_format(&request)
        .expect("conversion should succeed");
    let messages = payload
        .get("messages")
        .and_then(Value::as_array)
        .expect("messages should exist");

    assert!(messages[1].get("phase").is_none());
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
fn responses_payload_omits_service_tier_for_non_native_openai_base_url() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        Some(models::openai::GPT_5_2.to_string()),
        Some("https://example.local/v1".to_string()),
        None,
        None,
        None,
        Some(priority_openai_config()),
        None,
    );

    let payload = provider
        .convert_to_openai_responses_format(&sample_request(models::openai::GPT_5_2))
        .expect("conversion should succeed");

    assert!(payload.get("service_tier").is_none());
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
        None,
    );

    assert_eq!(
        provider.prompt_cache_settings.prompt_cache_retention,
        Some("72h".to_owned())
    );
}

#[test]
fn provider_from_config_respects_websocket_mode_opt_in() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_string()),
        Some(models::openai::GPT_5_2.to_string()),
        None,
        None,
        None,
        None,
        Some(OpenAIConfig {
            websocket_mode: true,
            ..Default::default()
        }),
        None,
    );

    assert!(provider.websocket_mode_enabled(models::openai::GPT_5_2));
}

#[test]
fn provider_from_config_respects_service_tier() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_string()),
        Some(models::openai::GPT_5_2.to_string()),
        None,
        None,
        None,
        None,
        Some(priority_openai_config()),
        None,
    );

    assert_eq!(
        provider.service_tier.map(OpenAIServiceTier::as_str),
        Some("priority")
    );
}

#[test]
fn chat_payload_omits_service_tier_for_non_native_openai_base_url() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        Some(models::openai::DEFAULT_MODEL.to_string()),
        Some("https://example.local/v1".to_string()),
        None,
        None,
        None,
        Some(priority_openai_config()),
        None,
    );

    let payload = provider
        .convert_to_openai_format(&sample_request(models::openai::DEFAULT_MODEL))
        .expect("conversion should succeed");

    assert!(payload.get("service_tier").is_none());
}

#[test]
fn test_parse_harmony_tool_name() {
    assert_eq!(
        OpenAIProvider::parse_harmony_tool_name("repo_browser.list_files"),
        "list_files"
    );
    assert_eq!(
        OpenAIProvider::parse_harmony_tool_name("container.exec"),
        "unified_exec"
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
    assert_eq!(tool_name, "unified_exec");
    assert_eq!(args["cmd"], serde_json::json!(["ls", "-la"]));
}

#[test]
fn chat_completions_uses_max_completion_tokens_field() {
    let provider = OpenAIProvider::from_config(
        Some(String::new()),
        Some(models::openai::DEFAULT_MODEL.to_string()),
        Some("https://api.openai.com/v1".to_string()),
        None,
        None,
        None,
        None,
        None,
    );
    let mut request = sample_request(models::openai::DEFAULT_MODEL);
    request.max_tokens = Some(512);

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
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5_2.to_string());
    let mut request = sample_request(models::openai::GPT_5_2);
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
fn responses_payload_defaults_gpt_5_4_reasoning_to_none() {
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5_4.to_string());
    let request = sample_request(models::openai::GPT_5_4);

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert_eq!(
        payload
            .get("reasoning")
            .and_then(Value::as_object)
            .and_then(|reasoning| reasoning.get("effort"))
            .and_then(Value::as_str),
        Some("none")
    );
}

#[test]
fn responses_payload_omits_sampling_parameters_for_gpt_5_4_high_reasoning() {
    let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5_4.to_string());
    let mut request = sample_request(models::openai::GPT_5_4);
    request.reasoning_effort = Some(crate::config::types::ReasoningEffortLevel::High);
    request.temperature = Some(0.4);
    request.top_p = Some(0.9);

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert!(
        payload.get("sampling_parameters").is_none(),
        "sampling parameters should be omitted when GPT-5.4 reasoning is above none"
    );
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
            models::openai::GPT,
            models::openai::GPT_5,
            models::openai::GPT_5_4,
            models::openai::GPT_5_4_PRO,
            models::openai::GPT_5_MINI,
            models::openai::GPT_5_NANO,
        ];

        for &model in &test_models {
            let provider = OpenAIProvider::with_model("test-key".to_owned(), model.to_owned());
            assert!(
                !provider.supports_streaming(),
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
        let mut config = PromptCachingConfig {
            enabled: true,
            ..Default::default()
        };
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
            None,
        );

        // Create a dummy request for a Responses API model
        // Must use an exact model name from RESPONSES_API_MODELS
        let request = provider::LLMRequest {
            messages: vec![provider::Message::user("Hello".to_string())],
            model: models::openai::GPT_5_3_CODEX.to_string(),
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
        let mut config = PromptCachingConfig {
            enabled: true,
            ..Default::default()
        };
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

#[tokio::test]
async fn responses_request_retries_with_fallback_model_after_not_found() {
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri(), models::openai::GPT_5_NANO);
    let seen_models = Arc::new(Mutex::new(Vec::new()));
    let seen_models_for_response = Arc::clone(&seen_models);

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(move |request: &wiremock::Request| {
            let payload: Value =
                serde_json::from_slice(&request.body).expect("request body should be valid json");
            let model = payload
                .get("model")
                .and_then(Value::as_str)
                .expect("responses payload should include a model");
            seen_models_for_response
                .lock()
                .expect("models mutex should not be poisoned")
                .push(model.to_string());

            match model {
                models::openai::GPT_5_NANO => {
                    ResponseTemplate::new(404).set_body_string("model_not_found")
                }
                models::openai::GPT_5_MINI => ResponseTemplate::new(200).set_body_json(json!({
                    "id": "resp_fallback",
                    "status": "completed",
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": "fallback response"
                        }]
                    }]
                })),
                other => ResponseTemplate::new(500)
                    .set_body_string(format!("unexpected fallback model: {other}")),
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let response = provider
        .generate(provider::LLMRequest {
            messages: vec![provider::Message::user("Hello".to_string())],
            model: models::openai::GPT_5_NANO.to_string(),
            ..Default::default()
        })
        .await
        .expect("fallback request should succeed");

    assert_eq!(response.content.as_deref(), Some("fallback response"));
    assert_eq!(
        seen_models
            .lock()
            .expect("models mutex should not be poisoned")
            .as_slice(),
        &[
            models::openai::GPT_5_NANO.to_string(),
            models::openai::GPT_5_MINI.to_string(),
        ]
    );
}

#[tokio::test]
async fn responses_stream_retries_with_fallback_model_after_not_found() {
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri(), models::openai::GPT_5_NANO);
    let seen_models = Arc::new(Mutex::new(Vec::new()));
    let seen_models_for_response = Arc::clone(&seen_models);

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(move |request: &wiremock::Request| {
            let payload: Value =
                serde_json::from_slice(&request.body).expect("request body should be valid json");
            let model = payload
                .get("model")
                .and_then(Value::as_str)
                .expect("responses payload should include a model");
            seen_models_for_response
                .lock()
                .expect("models mutex should not be poisoned")
                .push(model.to_string());

            match model {
                models::openai::GPT_5_NANO => {
                    ResponseTemplate::new(404).set_body_string("model_not_found")
                }
                models::openai::GPT_5_MINI => ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_json(json!({
                        "id": "resp_stream_fallback",
                        "status": "completed",
                        "output": [{
                            "type": "message",
                            "role": "assistant",
                            "content": [{
                                "type": "output_text",
                                "text": "fallback stream response"
                            }]
                        }]
                    })),
                other => ResponseTemplate::new(500)
                    .set_body_string(format!("unexpected fallback model: {other}")),
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let mut stream = provider
        .stream(provider::LLMRequest {
            messages: vec![provider::Message::user("Hello".to_string())],
            model: models::openai::GPT_5_NANO.to_string(),
            ..Default::default()
        })
        .await
        .expect("fallback stream should succeed");

    let mut completed = None;
    while let Some(event) = stream.next().await {
        match event.expect("stream event should parse") {
            provider::LLMStreamEvent::Completed { response } => completed = Some(response),
            provider::LLMStreamEvent::Token { .. }
            | provider::LLMStreamEvent::Reasoning { .. }
            | provider::LLMStreamEvent::ReasoningStage { .. } => {}
        }
    }

    let response = completed.expect("stream should finish with a completed response");
    assert_eq!(
        response.content.as_deref(),
        Some("fallback stream response")
    );
    assert_eq!(
        seen_models
            .lock()
            .expect("models mutex should not be poisoned")
            .as_slice(),
        &[
            models::openai::GPT_5_NANO.to_string(),
            models::openai::GPT_5_MINI.to_string(),
        ]
    );
}
