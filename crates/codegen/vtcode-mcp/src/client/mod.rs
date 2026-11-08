use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use chrono::Utc;
use parking_lot::RwLock;
use rmcp::model::{CallToolResult, ClientCapabilities, InitializeRequestParams, RootsCapabilities};
use rustc_hash::FxHashMap;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};
use vtcode_commons::fs::{ensure_dir_exists, write_file_with_context};
use vtcode_config::mcp::{McpAllowListConfig, McpClientConfig, McpProviderConfig, McpTransportConfig, McpUiAppsTier};

use super::McpSandboxContext;
use super::{
    McpClientStatus, McpElicitationHandler, McpPromptDetail, McpPromptInfo, McpProvider, McpResourceData,
    McpResourceInfo, McpToolExecutor, McpToolInfo, apply_ui_extension_gate, format_tool_markdown,
    reject_unsupported_ui_apps_tier, sanitize_filename,
};
use crate::connection_pool::McpConnectionPool;

mod status;

pub(super) struct McpClientState {
    providers: FxHashMap<String, Arc<McpProvider>>,
    allowlist: McpAllowListConfig,
    tool_provider_index: FxHashMap<String, String>,
    resource_provider_index: FxHashMap<String, String>,
    prompt_provider_index: FxHashMap<String, String>,
}

/// Stable provider order keeps the aggregated catalog (and LLM prompt-cache prefix) deterministic.
fn providers_sorted_by_name<V: Clone>(providers: &FxHashMap<String, V>) -> Vec<V> {
    let mut entries: Vec<(&String, &V)> = providers.iter().collect();
    entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
    entries.into_iter().map(|(_, provider)| provider.clone()).collect()
}

pub struct McpClient {
    pub(super) config: McpClientConfig,
    pub(super) state: RwLock<McpClientState>,
    elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
    sandbox_context: Option<McpSandboxContext>,
}

impl McpClient {
    /// Create a new MCP client from the configuration.
    pub fn new(config: McpClientConfig) -> Self {
        Self::with_sandbox_context(config, None)
    }

    /// Create an MCP client with an optional inherited stdio sandbox context.
    pub fn with_sandbox_context(config: McpClientConfig, sandbox_context: Option<McpSandboxContext>) -> Self {
        let allowlist = config.allowlist.clone();

        Self {
            config,
            state: RwLock::new(McpClientState {
                providers: FxHashMap::default(),
                allowlist,
                tool_provider_index: FxHashMap::default(),
                resource_provider_index: FxHashMap::default(),
                prompt_provider_index: FxHashMap::default(),
            }),
            elicitation_handler: None,
            sandbox_context,
        }
    }

    /// Register a handler used to satisfy elicitation requests from providers.
    pub fn set_elicitation_handler(&mut self, handler: Arc<dyn McpElicitationHandler>) {
        self.elicitation_handler = Some(handler);
    }

    /// MCP Apps presentation tier from `[mcp.ui].apps`.
    ///
    /// Execution consults this before enriching results: `Off` refuses app UI
    /// (marking refused URIs under [`crate::types::MCP_APP_SUPPRESSED_KEY`])
    /// instead of fetching `ui://` resources.
    #[must_use]
    pub fn ui_apps_tier(&self) -> McpUiAppsTier {
        self.config.ui.apps
    }

    /// Resolve `(connection timeout, tool timeout)` in seconds.
    ///
    /// `connection_timeout_seconds` bounds the connect-and-handshake phase and
    /// `request_timeout_seconds` the per-tool call budget. They are distinct on
    /// purpose: a provider that never finishes connecting must fail on the short
    /// connection key, not consume the tool budget. Both clamp to at least one
    /// second so a `0` cannot produce an instantly-failing deadline.
    fn pool_timeouts(&self) -> (u64, u64) {
        (self.config.connection_timeout_seconds.max(1), self.config.request_timeout_seconds.max(1))
    }

