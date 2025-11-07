use mcp_types::*;
use serde_json::{json, Value};
use std::fs;

fn get_schema() -> Value {
    let schema_str =
        fs::read_to_string("spec/2024-11-05-schema.json").expect("Failed to read schema file");

    serde_json::from_str(&schema_str).expect("Failed to parse schema JSON")
}

fn validate_against_definition(instance: Value, definition_name: &str) {
    let schema = get_schema();
    let definitions = schema
        .get("definitions")
        .expect("Schema must have definitions");
    let schema_for_type = json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "$ref": "#/definitions/".to_string() + definition_name,
        "definitions": definitions
    });

    assert!(
        jsonschema::validate(&schema_for_type, &instance).is_ok(),
        "Validation failed for {}: {:?}",
        definition_name,
        instance
    );
}

#[test]
fn test_tool_schema() {
    let tool = Tool {
        description: Some("A test tool".to_string()),
        input_schema: ToolInputSchema {
            properties: Default::default(),
            required: vec![],
            type_: "object".to_string(),
        },
        name: "test-tool".to_string(),
    };

    validate_against_definition(serde_json::to_value(tool).unwrap(), "Tool");
}

#[test]
fn test_tool_input_schema() {
    let schema = ToolInputSchema {
        properties: Default::default(),
        required: vec![],
        type_: "object".to_string(),
    };

    validate_against_definition(
        serde_json::to_value(schema).unwrap(),
        "Tool/properties/inputSchema",
    );
}

#[test]
fn test_list_tools_request() {
    let request = ListToolsRequest {
        method: "tools/list".to_string(),
        params: Some(ListToolsRequestParams {
            cursor: Some("test-cursor".to_string()),
        }),
    };

    validate_against_definition(serde_json::to_value(request).unwrap(), "ListToolsRequest");
}

#[test]
fn test_list_tools_result() {
    let result = ListToolsResult {
        meta: Default::default(),
        next_cursor: Some("next-page".to_string()),
        tools: vec![],
    };

    validate_against_definition(serde_json::to_value(result).unwrap(), "ListToolsResult");
}

#[test]
fn test_tool_list_changed_notification() {
    let notification = ToolListChangedNotification {
        method: "notifications/tools/list_changed".to_string(),
        params: Some(ToolListChangedNotificationParams {
            meta: Default::default(),
        }),
    };

    validate_against_definition(
        serde_json::to_value(notification).unwrap(),
        "ToolListChangedNotification",
    );
}

#[test]
fn test_text_resource_contents() {
    let contents = TextResourceContents {
        mime_type: Some("text/plain".to_string()),
        text: "Sample text".to_string(),
        uri: "file:///test.txt".to_string(),
    };

    validate_against_definition(
        serde_json::to_value(contents).unwrap(),
        "TextResourceContents",
    );
}

#[test]
fn test_text_content() {
    let content = TextContent {
        annotations: None,
        text: "Sample text".to_string(),
        type_: "text".to_string(),
    };

    validate_against_definition(serde_json::to_value(content).unwrap(), "TextContent");
}

#[test]
fn test_annotated() {
    let annotated = Annotated {
        annotations: Some(AnnotatedAnnotations {
            audience: vec![Role::Assistant],
            priority: Some(0.5),
        }),
    };

    validate_against_definition(serde_json::to_value(annotated).unwrap(), "Annotated");
}

#[test]
fn test_blob_resource_contents() {
    let contents = BlobResourceContents {
        blob: "base64data".to_string(),
        mime_type: Some("image/png".to_string()),
        uri: "file:///test.png".to_string(),
    };

    validate_against_definition(
        serde_json::to_value(contents).unwrap(),
        "BlobResourceContents",
    );
}

#[test]
fn test_call_tool_request() {
    let request = CallToolRequest {
        method: "tools/call".to_string(),
        params: CallToolRequestParams {
            arguments: Default::default(),
            name: "test-tool".to_string(),
        },
    };

    validate_against_definition(serde_json::to_value(request).unwrap(), "CallToolRequest");
}

