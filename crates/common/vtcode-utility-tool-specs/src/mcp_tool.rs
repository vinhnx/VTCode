use rmcp::model::Tool;
use serde_json::Value;

/// MCP Apps extension identifier (`io.modelcontextprotocol/ui`).
pub const MCP_UI_EXTENSION_ID: &str = "io.modelcontextprotocol/ui";

/// MIME type a `ui://` app resource must serve.
pub const MCP_UI_RESOURCE_MIME_TYPE: &str = "text/html;profile=mcp-app";

/// URI scheme identifying MCP Apps UI resources.
pub const MCP_UI_RESOURCE_SCHEME: &str = "ui://";

/// Deprecated `_meta` key for the app resource URI. Tolerated on read, never emitted.
const DEPRECATED_UI_RESOURCE_URI_KEY: &str = "ui/resourceUri";

/// Permission-policy features a UI resource may request (SEP-1865 `UIResourceMeta.permissions`).
const UI_PERMISSION_KEYS: [&str; 4] = ["camera", "microphone", "geolocation", "clipboardWrite"];

/// Linkage from an MCP tool to its MCP Apps UI resource.
///
/// Invariant: `resource_uri` is non-empty and starts with [`MCP_UI_RESOURCE_SCHEME`].
/// The only constructor site is [`parse_mcp_tool`], which enforces this fail-closed:
/// any missing, mistyped, or non-`ui://` value yields `None` instead of a link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpUiToolMeta {
    pub resource_uri: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedMcpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    pub ui: Option<McpUiToolMeta>,
    pub visibility: McpToolVisibility,
}

/// Tool accessibility between model and app (SEP-1865 `_meta.ui.visibility`).
///
/// The default is visible to both, matching the spec's omitted-means-both rule.
/// Hosts must hide tools without model visibility from the agent's tool list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct McpToolVisibility {
    pub model: bool,
    pub app: bool,
}

impl Default for McpToolVisibility {
    fn default() -> Self {
        Self { model: true, app: true }
    }
}

#[must_use]
pub fn parse_mcp_tool(tool: &Tool) -> ParsedMcpTool {
    let meta = tool.meta.as_ref().map(|meta| &meta.0);
    ParsedMcpTool {
        name: tool.name.to_string(),
        description: tool.description.clone().unwrap_or_default().to_string(),
        input_schema: serde_json::to_value(&tool.input_schema).unwrap_or(Value::Null),
        output_schema: tool.output_schema.as_ref().and_then(|schema| serde_json::to_value(schema).ok()),
        ui: parse_ui_meta(meta),
        visibility: parse_ui_tool_visibility(meta),
    }
}

/// Parse `_meta.ui.visibility` into model/app accessibility.
///
/// Omitted (or an absent `ui` object) means visible to both. A present array
/// enables exactly the named audiences; unknown entries are ignored for
/// forward compatibility. A present-but-malformed value hides the tool from
/// the model (fail-closed: a mistyped audience list must not leak an
/// app-only tool into the agent's tool list).
#[must_use]
pub fn parse_ui_tool_visibility(meta: Option<&serde_json::Map<String, Value>>) -> McpToolVisibility {
    let Some(ui) = meta.and_then(|meta| meta.get("ui")).and_then(Value::as_object) else {
        return McpToolVisibility::default();
    };
    let Some(visibility) = ui.get("visibility") else {
        return McpToolVisibility::default();
    };
    let Value::Array(entries) = visibility else {
        return McpToolVisibility { model: false, app: true };
    };
    McpToolVisibility {
        model: entries.iter().any(|entry| entry == "model"),
        app: entries.iter().any(|entry| entry == "app"),
    }
}

/// Declared sandbox posture of an MCP Apps UI resource (SEP-1865 `UIResourceMeta`).
///
/// Disclosure data only: hosts may restrict further and must never loosen.
/// All lists preserve server order; mistyped entries are skipped.
///
/// Serializes with the spec wire names (`connectDomains`, …) for disclosure payloads.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpUiResourceMeta {
    /// Origins for network requests (`csp.connectDomains`).
    pub connect_domains: Vec<String>,
    /// Origins for static resources (`csp.resourceDomains`).
    pub resource_domains: Vec<String>,
    /// Origins for nested iframes (`csp.frameDomains`).
    pub frame_domains: Vec<String>,
    /// Allowed base URIs (`csp.baseUriDomains`).
    pub base_uri_domains: Vec<String>,
    /// Requested permission-policy features, in spec order (`permissions`).
    pub permissions: Vec<String>,
    /// Dedicated sandbox origin for the view (`domain`).
    pub domain: Option<String>,
}

