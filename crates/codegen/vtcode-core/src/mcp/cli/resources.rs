//! Resource inspection subcommands: `resources`, `app`.

use super::args::{AppArgs, ResourcesArgs, validate_provider_name};
use crate::cli::input_hardening::validate_agent_safe_text;
use crate::config::loader::ConfigManager;
use crate::mcp::{
    MCP_UI_RESOURCE_MIME_TYPE, MCP_UI_RESOURCE_SCHEME, McpClient, McpResourceData, McpResourceInfo, ResourceContents,
    disclose_app_resource,
};
use anyhow::{Context, Result, bail};
use serde_json::json;
use std::path::Path;
use vtcode_commons::formatting::format_size;
use vtcode_commons::sanitizer::{join_sanitized_truncated, sanitize_terminal_text};

pub(crate) async fn run_resources(args: ResourcesArgs, workspace: &Path) -> Result<()> {
    if let Some(provider) = &args.provider {
        validate_provider_name(provider)?;
    }

    let client = connect_client(workspace).await?;
    let mut resources = client.list_resources().await?;
    if let Some(provider) = &args.provider {
        resources.retain(|resource| &resource.provider == provider);
    }
    resources.sort_by(|a, b| (&a.provider, &a.uri).cmp(&(&b.provider, &b.uri)));

    if args.json {
        let payload: Vec<_> = resources.iter().map(resource_info_json).collect();
        let output = serde_json::to_string_pretty(&payload).context("failed to serialize MCP resources to JSON")?;
        println!("{output}");
        return Ok(());
    }

    if resources.is_empty() {
        println!("No MCP resources found. Use `vtcode mcp add <name> --command <binary>` to register a provider.");
        return Ok(());
    }

    print_resources_table(&resource_table_rows(&resources));
    Ok(())
}

pub(crate) async fn run_app(args: AppArgs, workspace: &Path) -> Result<()> {
    validate_resource_uri(&args.uri)?;

    let client = connect_client(workspace).await?;
    let data = client
        .read_resource(&args.uri)
        .await
        .with_context(|| format!("failed to read MCP resource '{}'", args.uri))?;

    if args.html {
        let bodies = text_bodies(&data);
        if bodies.is_empty() {
            bail!(
                "resource '{}' has no text content ({} block(s), none text); omit --html for the summary",
                args.uri,
                data.contents.len()
            );
        }
        for (index, body) in bodies.iter().enumerate() {
            if index > 0 {
                println!();
            }
            println!("{body}");
        }
        return Ok(());
    }

    if args.json {
        let output = serde_json::to_string_pretty(&app_report_json(&data))
            .context("failed to serialize MCP resource inspection to JSON")?;
        println!("{output}");
        return Ok(());
    }

    for line in app_report_lines(&data) {
        println!("{line}");
    }
    Ok(())
}

async fn connect_client(workspace: &Path) -> Result<McpClient> {
    // Read-only inspection of the user's own configured servers, explicitly
    // invoked: like the other `mcp` commands this runs unsandboxed rather than
    // deriving the session sandbox policy. Child stderr stays piped either way.
    // Workspace-layered loading mirrors what the agent runtime sees.
    let config = ConfigManager::load_from_workspace(workspace)
        .with_context(|| format!("failed to load configuration for {}", workspace.display()))?
        .config()
        .clone();
    let mut client = McpClient::new(config.mcp.clone());
    client.initialize().await?;
    Ok(client)
}

fn validate_resource_uri(uri: &str) -> Result<()> {
    if uri.trim().is_empty() {
        bail!("resource URI must not be empty");
    }
    // `validate_agent_safe_text` permits newlines for prompts; URIs never do.
    if uri.chars().any(|character| character.is_whitespace()) {
        bail!("resource URI must not contain whitespace");
    }
    validate_agent_safe_text("resource uri", uri)?;
    Ok(())
}

