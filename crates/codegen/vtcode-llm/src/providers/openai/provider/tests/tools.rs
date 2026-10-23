use super::*;

#[test]
fn serialize_tools_wraps_function_definition() {
    let serialized = tool_serialization::serialize_tools(&[sample_tool()], models::openai::DEFAULT_MODEL)
        .expect("tools should serialize");
    let tool = serialized.as_array().expect("array")[0].as_object().expect("object");
    assert_eq!(tool.get("type").and_then(Value::as_str), Some("function"));
    assert!(tool.contains_key("function"));
    assert_str_field_obj(tool, "name", "search_workspace");
    assert_eq!(tool.get("description").and_then(Value::as_str).unwrap_or_default(), "Search project files");

    let func = tool
        .get("function")
        .and_then(Value::as_object)
        .expect("function payload missing");
    assert_str_field_obj(func, "name", "search_workspace");
    assert!(func.contains_key("parameters"));
    assert_eq!(tool.get("parameters").and_then(Value::as_object), func.get("parameters").and_then(Value::as_object));
}

#[test]
fn serialize_tools_dedupes_duplicate_names() {
    let dup =
        provider::ToolDefinition::function("search_workspace".to_owned(), "dup".to_owned(), json!({"type": "object"}));
    let serialized = tool_serialization::serialize_tools(&[sample_tool(), dup], models::openai::DEFAULT_MODEL)
        .expect("tools should serialize cleanly");
    assert_eq!(serialized.as_array().expect("array").len(), 1, "duplicate names should be dropped");
}

#[test]
fn code_search_tool_serialization_preserves_simple_constraints() {
    let tool = provider::ToolDefinition::function(
        vtcode_config::constants::tools::CODE_SEARCH.to_owned(),
        "Search code".to_owned(),
        vtcode_utility_tool_specs::code_search_parameters(),
    );
    let serialized = tool_serialization::serialize_tools(std::slice::from_ref(&tool), models::openai::DEFAULT_MODEL)
        .expect("chat tools should serialise");
    let chat_parameters = &serialized.as_array().expect("chat tool array")[0]["parameters"];

    assert_eq!(chat_parameters["required"], json!(["query"]));
    assert_eq!(chat_parameters["additionalProperties"], false);
    let mut property_names = chat_parameters["properties"]
        .as_object()
        .expect("chat properties")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    property_names.sort_unstable();
    assert_eq!(property_names, ["file_types", "max_results", "path", "query", "result_types"]);
    assert_eq!(chat_parameters["properties"]["max_results"]["minimum"], 1);
    assert_eq!(chat_parameters["properties"]["max_results"]["maximum"], 100);
    assert!(chat_parameters.get("anyOf").is_none());

    let responses =
        tool_serialization::serialize_tools_for_responses(&[tool], None).expect("responses tools should serialise");
    let responses_parameters = &responses.as_array().expect("responses tool array")[0]["parameters"];
    assert_eq!(responses_parameters, chat_parameters);
}

#[test]
fn responses_tools_dedupes_apply_patch_and_function() {
    let tools = vec![
        provider::ToolDefinition::apply_patch("Apply patches".to_owned()),
        provider::ToolDefinition::function("apply_patch".to_owned(), "alt apply".to_owned(), json!({"type": "object"})),
    ];
    let serialized =
        tool_serialization::serialize_tools_for_responses(&tools, None).expect("responses tools should serialize");
    let arr = serialized.as_array().expect("array");
    assert_eq!(arr.len(), 1, "apply_patch should be deduped");
    assert_eq!(arr[0].get("type").and_then(Value::as_str), Some("function"));
    assert_eq!(arr[0].get("name").and_then(Value::as_str), Some("apply_patch"));
}

#[test]
fn responses_payload_serializes_hosted_tool_search_and_deferred_function() {
    let tools = vec![
        provider::ToolDefinition::hosted_tool_search(),
        provider::ToolDefinition::function(
            "search_docs".to_owned(),
            "Search internal docs".to_owned(),
            json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false}),
        ).with_defer_loading(true),
    ];
    let payload =
        tool_serialization::serialize_tools_for_responses(&tools, None).expect("tools should serialize for responses");
    let arr = payload.as_array().expect("tool array");
    assert!(arr.iter().any(|t| t.get("type").and_then(Value::as_str) == Some("tool_search")));
    let deferred = arr
        .iter()
        .find(|t| t.get("name").and_then(Value::as_str) == Some("search_docs"))
        .expect("deferred function should be present");
    assert_eq!(deferred["defer_loading"], json!(true));
}

