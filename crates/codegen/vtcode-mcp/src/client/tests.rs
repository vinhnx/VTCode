use super::McpClient;
use rustc_hash::FxHashMap;
use vtcode_config::mcp::{
    McpClientConfig, McpHttpServerConfig, McpProviderConfig, McpRequirementsConfig, McpStdioServerConfig,
    McpTransportConfig,
};

fn base_config() -> McpClientConfig {
    McpClientConfig {
        enabled: true,
        requirements: McpRequirementsConfig {
            enforce: true,
            allowed_stdio_commands: vec!["uvx".to_string()],
            allowed_http_endpoints: vec!["https://allowed.example/mcp".to_string()],
        },
        ..McpClientConfig::default()
    }
}

#[test]
fn requirements_allow_matching_stdio_command() {
    let client = McpClient::new(base_config());
    let provider = McpProviderConfig {
        name: "time".to_string(),
        transport: McpTransportConfig::Stdio(McpStdioServerConfig {
            command: "uvx".to_string(),
            args: vec![],
            working_directory: None,
        }),
        ..McpProviderConfig::default()
    };

    assert!(client.requirement_mismatch_reason(&provider).is_none());
}

#[test]
fn requirements_block_unmatched_stdio_command() {
    let client = McpClient::new(base_config());
    let provider = McpProviderConfig {
        name: "time".to_string(),
        transport: McpTransportConfig::Stdio(McpStdioServerConfig {
            command: "npx".to_string(),
            args: vec![],
            working_directory: None,
        }),
        ..McpProviderConfig::default()
    };

    assert!(
        client
            .requirement_mismatch_reason(&provider)
            .is_some_and(|reason| reason.contains("not allowlisted"))
    );
}

#[test]
fn requirements_block_unmatched_http_endpoint() {
    let client = McpClient::new(base_config());
    let provider = McpProviderConfig {
        name: "remote".to_string(),
        transport: McpTransportConfig::Http(McpHttpServerConfig {
            endpoint: "https://blocked.example/mcp".to_string(),
            ..McpHttpServerConfig::default()
        }),
        ..McpProviderConfig::default()
    };

    assert!(
        client
            .requirement_mismatch_reason(&provider)
            .is_some_and(|reason| reason.contains("not allowlisted"))
    );
}

#[tokio::test]
async fn list_servers_includes_configured_provider_metadata() {
    let mut config = base_config();
    config.providers = vec![McpProviderConfig {
        name: "calendar".to_string(),
        transport: McpTransportConfig::Http(McpHttpServerConfig {
            endpoint: "https://calendar.example/mcp".to_string(),
            ..McpHttpServerConfig::default()
        }),
        ..McpProviderConfig::default()
    }];

    let client = McpClient::new(config);
    let servers = client.list_servers().await;
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0]["name"], "calendar");
    assert_eq!(servers[0]["connected"], false);
    assert_eq!(servers[0]["connection_state"], "disconnected");
    assert_eq!(servers[0]["transport"], "http");
    assert_eq!(servers[0]["target"], "https://calendar.example/mcp");
    assert!(
        servers[0]["negotiated_protocol_version"].is_null(),
        "disconnected servers have no negotiated version"
    );
}

#[tokio::test]
async fn connect_server_rejects_unknown_server_name() {
    let client = McpClient::new(base_config());
    let err = Box::pin(client.connect_server("missing"))
        .await
        .expect_err("missing server should error");
    assert!(err.to_string().contains("not configured"));
}

#[tokio::test]
async fn disconnect_server_rejects_unknown_server_name() {
    let client = McpClient::new(base_config());
    let err = client
        .disconnect_server("missing")
        .await
        .expect_err("missing server should error");
    assert!(err.to_string().contains("not connected"));
}

#[test]
fn providers_sorted_by_name_ignores_insertion_order() {
    let names = ["zeta", "Alpha", "alpha", "beta", "alpha2"];
    let forward: FxHashMap<String, usize> =
        names.iter().enumerate().map(|(idx, name)| ((*name).to_owned(), idx)).collect();
    let reverse: FxHashMap<String, usize> = names
        .iter()
        .enumerate()
        .rev()
        .map(|(idx, name)| ((*name).to_owned(), idx))
        .collect();

    // Values are the original indices of Alpha, alpha, alpha2, beta, zeta.
    let expected = vec![1, 2, 4, 3, 0];
    assert_eq!(super::providers_sorted_by_name(&forward), expected);
    assert_eq!(super::providers_sorted_by_name(&reverse), expected);
}