/// Extract the UI sandbox posture from a resource's `_meta` map.
///
/// Returns `None` when `_meta.ui` is absent or not an object. A present object
/// parses fail-soft: unknown permission keys are ignored for forward
/// compatibility, and non-string domain entries are skipped.
#[must_use]
pub fn parse_ui_resource_meta(meta: Option<&serde_json::Map<String, Value>>) -> Option<McpUiResourceMeta> {
    let ui = meta?.get("ui")?.as_object()?;
    let csp = ui.get("csp").and_then(Value::as_object);
    let permissions = ui.get("permissions").and_then(Value::as_object);
    Some(McpUiResourceMeta {
        connect_domains: string_list(csp, "connectDomains"),
        resource_domains: string_list(csp, "resourceDomains"),
        frame_domains: string_list(csp, "frameDomains"),
        base_uri_domains: string_list(csp, "baseUriDomains"),
        permissions: UI_PERMISSION_KEYS
            .into_iter()
            .filter(|key| {
                permissions.is_some_and(
                    |map| matches!(map.get(*key), Some(value) if !value.is_null() && *value != Value::Bool(false)),
                )
            })
            .map(str::to_owned)
            .collect(),
        domain: ui.get("domain").and_then(Value::as_str).map(str::to_owned),
    })
}