    /// Establish connections to all configured providers and complete the
    /// MCP handshake.
    pub async fn initialize(&mut self) -> Result<()> {
        if !self.config.enabled {
            info!("MCP client is disabled in configuration");
            return Ok(());
        }
        reject_unsupported_ui_apps_tier(&self.config.ui)?;

        info!("Initializing MCP client with {} configured providers", self.config.providers.len());

        let max_connections = self.config.max_concurrent_connections.max(1);
        let (connection_timeout_seconds, tool_timeout_seconds) = self.pool_timeouts();
        let pool = McpConnectionPool::new(max_connections, connection_timeout_seconds);
        let tool_timeout = Some(Duration::from_secs(tool_timeout_seconds));
        let allowlist_snapshot = self.state.read().allowlist.clone();

        let provider_configs: Vec<McpProviderConfig> =
            self.config.providers.iter().filter(|c| c.enabled).cloned().collect();

        let results = pool
            .initialize_providers_parallel(
                provider_configs,
                self.elicitation_handler.clone(),
                self.sandbox_context.clone(),
                tool_timeout,
                &allowlist_snapshot,
                self.config.ui.apps.advertises_ui_extension(),
            )
            .await
            .map_err(|e| anyhow::anyhow!("MCP connection pool initialization failed: {e}"))?;

        let mut initialized = FxHashMap::default();
        for (name, provider) in results {
            drop(initialized.insert(name, provider));
        }

        self.state.write().providers = initialized;
        info!("MCP client initialization complete. Active providers: {}", self.state.read().providers.len());

        Ok(())
    }