#[test]
fn record_tool_provider_keeps_first_provider_by_name_regardless_of_connect_order() {
    use super::McpToolInfo;
    use serde_json::json;

    let tool = |provider: &str, name: &str| McpToolInfo {
        name: name.to_owned(),
        description: String::new(),
        provider: provider.to_owned(),
        input_schema: json!({}),
        output_schema: None,
    };

    for order in [["alpha", "beta"], ["beta", "alpha"]] {
        let client = McpClient::new(base_config());
        for provider in order {
            client.record_tool_provider(
                provider,
                &[tool(provider, "shared"), tool(provider, &format!("{provider}_only"))],
            );
        }
        assert_eq!(client.provider_for_tool("shared").as_deref(), Some("alpha"), "order {order:?}");
        assert_eq!(client.provider_for_tool("beta_only").as_deref(), Some("beta"));
    }
}
#[test]
fn format_tool_result_preserves_result_shapes_and_error_flags() {
    use serde_json::json;

    let public_text = json!([{"type": "text", "text": "ordinary output"}]);
    for flag in [None, Some(false)] {
        for (content, structured) in [
            (json!([]), None),
            (public_text.clone(), None),
            (json!([]), Some(json!({"answer": 42, "_meta": {"public": true}}))),
            (public_text.clone(), Some(json!([3, {"nested": [false, null]}]))),
            (json!([]), Some(json!({}))),
            (json!([]), Some(json!(null))),
            (json!([]), Some(json!("scalar"))),
        ] {
            let mut result: super::CallToolResult = serde_json::from_value(json!({"content": content})).unwrap();
            result.is_error = flag;
            result.structured_content = structured.clone();
            let actual = McpClient::format_tool_result("source", "lookup", result).unwrap();
            let mut expected = json!({"provider": "source", "tool": "lookup"});
            if content != json!([]) {
                expected["content"] = content;
            }
            if let Some(structured) = structured {
                expected["structuredContent"] = structured;
            }
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn format_tool_result_hides_protocol_metadata_only() {
    use serde_json::json;

    let public_content = json!([
        {"type": "text", "text": "{\"_meta\":\"public text\"}",
         "annotations": {"audience": ["user"], "priority": 0.75}},
        {"type": "image", "data": "aW1hZ2U=", "mimeType": "image/png"},
        {"type": "audio", "data": "YXVkaW8=", "mimeType": "audio/wav"},
        {"type": "resource", "resource": {
            "uri": "file:///public.txt", "mimeType": "text/plain", "text": "public resource"}},
        {"type": "resource", "resource": {
            "uri": "file:///public.bin", "mimeType": "application/octet-stream", "blob": "YmxvYg=="}},
        {"type": "resource_link", "uri": "https://example.com/item", "name": "public link",
         "description": "description", "size": 17}
    ]);
    let mut private_content = public_content.clone();
    for (index, block) in private_content.as_array_mut().unwrap().iter_mut().enumerate() {
        block["_meta"] = json!({"message": format!("PRIVATE_BLOCK_{index}")});
        if block["type"] == "resource" {
            block["resource"]["_meta"] = json!({"message": format!("PRIVATE_RESOURCE_{index}")});
        }
    }
    let structured = json!({"_meta": {"message": "public application data"},
                            "nested": [{"_meta": 23}], "order": [9, 2]});
    let result = serde_json::from_value(json!({
        "content": private_content, "structuredContent": structured,
        "_meta": {"message": "PRIVATE_RESULT"}
    }))
    .unwrap();
    let actual = McpClient::format_tool_result("source", "lookup", result).unwrap();
    assert_eq!(
        actual,
        json!({"provider": "source", "tool": "lookup",
                             "content": public_content, "structuredContent": structured})
    );
    assert!(!actual.to_string().contains("PRIVATE_"));
}

#[test]
fn format_tool_result_errors_use_first_nonempty_public_text() {
    use serde_json::json;

    for (content, expected_message) in [
        (
            json!([{ "type": "text", "text": " \n" },
                { "type": "image", "data": "aW1hZ2U=", "mimeType": "image/png" },
                { "type": "text", "text": "Public failure" },
                { "type": "text", "text": "Later failure" }]),
            "Public failure",
        ),
        (json!([]), "Unknown MCP tool error"),
        (json!([{ "type": "text", "text": "" }]), "Unknown MCP tool error"),
    ] {
        let result = serde_json::from_value(json!({
            "isError": true, "content": content,
            "structuredContent": {"message": "PRIVATE_STRUCTURED_ERROR"},
            "_meta": {"message": "PRIVATE_RESULT_ERROR"}
        }))
        .unwrap();
        let error = McpClient::format_tool_result("source", "lookup", result).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("MCP tool 'lookup' on provider 'source' reported an error: {expected_message}")
        );
    }
}
