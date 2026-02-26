use crate::config::mcp::{
    McpAllowListConfig, McpClientConfig, McpProviderConfig, McpTransportConfig,
};
use crate::utils::file_utils::{ensure_dir_exists, write_file_with_context};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use chrono::Utc;
use parking_lot::RwLock;
use rmcp::model::{
    CallToolResult, ClientCapabilities, Implementation, InitializeRequestParams, RootsCapabilities,
};
use rustc_hash::FxHashMap;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use super::{
    McpClientStatus, McpElicitationHandler, McpPromptDetail, McpPromptInfo, McpProvider,
    McpResourceData, McpResourceInfo, McpToolExecutor, McpToolInfo, format_tool_markdown,
    sanitize_filename,
};

pub struct McpClient {
    config: McpClientConfig,
    providers: RwLock<FxHashMap<String, Arc<McpProvider>>>,
    allowlist: RwLock<McpAllowListConfig>,
    tool_provider_index: RwLock<FxHashMap<String, String>>,
    resource_provider_index: RwLock<FxHashMap<String, String>>,
    prompt_provider_index: RwLock<FxHashMap<String, String>>,
    elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
}

impl McpClient {
    /// Create a new MCP client from the configuration.
    pub fn new(config: McpClientConfig) -> Self {
        let allowlist = config.allowlist.clone();

        Self {
            config,
            providers: RwLock::new(FxHashMap::default()),
            allowlist: RwLock::new(allowlist),
            tool_provider_index: RwLock::new(FxHashMap::default()),
            resource_provider_index: RwLock::new(FxHashMap::default()),
            prompt_provider_index: RwLock::new(FxHashMap::default()),
            elicitation_handler: None,
        }
    }

    /// Register a handler used to satisfy elicitation requests from providers.
    pub fn set_elicitation_handler(&mut self, handler: Arc<dyn McpElicitationHandler>) {
        self.elicitation_handler = Some(handler);
    }

    /// Establish connections to all configured providers and complete the
    /// MCP handshake.
    pub async fn initialize(&mut self) -> Result<()> {
        if !self.config.enabled {
            info!("MCP client is disabled in configuration");
            return Ok(());
        }

        info!(
            "Initializing MCP client with {} configured providers",
            self.config.providers.len()
        );

        // Sequential initialization
        self.initialize_sequential().await
    }

    /// Initialize providers sequentially (fallback method)
    async fn initialize_sequential(&mut self) -> Result<()> {
        let tool_timeout = self.tool_timeout();
        let allowlist_snapshot = self.allowlist.read().clone();

        let mut initialized = FxHashMap::default();

        for provider_config in &self.config.providers {
            if !provider_config.enabled {
                debug!(
                    "MCP provider '{}' is disabled; skipping",
                    provider_config.name
                );
                continue;
            }

            if matches!(provider_config.transport, McpTransportConfig::Http(_))
                && !self.config.experimental_use_rmcp_client
            {
                warn!(
                    "Skipping MCP HTTP provider '{}' because experimental_use_rmcp_client is disabled",
                    provider_config.name
                );
                continue;
            }

            match McpProvider::connect(provider_config.clone(), self.elicitation_handler.clone())
                .await
            {
                Ok(provider) => {
                    let provider_startup_timeout = self.resolve_startup_timeout(provider_config);
                    if let Err(err) = provider
                        .initialize(
                            self.build_initialize_params(&provider),
                            provider_startup_timeout,
                            tool_timeout,
                            &allowlist_snapshot,
                        )
                        .await
                    {
                        error!(
                            "Failed to initialize MCP provider '{}': {err}",
                            provider_config.name
                        );
                        continue;
                    }

                    if let Err(err) = provider
                        .refresh_tools(&allowlist_snapshot, tool_timeout)
                        .await
                    {
                        warn!(
                            "Failed to fetch tools for provider '{}': {err}",
                            provider_config.name
                        );
                    } else if let Some(cache) = provider.cached_tools().await {
                        self.record_tool_provider(&provider.name, &cache);
                    }

                    initialized.insert(provider.name.clone(), Arc::new(provider));
                    info!(
                        "Successfully initialized MCP provider '{}'",
                        provider_config.name
                    );
                }
                Err(err) => {
                    error!(
                        "Failed to connect to MCP provider '{}': {err}",
                        provider_config.name
                    );
                }
            }
        }

        *self.providers.write() = initialized;
        info!(
            "MCP client initialization complete. Active providers: {}",
            self.providers.read().len()
        );

        Ok(())
    }