#[test]
fn test_call_tool_result() {
    let result = CallToolResult {
        content: vec![CallToolResultContentItem::TextContent(TextContent {
            annotations: None,
            text: "Result text".to_string(),
            type_: "text".to_string(),
        })],
        is_error: Some(false),
        meta: Default::default(),
    };

    validate_against_definition(serde_json::to_value(result).unwrap(), "CallToolResult");
}

#[test]
fn test_cancelled_notification() {
    let notification = CancelledNotification {
        method: "notifications/cancelled".to_string(),
        params: CancelledNotificationParams {
            reason: Some("User cancelled".to_string()),
            request_id: RequestId::String("123".to_string()),
        },
    };

    validate_against_definition(
        serde_json::to_value(notification).unwrap(),
        "CancelledNotification",
    );
}

#[test]
fn test_client_capabilities() {
    let capabilities = ClientCapabilities {
        experimental: Default::default(),
        roots: Some(ClientCapabilitiesRoots {
            list_changed: Some(true),
        }),
        sampling: Default::default(),
        elicitation: Default::default(),
    };

    validate_against_definition(
        serde_json::to_value(capabilities).unwrap(),
        "ClientCapabilities",
    );
}

#[test]
fn test_complete_request() {
    let request = CompleteRequest {
        method: "completion/complete".to_string(),
        params: CompleteRequestParams {
            argument: CompleteRequestParamsArgument {
                name: "test-arg".to_string(),
                value: "test-value".to_string(),
            },
            ref_: CompleteRequestParamsRef::PromptReference(PromptReference {
                name: "test-prompt".to_string(),
                type_: "ref/prompt".to_string(),
            }),
        },
    };

    validate_against_definition(serde_json::to_value(request).unwrap(), "CompleteRequest");
}

#[test]
fn test_complete_result() {
    let result = CompleteResult {
        completion: CompleteResultCompletion {
            has_more: Some(false),
            total: Some(1),
            values: vec!["completion".to_string()],
        },
        meta: Default::default(),
    };

    validate_against_definition(serde_json::to_value(result).unwrap(), "CompleteResult");
}

#[test]
fn test_embedded_resource() {
    let resource = EmbeddedResource {
        annotations: None,
        resource: TextResourceContents {
            mime_type: Some("text/plain".to_string()),
            text: "Embedded text".to_string(),
            uri: "file:///test.txt".to_string(),
        }
        .into(),
        type_: "resource".to_string(),
    };

    validate_against_definition(serde_json::to_value(resource).unwrap(), "EmbeddedResource");
}

#[test]
fn test_image_content() {
    let content = ImageContent {
        annotations: None,
        data: "base64data".to_string(),
        mime_type: "image/png".to_string(),
        type_: "image".to_string(),
    };

    validate_against_definition(serde_json::to_value(content).unwrap(), "ImageContent");
}

#[test]
fn test_initialize_request() {
    let request = InitializeRequest {
        method: "initialize".to_string(),
        params: InitializeRequestParams {
            capabilities: ClientCapabilities {
                experimental: Default::default(),
                roots: Some(ClientCapabilitiesRoots {
                    list_changed: Some(true),
                }),
                sampling: Default::default(),
                elicitation: Default::default(),
            },
            client_info: Implementation {
                name: "test-client".to_string(),
                version: "1.0.0".to_string(),
                title: None,
            },
            protocol_version: "2024-11-05".to_string(),
        },
    };

    validate_against_definition(serde_json::to_value(request).unwrap(), "InitializeRequest");
}