/// Collect the string entries of `key` from an optional JSON object.
///
/// Non-array values and non-string entries yield no items rather than an error.
fn string_list(obj: Option<&serde_json::Map<String, Value>>, key: &str) -> Vec<String> {
    obj.and_then(|map| map.get(key))
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Extract the MCP Apps linkage from a tool's `_meta` map.
///
/// Reads `_meta.ui.resourceUri`, falling back to the deprecated `_meta["ui/resourceUri"]`
/// only when the canonical value is absent. A present-but-invalid canonical value links
/// nothing rather than falling through. Returns `None` unless the winning value is a
/// string with a non-empty `ui://` URI.
fn parse_ui_meta(meta: Option<&serde_json::Map<String, Value>>) -> Option<McpUiToolMeta> {
    let meta = meta?;
    let canonical = meta
        .get("ui")
        .and_then(Value::as_object)
        .and_then(|ui| ui.get("resourceUri"))
        .and_then(Value::as_str);
    let deprecated = meta.get(DEPRECATED_UI_RESOURCE_URI_KEY).and_then(Value::as_str);
    let uri = canonical.or(deprecated)?;
    if uri.len() > MCP_UI_RESOURCE_SCHEME.len() && uri.starts_with(MCP_UI_RESOURCE_SCHEME) {
        Some(McpUiToolMeta { resource_uri: uri.to_owned() })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{McpToolVisibility, McpUiToolMeta, ParsedMcpTool, parse_mcp_tool};
    use rmcp::model::{MetaObject, Tool};
    use serde_json::json;
    use std::sync::Arc;

    fn empty_schema() -> Arc<serde_json::Map<String, serde_json::Value>> {
        Arc::new(serde_json::from_value(json!({})).expect("json object"))
    }

    fn tool_with_meta(meta: serde_json::Value) -> Tool {
        let mut tool = Tool::new_with_raw("app-tool", None, empty_schema());
        let map: serde_json::Map<String, serde_json::Value> = serde_json::from_value(meta).expect("meta object");
        tool.meta = Some(MetaObject(map));
        tool
    }

    #[test]
    fn parse_mcp_tool_visibility_matrix() {
        use super::McpToolVisibility as Visibility;

        // Omitted everywhere means both.
        let tool = Tool::new_with_raw("plain", None, empty_schema());
        assert_eq!(parse_mcp_tool(&tool).visibility, Visibility::default());
        let tool = tool_with_meta(json!({"ui": {"resourceUri": "ui://demo/app.html"}}));
        assert_eq!(parse_mcp_tool(&tool).visibility, Visibility::default());

        // Explicit audiences enable exactly what they name.
        let tool = tool_with_meta(json!({"ui": {"visibility": ["model", "app"]}}));
        assert_eq!(parse_mcp_tool(&tool).visibility, Visibility { model: true, app: true });
        let tool = tool_with_meta(json!({"ui": {"visibility": ["app"]}}));
        assert_eq!(parse_mcp_tool(&tool).visibility, Visibility { model: false, app: true });
        let tool = tool_with_meta(json!({"ui": {"visibility": ["model"]}}));
        assert_eq!(parse_mcp_tool(&tool).visibility, Visibility { model: true, app: false });
        let tool = tool_with_meta(json!({"ui": {"visibility": []}}));
        assert_eq!(parse_mcp_tool(&tool).visibility, Visibility { model: false, app: false });

        // Unknown entries are ignored; malformed values hide from the model.
        let tool = tool_with_meta(json!({"ui": {"visibility": ["model", "hologram"]}}));
        assert_eq!(parse_mcp_tool(&tool).visibility, Visibility { model: true, app: false });
        let tool = tool_with_meta(json!({"ui": {"visibility": "model"}}));
        assert_eq!(parse_mcp_tool(&tool).visibility, Visibility { model: false, app: true });
        let tool = tool_with_meta(json!({"ui": {"visibility": {"model": true}}}));
        assert_eq!(parse_mcp_tool(&tool).visibility, Visibility { model: false, app: true });
    }

    #[test]
    fn parse_mcp_tool_preserves_name_description_and_input_schema() {
        let input_schema = Arc::new(
            serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                }
            }))
            .expect("json object"),
        );
        let tool = Tool::new("search-docs", "Search documentation", input_schema);

        let parsed = parse_mcp_tool(&tool);
        assert_eq!(
            parsed,
            ParsedMcpTool {
                name: "search-docs".to_string(),
                description: "Search documentation".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"}
                    }
                }),
                output_schema: None,
                ui: None,
                visibility: McpToolVisibility::default(),
            }
        );
    }

    #[test]
    fn parse_mcp_tool_defaults_missing_description() {
        let tool = Tool::new_with_raw("search-docs", None, empty_schema());

        let parsed = parse_mcp_tool(&tool);
        assert_eq!(parsed.description, "");
        assert_eq!(parsed.input_schema, json!({}));
        assert_eq!(parsed.output_schema, None);
        assert_eq!(parsed.ui, None);
    }

    #[test]
    fn parse_mcp_tool_links_canonical_ui_resource_uri() {
        let tool = tool_with_meta(json!({"ui": {"resourceUri": "ui://get-time/mcp-app.html"}}));

        let parsed = parse_mcp_tool(&tool);
        assert_eq!(
            parsed.ui,
            Some(McpUiToolMeta {
                resource_uri: "ui://get-time/mcp-app.html".to_string(),
            })
        );
    }

    #[test]
    fn parse_mcp_tool_tolerates_deprecated_ui_resource_uri_key() {
        let tool = tool_with_meta(json!({"ui/resourceUri": "ui://legacy/app.html"}));

        let parsed = parse_mcp_tool(&tool);
        assert_eq!(parsed.ui, Some(McpUiToolMeta { resource_uri: "ui://legacy/app.html".to_string() }));
    }

    #[test]
    fn parse_mcp_tool_prefers_canonical_over_deprecated_key() {
        let tool = tool_with_meta(json!({
            "ui": {"resourceUri": "ui://canonical/app.html"},
            "ui/resourceUri": "ui://deprecated/app.html",
        }));

        let parsed = parse_mcp_tool(&tool);
        assert_eq!(
            parsed.ui,
            Some(McpUiToolMeta {
                resource_uri: "ui://canonical/app.html".to_string(),
            })
        );
    }

    #[test]
    fn parse_mcp_tool_rejects_non_ui_scheme_without_fallback() {
        let tool = tool_with_meta(json!({
            "ui": {"resourceUri": "https://example.com/app.html"},
            "ui/resourceUri": "ui://fallback/app.html",
        }));

        // A present-but-invalid canonical value does not fall through to the
        // deprecated key; mixed-shape metadata links nothing.
        let parsed = parse_mcp_tool(&tool);
        assert_eq!(parsed.ui, None);
    }

    #[test]
    fn parse_ui_resource_meta_reads_csp_permissions_and_domain() {
        let meta: serde_json::Map<String, serde_json::Value> = serde_json::from_value(json!({
            "ui": {
                "csp": {
                    "connectDomains": ["https://api.example.com", "wss://realtime.example.com"],
                    "resourceDomains": ["https://cdn.jsdelivr.net"],
                    "frameDomains": [],
                    "baseUriDomains": ["https://cdn.example.com"],
                },
                "permissions": {"camera": {}, "clipboardWrite": {}},
                "domain": "app.example.com",
            }
        }))
        .expect("meta object");

        let parsed = super::parse_ui_resource_meta(Some(&meta)).expect("ui meta");
        assert_eq!(parsed.connect_domains, ["https://api.example.com", "wss://realtime.example.com"]);
        assert_eq!(parsed.resource_domains, ["https://cdn.jsdelivr.net"]);
        assert!(parsed.frame_domains.is_empty());
        assert_eq!(parsed.base_uri_domains, ["https://cdn.example.com"]);
        assert_eq!(parsed.permissions, ["camera", "clipboardWrite"]);
        assert_eq!(parsed.domain.as_deref(), Some("app.example.com"));
    }

    #[test]
    fn ui_resource_meta_serializes_with_spec_wire_names() {
        let meta = super::McpUiResourceMeta {
            connect_domains: vec!["https://api.example.com".to_string()],
            permissions: vec!["camera".to_string()],
            domain: Some("app.example.com".to_string()),
            ..Default::default()
        };
        let value = serde_json::to_value(&meta).expect("serialize ui meta");
        assert_eq!(value["connectDomains"], json!(["https://api.example.com"]));
        assert_eq!(value["resourceDomains"], json!([]));
        assert_eq!(value["permissions"], json!(["camera"]));
        assert_eq!(value["domain"], json!("app.example.com"));
    }

    #[test]
    fn parse_ui_resource_meta_ignores_unknown_permission_keys() {
        let meta: serde_json::Map<String, serde_json::Value> = serde_json::from_value(json!({
            "ui": {"permissions": {"camera": {}, "future-feature": {}}}
        }))
        .expect("meta object");

        let parsed = super::parse_ui_resource_meta(Some(&meta)).expect("ui meta");
        assert_eq!(parsed.permissions, ["camera"]);
    }

    #[test]
    fn parse_ui_resource_meta_treats_null_and_false_permissions_as_absent() {
        let meta: serde_json::Map<String, serde_json::Value> = serde_json::from_value(json!({
            "ui": {"permissions": {"camera": false, "microphone": null, "geolocation": {}}}
        }))
        .expect("meta object");

        let parsed = super::parse_ui_resource_meta(Some(&meta)).expect("ui meta");
        assert_eq!(parsed.permissions, ["geolocation"]);
    }

    #[test]
    fn parse_ui_resource_meta_skips_mistyped_entries_fail_soft() {
        let meta: serde_json::Map<String, serde_json::Value> = serde_json::from_value(json!({
            "ui": {
                "csp": {
                    "connectDomains": ["https://api.example.com", 42, null],
                    "resourceDomains": "https://cdn.example.com",
                },
                "domain": 7,
            }
        }))
        .expect("meta object");

        let parsed = super::parse_ui_resource_meta(Some(&meta)).expect("ui meta");
        assert_eq!(parsed.connect_domains, ["https://api.example.com"]);
        assert!(parsed.resource_domains.is_empty());
        assert_eq!(parsed.domain, None);
    }

    #[test]
    fn parse_ui_resource_meta_returns_none_without_ui_object() {
        assert_eq!(super::parse_ui_resource_meta(None), None);
        for meta in [
            json!({}),
            json!({"ui": "ui://not-an-object"}),
            json!({"ui": []}),
            json!({"traceparent": "00-abc-def-01"}),
        ] {
            let map: serde_json::Map<String, serde_json::Value> = serde_json::from_value(meta).expect("meta object");
            assert_eq!(super::parse_ui_resource_meta(Some(&map)), None);
        }
    }

    #[test]
    fn parse_mcp_tool_rejects_malformed_ui_meta() {
        for meta in [
            json!({}),
            json!({"ui": "ui://not-an-object/app.html"}),
            json!({"ui": {}}),
            json!({"ui": {"resourceUri": 42}}),
            json!({"ui": {"resourceUri": ""}}),
            json!({"ui": {"resourceUri": "ui://"}}),
            json!({"ui": {"resourceUri": "file:///local/app.html"}}),
            json!({"ui/resourceUri": "https://example.com/app.html"}),
        ] {
            let parsed = parse_mcp_tool(&tool_with_meta(meta));
            assert_eq!(parsed.ui, None);
        }
    }

    #[test]
    fn parse_mcp_tool_captures_output_schema() {
        let input_schema = Arc::new(serde_json::from_value(json!({})).expect("json object"));
        let output_schema = Arc::new(
            serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "answer": {"type": "string"}
                },
                "required": ["answer"]
            }))
            .expect("json object"),
        );
        let mut tool = Tool::new_with_raw("ask_question", None, input_schema);
        tool.output_schema = Some(output_schema);

        let parsed = parse_mcp_tool(&tool);
        assert_eq!(
            parsed.output_schema,
            Some(json!({
                "type": "object",
                "properties": {
                    "answer": {"type": "string"}
                },
                "required": ["answer"]
            }))
        );
    }
}
