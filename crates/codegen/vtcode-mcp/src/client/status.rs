use super::*;

impl McpClient {
    /// Current status snapshot for UI/debugging purposes.
    pub fn get_status(&self) -> McpClientStatus {
        let state = self.state.read();
        let providers = &state.providers;
        // Use iterator to collect keys directly without intermediate push
        let mut configured_providers: Vec<String> = providers.keys().cloned().collect();
        configured_providers.sort_unstable();
        McpClientStatus {
            enabled: self.config.enabled,
            provider_count: providers.len(),
            active_connections: providers.len(),
            configured_providers,
        }
    }

    /// Return configured MCP servers and their current connection state.
    ///
    /// Connected servers additionally report the protocol version settled
    /// during the handshake (`negotiated_protocol_version`), which may differ
    /// from the configured version when the server negotiates down.
    pub async fn list_servers(&self) -> Vec<Value> {
        let live: Vec<(McpProviderConfig, Option<Arc<McpProvider>>)> = {
            let state = self.state.read();
            self.config
                .providers
                .iter()
                .map(|provider_config| (provider_config.clone(), state.providers.get(&provider_config.name).cloned()))
                .collect()
        };
        let mut servers = Vec::with_capacity(live.len());
        for (provider_config, provider) in live {
            let negotiated = match &provider {
                Some(provider) => provider.negotiated_protocol_version().await,
                None => None,
            };
            let connected = provider.is_some();
            let (transport, target) = match &provider_config.transport {
                McpTransportConfig::Stdio(stdio) => ("stdio", Value::String(stdio.command.clone())),
                McpTransportConfig::Http(http) => ("http", Value::String(http.endpoint.clone())),
            };

            servers.push(json!({
                "name": provider_config.name,
                "enabled": provider_config.enabled,
                "connected": connected,
                "connection_state": if connected { "connected" } else { "disconnected" },
                "transport": transport,
                "target": target,
                "negotiated_protocol_version": negotiated.map(Value::String).unwrap_or(Value::Null),
            }));
        }
        servers
    }

    /// Return whether model-callable lifecycle tools are enabled by config.
    pub fn allow_model_lifecycle_control(&self) -> bool {
        self.config.lifecycle.allow_model_control
    }

    /// Connect one configured MCP server by name.
    pub async fn connect_server(&self, server_name: &str) -> Result<()> {
        if !self.config.enabled {
            bail!("MCP support is disabled in the current configuration");
        }
        reject_unsupported_ui_apps_tier(&self.config.ui)?;

        if self.state.read().providers.contains_key(server_name) {
            return Ok(());
        }

        let provider_config = self
            .config
            .providers
            .iter()
            .find(|provider| provider.name == server_name)
            .cloned()
            .ok_or_else(|| anyhow!("MCP server '{server_name}' is not configured"))?;

        if !provider_config.enabled {
            bail!("MCP server '{server_name}' is configured but disabled");
        }

        if let Some(reason) = self.requirement_mismatch_reason(&provider_config) {
            bail!("Cannot connect MCP server '{}': {}", provider_config.name, reason);
        }

        if matches!(provider_config.transport, McpTransportConfig::Http(_)) && !self.config.experimental_use_rmcp_client
        {
            bail!(
                "Cannot connect MCP HTTP server '{}' while experimental_use_rmcp_client is disabled",
                provider_config.name
            );
        }

        let allowlist_snapshot = self.state.read().allowlist.clone();
        let tool_timeout = self.tool_timeout();
        let provider = self
            .connect_and_initialize_provider(&provider_config, &allowlist_snapshot, tool_timeout)
            .await?;

        if let Err(err) = provider.cached_tools_or_refresh_shared(&allowlist_snapshot, tool_timeout).await {
            warn!("Connected MCP server '{}' but failed to refresh tools: {err}", server_name);
        } else if let Some(cache) = provider.cached_tools_shared().await {
            self.record_tool_provider(&provider.name, &cache);
        }

        drop(self.state.write().providers.insert(provider.name.clone(), Arc::new(provider)));
        Ok(())
    }

    /// Disconnect one active MCP server by name.
    pub async fn disconnect_server(&self, server_name: &str) -> Result<()> {
        let provider = {
            let mut state = self.state.write();
            let provider = state
                .providers
                .remove(server_name)
                .ok_or_else(|| anyhow!("MCP server '{server_name}' is not connected"))?;
            state
                .tool_provider_index
                .retain(|_, provider_name| provider_name != server_name);
            state
                .resource_provider_index
                .retain(|_, provider_name| provider_name != server_name);
            state
                .prompt_provider_index
                .retain(|_, provider_name| provider_name != server_name);
            provider
        };

        provider.shutdown().await?;
        Ok(())
    }