#[test]
fn test_initialize_result() {
    let result = InitializeResult {
        capabilities: ServerCapabilities {
            experimental: Default::default(),
            logging: Default::default(),
            prompts: Some(ServerCapabilitiesPrompts {
                list_changed: Some(true),
            }),
            resources: Some(ServerCapabilitiesResources {
                list_changed: Some(true),
                subscribe: Some(true),
            }),
            tools: Some(ServerCapabilitiesTools {
                list_changed: Some(true),
            }),
        },
        instructions: Some("Test instructions".to_string()),
        protocol_version: "2024-11-05".to_string(),
        server_info: Implementation {
            name: "test-server".to_string(),
            version: "1.0.0".to_string(),
            title: None,
        },
        meta: Default::default(),
    };

    validate_against_definition(serde_json::to_value(result).unwrap(), "InitializeResult");
}

#[test]
fn test_initialized_notification() {
    let notification = InitializedNotification {
        method: "notifications/initialized".to_string(),
        params: None,
    };

    validate_against_definition(
        serde_json::to_value(notification).unwrap(),
        "InitializedNotification",
    );
}

#[test]
fn test_list_prompts_request() {
    let request = ListPromptsRequest {
        method: "prompts/list".to_string(),
        params: Some(ListPromptsRequestParams {
            cursor: Some("test-cursor".to_string()),
        }),
    };

    validate_against_definition(serde_json::to_value(request).unwrap(), "ListPromptsRequest");
}

#[test]
fn test_list_prompts_result() {
    let result = ListPromptsResult {
        meta: Default::default(),
        next_cursor: Some("next-page".to_string()),
        prompts: vec![Prompt {
            arguments: vec![PromptArgument {
                description: Some("Test argument".to_string()),
                name: "test-arg".to_string(),
                required: Some(true),
            }],
            description: Some("Test prompt".to_string()),
            name: "test-prompt".to_string(),
        }],
    };

    validate_against_definition(serde_json::to_value(result).unwrap(), "ListPromptsResult");
}

#[test]
fn test_list_resources_request() {
    let request = ListResourcesRequest {
        method: "resources/list".to_string(),
        params: Some(ListResourcesRequestParams {
            cursor: Some("test-cursor".to_string()),
        }),
    };

    validate_against_definition(
        serde_json::to_value(request).unwrap(),
        "ListResourcesRequest",
    );
}

#[test]
fn test_list_resources_result() {
    let result = ListResourcesResult {
        meta: Default::default(),
        next_cursor: Some("next-page".to_string()),
        resources: vec![Resource {
            annotations: None,
            description: Some("Test resource".to_string()),
            mime_type: Some("text/plain".to_string()),
            name: "test-resource".to_string(),
            size: Some(100),
            uri: "file:///test.txt".to_string(),
        }],
    };

    validate_against_definition(serde_json::to_value(result).unwrap(), "ListResourcesResult");
}

#[test]
fn test_list_roots_request() {
    let request = ListRootsRequest {
        method: "roots/list".to_string(),
        params: Default::default(),
    };

    validate_against_definition(serde_json::to_value(request).unwrap(), "ListRootsRequest");
}

#[test]
fn test_list_roots_result() {
    let result = ListRootsResult {
        meta: Default::default(),
        roots: vec![Root {
            name: Some("Test root".to_string()),
            uri: "file:///test".to_string(),
        }],
    };

    validate_against_definition(serde_json::to_value(result).unwrap(), "ListRootsResult");
}

#[test]
fn test_logging_message_notification() {
    let notification = LoggingMessageNotification {
        method: "notifications/message".to_string(),
        params: LoggingMessageNotificationParams {
            data: serde_json::Value::String("Test message".to_string()),
            level: LoggingLevel::Info,
            logger: Some("test-logger".to_string()),
        },
    };

    validate_against_definition(
        serde_json::to_value(notification).unwrap(),
        "LoggingMessageNotification",
    );
}

#[test]
fn test_progress_notification() {
    let notification = ProgressNotification {
        method: "notifications/progress".to_string(),
        params: ProgressNotificationParams {
            progress: 50.0,
            progress_token: ProgressToken::String("test-token".to_string()),
            total: Some(100.0),
        },
    };

    validate_against_definition(
        serde_json::to_value(notification).unwrap(),
        "ProgressNotification",
    );
}