    /// Validate tool arguments based on security configuration
    fn validate_tool_arguments(&self, _tool_name: &str, args: &Value) -> Result<()> {
        // Check argument size
        if self.config.security.validation.max_argument_size > 0 {
            let args_size = serde_json::to_string(args).map_or(0, |s| u32::try_from(s.len()).unwrap_or(u32::MAX));

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
            && (path.contains("../") || path.starts_with("../") || path.contains("..\\") || path.starts_with("..\\"))
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
    async fn execute_tool_with_validation(&self, tool_name: &str, args: Value) -> Result<Value> {
        self.execute_tool_with_validation_ref(tool_name, &args).await
    }

    // Internal reference-taking implementation to avoid cloning when not necessary.
    async fn execute_tool_with_validation_ref(&self, tool_name: &str, args: &Value) -> Result<Value> {
        if !self.config.enabled {
            return Err(anyhow!("MCP support is disabled in the current configuration"));
        }

        self.validate_tool_arguments(tool_name, args)?;

        let provider = self.resolve_provider_for_tool(tool_name).await?;
        let allowlist_snapshot = self.state.read().allowlist.clone();
        // Defense in depth: listing hides app-only tools, but a model could
        // still name one directly. Unknown names fall through to the
        // provider's own error below.
        let tools = provider
            .cached_tools_or_refresh_shared(&allowlist_snapshot, self.tool_timeout())
            .await?;
        if let Some(tool) = tools.iter().find(|tool| tool.name == tool_name)
            && !tool.visibility.model
        {
            bail!("MCP tool '{tool_name}' is not available to the model (app-only visibility)");
        }
        let result = provider
            .call_tool(tool_name, args, self.tool_timeout(), &allowlist_snapshot)
            .await?;

        Self::format_tool_result(&provider.name, tool_name, result)
    }

    /// Refresh the internal allow list at runtime.
    pub fn update_allowlist(&self, allowlist: McpAllowListConfig) {
        let providers: Vec<Arc<McpProvider>> = {
            let mut state = self.state.write();
            state.allowlist = allowlist;
            state.tool_provider_index.clear();
            state.resource_provider_index.clear();
            state.prompt_provider_index.clear();
            state.providers.values().cloned().collect()
        };

        for provider in providers {
            provider.invalidate_caches();
        }
    }

    /// Current allow list snapshot.
    pub fn current_allowlist(&self) -> McpAllowListConfig {
        self.state.read().allowlist.clone()
    }

    /// Return the provider name serving the given tool if previously cached.
    pub fn provider_for_tool(&self, tool_name: &str) -> Option<String> {
        self.state.read().tool_provider_index.get(tool_name).cloned()
    }

    /// Return the provider responsible for the given resource URI if known.
    pub fn provider_for_resource(&self, uri: &str) -> Option<String> {
        self.state.read().resource_provider_index.get(uri).cloned()
    }

    /// Return the provider that exposes the given prompt if known.
    pub fn provider_for_prompt(&self, prompt_name: &str) -> Option<String> {
        self.state.read().prompt_provider_index.get(prompt_name).cloned()
    }

    /// Execute a tool call on the appropriate provider.
    pub async fn execute_tool(&self, tool_name: &str, args: Value) -> Result<Value> {
        self.execute_tool_with_validation(tool_name, args).await
    }

    /// List all model-visible tools from all active providers.
    ///
    /// Tools without model visibility (SEP-1865 `visibility: ["app"]`) are
    /// hidden here; provider caches keep them for future app-side callers.
    pub async fn list_tools(&self) -> Result<Vec<McpToolInfo>> {
        Ok(self
            .collect_tools(false)
            .await?
            .into_iter()
            .filter(|tool| tool.visibility.model)
            .collect())
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
        let allowlist_snapshot = self.state.read().allowlist.clone();
        let data = provider.read_resource(uri, self.request_timeout(), &allowlist_snapshot).await?;
        drop(self.state.write().resource_provider_index.insert(uri.into(), provider_name));
        Ok(data)
    }

    /// Retrieve a rendered prompt from its originating provider.
    pub async fn get_prompt(
        &self,
        prompt_name: &str,
        arguments: Option<hashbrown::HashMap<String, String>>,
    ) -> Result<McpPromptDetail> {
        let provider = self.resolve_provider_for_prompt(prompt_name).await?;
        let provider_name = provider.name.clone();
        let allowlist_snapshot = self.state.read().allowlist.clone();
        let prompt = provider
            .get_prompt(prompt_name, arguments.unwrap_or_default(), self.request_timeout(), &allowlist_snapshot)
            .await?;
        drop(
            self.state
                .write()
                .prompt_provider_index
                .insert(prompt_name.into(), provider_name),
        );
        Ok(prompt)
    }

    /// Shutdown all active provider connections.
    pub async fn shutdown(&self) -> Result<()> {
        let providers: Vec<Arc<McpProvider>> = {
            let mut state = self.state.write();
            let values: Vec<_> = state.providers.values().cloned().collect();
            state.providers.clear();
            state.tool_provider_index.clear();
            state.resource_provider_index.clear();
            state.prompt_provider_index.clear();
            values
        };

        if providers.is_empty() {
            info!("No active MCP connections to shutdown");
            return Ok(());
        }

        info!("Shutting down {} MCP providers", providers.len());
        for provider in providers {
            if let Err(err) = provider.shutdown().await {
                warn!("Provider '{}' shutdown returned error: {err}", provider.name);
            }
        }
        Ok(())
    }

    async fn collect_tools(&self, force_refresh: bool) -> Result<Vec<McpToolInfo>> {
        // Collect provider references in one pass
        let (providers, allowlist) = {
            let state = self.state.read();
            (providers_sorted_by_name(&state.providers), state.allowlist.clone())
        };

        if providers.is_empty() {
            return Ok(Vec::new());
        }

        let timeout = self.tool_timeout();
        let mut all_tools = Vec::with_capacity(128);
        let mut index_updates: FxHashMap<String, String> = FxHashMap::with_capacity_and_hasher(128, Default::default());

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
                        let _ = index_updates.entry(tool.name.clone()).or_insert_with(|| provider_name.clone());
                    }
                    all_tools.extend(tools);
                }
                Err(err) => {
                    warn!("Failed to list tools for provider '{}': {err}", provider_name);
                }
            }
        }

        if !index_updates.is_empty() || force_refresh {
            let mut state = self.state.write();
            if index_updates.is_empty() {
                state.tool_provider_index.clear();
            } else {
                state.tool_provider_index = index_updates;
            }
        }

        Ok(all_tools)
    }

    async fn collect_resources(&self, force_refresh: bool) -> Result<Vec<McpResourceInfo>> {
        // Collect provider references in one pass
        let (providers, allowlist) = {
            let state = self.state.read();
            (providers_sorted_by_name(&state.providers), state.allowlist.clone())
        };

        if providers.is_empty() {
            self.state.write().resource_provider_index.clear();
            return Ok(Vec::new());
        }

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
                    warn!("Failed to list resources for provider '{}': {err}", provider.name);
                }
            }
        }

        let mut state = self.state.write();
        let index = &mut state.resource_provider_index;
        index.clear();
        for resource in &all_resources {
            let _ = index.entry(resource.uri.clone()).or_insert_with(|| resource.provider.clone());
        }

        Ok(all_resources)
    }

    async fn collect_prompts(&self, force_refresh: bool) -> Result<Vec<McpPromptInfo>> {
        // Collect provider references in one pass
        let (providers, allowlist) = {
            let state = self.state.read();
            (providers_sorted_by_name(&state.providers), state.allowlist.clone())
        };

        if providers.is_empty() {
            self.state.write().prompt_provider_index.clear();
            return Ok(Vec::new());
        }

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
                    warn!("Failed to list prompts for provider '{}': {err}", provider.name);
                }
            }
        }

        let mut state = self.state.write();
        let index = &mut state.prompt_provider_index;
        index.clear();
        for prompt in &all_prompts {
            let _ = index.entry(prompt.name.clone()).or_insert_with(|| prompt.provider.clone());
        }

        Ok(all_prompts)
    }

    async fn resolve_provider_for_tool(&self, tool_name: &str) -> Result<Arc<McpProvider>> {
        if !self.config.enabled {
            return Err(anyhow!("MCP support is disabled in the current configuration"));
        }

        if let Some(provider) = self.provider_for_tool(tool_name)
            && let Some(found) = self.state.read().providers.get(&provider)
        {
            return Ok(found.clone());
        }

        let (allowlist, providers) = {
            let state = self.state.read();
            (state.allowlist.clone(), providers_sorted_by_name(&state.providers))
        };
        let timeout = self.tool_timeout();

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
                    drop(
                        self.state
                            .write()
                            .tool_provider_index
                            .insert(tool_name.into(), provider.name.clone()),
                    );
                    return Ok(provider);
                }
                Ok(false) => continue,
                Err(err) => {
                    warn!("Error checking tool '{}' on provider '{}': {err}", tool_name, provider.name);
                }
            }
        }

        match self.collect_tools(true).await {
            Ok(_) => {
                if let Some(provider) = self.provider_for_tool(tool_name)
                    && let Some(found) = self.state.read().providers.get(&provider)
                {
                    return Ok(found.clone());
                }
            }
            Err(err) => {
                warn!("Failed to refresh MCP tool caches while resolving '{}': {err}", tool_name);
            }
        }

        Err(anyhow!(
            "Tool '{tool_name}' not found on any MCP provider.\n\n\
            To use this tool:\n\
            1. Install the MCP server: `uv tool install mcp-server-{tool_name}`\n\
            2. Add to vtcode.toml:\n   \
               [[mcp.providers]]\n   \
               name = \"{tool_name}\"\n   \
               command = \"uvx\"\n   \
               args = [\"mcp-server-{tool_name}\"]\n\
            3. Restart VT Code\n\n\
            Or use the built-in alternative if available (e.g., web_fetch instead of mcp_fetch)"
        ))
    }

    async fn resolve_provider_for_resource(&self, uri: &str) -> Result<Arc<McpProvider>> {
        if let Some(provider) = self.provider_for_resource(uri)
            && let Some(found) = self.state.read().providers.get(&provider)
        {
            return Ok(found.clone());
        }

        let (allowlist, providers) = {
            let state = self.state.read();
            (state.allowlist.clone(), providers_sorted_by_name(&state.providers))
        };
        let timeout = self.request_timeout();

        for provider in providers {
            match provider.has_resource(uri, &allowlist, timeout).await {
                Ok(true) => {
                    drop(
                        self.state
                            .write()
                            .resource_provider_index
                            .insert(uri.into(), provider.name.clone()),
                    );
                    return Ok(provider);
                }
                Ok(false) => continue,
                Err(err) => {
                    warn!("Error checking resource '{}' on provider '{}': {err}", uri, provider.name);
                }
            }
        }

        Err(anyhow!("Resource '{uri}' not found on any MCP provider"))
    }

    async fn resolve_provider_for_prompt(&self, prompt_name: &str) -> Result<Arc<McpProvider>> {
        if let Some(provider) = self.provider_for_prompt(prompt_name)
            && let Some(found) = self.state.read().providers.get(&provider)
        {
            return Ok(found.clone());
        }

        let (allowlist, providers) = {
            let state = self.state.read();
            (state.allowlist.clone(), providers_sorted_by_name(&state.providers))
        };
        let timeout = self.request_timeout();

        for provider in providers {
            match provider.has_prompt(prompt_name, &allowlist, timeout).await {
                Ok(true) => {
                    drop(
                        self.state
                            .write()
                            .prompt_provider_index
                            .insert(prompt_name.into(), provider.name.clone()),
                    );
                    return Ok(provider);
                }
                Ok(false) => continue,
                Err(err) => {
                    warn!("Error checking prompt '{}' on provider '{}': {err}", prompt_name, provider.name);
                }
            }
        }

        Err(anyhow!("Prompt '{prompt_name}' not found on any MCP provider"))
    }

    pub(super) fn record_tool_provider(&self, provider: &str, tools: &[McpToolInfo]) {
        let mut state = self.state.write();
        let index = &mut state.tool_provider_index;
        for tool in tools {
            let _ = index
                .entry(tool.name.clone())
                .and_modify(|owner| {
                    if provider < owner.as_str() {
                        *owner = provider.to_string();
                    }
                })
                .or_insert_with(|| provider.to_string());
        }
    }

    async fn connect_and_initialize_provider(
        &self,
        provider_config: &McpProviderConfig,
        allowlist_snapshot: &McpAllowListConfig,
        tool_timeout: Option<Duration>,
    ) -> Result<McpProvider> {
        let total_attempts = self.provider_retry_attempts();
        let mut last_error: Option<anyhow::Error> = None;

        for attempt_idx in 0..total_attempts {
            let attempt_number = attempt_idx + 1;
            match self
                .connect_and_initialize_provider_once(provider_config, allowlist_snapshot, tool_timeout)
                .await
            {
                Ok(provider) => return Ok(provider),
                Err(err) => {
                    if attempt_number == total_attempts {
                        return Err(err);
                    }

                    let retries_remaining = total_attempts - attempt_number;
                    warn!(
                        provider = provider_config.name.as_str(),
                        attempt = attempt_number,
                        retries_remaining,
                        error = %err,
                        "MCP provider initialization failed; retrying"
                    );
                    last_error = Some(err);
                    tokio::time::sleep(Self::provider_retry_delay(attempt_idx)).await;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("Failed to initialize MCP provider '{}'", provider_config.name)))
    }

    async fn connect_and_initialize_provider_once(
        &self,
        provider_config: &McpProviderConfig,
        allowlist_snapshot: &McpAllowListConfig,
        tool_timeout: Option<Duration>,
    ) -> Result<McpProvider> {
        let provider = McpProvider::connect(
            provider_config.clone(),
            self.elicitation_handler.clone(),
            self.sandbox_context.clone(),
            self.config.ui.apps.advertises_ui_extension(),
        )
        .await
        .with_context(|| format!("Failed to connect to MCP provider '{}'", provider_config.name))?;
        let provider_startup_timeout = self.resolve_startup_timeout(provider_config);
        provider
            .initialize(self.build_initialize_params(), provider_startup_timeout, tool_timeout, allowlist_snapshot)
            .await
            .with_context(|| format!("Failed to initialize MCP provider '{}'", provider_config.name))?;
        Ok(provider)
    }

    fn startup_timeout(&self) -> Option<Duration> {
        match self.config.startup_timeout_seconds {
            Some(0) => None,
            Some(value) => Some(Duration::from_secs(value)),
            None => self.request_timeout(),
        }
    }

    pub(super) fn requirement_mismatch_reason(&self, provider_config: &McpProviderConfig) -> Option<String> {
        let requirements = &self.config.requirements;
        if !requirements.enforce {
            return None;
        }

        match &provider_config.transport {
            McpTransportConfig::Stdio(stdio) => {
                if requirements
                    .allowed_stdio_commands
                    .iter()
                    .any(|allowed| allowed == &stdio.command)
                {
                    None
                } else {
                    Some(format!("stdio command '{}' is not allowlisted", stdio.command))
                }
            }
            McpTransportConfig::Http(http) => {
                if requirements
                    .allowed_http_endpoints
                    .iter()
                    .any(|allowed| allowed == &http.endpoint)
                {
                    None
                } else {
                    Some(format!("HTTP endpoint '{}' is not allowlisted", http.endpoint))
                }
            }
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

    fn provider_retry_attempts(&self) -> usize {
        self.config.retry_attempts.try_into().unwrap_or(usize::MAX).saturating_add(1)
    }

    #[must_use]
    fn provider_retry_delay(attempt_idx: usize) -> Duration {
        // Canonical 250ms-to-5s curve with the standard jitter: concurrent
        // provider reconnects must not retry in lockstep.
        let attempt = u32::try_from(attempt_idx).unwrap_or(u32::MAX);
        vtcode_commons::retry::RetryPolicy::from_retries(
            u32::MAX,
            Duration::from_millis(250),
            Duration::from_secs(5),
            2.0,
        )
        .with_jitter(vtcode_commons::retry::RetryPolicy::DEFAULT_JITTER)
        .delay_for_attempt(attempt)
    }

    pub(super) fn tool_timeout(&self) -> Option<Duration> {
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

    fn build_initialize_params(&self) -> InitializeRequestParams {
        let mut capabilities = ClientCapabilities::default();
        {
            let mut roots_cap = RootsCapabilities::default();
            roots_cap.list_changed = Some(true);
            capabilities.roots = Some(roots_cap);
        }

        if self.elicitation_handler.is_some() {
            // Elicitation is now a first-class capability in rmcp
            capabilities.elicitation = Some(
                rmcp::model::ElicitationCapability::new()
                    .with_form(rmcp::model::FormElicitationCapability::new().with_schema_validation(true)),
            );
        }

        apply_ui_extension_gate(&mut capabilities, self.config.ui.apps.advertises_ui_extension());

        InitializeRequestParams::new(capabilities, super::utils::build_client_implementation())
            .with_protocol_version(super::rmcp_client::latest_protocol_version())
    }

    pub(super) fn normalize_arguments(args: &Value) -> Map<String, Value> {
        match args {
            Value::Null => Map::new(),
            Value::Object(map) => map.clone(),
            other => {
                let mut map = Map::new();
                drop(map.insert("value".to_owned(), other.clone()));
                map
            }
        }
    }

    fn format_tool_result(provider_name: &str, tool_name: &str, result: CallToolResult) -> Result<Value> {
        if result.is_error == Some(true) {
            let message = result
                .content
                .iter()
                .filter_map(|block| block.as_text())
                .map(|block| block.text.as_str())
                .find(|text| !text.trim().is_empty())
                .unwrap_or("Unknown MCP tool error");
            return Err(anyhow!("MCP tool '{tool_name}' on provider '{provider_name}' reported an error: {message}"));
        }

        let mut payload = Map::new();
        drop(payload.insert("provider".into(), Value::String(provider_name.to_string())));
        drop(payload.insert("tool".into(), Value::String(tool_name.to_string())));

        if !result.content.is_empty() {
            let mut content = serde_json::to_value(result.content)?;
            if let Some(blocks) = content.as_array_mut() {
                for block in blocks {
                    if let Some(block) = block.as_object_mut() {
                        // Strip only protocol envelopes, never application data or text.
                        drop(block.remove("_meta"));
                        if block.get("type").and_then(Value::as_str) == Some("resource")
                            && let Some(resource) = block.get_mut("resource").and_then(Value::as_object_mut)
                        {
                            drop(resource.remove("_meta"));
                        }
                    }
                }
            }
            drop(payload.insert("content".into(), content));
        }
        if let Some(structured_content) = result.structured_content {
            drop(payload.insert("structuredContent".into(), structured_content));
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
        self.list_tools().await
    }

    async fn has_mcp_tool(&self, tool_name: &str) -> Result<bool> {
        if !self.config.enabled {
            return Ok(false);
        }

        if self.provider_for_tool(tool_name).is_some() {
            return Ok(true);
        }

        if self.state.read().providers.is_empty() {
            if self.config.providers.is_empty() {
                return Ok(false);
            }

            bail!("No MCP providers are currently connected. Ensure MCP initialization completed successfully.");
        }

        let tools = self.collect_tools(false).await?;
        Ok(tools.iter().any(|tool| tool.name == tool_name))
    }

    fn get_status(&self) -> McpClientStatus {
        self.get_status()
    }
}

#[cfg(test)]
mod tests;
