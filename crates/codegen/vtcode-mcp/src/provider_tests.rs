use super::mcp_tool_call_span;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tracing_subscriber::{fmt::format::FmtSpan, prelude::*};
use vtcode_config::mcp::{McpHttpServerConfig, McpStdioServerConfig, McpTransportConfig};

/// Minimal clonable buffered writer for tests. Implements `Write` so it can
/// be passed to `tracing_subscriber::fmt::layer().with_writer(..)`.
#[derive(Clone)]
struct TestWriter(Arc<Mutex<BufWriter<File>>>);

impl TestWriter {
    fn open(path: &std::path::Path) -> Self {
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .expect("open trace log");
        Self(Arc::new(Mutex::new(BufWriter::new(file))))
    }

    fn flush(&self) {
        if let Ok(mut w) = self.0.lock() {
            drop(w.flush());
        }
    }
}

impl Write for TestWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().map_err(|e| std::io::Error::other(e.to_string()))?.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().map_err(|e| std::io::Error::other(e.to_string()))?.flush()
    }
}

#[test]
fn mcp_tool_call_span_records_http_metadata() {
    let tempdir = tempdir().expect("tempdir");
    let log_file = tempdir.path().join("trace.log");
    let writer = TestWriter::open(&log_file);
    let writer_for_layer = writer.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_writer(move || writer_for_layer.clone())
            .with_span_events(FmtSpan::FULL)
            .with_ansi(false),
    );
    let _guard = tracing::subscriber::set_default(subscriber);

    {
        let span = mcp_tool_call_span(
            "calendar",
            "get_events",
            &McpTransportConfig::Http(McpHttpServerConfig {
                endpoint: "https://example.com:8443/mcp".to_string(),
                api_key_env: None,
                oauth: None,
                protocol_version: "2024-11-05".to_string(),
                handshake: McpHttpHandshakeMode::Legacy,
                http_headers: Default::default(),
                env_http_headers: Default::default(),
            }),
        );
        let _entered = span.enter();
    }

    writer.flush();
    let logs = fs::read_to_string(&log_file).expect("trace log");
    assert!(logs.contains("mcp.tools.call"));
    assert!(logs.contains("provider=\"calendar\""));
    assert!(logs.contains("tool=\"get_events\""));
    assert!(logs.contains("rpc_system=\"jsonrpc\""));
    assert!(logs.contains("rpc_method=\"tools/call\""));
    assert!(logs.contains("transport=\"streamable_http\""));
    assert!(logs.contains("server_address=\"example.com\""));
    assert!(logs.contains("server_port=8443"));
}

#[test]
fn mcp_tool_call_span_defaults_stdio_transport() {
    let span = mcp_tool_call_span(
        "filesystem",
        "read_file",
        &McpTransportConfig::Stdio(McpStdioServerConfig {
            command: "rmcp-server".to_string(),
            args: Vec::new(),
            working_directory: None,
        }),
    );

    assert_eq!(span.metadata().expect("metadata").name(), "mcp.tools.call");
}

use rmcp::model::{ProtocolVersion, ServerCapabilities, ServerPeerInfo};
use rmcp::service::ClientLifecycleMode;
use vtcode_config::mcp::{McpHttpHandshakeMode, McpProviderConfig, McpSandboxOverrideConfig};

async fn stdio_provider(name: &str) -> super::McpProvider {
    let config = McpProviderConfig {
        name: name.to_string(),
        transport: McpTransportConfig::Stdio(McpStdioServerConfig {
            command: "true".to_string(),
            args: Vec::new(),
            working_directory: None,
        }),
        ..McpProviderConfig::default()
    };
    super::McpProvider::connect(config, None, None, false)
        .await
        .expect("stdio provider connects")
}

async fn http_provider(name: &str, handshake: McpHttpHandshakeMode) -> super::McpProvider {
    let config = McpProviderConfig {
        name: name.to_string(),
        transport: McpTransportConfig::Http(McpHttpServerConfig {
            endpoint: "https://example.com/mcp".to_string(),
            handshake,
            ..McpHttpServerConfig::default()
        }),
        ..McpProviderConfig::default()
    };
    super::McpProvider::connect(config, None, None, false)
        .await
        .expect("http provider connects")
}

#[tokio::test]
async fn connect_rejects_sandbox_overrides_without_session_context() {
    let mut config = McpProviderConfig {
        name: "sandboxed".to_string(),
        transport: McpTransportConfig::Stdio(McpStdioServerConfig {
            command: "true".to_string(),
            args: Vec::new(),
            working_directory: None,
        }),
        ..McpProviderConfig::default()
    };
    config.sandbox = Some(McpSandboxOverrideConfig {
        max_memory_mb: Some(256),
        ..McpSandboxOverrideConfig::default()
    });
    let err = super::McpProvider::connect(config, None, None, false)
        .await
        .err()
        .expect("overrides without session context must fail before spawn");
    let message = format!("{err:#}");
    assert!(message.contains("unsatisfiable sandbox overrides"), "unexpected error: {message}");
    assert!(message.contains("session sandboxing is disabled"), "unexpected error: {message}");
}