    /// Validate tool arguments based on security configuration
    fn validate_tool_arguments(&self, _tool_name: &str, args: &Value) -> Result<()> {
        // Check argument size
        if self.config.security.validation.max_argument_size > 0 {
            let args_size = serde_json::to_string(args).map_or(0, |s| s.len()) as u32;

            if args_size > self.config.security.validation.max_argument_size {
                return Err(anyhow::anyhow!(
                    "Tool arguments exceed maximum size of {} bytes",
                    self.config.security.validation.max_argument_size
                ));
            }
        }

        // Check for path traversal in file-related arguments
        if self.config.security.validation.path_traversal_protection
            && let Some(path) = args.get("path").and_then(|v| v.as_str())
            && (path.contains("../")
                || path.starts_with("../")
                || path.contains("..\\")
                || path.starts_with("..\\"))
        {
            return Err(anyhow::anyhow!("Path traversal detected in arguments"));
        }

        Ok(())
    }

    /// Execute a tool call after validating arguments.
    ///
    /// Public-facing version that takes ownership of `args` for compatibility
    /// with existing callers. Delegates to the reference-taking implementation
    /// to avoid unnecessary cloning when the caller already has a reference.
    pub async fn execute_tool_with_validation(
        &self,
        tool_name: &str,
        args: Value,
    ) -> Result<Value> {
        self.execute_tool_with_validation_ref(tool_name, &args)
            .await
    }

    // Internal reference-taking implementation to avoid cloning when not necessary.
    async fn execute_tool_with_validation_ref(
        &self,
        tool_name: &str,
        args: &Value,
    ) -> Result<Value> {
        if !self.config.enabled {
            return Err(anyhow!(
                "MCP support is disabled in the current configuration"
            ));
        }

        self.validate_tool_arguments(tool_name, args)?;

        let provider = self.resolve_provider_for_tool(tool_name).await?;
        let allowlist_snapshot = self.allowlist.read().clone();
        let result = provider
            .call_tool(tool_name, args, self.tool_timeout(), &allowlist_snapshot)
            .await?;

        Self::format_tool_result(&provider.name, tool_name, result)
    }

    /// Refresh the internal allow list at runtime.
    pub fn update_allowlist(&self, allowlist: McpAllowListConfig) {
        *self.allowlist.write() = allowlist;
        self.tool_provider_index.write().clear();
        self.resource_provider_index.write().clear();
        self.prompt_provider_index.write().clear();

        for provider in self.providers.read().values() {
            provider.invalidate_caches();
        }
    }

    /// Current allow list snapshot.
    pub fn current_allowlist(&self) -> McpAllowListConfig {
        self.allowlist.read().clone()
    }

    /// Return the provider name serving the given tool if previously cached.
    pub fn provider_for_tool(&self, tool_name: &str) -> Option<String> {
        self.tool_provider_index.read().get(tool_name).cloned()
    }

    /// Return the provider responsible for the given resource URI if known.
    pub fn provider_for_resource(&self, uri: &str) -> Option<String> {
        self.resource_provider_index.read().get(uri).cloned()
    }

    /// Return the provider that exposes the given prompt if known.
    pub fn provider_for_prompt(&self, prompt_name: &str) -> Option<String> {
        self.prompt_provider_index.read().get(prompt_name).cloned()
    }

    /// Execute a tool call on the appropriate provider.
    pub async fn execute_tool(&self, tool_name: &str, args: Value) -> Result<Value> {
        self.execute_tool_with_validation(tool_name, args).await
    }

    /// List all tools from all active providers.
    pub async fn list_tools(&self) -> Result<Vec<McpToolInfo>> {
        self.collect_tools(false).await
    }

    /// List all resources exposed by connected MCP providers.
    pub async fn list_resources(&self) -> Result<Vec<McpResourceInfo>> {
        self.collect_resources(false).await
    }

    /// Force refresh and list resources from providers.
    pub async fn refresh_resources(&self) -> Result<Vec<McpResourceInfo>> {
        self.collect_resources(true).await
    }

    /// List all prompts advertised by connected MCP providers.
    pub async fn list_prompts(&self) -> Result<Vec<McpPromptInfo>> {
        self.collect_prompts(false).await
    }

    /// Force refresh and list prompts from providers.
    pub async fn refresh_prompts(&self) -> Result<Vec<McpPromptInfo>> {
        self.collect_prompts(true).await
    }