#[test]
fn test_list_resource_templates_request() {
    let request = ListResourceTemplatesRequest {
        method: "resources/templates/list".to_string(),
        params: Some(ListResourceTemplatesRequestParams {
            cursor: Some("test-cursor".to_string()),
        }),
    };

    validate_against_definition(
        serde_json::to_value(request).unwrap(),
        "ListResourceTemplatesRequest",
    );
}

#[test]
fn test_list_resource_templates_result() {
    let result = ListResourceTemplatesResult {
        meta: Default::default(),
        next_cursor: Some("next-page".to_string()),
        resource_templates: vec![ResourceTemplate {
            annotations: None,
            description: Some("Test template".to_string()),
            mime_type: Some("text/plain".to_string()),
            name: "test-template".to_string(),
            uri_template: "file:///test/{param}".to_string(),
        }],
    };

    validate_against_definition(
        serde_json::to_value(result).unwrap(),
        "ListResourceTemplatesResult",
    );
}

#[test]
fn test_read_resource_request() {
    let request = ReadResourceRequest {
        method: "resources/read".to_string(),
        params: ReadResourceRequestParams {
            uri: "file:///test.txt".to_string(),
        },
    };

    validate_against_definition(
        serde_json::to_value(request).unwrap(),
        "ReadResourceRequest",
    );
}

#[test]
fn test_read_resource_result() {
    let result = ReadResourceResult {
        contents: vec![ReadResourceResultContentsItem::TextResourceContents(
            TextResourceContents {
                mime_type: Some("text/plain".to_string()),
                text: "Test content".to_string(),
                uri: "file:///test.txt".to_string(),
            },
        )],
        meta: Default::default(),
    };

    validate_against_definition(serde_json::to_value(result).unwrap(), "ReadResourceResult");
}

#[test]
fn test_resource_updated_notification() {
    let notification = ResourceUpdatedNotification {
        method: "notifications/resources/updated".to_string(),
        params: ResourceUpdatedNotificationParams {
            uri: "file:///test.txt".to_string(),
        },
    };

    validate_against_definition(
        serde_json::to_value(notification).unwrap(),
        "ResourceUpdatedNotification",
    );
}

#[test]
fn test_subscribe_request() {
    let request = SubscribeRequest {
        method: "resources/subscribe".to_string(),
        params: SubscribeRequestParams {
            uri: "file:///test.txt".to_string(),
        },
    };

    validate_against_definition(serde_json::to_value(request).unwrap(), "SubscribeRequest");
}

#[test]
fn test_unsubscribe_request() {
    let request = UnsubscribeRequest {
        method: "resources/unsubscribe".to_string(),
        params: UnsubscribeRequestParams {
            uri: "file:///test.txt".to_string(),
        },
    };

    validate_against_definition(serde_json::to_value(request).unwrap(), "UnsubscribeRequest");
}

#[test]
fn test_set_level_request() {
    let request = SetLevelRequest {
        method: "logging/setLevel".to_string(),
        params: SetLevelRequestParams {
            level: LoggingLevel::Debug,
        },
    };

    validate_against_definition(serde_json::to_value(request).unwrap(), "SetLevelRequest");
}

#[test]
fn test_prompt_list_changed_notification() {
    let notification = PromptListChangedNotification {
        method: "notifications/prompts/list_changed".to_string(),
        params: None,
    };

    validate_against_definition(
        serde_json::to_value(notification).unwrap(),
        "PromptListChangedNotification",
    );
}

#[test]
fn test_resource_list_changed_notification() {
    let notification = ResourceListChangedNotification {
        method: "notifications/resources/list_changed".to_string(),
        params: None,
    };

    validate_against_definition(
        serde_json::to_value(notification).unwrap(),
        "ResourceListChangedNotification",
    );
}

