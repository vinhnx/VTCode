//! Bounded MCP app-resource reads for desktop static previews.
//!
//! Mirrors `vtcode mcp app`: workspace-layered configuration, a per-call client,
//! and one `resources/read`. The service holds no long-lived MCP connections,
//! and previews are explicit on-expand fetches, so per-call construction keeps
//! this honest without a client cache (a future step if profiling demands it).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use vtcode_core::cli::input_hardening::validate_agent_safe_text;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::mcp::{MCP_UI_RESOURCE_SCHEME, McpClient, McpResourceData, ResourceContents};

use super::protocol;

const MAX_URI_BYTES: usize = 2048;
const MAX_REQUEST_ID_BYTES: usize = 128;

pub(super) async fn read(workspace: PathBuf, params: protocol::McpAppReadParams) -> Result<protocol::McpAppReadResult> {
    validate_params(&params)?;
    let data = read_resource(&workspace, &params.uri).await?;
    Ok(bound_body(&params, &data))
}

fn validate_params(params: &protocol::McpAppReadParams) -> Result<()> {
    if params.request_id.is_empty() || params.request_id.len() > MAX_REQUEST_ID_BYTES {
        bail!("invalid MCP app read request identity");
    }
    if params.uri.trim().is_empty() || params.uri.len() > MAX_URI_BYTES {
        bail!("invalid MCP app resource URI");
    }
    if !params.uri.starts_with(MCP_UI_RESOURCE_SCHEME) {
        bail!("MCP app reads require a `ui://` resource URI");
    }
    // `validate_agent_safe_text` permits newlines for prompts; URIs never do.
    if params.uri.chars().any(|character| character.is_whitespace()) {
        bail!("MCP app resource URI must not contain whitespace");
    }
    validate_agent_safe_text("resource uri", &params.uri)?;
    Ok(())
}

async fn read_resource(workspace: &Path, uri: &str) -> Result<McpResourceData> {
    let config = ConfigManager::load_from_workspace(workspace)
        .with_context(|| format!("failed to load configuration for {}", workspace.display()))?
        .config()
        .clone();
    let mut client = McpClient::new(config.mcp.clone());
    client.initialize().await?;
    client
        .read_resource(uri)
        .await
        .with_context(|| format!("failed to read MCP resource '{uri}'"))
}

/// Concatenate text blocks up to [`protocol::MAX_MCP_APP_BODY_BYTES`].
///
/// Blobs carry no previewable text and contribute nothing to the body. The cut
/// lands on a UTF-8 boundary so the preview never ends mid-character.
fn bound_body(params: &protocol::McpAppReadParams, data: &McpResourceData) -> protocol::McpAppReadResult {
    let mut body = String::new();
    let mut total_bytes: u64 = 0;
    let mut mime_type = None;
    for content in &data.contents {
        let ResourceContents::TextResourceContents { text, mime_type: mime, .. } = content else {
            continue;
        };
        if mime_type.is_none() {
            mime_type = mime.clone();
        }
        total_bytes = total_bytes.saturating_add(u64::try_from(text.len()).unwrap_or(u64::MAX));
        if body.len() >= protocol::MAX_MCP_APP_BODY_BYTES {
            continue;
        }
        let take = protocol::MAX_MCP_APP_BODY_BYTES.saturating_sub(body.len()).min(text.len());
        body.push_str(&text[..text.floor_char_boundary(take)]);
    }
    let truncated = total_bytes > u64::try_from(body.len()).unwrap_or(u64::MAX);
    protocol::McpAppReadResult {
        uri: params.uri.clone(),
        request_id: params.request_id.clone(),
        mime_type,
        body,
        total_bytes,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_REQUEST_ID_BYTES, MAX_URI_BYTES, bound_body, validate_params};
    use serde_json::Map;
    use vtcode_core::mcp::{McpResourceData, ResourceContents};

    fn params(uri: &str) -> super::protocol::McpAppReadParams {
        super::protocol::McpAppReadParams {
            uri: uri.to_string(),
            request_id: "app-1".to_string(),
        }
    }

    fn text_block(text: &str, mime_type: Option<&str>) -> ResourceContents {
        ResourceContents::TextResourceContents {
            uri: "ui://demo/app.html".to_string(),
            mime_type: mime_type.map(str::to_owned),
            text: text.to_string(),
            meta: None,
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
    fn validate_params_accepts_ui_uri_and_rejects_the_rest() {
        validate_params(&params("ui://demo/app.html")).expect("valid ui uri");
        for uri in [
            "",
            "   ",
            "file:///plain.txt",
            "https://example.com/app",
            "ui://bad\nuri",
        ] {
            validate_params(&params(uri)).expect_err("invalid uri");
        }
        let oversized = format!("ui://x/{}", "a".repeat(MAX_URI_BYTES));
        validate_params(&params(&oversized)).expect_err("oversized uri");
        let mut bad_id = params("ui://demo/app.html");
        bad_id.request_id.clear();
        validate_params(&bad_id).expect_err("empty request id");
        bad_id.request_id = "r".repeat(MAX_REQUEST_ID_BYTES + 1);
        validate_params(&bad_id).expect_err("oversized request id");
    }

    #[test]
    fn bound_body_concatenates_text_blocks_and_reports_totals() {
        let data = resource_data(vec![
            text_block("<h1>", Some("text/html;profile=mcp-app")),
            text_block("Hi", Some("text/plain")),
            ResourceContents::BlobResourceContents {
                uri: "ui://demo/app.html".to_string(),
                mime_type: Some("image/png".to_string()),
                blob: "QUJD".to_string(),
                meta: None,
            },
        ]);

        let result = bound_body(&params("ui://demo/app.html"), &data);
        assert_eq!(result.body, "<h1>Hi");
        assert_eq!(result.mime_type.as_deref(), Some("text/html;profile=mcp-app"));
        assert_eq!(result.total_bytes, 6);
        assert!(!result.truncated);
    }

    #[test]
    fn bound_body_truncates_on_char_boundary_and_flags_it() {
        // Multibyte tail: the cut must not split the final ellipsis.
        let text = format!("{}…", "a".repeat(super::protocol::MAX_MCP_APP_BODY_BYTES));
        let data = resource_data(vec![text_block(&text, None)]);

        let result = bound_body(&params("ui://demo/app.html"), &data);
        assert_eq!(result.body.len(), super::protocol::MAX_MCP_APP_BODY_BYTES);
        assert!(result.body.is_char_boundary(result.body.len()));
        assert!(!result.body.ends_with('…'));
        assert_eq!(result.total_bytes, u64::try_from(text.len()).expect("small body"));
        assert!(result.truncated);
        assert_eq!(result.mime_type, None);
    }

    #[test]
    fn bound_body_without_text_blocks_is_empty_but_counted() {
        let data = resource_data(vec![ResourceContents::BlobResourceContents {
            uri: "ui://demo/app.html".to_string(),
            mime_type: None,
            blob: "QUJD".to_string(),
            meta: None,
        }]);

        let result = bound_body(&params("ui://demo/app.html"), &data);
        assert!(result.body.is_empty());
        assert_eq!(result.total_bytes, 0);
        assert!(!result.truncated);
    }
}
