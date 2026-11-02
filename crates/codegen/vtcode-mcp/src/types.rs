//! Core MCP data types for tool, resource, and prompt information.

use rmcp::model::{ElicitationAction, PromptArgument, PromptMessage, ResourceContents};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub use vtcode_utility_tool_specs::{
    MCP_UI_EXTENSION_ID, MCP_UI_RESOURCE_MIME_TYPE, MCP_UI_RESOURCE_SCHEME, McpToolVisibility, McpUiResourceMeta,
    McpUiToolMeta, parse_ui_resource_meta,
};

/// Information about an MCP tool exposed by a provider.
#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub provider: String,
    pub input_schema: Value,
    /// Optional output schema (`outputSchema`), when the server advertises
    /// the structure of the tool's result. Carried for discovery fidelity so
    /// the Full detail level returns the complete definition; execution paths
    /// treat it as advisory metadata.
    pub output_schema: Option<Value>,
    /// MCP Apps linkage (`_meta.ui.resourceUri`), when the server declares a
    /// `ui://` UI resource for this tool. Advisory display metadata only.
    pub ui: Option<McpUiToolMeta>,
    /// Tool accessibility between model and app (`_meta.ui.visibility`).
    /// Model-facing listings hide tools without model visibility.
    pub visibility: McpToolVisibility,
}

/// Summary of an MCP resource exposed by a provider.
#[derive(Debug, Clone)]
pub struct McpResourceInfo {
    /// Provider that exposed the resource.
    pub provider: String,
    /// Canonical resource URI.
    pub uri: String,
    /// Human-readable resource name.
    pub name: String,
    /// Optional resource description.
    pub description: Option<String>,
    /// Advertised MIME type, when the server provides one.
    pub mime_type: Option<String>,
    /// Advertised size in bytes, when the server provides one.
    pub size: Option<i64>,
}

/// Resource contents fetched from an MCP provider.
///
/// Per-content `_meta` (including MCP Apps `_meta.ui`) is preserved on each
/// [`ResourceContents`] entry; `meta` carries the result-envelope metadata.
#[derive(Debug, Clone)]
pub struct McpResourceData {
    /// Provider the resource was read from.
    pub provider: String,
    /// URI that was read.
    pub uri: String,
    /// Content blocks returned by `resources/read`.
    pub contents: Vec<ResourceContents>,
    /// Envelope `_meta` from the read result.
    pub meta: Map<String, Value>,
}

/// Summary of an MCP prompt exposed by a provider.
#[derive(Debug, Clone)]
pub struct McpPromptInfo {
    pub(crate) provider: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) arguments: Vec<PromptArgument>,
}

/// Fully rendered MCP prompt ready for use.
#[derive(Debug, Clone)]
pub struct McpPromptDetail {
    pub(crate) provider: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) messages: Vec<PromptMessage>,
    pub(crate) meta: Map<String, Value>,
}

/// Snapshot describing the MCP client at runtime.
#[derive(Debug, Clone)]
pub struct McpClientStatus {
    pub enabled: bool,
    pub provider_count: usize,
    pub active_connections: usize,
    pub configured_providers: Vec<String>,
}

/// Request payload for handling elicitation prompts from MCP providers.
#[derive(Debug, Clone)]
pub struct McpElicitationRequest {
    pub message: String,
    pub requested_schema: Value,
    pub(crate) meta: Option<Value>,
}

/// Result returned by an elicitation handler after interacting with the user.
#[derive(Debug, Clone)]
pub struct McpElicitationResponse {
    pub action: ElicitationAction,
    pub content: Option<Value>,
    pub meta: Option<Value>,
}

// === File Upload Types (PR #15197) ===

/// Metadata key used in tool input schemas to indicate file upload parameters.
pub const OPENAI_FILE_PARAMS_META_KEY: &str = "_meta";
pub const OPENAI_FILE_PARAMS_VALUE: &str = "openai/fileParams";

/// Shape of a file payload provided by the client after upload.
///
/// This is the object shape that MCP tools expecting file uploads expect to receive.
/// The client uploads local files to the backend and replaces the local file path
/// with this structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidedFilePayload {
    /// The file ID returned from the upload (maps to backend file reference)
    #[serde(rename = "fileId")]
    file_id: String,
    /// The original file name
    #[serde(rename = "fileName")]
    file_name: String,
    /// MIME type of the uploaded file
    #[serde(rename = "mimeType")]
    mime_type: String,
    /// File size in bytes
    #[serde(rename = "fileSize")]
    file_size: i64,
}

/// Result of uploading a local file to the backend.
#[derive(Debug, Clone)]
pub struct FileUploadResult {
    /// The file ID from the backend upload
    file_id: String,
    /// The payload to substitute in the tool call arguments
    payload: ProvidedFilePayload,
}

/// Masked schema entry for a file parameter in a tool's input schema.
///
/// When a tool declares `_meta["openai/fileParams"]` in its input schema,
/// we mask the raw file payload object and instead instruct the model to
/// provide an absolute local file path. The client then handles the upload
/// and argument rewriting transparently.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileParamSchemaEntry {
    /// The original property name in the tool's input schema
    property_name: String,
    /// Description shown to the model for this file parameter
    description: String,
    /// Whether this file parameter is required
    required: bool,
}

/// Tool-result key carrying per-URI app disclosure (`{uri: McpAppDisclosure}`).
///
/// Attached best-effort after execution when a result references `ui://`
/// resources; absent when nothing was fetched. Surfaces render the linkage
/// row alone when the key (or an entry) is missing.
pub const MCP_APP_DISCLOSURE_KEY: &str = "uiDisclosure";