#[test]
fn test_get_prompt_request() {
    let request = GetPromptRequest {
        method: "prompts/get".to_string(),
        params: GetPromptRequestParams {
            arguments: Default::default(),
            name: "test-prompt".to_string(),
        },
    };

    validate_against_definition(serde_json::to_value(request).unwrap(), "GetPromptRequest");
}

#[test]
fn test_get_prompt_result() {
    let result = GetPromptResult {
        description: Some("Test prompt".to_string()),
        messages: vec![PromptMessage {
            content: PromptMessageContent::TextContent(TextContent {
                annotations: None,
                text: "Test message".to_string(),
                type_: "text".to_string(),
            }),
            role: Role::Assistant,
        }],
        meta: Default::default(),
    };

    validate_against_definition(serde_json::to_value(result).unwrap(), "GetPromptResult");
}

#[test]
fn test_create_message_request() {
    let request = CreateMessageRequest {
        method: "sampling/createMessage".to_string(),
        params: CreateMessageRequestParams {
            include_context: Some(CreateMessageRequestParamsIncludeContext::None),
            max_tokens: 100,
            messages: vec![SamplingMessage {
                content: SamplingMessageContent::TextContent(TextContent {
                    annotations: None,
                    text: "Test message".to_string(),
                    type_: "text".to_string(),
                }),
                role: Role::User,
            }],
            metadata: Default::default(),
            model_preferences: Some(ModelPreferences {
                cost_priority: Some(0.5),
                hints: vec![ModelHint {
                    name: Some("test-hint".to_string()),
                }],
                intelligence_priority: Some(0.7),
                speed_priority: Some(0.3),
            }),
            stop_sequences: vec!["stop".to_string()],
            system_prompt: Some("Test system prompt".to_string()),
            temperature: Some(0.7),
        },
    };

    validate_against_definition(
        serde_json::to_value(request).unwrap(),
        "CreateMessageRequest",
    );
}

#[test]
fn test_create_message_result() {
    let result = CreateMessageResult {
        content: CreateMessageResultContent::TextContent(TextContent {
            annotations: None,
            text: "Test response".to_string(),
            type_: "text".to_string(),
        }),
        meta: Default::default(),
        model: "test-model".to_string(),
        role: Role::Assistant,
        stop_reason: Some("completed".to_string()),
    };

    validate_against_definition(serde_json::to_value(result).unwrap(), "CreateMessageResult");
}

#[test]
fn test_ping_request() {
    let request = PingRequest {
        method: "ping".to_string(),
        params: Default::default(),
    };

    validate_against_definition(serde_json::to_value(request).unwrap(), "PingRequest");
}

#[test]
fn test_jsonrpc_request() {
    let request = JsonRpcRequest {
        id: RequestId::String("test-id".to_string()),
        jsonrpc: "2.0".to_string(),
        method: "test-method".to_string(),
        params: Default::default(),
    };

    validate_against_definition(serde_json::to_value(request).unwrap(), "JSONRPCRequest");
}

#[test]
fn test_jsonrpc_response() {
    let response = JsonRpcResponse {
        id: RequestId::String("test-id".to_string()),
        jsonrpc: "2.0".to_string(),
        result: McpResult {
            meta: Default::default(),
        },
    };

    validate_against_definition(serde_json::to_value(response).unwrap(), "JSONRPCResponse");
}

#[test]
fn test_jsonrpc_error() {
    let error = JsonRpcError {
        error: Error {
            code: 123,
            data: Some(serde_json::Value::String("test-data".to_string())),
            message: "Test error".to_string(),
        },
        id: RequestId::String("test-id".to_string()),
        jsonrpc: "2.0".to_string(),
    };

    validate_against_definition(serde_json::to_value(error).unwrap(), "JSONRPCError");
}

#[test]
fn test_resource_reference() {
    let reference = ResourceReference {
        type_: "ref/resource".to_string(),
        uri: "file:///test/{param}".to_string(),
    };

    validate_against_definition(
        serde_json::to_value(reference).unwrap(),
        "ResourceReference",
    );
}