/// Maximum disclosed entries per domain/permission list; the remainder folds into a count.
const MAX_DISCLOSED_ENTRIES: usize = 8;

fn resource_table_rows(resources: &[McpResourceInfo]) -> Vec<[String; 5]> {
    resources
        .iter()
        .map(|resource| {
            [
                // Controls neutralized; URIs stay full-length for copy-paste into `mcp app`.
                sanitize_terminal_text(&resource.uri, usize::MAX),
                resource.provider.clone(),
                sanitize_terminal_text(&resource.name, usize::MAX),
                resource
                    .mime_type
                    .as_deref()
                    .map_or_else(|| "-".to_owned(), |mime| sanitize_terminal_text(mime, usize::MAX)),
                resource
                    .size
                    .filter(|size| *size >= 0)
                    .map_or_else(|| "-".to_owned(), |size| format_size(u64::try_from(size).unwrap_or(0))),
            ]
        })
        .collect()
}

fn resource_info_json(info: &McpResourceInfo) -> serde_json::Value {
    json!({
        "uri": info.uri,
        "provider": info.provider,
        "name": info.name,
        "description": info.description,
        "mimeType": info.mime_type,
        "size": info.size,
    })
}

fn app_report_lines(data: &McpResourceData) -> Vec<String> {
    let disclosure = disclose_app_resource(data);
    let mut lines = vec![sanitize_terminal_text(&data.uri, usize::MAX)];
    lines.push(format!("  provider: {}", data.provider));
    let mime_type = disclosure.mime_type.as_deref().unwrap_or("unknown");
    lines.push(format!(
        "  type: {} ({})",
        sanitize_terminal_text(mime_type, usize::MAX),
        format_size(disclosure.bytes)
    ));
    lines.push(format!("  blocks: {}", disclosure.blocks));
    if data.uri.starts_with(MCP_UI_RESOURCE_SCHEME)
        && disclosure.mime_type.as_deref() != Some(MCP_UI_RESOURCE_MIME_TYPE)
    {
        lines.push(format!("  note: ui:// resources should serve {MCP_UI_RESOURCE_MIME_TYPE}"));
    }
    if let Some(ui) = &disclosure.ui {
        if !ui.permissions.is_empty() {
            lines.push(format!(
                "  permissions: {}",
                join_sanitized_truncated(&ui.permissions, MAX_DISCLOSED_ENTRIES, usize::MAX)
            ));
        }
        for (label, domains) in [
            ("csp.connect", &ui.connect_domains),
            ("csp.resource", &ui.resource_domains),
            ("csp.frame", &ui.frame_domains),
            ("csp.base-uri", &ui.base_uri_domains),
        ] {
            if !domains.is_empty() {
                lines.push(format!(
                    "  {label}: {}",
                    join_sanitized_truncated(domains, MAX_DISCLOSED_ENTRIES, usize::MAX)
                ));
            }
        }
        if let Some(domain) = &ui.domain {
            lines.push(format!("  domain: {}", sanitize_terminal_text(domain, usize::MAX)));
        }
    }
    lines
}

fn app_report_json(data: &McpResourceData) -> serde_json::Value {
    let disclosure = disclose_app_resource(data);
    json!({
        "uri": disclosure.uri,
        "provider": data.provider,
        "mimeType": disclosure.mime_type,
        "bytes": disclosure.bytes,
        "blocks": disclosure.blocks,
        "ui": disclosure.ui,
    })
}

fn text_bodies(data: &McpResourceData) -> Vec<&str> {
    data.contents
        .iter()
        .filter_map(|content| match content {
            ResourceContents::TextResourceContents { text, .. } => Some(text.as_str()),
            ResourceContents::BlobResourceContents { .. } | _ => None,
        })
        .collect()
}