    /// Read a single resource from its originating provider.
    pub async fn read_resource(&self, uri: &str) -> Result<McpResourceData> {
        let provider = self.resolve_provider_for_resource(uri).await?;
        let provider_name = provider.name.clone();
        let allowlist_snapshot = self.allowlist.read().clone();
        let data = provider
            .read_resource(uri, self.request_timeout(), &allowlist_snapshot)
            .await?;
        self.resource_provider_index
            .write()
            .insert(uri.into(), provider_name);
        Ok(data)
    }

    /// Retrieve a rendered prompt from its originating provider.
    pub async fn get_prompt(
        &self,
        prompt_name: &str,
        arguments: Option<std::collections::HashMap<String, String>>,
    ) -> Result<McpPromptDetail> {
        let provider = self.resolve_provider_for_prompt(prompt_name).await?;
        let provider_name = provider.name.clone();
        let allowlist_snapshot = self.allowlist.read().clone();
        let prompt = provider
            .get_prompt(
                prompt_name,
                arguments.unwrap_or_default(),
                self.request_timeout(),
                &allowlist_snapshot,
            )
            .await?;
        self.prompt_provider_index
            .write()
            .insert(prompt_name.into(), provider_name);
        Ok(prompt)
    }

    /// Shutdown all active provider connections.
    pub async fn shutdown(&self) -> Result<()> {
        let providers: Vec<Arc<McpProvider>> = {
            let mut guard = self.providers.write();
            let values: Vec<_> = guard.values().cloned().collect();
            guard.clear();
            values
        };

        if providers.is_empty() {
            info!("No active MCP connections to shutdown");
            return Ok(());
        }

        info!("Shutting down {} MCP providers", providers.len());
        for provider in providers {
            if let Err(err) = provider.shutdown().await {
                warn!(
                    "Provider '{}' shutdown returned error: {err}",
                    provider.name
                );
            }
        }

        self.tool_provider_index.write().clear();
        self.resource_provider_index.write().clear();
        self.prompt_provider_index.write().clear();
        Ok(())
    }