#[test]
fn test_prompt_reference() {
    let reference = PromptReference {
        name: "test-prompt".to_string(),
        type_: "ref/prompt".to_string(),
    };

    validate_against_definition(serde_json::to_value(reference).unwrap(), "PromptReference");
}

#[test]
fn test_sampling_message() {
    let message = SamplingMessage {
        content: SamplingMessageContent::TextContent(TextContent {
            annotations: None,
            text: "Test message".to_string(),
            type_: "text".to_string(),
        }),
        role: Role::User,
    };

    validate_against_definition(serde_json::to_value(message).unwrap(), "SamplingMessage");
}

#[test]
fn test_prompt_message() {
    let message = PromptMessage {
        content: PromptMessageContent::TextContent(TextContent {
            annotations: None,
            text: "Test message".to_string(),
            type_: "text".to_string(),
        }),
        role: Role::Assistant,
    };

    validate_against_definition(serde_json::to_value(message).unwrap(), "PromptMessage");
}

#[test]
fn test_jsonrpc_notification() {
    let notification = JsonRpcNotification {
        jsonrpc: "2.0".to_string(),
        method: "test-method".to_string(),
        params: Default::default(),
    };

    validate_against_definition(
        serde_json::to_value(notification).unwrap(),
        "JSONRPCNotification",
    );
}

#[test]
fn test_roots_list_changed_notification() {
    let notification = RootsListChangedNotification {
        method: "notifications/roots/list_changed".to_string(),
        params: None,
    };

    validate_against_definition(
        serde_json::to_value(notification).unwrap(),
        "RootsListChangedNotification",
    );
}

#[test]
fn test_model_preferences() {
    let preferences = ModelPreferences {
        cost_priority: Some(0.5),
        hints: vec![ModelHint {
            name: Some("test-hint".to_string()),
        }],
        intelligence_priority: Some(0.7),
        speed_priority: Some(0.3),
    };

    validate_against_definition(
        serde_json::to_value(preferences).unwrap(),
        "ModelPreferences",
    );
}

#[test]
fn test_result() {
    let result = McpResult {
        meta: {
            let mut map = serde_json::Map::new();
            map.insert(
                "test-key".to_string(),
                serde_json::Value::String("test-value".to_string()),
            );
            map
        },
    };

    validate_against_definition(serde_json::to_value(result).unwrap(), "Result");
}

#[test]
fn test_model_hint() {
    // Test string variant
    let hint_string = ModelHint {
        name: Some("gpt-4".to_string()),
    };

    validate_against_definition(serde_json::to_value(hint_string).unwrap(), "ModelHint");
}

#[test]
fn test_implementation() {
    let implementation = Implementation {
        name: "test-implementation".to_string(),
        version: "1.0.0".to_string(),
        title: None,
    };

    validate_against_definition(
        serde_json::to_value(implementation).unwrap(),
        "Implementation",
    );
}

#[test]
fn test_sampling_message_content() {
    // Test TextContent variant
    let text_content = SamplingMessageContent::TextContent(TextContent {
        annotations: None,
        text: "Test message".to_string(),
        type_: "text".to_string(),
    });
    validate_against_definition(
        serde_json::to_value(text_content).unwrap(),
        "SamplingMessage/properties/content",
    );

    // Test ImageContent variant
    let image_content = SamplingMessageContent::ImageContent(ImageContent {
        annotations: None,
        data: "base64data".to_string(),
        mime_type: "image/png".to_string(),
        type_: "image".to_string(),
    });
    validate_against_definition(
        serde_json::to_value(image_content).unwrap(),
        "SamplingMessage/properties/content",
    );
}

#[test]
fn test_create_message_request_params_include_context() {
    // Test all valid values
    let values = vec!["allServers", "none", "thisServer"];

    for value in values {
        let include_context = value.to_string();
        validate_against_definition(
            serde_json::to_value(&include_context).unwrap(),
            "CreateMessageRequest/properties/params/properties/includeContext",
        );
    }
}
