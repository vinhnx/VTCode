use hashbrown::HashMap;
use std::path::Path;
use tracing::warn;

use vtcode_agent_plugins::{LoadedPlugin, plugin_roots_for};
use vtcode_config::mcp::McpProviderConfig;

pub fn discover_plugin_mcp_providers(workspace_root: &Path) -> Vec<McpProviderConfig> {
    let mut providers = Vec::new();

    let roots = plugin_roots_for(workspace_root);

    // Deduplicate roots so a workspace under the home directory does not cause
    // the same plugin root to be scanned twice.
    let mut seen = std::collections::HashSet::new();
    let mut unique_roots = Vec::new();
    for root in roots {
        if seen.insert(root.clone()) {
            unique_roots.push(root);
        }
    }

    for root in unique_roots {
        if !root.is_dir() {
            continue;
        }
        let entries = match std::fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::debug!(root = %root.display(), error = %e, "failed to read plugin root directory");
                continue;
            }
        };
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            match LoadedPlugin::load_from_dir(&path) {
                Ok(loaded) => {
                    if let Some(mcp_config) = loaded.mcp {
                        for (server_name, server) in mcp_config.servers {
                            match map_server_to_provider(&loaded.manifest.name, &server_name, server, &loaded.root) {
                                Ok(provider) => providers.push(provider),
                                Err(e) => {
                                    warn!(plugin = %loaded.manifest.name, server = %server_name, error = %e, "skipping plugin MCP server")
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(root = %path.display(), error = %e, "failed to load agent plugin for MCP discovery");
                }
            }
        }
    }

    providers
}

fn map_server_to_provider(
    plugin_name: &str,
    server_name: &str,
    server: vtcode_agent_plugins::ServerConfig,
    plugin_root: &Path,
) -> Result<McpProviderConfig, vtcode_agent_plugins::PluginError> {
    let provider_name = format!("{}.{}", plugin_name, server_name);

    match server {
        vtcode_agent_plugins::ServerConfig::Stdio(stdio) => {
            // §9.1: dedicated writable PLUGIN_DATA outside the package so it
            // survives plugin updates. Fail closed when it cannot be created:
            // continuing with a missing base would let `${PLUGIN_DATA}` cwds
            // resolve against a nonexistent dir (see expansion.rs).
            let plugin_data = vtcode_agent_plugins::default_plugin_data_dir(plugin_name);
            if let Err(e) = std::fs::create_dir_all(&plugin_data) {
                return Err(vtcode_agent_plugins::PluginError::InvalidMcp(format!(
                    "server '{server_name}' cannot use plugin data dir {}: {e}",
                    plugin_data.display()
                )));
            }

            // Defense in depth: parse already rejects reserved keys, but a
            // manually constructed config must still be invalid.
            for key in stdio.env.keys() {
                if vtcode_agent_plugins::is_reserved_env_key(key) {
                    return Err(vtcode_agent_plugins::PluginError::InvalidMcp(format!(
                        "server '{server_name}' env must not set reserved '{key}'"
                    )));
                }
            }
            if let Err(reason) = vtcode_agent_plugins::validate_command_token(&stdio.command) {
                return Err(vtcode_agent_plugins::PluginError::InvalidMcp(format!(
                    "server '{server_name}' has invalid command: {reason}"
                )));
            }
            if let Some(ref raw_cwd) = stdio.cwd {
                if let Err(reason) = vtcode_agent_plugins::validate_cwd_form(raw_cwd) {
                    return Err(vtcode_agent_plugins::PluginError::InvalidMcp(format!(
                        "server '{server_name}' has invalid cwd: {reason}"
                    )));
                }
            }

            // Resolve plugin-relative commands eagerly so the spawn uses the
            // canonical absolute path. Passing the raw "./bin/server" would let
            // the OS re-resolve it at spawn time, defeating the containment
            // check (e.g. a symlink that points outside the plugin root).
            let command = if stdio.command.starts_with("./") {
                vtcode_agent_plugins::validate_plugin_relative(&stdio.command, plugin_root)
                    .map_err(|e| vtcode_agent_plugins::PluginError::PathEscape(e.to_string()))?
                    .to_string_lossy()
                    .to_string()
            } else {
                stdio.command
            };

            // Filesystem-resolved roots for expansion and subprocess env.
            let canonical_root =
                vtcode_commons::canonicalize(plugin_root).unwrap_or_else(|_| plugin_root.to_path_buf());
            let canonical_data = vtcode_commons::canonicalize(&plugin_data).unwrap_or_else(|_| plugin_data.clone());

            let mut env = stdio
                .env
                .into_iter()
                .map(|(k, v)| (k, vtcode_agent_plugins::expand_placeholders(&v, &canonical_root, &canonical_data)))
                .collect::<HashMap<String, String>>();
            // §9.1: overlay configured env on the client base, then set
            // reserved vars last, replacing equivalents per platform semantics.
            #[cfg(windows)]
            {
                env.retain(|k, _| k.to_ascii_lowercase() != "plugin_root" && k.to_ascii_lowercase() != "plugin_data");
            }
            env.insert("PLUGIN_ROOT".into(), canonical_root.to_string_lossy().to_string());
            env.insert("PLUGIN_DATA".into(), canonical_data.to_string_lossy().to_string());

            let resolved_cwd =
                vtcode_agent_plugins::resolve_cwd(stdio.cwd.as_deref(), &canonical_root, &canonical_data)
                    .map_err(|e| vtcode_agent_plugins::PluginError::PathEscape(e.to_string()))?;
            let cwd = resolved_cwd.to_string_lossy().to_string();

            let args = stdio
                .args
                .iter()
                .map(|a| vtcode_agent_plugins::expand_placeholders(a, &canonical_root, &canonical_data))
                .collect();

            Ok(McpProviderConfig {
                name: provider_name,
                transport: vtcode_config::mcp::McpTransportConfig::Stdio(vtcode_config::mcp::McpStdioServerConfig {
                    command,
                    args,
                    working_directory: Some(cwd),
                }),
                env,
                ..McpProviderConfig::default()
            })
        }
        vtcode_agent_plugins::ServerConfig::StreamableHttp(http) => Ok(McpProviderConfig {
            name: provider_name,
            transport: vtcode_config::mcp::McpTransportConfig::Http(vtcode_config::mcp::McpHttpServerConfig {
                endpoint: http.url,
                api_key_env: None,
                oauth: None,
                protocol_version: vtcode_config::mcp::MCP_STABLE_PROTOCOL_VERSION.into(),
                handshake: vtcode_config::mcp::McpHttpHandshakeMode::Legacy,
                http_headers: http.headers.into_iter().collect(),
                env_http_headers: HashMap::new(),
            }),
            ..McpProviderConfig::default()
        }),
        vtcode_agent_plugins::ServerConfig::Sse(_) => {
            Err(vtcode_agent_plugins::PluginError::InvalidMcp("SSE transport is not supported by VT Code".into()))
        }
    }
}