    /// Sync MCP tool descriptions to files for dynamic context discovery
    ///
    /// This implements Cursor-style dynamic context discovery:
    /// - Tool descriptions are written to `.vtcode/mcp/tools/{provider}/{tool}.md`
    /// - Status is written to `.vtcode/mcp/status.json`
    /// - Agents can discover tools via grep/read_file without loading all schemas
    ///
    /// Returns the paths to written files (index path, tool count)
    pub async fn sync_tools_to_files(&self, workspace_root: &Path) -> Result<(PathBuf, usize)> {
        let tools = self.list_tools().await?;
        let mcp_dir = workspace_root.join(".vtcode").join("mcp");
        let tools_dir = mcp_dir.join("tools");

        // Create directories
        ensure_dir_exists(&tools_dir)
            .await
            .with_context(|| format!("Failed to create MCP tools directory: {}", tools_dir.display()))?;

        // Group tools by provider
        let mut by_provider: BTreeMap<String, Vec<&McpToolInfo>> = BTreeMap::new();
        for tool in &tools {
            by_provider.entry(tool.provider.clone()).or_default().push(tool);
        }

        // Write tool files per provider
        for (provider, provider_tools) in &by_provider {
            let provider_dir = tools_dir.join(sanitize_filename(provider));
            ensure_dir_exists(&provider_dir)
                .await
                .with_context(|| format!("Failed to create provider directory: {}", provider_dir.display()))?;

            for tool in provider_tools {
                let tool_content = format_tool_markdown(tool);
                let tool_path = provider_dir.join(format!("{}.md", sanitize_filename(&tool.name)));
                write_file_with_context(&tool_path, &tool_content, "MCP tool file")
                    .await
                    .with_context(|| format!("Failed to write tool file: {}", tool_path.display()))?;
            }
        }

        // Write index file
        let index_content = self.generate_tools_index(&tools, &by_provider);
        let index_path = tools_dir.join("INDEX.md");
        write_file_with_context(&index_path, &index_content, "MCP tools index")
            .await
            .with_context(|| format!("Failed to write MCP tools index: {}", index_path.display()))?;

        // Write status file
        let status = self.generate_status_json();
        let status_path = mcp_dir.join("status.json");
        let status_json = serde_json::to_string_pretty(&status)?;
        write_file_with_context(&status_path, &status_json, "MCP status")
            .await
            .with_context(|| format!("Failed to write MCP status: {}", status_path.display()))?;

        info!(
            tools = tools.len(),
            providers = by_provider.len(),
            index = %index_path.display(),
            "Synced MCP tool descriptions to files"
        );

        Ok((index_path, tools.len()))
    }

    /// Generate INDEX.md content for MCP tools
    fn generate_tools_index(&self, tools: &[McpToolInfo], by_provider: &BTreeMap<String, Vec<&McpToolInfo>>) -> String {
        let mut content = String::new();
        content.push_str("# MCP Tools Index\n\n");
        content.push_str("This file lists all available MCP tools for dynamic discovery.\n");
        content.push_str("Use `read_file` on individual tool files for full schema details.\n\n");

        if tools.is_empty() {
            content.push_str("*No MCP tools available.*\n\n");
            content.push_str("Configure MCP servers in `vtcode.toml` or `.mcp.json`.\n");
        } else {
            content.push_str(&format!("**Total Tools**: {}\n\n", tools.len()));

            // Summary table
            content.push_str("## Quick Reference\n\n");
            content.push_str("| Provider | Tool | Description |\n");
            content.push_str("|----------|------|-------------|\n");

            for tool in tools {
                let desc = tool.description.lines().next().unwrap_or(&tool.description);
                let desc_truncated = vtcode_commons::formatting::truncate_byte_budget(desc, 57, "...");
                content.push_str(&format!(
                    "| {} | `{}` | {} |\n",
                    tool.provider,
                    tool.name,
                    desc_truncated.replace('|', "\\|")
                ));
            }

            // Per-provider sections
            content.push_str("\n## Tools by Provider\n\n");
            for (provider, provider_tools) in by_provider {
                content.push_str(&format!("### {provider}\n\n"));
                for tool in provider_tools {
                    content.push_str(&format!(
                        "- **{}**: {}\n  - Path: `.vtcode/mcp/tools/{}/{}.md`\n",
                        tool.name,
                        tool.description.lines().next().unwrap_or(&tool.description),
                        sanitize_filename(provider),
                        sanitize_filename(&tool.name)
                    ));
                }
                content.push('\n');
            }
        }

        content.push_str("\n---\n");
        content.push_str("*Generated automatically. Do not edit manually.*\n");

        content
    }

    /// Generate status.json content
    fn generate_status_json(&self) -> Value {
        let status = self.get_status();
        json!({
            "enabled": status.enabled,
            "provider_count": status.provider_count,
            "active_connections": status.active_connections,
            "configured_providers": status.configured_providers,
            "last_updated": Utc::now().to_rfc3339(),
        })
    }
}