#[tokio::test]
async fn handshake_lifecycle_defaults_to_legacy_initialize_for_http() {
    let provider = http_provider("legacy", McpHttpHandshakeMode::Legacy).await;
    assert!(matches!(provider.handshake_lifecycle(), ClientLifecycleMode::Initialize));
}

#[tokio::test]
async fn handshake_lifecycle_uses_auto_for_stdio_and_opt_in_http() {
    let stdio = stdio_provider("local").await;
    match stdio.handshake_lifecycle() {
        ClientLifecycleMode::Auto { preferred_versions, legacy_version } => {
            let versions: Vec<String> = preferred_versions.iter().map(ToString::to_string).collect();
            assert_eq!(versions, vec!["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"]);
            assert_eq!(legacy_version.map(|version| version.to_string()), Some("2024-11-05".to_string()));
        }
        other => panic!("expected Auto lifecycle, got {other:?}"),
    }

    let modern = http_provider("modern", McpHttpHandshakeMode::Auto).await;
    assert!(matches!(modern.handshake_lifecycle(), ClientLifecycleMode::Auto { .. }));
}

#[tokio::test]
async fn negotiated_protocol_version_tracks_last_handshake() {
    let provider = http_provider("probe", McpHttpHandshakeMode::Legacy).await;
    assert_eq!(provider.negotiated_protocol_version().await, None);

    *provider.initialize_result.lock().await =
        Some(ServerPeerInfo::new(ProtocolVersion::V_2025_11_25, ServerCapabilities::default()));
    assert_eq!(provider.negotiated_protocol_version().await.as_deref(), Some("2025-11-25"));
}

#[test]
fn filtered_catalogs_are_sorted_regardless_of_server_order() {
    use super::{filter_prompts_sorted, filter_resources_sorted, filter_tools_sorted};
    use rmcp::model::{Prompt, Resource, Tool};
    use serde_json::Map;
    use std::sync::Arc;
    use vtcode_config::mcp::McpAllowListConfig;

    let allowlist = McpAllowListConfig::default();
    // Uppercase sorts before lowercase and a prefix sorts before its extension in byte order.
    let tool_names = ["search_v2", "Zeta", "search", "alpha"];
    let tools = tool_names
        .iter()
        .map(|name| Tool::new(*name, "desc", Arc::new(Map::new())))
        .collect();
    let sorted_tools: Vec<String> = filter_tools_sorted("p", tools, &allowlist)
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert_eq!(sorted_tools, ["Zeta", "alpha", "search", "search_v2"]);

    let resources = ["file:///b", "file:///a/z", "file:///a"]
        .iter()
        .map(|uri| Resource::new(*uri, "r"))
        .collect();
    let sorted_resources: Vec<String> = filter_resources_sorted("p", resources, &allowlist)
        .into_iter()
        .map(|resource| resource.uri)
        .collect();
    assert_eq!(sorted_resources, ["file:///a", "file:///a/z", "file:///b"]);

    let prompts = ["review", "Explain", "review2"]
        .iter()
        .map(|name| Prompt::new(*name, None::<String>, None))
        .collect();
    let sorted_prompts: Vec<String> = filter_prompts_sorted("p", prompts, &allowlist)
        .into_iter()
        .map(|prompt| prompt.name)
        .collect();
    assert_eq!(sorted_prompts, ["Explain", "review", "review2"]);
}

#[test]
fn reconnect_initialize_params_gate_ui_extension_on_flag() {
    let advertised = super::build_reconnect_initialize_params(true);
    assert!(
        advertised
            .capabilities
            .extensions
            .as_ref()
            .is_some_and(|extensions| extensions.contains_key("io.modelcontextprotocol/ui")),
    );
    let silent = super::build_reconnect_initialize_params(false);
    assert_eq!(silent.capabilities.extensions, None);
}

#[tokio::test]
async fn connect_retains_ui_advertisement_flag_for_reconnects() {
    let config = McpProviderConfig {
        name: "flagged".to_string(),
        transport: McpTransportConfig::Stdio(McpStdioServerConfig {
            command: "true".to_string(),
            args: Vec::new(),
            working_directory: None,
        }),
        ..McpProviderConfig::default()
    };
    let provider = super::McpProvider::connect(config, None, None, true)
        .await
        .expect("stdio provider connects");
    assert!(provider.advertise_ui_apps);
}