    /// Current status snapshot for UI/debugging purposes.
    pub fn get_status(&self) -> McpClientStatus {
        let providers = self.providers.read();
        // Use iterator to collect keys directly without intermediate push
        let configured_providers: Vec<String> = providers.keys().cloned().collect();
        McpClientStatus {
            enabled: self.config.enabled,
            provider_count: providers.len(),
            active_connections: providers.len(),
            configured_providers,
        }
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
        ensure_dir_exists(&tools_dir).await.with_context(|| {
            format!(
                "Failed to create MCP tools directory: {}",
                tools_dir.display()
            )
        })?;

        // Group tools by provider
        let mut by_provider: FxHashMap<String, Vec<&McpToolInfo>> = FxHashMap::default();
        for tool in &tools {
            by_provider
                .entry(tool.provider.clone())
                .or_default()
                .push(tool);
        }

        // Write tool files per provider
        for (provider, provider_tools) in &by_provider {
            let provider_dir = tools_dir.join(sanitize_filename(provider));
            ensure_dir_exists(&provider_dir).await.with_context(|| {
                format!(
                    "Failed to create provider directory: {}",
                    provider_dir.display()
                )
            })?;

            for tool in provider_tools {
                let tool_content = format_tool_markdown(tool);
                let tool_path = provider_dir.join(format!("{}.md", sanitize_filename(&tool.name)));
                write_file_with_context(&tool_path, &tool_content, "MCP tool file")
                    .await
                    .with_context(|| {
                        format!("Failed to write tool file: {}", tool_path.display())
                    })?;
            }
        }

        // Write index file
        let index_content = self.generate_tools_index(&tools, &by_provider);
        let index_path = tools_dir.join("INDEX.md");
        write_file_with_context(&index_path, &index_content, "MCP tools index")
            .await
            .with_context(|| {
                format!("Failed to write MCP tools index: {}", index_path.display())
            })?;

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
    fn generate_tools_index(
        &self,
        tools: &[McpToolInfo],
        by_provider: &FxHashMap<String, Vec<&McpToolInfo>>,
    ) -> String {
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
                let desc_truncated = if desc.len() > 60 {
                    format!("{}...", &desc[..57])
                } else {
                    desc.to_string()
                };
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
                content.push_str(&format!("### {}\n\n", provider));
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

    async fn collect_tools(&self, force_refresh: bool) -> Result<Vec<McpToolInfo>> {
        // Collect provider references in one pass
        let providers: Vec<Arc<McpProvider>> = self.providers.read().values().cloned().collect();

        if providers.is_empty() {
            return Ok(Vec::new());
        }

        let allowlist = self.allowlist.read().clone();
        let timeout = self.tool_timeout();
        let mut all_tools = Vec::with_capacity(128);
        let mut index_updates: FxHashMap<String, String> =
            FxHashMap::with_capacity_and_hasher(128, Default::default());

        for provider in providers {
            let provider_name = provider.name.clone();
            let tools = if force_refresh {
                provider.refresh_tools(&allowlist, timeout).await
            } else {
                provider.list_tools(&allowlist, timeout).await
            };

            match tools {
                Ok(tools) => {
                    for tool in &tools {
                        index_updates.insert(tool.name.clone(), provider_name.clone());
                    }
                    all_tools.extend(tools);
                }
                Err(err) => {
                    warn!(
                        "Failed to list tools for provider '{}': {err}",
                        provider_name
                    );
                }
            }
        }

        if !index_updates.is_empty() {
            *self.tool_provider_index.write() = index_updates;
        } else if force_refresh {
            self.tool_provider_index.write().clear();
        }

        Ok(all_tools)
    }

    async fn collect_resources(&self, force_refresh: bool) -> Result<Vec<McpResourceInfo>> {
        // Collect provider references in one pass
        let providers: Vec<Arc<McpProvider>> = self.providers.read().values().cloned().collect();

        if providers.is_empty() {
            self.resource_provider_index.write().clear();
            return Ok(Vec::new());
        }

        let allowlist = self.allowlist.read().clone();
        let timeout = self.request_timeout();
        let mut all_resources = Vec::with_capacity(64);

        for provider in providers {
            let resources = if force_refresh {
                provider.refresh_resources(&allowlist, timeout).await
            } else {
                provider.list_resources(&allowlist, timeout).await
            };

            match resources {
                Ok(resources) => {
                    all_resources.extend(resources);
                }
                Err(err) => {
                    warn!(
                        "Failed to list resources for provider '{}': {err}",
                        provider.name
                    );
                }
            }
        }

        let mut index = self.resource_provider_index.write();
        index.clear();
        for resource in &all_resources {
            index.insert(resource.uri.clone(), resource.provider.clone());
        }

        Ok(all_resources)
    }

    async fn collect_prompts(&self, force_refresh: bool) -> Result<Vec<McpPromptInfo>> {
        // Collect provider references in one pass
        let providers: Vec<Arc<McpProvider>> = self.providers.read().values().cloned().collect();

        if providers.is_empty() {
            self.prompt_provider_index.write().clear();
            return Ok(Vec::new());
        }

        let allowlist = self.allowlist.read().clone();
        let timeout = self.request_timeout();
        let mut all_prompts = Vec::with_capacity(32);

        for provider in providers {
            let prompts = if force_refresh {
                provider.refresh_prompts(&allowlist, timeout).await
            } else {
                provider.list_prompts(&allowlist, timeout).await
            };

            match prompts {
                Ok(prompts) => {
                    all_prompts.extend(prompts);
                }
                Err(err) => {
                    warn!(
                        "Failed to list prompts for provider '{}': {err}",
                        provider.name
                    );
                }
            }
        }

        let mut index = self.prompt_provider_index.write();
        index.clear();
        for prompt in &all_prompts {
            index.insert(prompt.name.clone(), prompt.provider.clone());
        }

        Ok(all_prompts)
    }

    async fn resolve_provider_for_tool(&self, tool_name: &str) -> Result<Arc<McpProvider>> {
        if !self.config.enabled {
            return Err(anyhow!(
                "MCP support is disabled in the current configuration"
            ));
        }

        if let Some(provider) = self.provider_for_tool(tool_name)
            && let Some(found) = self.providers.read().get(&provider)
        {
            return Ok(found.clone());
        }

        let allowlist = self.allowlist.read().clone();
        let timeout = self.tool_timeout();
        let providers: Vec<Arc<McpProvider>> = self.providers.read().values().cloned().collect();

        if providers.is_empty() {
            if self.config.providers.is_empty() {
                return Err(anyhow!(
                    "No MCP providers are configured. Use `vtcode mcp add` or update vtcode.toml to register one."
                ));
            }

            return Err(anyhow!(
                "No MCP providers are currently connected. Ensure MCP initialization completed successfully."
            ));
        }

        for provider in providers {
            match provider.has_tool(tool_name, &allowlist, timeout).await {
                Ok(true) => {
                    self.tool_provider_index
                        .write()
                        .insert(tool_name.into(), provider.name.clone());
                    return Ok(provider);
                }
                Ok(false) => continue,
                Err(err) => {
                    warn!(
                        "Error checking tool '{}' on provider '{}': {err}",
                        tool_name, provider.name
                    );
                }
            }
        }

        match self.collect_tools(true).await {
            Ok(_) => {
                if let Some(provider) = self.provider_for_tool(tool_name)
                    && let Some(found) = self.providers.read().get(&provider)
                {
                    return Ok(found.clone());
                }
            }
            Err(err) => {
                warn!(
                    "Failed to refresh MCP tool caches while resolving '{}': {err}",
                    tool_name
                );
            }
        }

        Err(anyhow!(
            "Tool '{}' not found on any MCP provider.\n\n\
            To use this tool:\n\
            1. Install the MCP server: `uv tool install mcp-server-{}`\n\
            2. Add to vtcode.toml:\n   \
               [[mcp.providers]]\n   \
               name = \"{}\"\n   \
               command = \"uvx\"\n   \
               args = [\"mcp-server-{}\"]\n\
            3. Restart VT Code\n\n\
            Or use the built-in alternative if available (e.g., web_fetch instead of mcp_fetch)",
            tool_name,
            tool_name,
            tool_name,
            tool_name
        ))
    }

    async fn resolve_provider_for_resource(&self, uri: &str) -> Result<Arc<McpProvider>> {
        if let Some(provider) = self.provider_for_resource(uri)
            && let Some(found) = self.providers.read().get(&provider)
        {
            return Ok(found.clone());
        }

        let allowlist = self.allowlist.read().clone();
        let timeout = self.request_timeout();
        let providers: Vec<Arc<McpProvider>> = self.providers.read().values().cloned().collect();

        for provider in providers {
            match provider.has_resource(uri, &allowlist, timeout).await {
                Ok(true) => {
                    self.resource_provider_index
                        .write()
                        .insert(uri.into(), provider.name.clone());
                    return Ok(provider);
                }
                Ok(false) => continue,
                Err(err) => {
                    warn!(
                        "Error checking resource '{}' on provider '{}': {err}",
                        uri, provider.name
                    );
                }
            }
        }

        Err(anyhow!("Resource '{}' not found on any MCP provider", uri))
    }

    async fn resolve_provider_for_prompt(&self, prompt_name: &str) -> Result<Arc<McpProvider>> {
        if let Some(provider) = self.provider_for_prompt(prompt_name)
            && let Some(found) = self.providers.read().get(&provider)
        {
            return Ok(found.clone());
        }

        let allowlist = self.allowlist.read().clone();
        let timeout = self.request_timeout();
        let providers: Vec<Arc<McpProvider>> = self.providers.read().values().cloned().collect();

        for provider in providers {
            match provider.has_prompt(prompt_name, &allowlist, timeout).await {
                Ok(true) => {
                    self.prompt_provider_index
                        .write()
                        .insert(prompt_name.into(), provider.name.clone());
                    return Ok(provider);
                }
                Ok(false) => continue,
                Err(err) => {
                    warn!(
                        "Error checking prompt '{}' on provider '{}': {err}",
                        prompt_name, provider.name
                    );
                }
            }
        }

        Err(anyhow!(
            "Prompt '{}' not found on any MCP provider",
            prompt_name
        ))
    }

    fn record_tool_provider(&self, provider: &str, tools: &[McpToolInfo]) {
        let mut index = self.tool_provider_index.write();
        for tool in tools {
            index.insert(tool.name.clone(), provider.to_string());
        }
    }

    fn startup_timeout(&self) -> Option<Duration> {
        match self.config.startup_timeout_seconds {
            Some(0) => None,
            Some(value) => Some(Duration::from_secs(value)),
            None => self.request_timeout(),
        }
    }

    fn resolve_startup_timeout(&self, provider_config: &McpProviderConfig) -> Option<Duration> {
        if let Some(timeout_ms) = provider_config.startup_timeout_ms {
            if timeout_ms == 0 {
                None
            } else {
                Some(Duration::from_millis(timeout_ms))
            }
        } else {
            self.startup_timeout()
        }
    }

    fn tool_timeout(&self) -> Option<Duration> {
        match self.config.tool_timeout_seconds {
            Some(0) => None,
            Some(value) => Some(Duration::from_secs(value)),
            None => self.request_timeout(),
        }
    }

    fn request_timeout(&self) -> Option<Duration> {
        if self.config.request_timeout_seconds == 0 {
            None
        } else {
            Some(Duration::from_secs(self.config.request_timeout_seconds))
        }
    }

    fn build_initialize_params(&self, _provider: &McpProvider) -> InitializeRequestParams {
        let mut capabilities = ClientCapabilities {
            roots: Some(RootsCapabilities {
                list_changed: Some(true),
            }),
            ..Default::default()
        };

        if self.elicitation_handler.is_some() {
            // Elicitation is now a first-class capability in rmcp
            capabilities.elicitation = Some(rmcp::model::ElicitationCapability {
                schema_validation: Some(true),
            });
        }

        InitializeRequestParams {
            meta: None,
            capabilities,
            client_info: Implementation {
                name: "vtcode".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
            protocol_version: rmcp::model::ProtocolVersion::V_2024_11_05,
        }
    }

    pub(super) fn normalize_arguments(args: &Value) -> Map<String, Value> {
        match args {
            Value::Null => Map::new(),
            Value::Object(map) => map.clone(),
            other => {
                let mut map = Map::new();
                map.insert("value".to_owned(), other.clone());
                map
            }
        }
    }

    fn format_tool_result(
        provider_name: &str,
        tool_name: &str,
        result: CallToolResult,
    ) -> Result<Value> {
        // Convert result to JSON to access fields flexibly
        let result_json = serde_json::to_value(&result)?;
        let result_obj = result_json.as_object();

        // Check for error - handle both rmcp's is_error field and meta message
        let is_error = result_obj
            .and_then(|o| o.get("isError"))
            .or_else(|| result_obj.and_then(|o| o.get("is_error")))
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if is_error {
            let mut message = result_obj
                .and_then(|o| o.get("_meta"))
                .or_else(|| result_obj.and_then(|o| o.get("meta")))
                .and_then(|m| m.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned);

            // Try to find text content in the content array
            if message.is_none()
                && let Some(content) = result_obj
                    .and_then(|o| o.get("content"))
                    .and_then(Value::as_array)
            {
                message = content
                    .iter()
                    .find_map(|block| block.get("text").and_then(Value::as_str).map(str::to_owned));
            }

            let message = message.unwrap_or_else(|| "Unknown MCP tool error".to_owned());
            return Err(anyhow!(
                "MCP tool '{}' on provider '{}' reported an error: {}",
                tool_name,
                provider_name,
                message
            ));
        }

        let mut payload = Map::new();
        payload.insert("provider".into(), Value::String(provider_name.to_string()));
        payload.insert("tool".into(), Value::String(tool_name.to_string()));

        // Add meta if present
        if let Some(meta) = result_obj
            .and_then(|o| o.get("_meta"))
            .or_else(|| result_obj.and_then(|o| o.get("meta")))
            .and_then(Value::as_object)
            && !meta.is_empty()
        {
            payload.insert("meta".into(), Value::Object(meta.clone()));
        }

        // Add content if present
        if let Some(content) = result_obj.and_then(|o| o.get("content"))
            && !content.is_null()
            && !content.as_array().map(|a| a.is_empty()).unwrap_or(true)
        {
            payload.insert("content".into(), content.clone());
        }

        Ok(Value::Object(payload))
    }
}

#[async_trait]
impl McpToolExecutor for McpClient {
    async fn execute_mcp_tool(&self, tool_name: &str, args: &Value) -> Result<Value> {
        self.execute_tool_with_validation_ref(tool_name, args).await
    }

    async fn list_mcp_tools(&self) -> Result<Vec<McpToolInfo>> {
        self.collect_tools(false).await
    }

    async fn has_mcp_tool(&self, tool_name: &str) -> Result<bool> {
        if !self.config.enabled {
            return Ok(false);
        }

        if self.provider_for_tool(tool_name).is_some() {
            return Ok(true);
        }

        if self.providers.read().is_empty() {
            if self.config.providers.is_empty() {
                return Ok(false);
            }

            bail!(
                "No MCP providers are currently connected. Ensure MCP initialization completed successfully."
            );
        }

        let tools = self.collect_tools(false).await?;
        Ok(tools.iter().any(|tool| tool.name == tool_name))
    }

    fn get_status(&self) -> McpClientStatus {
        self.get_status()
    }
}