fn print_resources_table(rows: &[[String; 5]]) {
    let mut widths = ["URI".len(), "Provider".len(), "Name".len(), "Type".len(), "Size".len()];

    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row.iter()) {
            *width = (*width).max(cell.len());
        }
    }

    let [uri_w, provider_w, name_w, type_w, size_w] = widths;

    println!(
        "{uri:<uri_w$}  {provider:<provider_w$}  {name:<name_w$}  {type:<type_w$}  {size:<size_w$}",
        uri = "URI",
        provider = "Provider",
        name = "Name",
        type = "Type",
        size = "Size",
    );

    for row in rows {
        println!(
            "{uri:<uri_w$}  {provider:<provider_w$}  {name:<name_w$}  {type:<type_w$}  {size:<size_w$}",
            uri = row[0],
            provider = row[1],
            name = row[2],
            type = row[3],
            size = row[4],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        app_report_json, app_report_lines, resource_info_json, resource_table_rows, text_bodies, validate_resource_uri,
    };
    use crate::mcp::{McpResourceData, McpResourceInfo, MetaObject, ResourceContents};
    use serde_json::json;

    fn resource_info(uri: &str) -> McpResourceInfo {
        McpResourceInfo {
            provider: "demo".to_string(),
            uri: uri.to_string(),
            name: "Demo".to_string(),
            description: None,
            mime_type: None,
            size: None,
        }
    }

    fn text_content(text: &str, mime_type: Option<&str>, meta: Option<serde_json::Value>) -> ResourceContents {
        ResourceContents::TextResourceContents {
            uri: "ui://demo/app.html".to_string(),
            mime_type: mime_type.map(str::to_owned),
            text: text.to_string(),
            meta: meta.map(|value| MetaObject(serde_json::from_value(value).expect("meta object"))),
        }
    }

    fn resource_data(uri: &str, contents: Vec<ResourceContents>) -> McpResourceData {
        McpResourceData {
            provider: "demo".to_string(),
            uri: uri.to_string(),
            contents,
            meta: serde_json::Map::new(),
        }
    }

    #[test]
    fn validate_resource_uri_accepts_schemes_and_rejects_unsafe_text() {
        for uri in [
            "ui://demo/app.html",
            "file:///report.txt",
            "https://example.com/manual?x=1#frag",
        ] {
            validate_resource_uri(uri).expect("valid uri");
        }
        for uri in [
            "",
            "   ",
            "bad\u{0000}uri",
            "bad\u{0007}uri",
            "ui://bad\nuri",
            "ui://bad uri",
        ] {
            validate_resource_uri(uri).expect_err("invalid uri");
        }
    }

    #[test]
    fn resource_table_rows_render_missing_and_negative_metadata_as_dash() {
        let mut full = resource_info("ui://demo/app.html");
        full.mime_type = Some("text/html;profile=mcp-app".to_string());
        full.size = Some(4312);
        let mut negative = resource_info("file:///odd.bin");
        negative.size = Some(-5);
        let rows = resource_table_rows(&[full, resource_info("file:///plain.txt"), negative]);

        assert_eq!(
            rows[0],
            [
                "ui://demo/app.html".to_string(),
                "demo".to_string(),
                "Demo".to_string(),
                "text/html;profile=mcp-app".to_string(),
                "4.2KB".to_string(),
            ]
        );
        assert_eq!(rows[1][3], "-");
        assert_eq!(rows[1][4], "-");
        assert_eq!(rows[2][4], "-");
    }

    #[test]
    fn resource_info_json_preserves_optional_fields() {
        let mut info = resource_info("file:///report.txt");
        info.description = Some("Report".to_string());
        info.mime_type = Some("text/plain".to_string());
        info.size = Some(12);

        let value = resource_info_json(&info);
        assert_eq!(value["uri"], "file:///report.txt");
        assert_eq!(value["mimeType"], "text/plain");
        assert_eq!(value["size"], 12);
        assert_eq!(resource_info_json(&resource_info("file:///x"))["mimeType"], serde_json::Value::Null);
    }

    #[test]
    fn app_report_lines_disclose_ui_posture_for_app_resource() {
        let data = resource_data(
            "ui://demo/app.html",
            vec![text_content(
                "<!doctype html>",
                Some("text/html;profile=mcp-app"),
                Some(json!({
                    "ui": {
                        "csp": {"connectDomains": ["https://api.example.com"]},
                        "permissions": {"camera": {}},
                    }
                })),
            )],
        );

        let lines = app_report_lines(&data);
        assert_eq!(lines[0], "ui://demo/app.html");
        assert!(lines.contains(&"  provider: demo".to_string()), "{lines:?}");
        assert!(lines.iter().any(|line| line.starts_with("  type: text/html;profile=mcp-app (")), "{lines:?}");
        assert!(lines.contains(&"  blocks: 1".to_string()), "{lines:?}");
        assert!(lines.contains(&"  permissions: camera".to_string()), "{lines:?}");
        assert!(lines.contains(&"  csp.connect: https://api.example.com".to_string()), "{lines:?}");
        assert!(!lines.iter().any(|line| line.starts_with("  note:")), "{lines:?}");
    }

    #[test]
    fn app_report_lines_warn_when_ui_scheme_serves_wrong_mime() {
        let data = resource_data("ui://demo/app.html", vec![text_content("<!doctype html>", Some("text/plain"), None)]);

        let lines = app_report_lines(&data);
        assert!(lines.iter().any(|line| line.contains("should serve text/html;profile=mcp-app")), "{lines:?}");
    }

    #[test]
    fn app_report_lines_omit_ui_section_without_meta() {
        let data = resource_data("file:///report.txt", vec![text_content("body", Some("text/plain"), None)]);

        let lines = app_report_lines(&data);
        assert!(!lines.iter().any(|line| line.contains("permissions:")), "{lines:?}");
        assert!(!lines.iter().any(|line| line.contains("csp.")), "{lines:?}");
        assert!(!lines.iter().any(|line| line.starts_with("  note:")), "{lines:?}");
    }

    #[test]
    fn app_report_json_carries_metadata_without_body() {
        let data = resource_data(
            "ui://demo/app.html",
            vec![text_content("SECRET_BODY", Some("text/html;profile=mcp-app"), None)],
        );

        let value = app_report_json(&data);
        assert_eq!(value["uri"], "ui://demo/app.html");
        assert_eq!(value["mimeType"], "text/html;profile=mcp-app");
        assert_eq!(value["bytes"], "SECRET_BODY".len());
        assert_eq!(value["blocks"], 1);
        assert_eq!(value["ui"], serde_json::Value::Null);
        assert!(!value.to_string().contains("SECRET_BODY"));
    }

    #[test]
    fn text_bodies_skips_blob_blocks() {
        let data = resource_data(
            "file:///mixed.bin",
            vec![
                ResourceContents::BlobResourceContents {
                    uri: "file:///mixed.bin".to_string(),
                    mime_type: None,
                    blob: "QUJD".to_string(),
                    meta: None,
                },
                text_content("hello", None, None),
            ],
        );

        assert_eq!(text_bodies(&data), ["hello"]);
    }

    #[test]
    fn display_joins_neutralize_control_characters() {
        use vtcode_commons::sanitizer::join_sanitized_truncated;

        let items = vec!["api.example.com\nINJECTED".to_string(), "ok.example.com".to_string()];
        assert_eq!(join_sanitized_truncated(&items, 8, usize::MAX), "api.example.com�INJECTED, ok.example.com");

        let mut info = resource_info("ui://demo/\u{0007}app.html");
        info.name = "Na\nme".to_string();
        info.mime_type = Some("text/html\r\n".to_string());
        let rows = resource_table_rows(&[info]);
        assert_eq!(rows[0][0], "ui://demo/�app.html");
        assert_eq!(rows[0][2], "Na�me");
        assert_eq!(rows[0][3], "text/html��");
    }
}