#[test]
fn responses_payload_emits_allowed_tools_for_native_openai_from_stable_catalogue() {
    let provider = native_openai_provider(models::openai::GPT_5);
    let request = responses_allowed_tools_request(models::openai::GPT_5);
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    let full_tools = payload["tools"].as_array().expect("tools should be present");
    assert_eq!(full_tools.len(), 3, "full stable catalogue should remain");
    assert_eq!(
        full_tools
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>(),
        vec!["search_workspace"]
    );
    assert!(
        full_tools
            .iter()
            .any(|tool| tool.get("type").and_then(Value::as_str) == Some("web_search")),
        "hosted web_search should remain in full catalogue"
    );

    let choice = payload["tool_choice"]
        .as_object()
        .expect("allowed_tools choice should be an object");
    assert_str_field_obj(choice, "type", "allowed_tools");
    assert_str_field_obj(choice, "mode", "auto");
    assert_eq!(
        choice["tools"].as_array().expect("allowed tools array"),
        &vec![
            json!({"type":"function", "name":"search_workspace"}),
            json!({"type":"web_search"})
        ]
    );
}

#[test]
fn responses_payload_emits_allowed_tools_for_chatgpt_backend() {
    let provider = chatgpt_backend_provider(models::openai::GPT_5);
    let request = responses_allowed_tools_request(models::openai::GPT_5);
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert_eq!(payload["tool_choice"]["type"].as_str(), Some("allowed_tools"));
    assert_eq!(payload["tool_choice"]["tools"][0], json!({"type":"function", "name":"search_workspace"}));
}

#[test]
fn responses_allowed_tools_references_match_serialized_catalogue_and_deduplicate() {
    let provider = native_openai_provider(models::openai::GPT_5);
    let mut request = responses_allowed_tools_request(models::openai::GPT_5);
    request.tools = Some(Arc::new(vec![
        sample_tool(),
        sample_tool(),
        provider::ToolDefinition::mcp(json!({"server_label":"docs", "server_url":"https://example.test/mcp"})),
    ]));
    request.tool_choice = Some(provider::ToolChoice::allowed_tools_auto(vec![
        "mcp".into(),
        "missing".into(),
        "search_workspace".into(),
        "search_workspace".into(),
    ]));
    let payload = provider.convert_to_openai_responses_format(&request).expect("conversion");
    assert_eq!(payload["tools"].as_array().unwrap().len(), 2);
    assert_eq!(
        payload["tool_choice"]["tools"],
        json!([
            {"type":"function", "name":"search_workspace"},
            {"type":"mcp", "server_label":"docs"},
        ])
    );
}

#[test]
fn responses_payload_omits_allowed_tools_for_compatible_endpoint() {
    let provider = compatible_endpoint_provider(models::openai::GPT_5, "https://example.test/v1");
    let request = responses_allowed_tools_request(models::openai::GPT_5);
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert_eq!(payload["tool_choice"].as_str(), Some("auto"));
}

#[test]
fn responses_payload_omits_allowed_tools_for_non_responses_model() {
    let provider = native_openai_provider(models::openai::GPT_OSS_20B);
    let request = responses_allowed_tools_request(models::openai::GPT_OSS_20B);
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert_eq!(payload["tool_choice"].as_str(), Some("auto"));
}

#[test]
fn responses_payload_preserves_any_of_enum_and_nullable_type_unions() {
    let tools = vec![provider::ToolDefinition::function(
        "schema_rich_tool".to_owned(),
        "Schema rich tool".to_owned(),
        json!({
            "type": "object",
            "properties": {
                "open": {
                    "anyOf": [
                        {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "ref_id": {"type": "string"},
                                    "lineno": {"type": ["integer", "null"]}
                                },
                                "required": ["ref_id"],
                                "additionalProperties": false
                            }
                        },
                        {"type": "null"}
                    ]
                },
                "response_length": {
                    "type": "string",
                    "enum": ["short", "medium", "long"]
                },
                "message": {"type": ["string", "null"]}
            },
            "additionalProperties": false
        }),
    )];

    let payload =
        tool_serialization::serialize_tools_for_responses(&tools, None).expect("tools should serialize for responses");
    let params = &payload.as_array().expect("tool array")[0]["parameters"];

    assert!(params["properties"]["open"]["anyOf"].is_array());
    assert_eq!(params["properties"]["response_length"]["enum"], json!(["short", "medium", "long"]));
    assert_eq!(params["properties"]["message"]["type"], json!(["string", "null"]));
}