/// Tool-result key listing app URIs refused by the `off` apps tier (`[String]`).
///
/// Attached (without any resource fetch) when a result references `ui://`
/// resources while `mcp.ui.apps` is `off`; absent otherwise. The runner maps
/// it to a sandbox-denied event. Like [`MCP_APP_DISCLOSURE_KEY`], display-only:
/// never part of model context.
pub const MCP_APP_SUPPRESSED_KEY: &str = "uiSuppressed";

/// Compact disclosure facts about an MCP resource for app presentation surfaces.
///
/// Built by [`disclose_app_resource`] from a `resources/read` result. Surfaces
/// render from these facts instead of re-parsing content blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAppDisclosure {
    /// URI that was read.
    pub uri: String,
    /// MIME type of the first content block, when the server provides one.
    pub mime_type: Option<String>,
    /// Total body bytes across content blocks (base64 length for blobs).
    pub bytes: u64,
    /// Number of content blocks.
    pub blocks: usize,
    /// Declared UI sandbox posture, when any block (or the envelope) carries `_meta.ui`.
    pub ui: Option<McpUiResourceMeta>,
}

/// Derive disclosure facts from a resource read result.
///
/// The UI posture comes from the first content block carrying `_meta.ui`,
/// falling back to the result envelope.
#[must_use]
pub fn disclose_app_resource(data: &McpResourceData) -> McpAppDisclosure {
    McpAppDisclosure {
        uri: data.uri.clone(),
        mime_type: data.contents.first().and_then(|content| match content {
            ResourceContents::TextResourceContents { mime_type, .. } => mime_type.clone(),
            ResourceContents::BlobResourceContents { mime_type, .. } => mime_type.clone(),
            _ => None,
        }),
        bytes: data
            .contents
            .iter()
            .map(|content| match content {
                ResourceContents::TextResourceContents { text, .. } => text.len(),
                ResourceContents::BlobResourceContents { blob, .. } => blob.len(),
                _ => 0,
            })
            .fold(0u64, |total, len| total.saturating_add(u64::try_from(len).unwrap_or(u64::MAX))),
        blocks: data.contents.len(),
        ui: data
            .contents
            .iter()
            .find_map(|content| {
                let meta = match content {
                    ResourceContents::TextResourceContents { meta, .. } => meta,
                    ResourceContents::BlobResourceContents { meta, .. } => meta,
                    _ => return None,
                };
                parse_ui_resource_meta(meta.as_ref().map(|meta| &meta.0))
            })
            .or_else(|| parse_ui_resource_meta(Some(&data.meta))),
    }
}

#[cfg(test)]
mod tests {
    use super::{Map, McpResourceData, Value, disclose_app_resource};
    use rmcp::model::{MetaObject, ResourceContents};
    use serde_json::json;

    fn text_block(text: &str, mime_type: Option<&str>, meta: Option<Value>) -> ResourceContents {
        ResourceContents::TextResourceContents {
            uri: "ui://demo/app.html".to_string(),
            mime_type: mime_type.map(str::to_owned),
            text: text.to_string(),
            meta: meta.map(|value| MetaObject(serde_json::from_value(value).expect("meta object"))),
        }
    }

    fn resource_data(contents: Vec<ResourceContents>) -> McpResourceData {
        McpResourceData {
            provider: "demo".to_string(),
            uri: "ui://demo/app.html".to_string(),
            contents,
            meta: Map::new(),
        }
    }

    #[test]
    fn disclosure_derives_mime_bytes_and_ui_posture() {
        let data = resource_data(vec![text_block(
            "<!doctype html>",
            Some("text/html;profile=mcp-app"),
            Some(json!({"ui": {"permissions": {"camera": {}}}})),
        )]);

        let disclosure = disclose_app_resource(&data);
        assert_eq!(disclosure.uri, "ui://demo/app.html");
        assert_eq!(disclosure.mime_type.as_deref(), Some("text/html;profile=mcp-app"));
        assert_eq!(disclosure.bytes, u64::try_from("<!doctype html>".len()).expect("small fixture"));
        assert_eq!(disclosure.blocks, 1);
        assert_eq!(disclosure.ui.as_ref().expect("ui posture").permissions, ["camera".to_string()]);

        let value = serde_json::to_value(&disclosure).expect("serialize disclosure");
        assert_eq!(value["mimeType"], "text/html;profile=mcp-app");
        assert_eq!(value["ui"]["permissions"], json!(["camera"]));
    }

    #[test]
    fn disclosure_prefers_first_block_with_ui_and_counts_blobs() {
        let data = resource_data(vec![
            ResourceContents::BlobResourceContents {
                uri: "ui://demo/app.html".to_string(),
                mime_type: None,
                blob: "QUJD".to_string(),
                meta: None,
            },
            text_block("b", None, Some(json!({"ui": {"permissions": {"microphone": {}}}}))),
        ]);

        let disclosure = disclose_app_resource(&data);
        assert_eq!(disclosure.mime_type, None);
        let expected = u64::try_from("QUJD".len() + "b".len()).expect("small fixture");
        assert_eq!(disclosure.bytes, expected);
        assert_eq!(disclosure.blocks, 2);
        assert_eq!(disclosure.ui.as_ref().expect("ui posture").permissions, ["microphone".to_string()]);
    }

    #[test]
    fn disclosure_without_ui_has_no_posture() {
        let data = resource_data(vec![text_block("body", Some("text/plain"), None)]);
        let disclosure = disclose_app_resource(&data);
        assert_eq!(disclosure.ui, None);
        assert_eq!(disclosure.blocks, 1);

        let empty = resource_data(Vec::new());
        let disclosure = disclose_app_resource(&empty);
        assert_eq!(disclosure.mime_type, None);
        assert_eq!(disclosure.bytes, 0);
        assert_eq!(disclosure.blocks, 0);
        assert_eq!(disclosure.ui, None);
    }
}