#[test]
fn chat_payload_serializes_deferred_function_for_tool_search() {
    let deferred = provider::ToolDefinition::function(
        "search_docs".to_owned(),
        "Search internal docs".to_owned(),
        json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false}),
    ).with_defer_loading(true);
    let payload =
        tool_serialization::serialize_tools(&[deferred], models::openai::GPT_5_6_SOL).expect("tools should serialize");
    assert_eq!(payload.as_array().expect("array")[0]["defer_loading"], json!(true));
}

#[test]
fn chat_completions_payload_uses_function_wrapper() {
    let provider = native_openai_provider(models::openai::DEFAULT_MODEL);
    let payload = chat_payload_for(models::openai::DEFAULT_MODEL, &provider);
    let tools = payload.get("tools").and_then(Value::as_array).expect("tools should exist");
    let tool = tools[0].as_object().expect("tool entry should be object");
    assert!(tool.contains_key("function"));
    assert_eq!(tool.get("name").and_then(Value::as_str), Some("search_workspace"));
}

#[test]
fn responses_payload_uses_function_wrapper() {
    let provider = native_openai_provider(models::openai::GPT_5);
    let payload = responses_payload_for(models::openai::GPT_5, &provider);
    let tools = payload.get("tools").and_then(Value::as_array).expect("tools should exist");
    let tool = tools[0].as_object().expect("tool entry should be object");
    assert_eq!(tool.get("type").and_then(Value::as_str), Some("function"));
    assert_eq!(tool.get("name").and_then(Value::as_str), Some("search_workspace"));
    assert!(tool.contains_key("parameters"));
}

#[test]
fn responses_function_tools_add_empty_properties_for_bare_object_schema() {
    let provider = native_openai_provider(models::openai::GPT_5_CODEX);
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user("Hello".to_owned())].into(),
        tools: Some(Arc::new(vec![provider::ToolDefinition::function(
            "vtcode-clippy".to_owned(),
            "Run clippy on the workspace".to_owned(),
            json!({"type": "object", "additionalProperties": true}),
        )])),
        model: models::openai::GPT_5_CODEX.to_string(),
        ..Default::default()
    };
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    let params = payload["tools"]
        .as_array()
        .expect("tool array")
        .iter()
        .find(|t| t.get("name").and_then(Value::as_str) == Some("vtcode-clippy"))
        .and_then(|t| t.get("parameters"))
        .expect("vtcode-clippy parameters");
    assert_eq!(params["type"].as_str(), Some("object"));
    assert_eq!(params["properties"], json!({}));
    assert_eq!(params["additionalProperties"], json!({"type": "string"}));
}

#[test]
fn responses_payload_serializes_hosted_web_search_tool() {
    let provider = native_openai_provider(models::openai::GPT_5);
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user("Find the latest VT Code news".to_owned())].into(),
        tools: Some(Arc::new(vec![provider::ToolDefinition::web_search(
            json!({"search_context_size": "medium"}),
        )])),
        model: models::openai::GPT_5.to_string(),
        ..Default::default()
    };
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    let tools = payload.get("tools").and_then(Value::as_array).expect("tools should exist");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].get("type").and_then(Value::as_str), Some("web_search"));
    assert_eq!(tools[0].get("search_context_size").and_then(Value::as_str), Some("medium"));
}

#[test]
fn responses_payload_serializes_file_search_tool() {
    let provider = native_openai_provider(models::openai::GPT_5);
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user("Search the docs vector store".to_owned())].into(),
        tools: Some(Arc::new(vec![provider::ToolDefinition::file_search(
            json!({"vector_store_ids": ["vs_docs"]}),
        )])),
        model: models::openai::GPT_5.to_string(),
        ..Default::default()
    };
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    let tools = payload.get("tools").and_then(Value::as_array).expect("tools should exist");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].get("type").and_then(Value::as_str), Some("file_search"));
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
    let provider = native_openai_provider(models::openai::GPT_5);
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user("Use both MCP servers".to_owned())].into(),
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
    let tools = payload.get("tools").and_then(Value::as_array).expect("tools should exist");
    assert_eq!(tools.len(), 2);
    assert!(
        tools
            .iter()
            .any(|t| t.get("server_label").and_then(Value::as_str) == Some("dmcp"))
    );
    assert!(
        tools
            .iter()
            .any(|t| t.get("server_label").and_then(Value::as_str) == Some("docs"))
    );
}
