//! MCP client management built on top of the Codex MCP building blocks.
//!
//! This module adapts the reference MCP client, server and type
//! definitions from <https://github.com/openai/codex> to integrate them
//! with VTCode's multi-provider configuration model. The original
//! implementation inside this project had grown organically and mixed a
//! large amount of bookkeeping logic with the lower level rmcp client
//! transport. The rewritten version keeps the VTCode specific surface
//! (allow lists, tool indexing, status reporting) but delegates the
//! actual protocol interaction to a lightweight `RmcpClient` adapter
//! that mirrors Codex' `mcp-client` crate. This dramatically reduces
//! the amount of bespoke glue we have to maintain while aligning the
//! behaviour with the upstream MCP implementations.

use crate::config::mcp::{
    McpAllowListConfig, McpClientConfig, McpProviderConfig, McpTransportConfig,
};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::Local;
use futures::FutureExt;
use iana_time_zone::get_timezone;
use mcp_types::{
    CallToolRequestParams, CallToolResult, CallToolResultContentItem, ClientCapabilities,
    ClientCapabilitiesRoots, GetPromptRequestParams, GetPromptResult, Implementation,
    InitializeRequestParams, InitializeResult, Prompt, PromptArgument, PromptMessage,
    ReadResourceRequestParams, ReadResourceResult, ReadResourceResultContentsItem, Resource,
    SUPPORTED_PROTOCOL_VERSIONS, Tool,
};
use parking_lot::RwLock;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use rmcp::handler::client::ClientHandler;
pub use rmcp::model::ElicitationAction;
use rmcp::model::{
    CancelledNotificationParam, CreateElicitationRequestParam, ListRootsResult, LoggingLevel,
    LoggingMessageNotificationParam, ProgressNotificationParam, ResourceUpdatedNotificationParam,
    Root,
};
use rmcp::service::{self, NotificationContext, RequestContext, RoleClient, RunningService};
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::transport::streamable_http_client::{
    StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Command;
use tokio::sync::{Mutex, Semaphore};
use tokio::time;
use tracing::{debug, error, info, warn};
use url::Url;

/// Information about an MCP tool exposed by a provider.
#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub provider: String,
    pub input_schema: Value,
}

/// Summary of an MCP resource exposed by a provider.
#[derive(Debug, Clone)]
pub struct McpResourceInfo {
    pub provider: String,
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<i64>,
}

/// Resource contents fetched from an MCP provider.
#[derive(Debug, Clone)]
pub struct McpResourceData {
    pub provider: String,
    pub uri: String,
    pub contents: Vec<ReadResourceResultContentsItem>,
    pub meta: Map<String, Value>,
}

/// Summary of an MCP prompt exposed by a provider.
#[derive(Debug, Clone)]
pub struct McpPromptInfo {
    pub provider: String,
    pub name: String,
    pub description: Option<String>,
    pub arguments: Vec<PromptArgument>,
}

/// Fully rendered MCP prompt ready for use.
#[derive(Debug, Clone)]
pub struct McpPromptDetail {
    pub provider: String,
    pub name: String,
    pub description: Option<String>,
    pub messages: Vec<PromptMessage>,
    pub meta: Map<String, Value>,
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
}

/// Result returned by an elicitation handler after interacting with the user.
#[derive(Debug, Clone)]
pub struct McpElicitationResponse {
    pub action: ElicitationAction,
    pub content: Option<Value>,
}

/// Callback interface used to resolve elicitation requests from MCP providers.
#[async_trait]
pub trait McpElicitationHandler: Send + Sync {
    async fn handle_elicitation(
        &self,
        provider: &str,
        request: McpElicitationRequest,
    ) -> Result<McpElicitationResponse>;
}

/// Trait abstraction used by the tool registry to talk to the MCP client.
#[async_trait]
pub trait McpToolExecutor: Send + Sync {
    async fn execute_mcp_tool(&self, tool_name: &str, args: Value) -> Result<Value>;
    async fn list_mcp_tools(&self) -> Result<Vec<McpToolInfo>>;
    async fn has_mcp_tool(&self, tool_name: &str) -> Result<bool>;
    fn get_status(&self) -> McpClientStatus;
}

/// High level MCP client responsible for managing multiple providers and
/// enforcing VTCode specific policies like tool allow lists.
pub struct McpClient {
    config: McpClientConfig,
    providers: RwLock<HashMap<String, Arc<McpProvider>>>,
    allowlist: RwLock<McpAllowListConfig>,
    tool_provider_index: RwLock<HashMap<String, String>>,
    resource_provider_index: RwLock<HashMap<String, String>>,
    prompt_provider_index: RwLock<HashMap<String, String>>,
    elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
}

const LOCAL_TIMEZONE_ENV_VAR: &str = "VT_LOCAL_TIMEZONE";
const TZ_ENV_VAR: &str = "TZ";
const TIMEZONE_ARGUMENT: &str = "timezone";

impl McpClient {
    /// Create a new MCP client from the configuration.
    pub fn new(config: McpClientConfig) -> Self {
        let allowlist = config.allowlist.clone();
        Self {
            config,
            providers: RwLock::new(HashMap::new()),
            allowlist: RwLock::new(allowlist),
            tool_provider_index: RwLock::new(HashMap::new()),
            resource_provider_index: RwLock::new(HashMap::new()),
            prompt_provider_index: RwLock::new(HashMap::new()),
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

        let startup_timeout = self.startup_timeout();
        let tool_timeout = self.tool_timeout();
        let allowlist_snapshot = self.allowlist.read().clone();

        let mut initialized = HashMap::new();

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
                    if let Err(err) = provider
                        .initialize(
                            self.build_initialize_params(&provider),
                            startup_timeout,
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
        let provider = self.resolve_provider_for_tool(tool_name).await?;
        let allowlist_snapshot = self.allowlist.read().clone();
        let result = provider
            .call_tool(tool_name, args, self.tool_timeout(), &allowlist_snapshot)
            .await?;
        Self::format_tool_result(&provider.name, tool_name, result)
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
            .insert(uri.to_string(), provider_name);
        Ok(data)
    }

    /// Retrieve a rendered prompt from its originating provider.
    pub async fn get_prompt(
        &self,
        prompt_name: &str,
        arguments: Option<HashMap<String, String>>,
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
            .insert(prompt_name.to_string(), provider_name);
        Ok(prompt)
    }

    /// Shutdown all active provider connections.
    pub async fn shutdown(&self) -> Result<()> {
        let providers: Vec<Arc<McpProvider>> = {
            let mut guard = self.providers.write();
            let values = guard.values().cloned().collect();
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
        McpClientStatus {
            enabled: self.config.enabled,
            provider_count: providers.len(),
            active_connections: providers.len(),
            configured_providers: providers.keys().cloned().collect(),
        }
    }

    async fn collect_tools(&self, force_refresh: bool) -> Result<Vec<McpToolInfo>> {
        let providers: Vec<Arc<McpProvider>> = self.providers.read().values().cloned().collect();

        if providers.is_empty() {
            return Ok(Vec::new());
        }

        let allowlist = self.allowlist.read().clone();
        let timeout = self.tool_timeout();
        let mut all_tools = Vec::new();

        for provider in providers {
            let tools = if force_refresh {
                provider.refresh_tools(&allowlist, timeout).await
            } else {
                provider.list_tools(&allowlist, timeout).await
            };

            match tools {
                Ok(tools) => {
                    self.record_tool_provider(&provider.name, &tools);
                    all_tools.extend(tools);
                }
                Err(err) => {
                    warn!(
                        "Failed to list tools for provider '{}': {err}",
                        provider.name
                    );
                }
            }
        }

        Ok(all_tools)
    }

    async fn collect_resources(&self, force_refresh: bool) -> Result<Vec<McpResourceInfo>> {
        let providers: Vec<Arc<McpProvider>> = self.providers.read().values().cloned().collect();

        if providers.is_empty() {
            self.resource_provider_index.write().clear();
            return Ok(Vec::new());
        }

        let allowlist = self.allowlist.read().clone();
        let timeout = self.request_timeout();
        let mut all_resources = Vec::new();

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
        let providers: Vec<Arc<McpProvider>> = self.providers.read().values().cloned().collect();

        if providers.is_empty() {
            self.prompt_provider_index.write().clear();
            return Ok(Vec::new());
        }

        let allowlist = self.allowlist.read().clone();
        let timeout = self.request_timeout();
        let mut all_prompts = Vec::new();

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
        if let Some(provider) = self.provider_for_tool(tool_name) {
            if let Some(found) = self.providers.read().get(&provider) {
                return Ok(found.clone());
            }
        }

        let allowlist = self.allowlist.read().clone();
        let timeout = self.tool_timeout();
        let providers: Vec<Arc<McpProvider>> = self.providers.read().values().cloned().collect();

        for provider in providers {
            match provider.has_tool(tool_name, &allowlist, timeout).await {
                Ok(true) => {
                    self.tool_provider_index
                        .write()
                        .insert(tool_name.to_string(), provider.name.clone());
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

        Err(anyhow!(
            "Tool '{}' not found on any MCP provider",
            tool_name
        ))
    }

    async fn resolve_provider_for_resource(&self, uri: &str) -> Result<Arc<McpProvider>> {
        if let Some(provider) = self.provider_for_resource(uri) {
            if let Some(found) = self.providers.read().get(&provider) {
                return Ok(found.clone());
            }
        }

        let allowlist = self.allowlist.read().clone();
        let timeout = self.request_timeout();
        let providers: Vec<Arc<McpProvider>> = self.providers.read().values().cloned().collect();

        for provider in providers {
            match provider.has_resource(uri, &allowlist, timeout).await {
                Ok(true) => {
                    self.resource_provider_index
                        .write()
                        .insert(uri.to_string(), provider.name.clone());
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
        if let Some(provider) = self.provider_for_prompt(prompt_name) {
            if let Some(found) = self.providers.read().get(&provider) {
                return Ok(found.clone());
            }
        }

        let allowlist = self.allowlist.read().clone();
        let timeout = self.request_timeout();
        let providers: Vec<Arc<McpProvider>> = self.providers.read().values().cloned().collect();

        for provider in providers {
            match provider.has_prompt(prompt_name, &allowlist, timeout).await {
                Ok(true) => {
                    self.prompt_provider_index
                        .write()
                        .insert(prompt_name.to_string(), provider.name.clone());
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

    fn build_initialize_params(&self, provider: &McpProvider) -> InitializeRequestParams {
        let mut capabilities = ClientCapabilities::default();
        capabilities.roots = Some(ClientCapabilitiesRoots {
            list_changed: Some(true),
        });

        InitializeRequestParams {
            capabilities,
            client_info: Implementation {
                name: "vtcode".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            protocol_version: provider.protocol_version.clone(),
        }
    }

    fn normalize_arguments(args: Value) -> Map<String, Value> {
        match args {
            Value::Null => Map::new(),
            Value::Object(map) => map,
            other => {
                let mut map = Map::new();
                map.insert("value".to_string(), other);
                map
            }
        }
    }

    fn format_tool_result(
        provider_name: &str,
        tool_name: &str,
        result: CallToolResult,
    ) -> Result<Value> {
        if result.is_error.unwrap_or(false) {
            let mut message = result
                .meta
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned);

            if message.is_none() {
                message = result.content.iter().find_map(|block| match block {
                    CallToolResultContentItem::TextContent(text) => Some(text.text.clone()),
                    _ => None,
                });
            }

            let message = message.unwrap_or_else(|| "Unknown MCP tool error".to_string());
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

        if !result.meta.is_empty() {
            payload.insert("meta".into(), Value::Object(result.meta.clone()));
        }

        if !result.content.is_empty() {
            payload.insert("content".into(), serde_json::to_value(result.content)?);
        }

        Ok(Value::Object(payload))
    }
}

#[async_trait]
impl McpToolExecutor for McpClient {
    async fn execute_mcp_tool(&self, tool_name: &str, args: Value) -> Result<Value> {
        self.execute_tool(tool_name, args).await
    }

    async fn list_mcp_tools(&self) -> Result<Vec<McpToolInfo>> {
        self.collect_tools(false).await
    }

    async fn has_mcp_tool(&self, tool_name: &str) -> Result<bool> {
        Ok(self.resolve_provider_for_tool(tool_name).await.is_ok())
    }

    fn get_status(&self) -> McpClientStatus {
        self.get_status()
    }
}

/// Wrapper around an individual MCP provider connection.
struct McpProvider {
    name: String,
    protocol_version: String,
    client: Arc<RmcpClient>,
    semaphore: Arc<Semaphore>,
    tools_cache: Mutex<Option<Vec<McpToolInfo>>>,
    resources_cache: Mutex<Option<Vec<McpResourceInfo>>>,
    prompts_cache: Mutex<Option<Vec<McpPromptInfo>>>,
    initialize_result: Mutex<Option<InitializeResult>>,
}

impl McpProvider {
    async fn connect(
        config: McpProviderConfig,
        elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
    ) -> Result<Self> {
        if config.name.trim().is_empty() {
            return Err(anyhow!("MCP provider name cannot be empty"));
        }

        let max_requests = std::cmp::max(1, config.max_concurrent_requests);

        let (client, protocol_version) = match &config.transport {
            McpTransportConfig::Stdio(stdio) => {
                let program = OsString::from(&stdio.command);
                let args: Vec<OsString> = stdio.args.iter().map(OsString::from).collect();
                let working_dir = stdio.working_directory.as_ref().map(PathBuf::from);
                let client = RmcpClient::new_stdio_client(
                    program,
                    args,
                    working_dir,
                    Some(config.env.clone()),
                    elicitation_handler.clone(),
                )
                .await?;
                (client, mcp_types::LATEST_PROTOCOL_VERSION.to_string())
            }
            McpTransportConfig::Http(http) => {
                if !SUPPORTED_PROTOCOL_VERSIONS
                    .iter()
                    .any(|supported| supported == &http.protocol_version)
                {
                    return Err(anyhow!(
                        "MCP HTTP provider '{}' requested unsupported protocol version '{}'",
                        config.name,
                        http.protocol_version
                    ));
                }

                let bearer_token = match http.api_key_env.as_ref() {
                    Some(var) => Some(std::env::var(var).with_context(|| {
                        format!("Missing MCP API key environment variable: {var}")
                    })?),
                    None => None,
                };

                let client = RmcpClient::new_streamable_http_client(
                    &config.name,
                    &http.endpoint,
                    bearer_token,
                    build_headers(&http.headers)?,
                    elicitation_handler.clone(),
                )
                .await?;
                (client, http.protocol_version.clone())
            }
        };

        Ok(Self {
            name: config.name,
            protocol_version,
            client: Arc::new(client),
            semaphore: Arc::new(Semaphore::new(max_requests)),
            tools_cache: Mutex::new(None),
            resources_cache: Mutex::new(None),
            prompts_cache: Mutex::new(None),
            initialize_result: Mutex::new(None),
        })
    }

    fn invalidate_caches(&self) {
        if let Ok(mut cache) = self.tools_cache.try_lock() {
            *cache = None;
        }
        if let Ok(mut cache) = self.resources_cache.try_lock() {
            *cache = None;
        }
        if let Ok(mut cache) = self.prompts_cache.try_lock() {
            *cache = None;
        }
    }

    async fn initialize(
        &self,
        params: InitializeRequestParams,
        startup_timeout: Option<Duration>,
        tool_timeout: Option<Duration>,
        allowlist: &McpAllowListConfig,
    ) -> Result<()> {
        let result = self.client.initialize(params, startup_timeout).await?;

        if !SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .any(|supported| supported == &result.protocol_version)
        {
            return Err(anyhow!(
                "MCP server for '{}' negotiated unsupported protocol version '{}'",
                self.name,
                result.protocol_version
            ));
        }

        *self.initialize_result.lock().await = Some(result);
        self.refresh_tools(allowlist, tool_timeout).await.ok();
        Ok(())
    }

    async fn list_tools(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpToolInfo>> {
        if let Some(cache) = self.tools_cache.lock().await.clone() {
            return Ok(cache);
        }

        self.refresh_tools(allowlist, timeout).await
    }

    async fn refresh_tools(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpToolInfo>> {
        let tools = self.client.list_all_tools(timeout).await?;
        let filtered = self.filter_tools(tools, allowlist);
        *self.tools_cache.lock().await = Some(filtered.clone());
        Ok(filtered)
    }

    async fn has_tool(
        &self,
        tool_name: &str,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        let tools = self.list_tools(allowlist, timeout).await?;
        Ok(tools.iter().any(|tool| tool.name == tool_name))
    }

    async fn call_tool(
        &self,
        tool_name: &str,
        args: Value,
        timeout: Option<Duration>,
        allowlist: &McpAllowListConfig,
    ) -> Result<CallToolResult> {
        if !allowlist.is_tool_allowed(&self.name, tool_name) {
            return Err(anyhow!(
                "Tool '{}' is blocked by the MCP allow list for provider '{}'",
                tool_name,
                self.name
            ));
        }

        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .context("Failed to acquire MCP request slot")?;
        let mut arguments = McpClient::normalize_arguments(args);
        self.add_argument_defaults(tool_name, &mut arguments, allowlist, timeout)
            .await
            .with_context(|| {
                format!(
                    "failed to prepare arguments for MCP tool '{}' on provider '{}'",
                    tool_name, self.name
                )
            })?;
        let params = CallToolRequestParams {
            name: tool_name.to_string(),
            arguments,
        };
        self.client.call_tool(params, timeout).await
    }

    async fn add_argument_defaults(
        &self,
        tool_name: &str,
        arguments: &mut Map<String, Value>,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<()> {
        let requires_timezone = self
            .tool_requires_field(tool_name, TIMEZONE_ARGUMENT, allowlist, timeout)
            .await?;
        ensure_timezone_argument(arguments, requires_timezone)?;
        Ok(())
    }

    async fn tool_requires_field(
        &self,
        tool_name: &str,
        field: &str,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        if let Some(tools) = self.tools_cache.lock().await.clone() {
            if let Some(tool) = tools.into_iter().find(|tool| tool.name == tool_name) {
                return Ok(schema_requires_field(&tool.input_schema, field));
            }
        }

        match self.refresh_tools(allowlist, timeout).await {
            Ok(tools) => Ok(tools
                .into_iter()
                .find(|tool| tool.name == tool_name)
                .map(|tool| schema_requires_field(&tool.input_schema, field))
                .unwrap_or(false)),
            Err(err) => {
                warn!(
                    "Failed to refresh tools while inspecting schema for '{}' on provider '{}': {err}",
                    tool_name, self.name
                );
                Ok(false)
            }
        }
    }

    async fn list_resources(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpResourceInfo>> {
        if let Some(cache) = self.resources_cache.lock().await.clone() {
            return Ok(cache);
        }

        self.refresh_resources(allowlist, timeout).await
    }

    async fn refresh_resources(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpResourceInfo>> {
        let resources = self.client.list_all_resources(timeout).await?;
        let filtered = self.filter_resources(resources, allowlist);
        *self.resources_cache.lock().await = Some(filtered.clone());
        Ok(filtered)
    }

    async fn has_resource(
        &self,
        uri: &str,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        let resources = self.list_resources(allowlist, timeout).await?;
        Ok(resources.iter().any(|resource| resource.uri == uri))
    }

    async fn read_resource(
        &self,
        uri: &str,
        timeout: Option<Duration>,
        allowlist: &McpAllowListConfig,
    ) -> Result<McpResourceData> {
        if !allowlist.is_resource_allowed(&self.name, uri) {
            return Err(anyhow!(
                "Resource '{}' is blocked by the MCP allow list for provider '{}'",
                uri,
                self.name
            ));
        }

        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .context("Failed to acquire MCP request slot")?;
        let params = ReadResourceRequestParams {
            uri: uri.to_string(),
        };
        let result = self.client.read_resource(params, timeout).await?;
        Ok(McpResourceData {
            provider: self.name.clone(),
            uri: uri.to_string(),
            contents: result.contents,
            meta: result.meta,
        })
    }

    async fn list_prompts(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpPromptInfo>> {
        if let Some(cache) = self.prompts_cache.lock().await.clone() {
            return Ok(cache);
        }

        self.refresh_prompts(allowlist, timeout).await
    }

    async fn refresh_prompts(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpPromptInfo>> {
        let prompts = self.client.list_all_prompts(timeout).await?;
        let filtered = self.filter_prompts(prompts, allowlist);
        *self.prompts_cache.lock().await = Some(filtered.clone());
        Ok(filtered)
    }

    async fn has_prompt(
        &self,
        prompt_name: &str,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        let prompts = self.list_prompts(allowlist, timeout).await?;
        Ok(prompts.iter().any(|prompt| prompt.name == prompt_name))
    }

    async fn get_prompt(
        &self,
        prompt_name: &str,
        arguments: HashMap<String, String>,
        timeout: Option<Duration>,
        allowlist: &McpAllowListConfig,
    ) -> Result<McpPromptDetail> {
        if !allowlist.is_prompt_allowed(&self.name, prompt_name) {
            return Err(anyhow!(
                "Prompt '{}' is blocked by the MCP allow list for provider '{}'",
                prompt_name,
                self.name
            ));
        }

        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .context("Failed to acquire MCP request slot")?;
        let params = GetPromptRequestParams {
            name: prompt_name.to_string(),
            arguments,
        };
        let result = self.client.get_prompt(params, timeout).await?;
        Ok(McpPromptDetail {
            provider: self.name.clone(),
            name: prompt_name.to_string(),
            description: result.description,
            messages: result.messages,
            meta: result.meta,
        })
    }

    async fn cached_tools(&self) -> Option<Vec<McpToolInfo>> {
        self.tools_cache.lock().await.clone()
    }

    async fn shutdown(&self) -> Result<()> {
        self.client.shutdown().await
    }

    fn filter_tools(&self, tools: Vec<Tool>, allowlist: &McpAllowListConfig) -> Vec<McpToolInfo> {
        tools
            .into_iter()
            .filter(|tool| allowlist.is_tool_allowed(&self.name, &tool.name))
            .map(|tool| McpToolInfo {
                description: tool.description.unwrap_or_default(),
                input_schema: serde_json::to_value(tool.input_schema).unwrap_or(Value::Null),
                provider: self.name.clone(),
                name: tool.name,
            })
            .collect()
    }

    fn filter_resources(
        &self,
        resources: Vec<Resource>,
        allowlist: &McpAllowListConfig,
    ) -> Vec<McpResourceInfo> {
        resources
            .into_iter()
            .filter(|resource| allowlist.is_resource_allowed(&self.name, &resource.uri))
            .map(|resource| McpResourceInfo {
                provider: self.name.clone(),
                uri: resource.uri,
                name: resource.name,
                description: resource.description,
                mime_type: resource.mime_type,
                size: resource.size,
            })
            .collect()
    }

    fn filter_prompts(
        &self,
        prompts: Vec<Prompt>,
        allowlist: &McpAllowListConfig,
    ) -> Vec<McpPromptInfo> {
        prompts
            .into_iter()
            .filter(|prompt| allowlist.is_prompt_allowed(&self.name, &prompt.name))
            .map(|prompt| McpPromptInfo {
                provider: self.name.clone(),
                name: prompt.name,
                description: prompt.description,
                arguments: prompt.arguments,
            })
            .collect()
    }
}

fn ensure_timezone_argument(
    arguments: &mut Map<String, Value>,
    requires_timezone: bool,
) -> Result<()> {
    if !requires_timezone || arguments.contains_key(TIMEZONE_ARGUMENT) {
        return Ok(());
    }

    let timezone = detect_local_timezone()
        .context("failed to determine a default timezone for MCP tool invocation")?;
    debug!("Injecting local timezone '{timezone}' for MCP tool call");
    arguments.insert(TIMEZONE_ARGUMENT.to_string(), Value::String(timezone));
    Ok(())
}

fn detect_local_timezone() -> Result<String> {
    if let Ok(value) = env::var(LOCAL_TIMEZONE_ENV_VAR) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    if let Ok(value) = env::var(TZ_ENV_VAR) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    match get_timezone() {
        Ok(timezone) => Ok(timezone),
        Err(err) => {
            let fallback = Local::now().format("%:z").to_string();
            warn!(
                "Falling back to numeric offset '{fallback}' after failing to resolve IANA timezone: {err}"
            );
            Ok(fallback)
        }
    }
}

fn schema_requires_field(schema: &Value, field: &str) -> bool {
    match schema {
        Value::Object(map) => {
            if map
                .get("required")
                .and_then(Value::as_array)
                .map(|items| items.iter().any(|item| item.as_str() == Some(field)))
                .unwrap_or(false)
            {
                return true;
            }

            for keyword in ["allOf", "anyOf", "oneOf"] {
                if let Some(subschemas) = map.get(keyword).and_then(Value::as_array) {
                    if subschemas
                        .iter()
                        .any(|subschema| schema_requires_field(subschema, field))
                    {
                        return true;
                    }
                }
            }

            if let Some(items) = map.get("items") {
                if schema_requires_field(items, field) {
                    return true;
                }
            }

            if let Some(properties) = map.get("properties").and_then(Value::as_object) {
                if let Some(property_schema) = properties.get(field) {
                    if schema_requires_field(property_schema, field) {
                        return true;
                    }
                }
            }

            false
        }
        _ => false,
    }
}

fn build_headers(headers: &HashMap<String, String>) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for (key, value) in headers {
        let name = HeaderName::from_bytes(key.as_bytes()).with_context(|| {
            format!("Invalid HTTP header name '{key}' in MCP provider configuration")
        })?;
        let header_value = HeaderValue::from_str(value).with_context(|| {
            format!("Invalid HTTP header value for '{key}' in MCP provider configuration")
        })?;
        map.insert(name, header_value);
    }
    Ok(map)
}

/// Lightweight adapter around the rmcp transport mirroring Codex' `RmcpClient` API.
struct RmcpClient {
    state: Mutex<ClientState>,
    elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
}

enum ClientState {
    Connecting {
        transport: Option<PendingTransport>,
    },
    Ready {
        service: Arc<RunningService<RoleClient, LoggingClientHandler>>,
    },
    Stopped,
}

enum PendingTransport {
    ChildProcess(TokioChildProcess),
    StreamableHttp(StreamableHttpClientTransport<reqwest::Client>),
}

impl RmcpClient {
    async fn new_stdio_client(
        program: OsString,
        args: Vec<OsString>,
        working_dir: Option<PathBuf>,
        env: Option<HashMap<String, String>>,
        elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
    ) -> Result<Self> {
        let mut command = Command::new(&program);
        command
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env_clear()
            .envs(create_env_for_mcp_server(env));

        if let Some(dir) = working_dir.as_ref() {
            command.current_dir(dir);
        }

        command.args(&args);

        let builder = TokioChildProcess::builder(command);
        let (transport, stderr) = builder.stderr(std::process::Stdio::piped()).spawn()?;

        if let Some(stderr) = stderr {
            let program_name = program.to_string_lossy().into_owned();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    debug!("MCP server stderr ({program_name}): {line}");
                }
            });
        }

        Ok(Self {
            state: Mutex::new(ClientState::Connecting {
                transport: Some(PendingTransport::ChildProcess(transport)),
            }),
            elicitation_handler,
        })
    }

    async fn new_streamable_http_client(
        server_name: &str,
        url: &str,
        bearer_token: Option<String>,
        headers: HeaderMap,
        elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
    ) -> Result<Self> {
        let mut config = StreamableHttpClientTransportConfig::with_uri(url.to_string());
        if let Some(token) = bearer_token {
            config = config.auth_header(token);
        }

        info!(
            "Connecting to MCP HTTP provider '{}' at {}",
            server_name, url
        );

        let client = if headers.is_empty() {
            reqwest::Client::builder().build()?
        } else {
            reqwest::Client::builder()
                .default_headers(headers)
                .build()?
        };

        let transport = StreamableHttpClientTransport::with_client(client, config);
        Ok(Self {
            state: Mutex::new(ClientState::Connecting {
                transport: Some(PendingTransport::StreamableHttp(transport)),
            }),
            elicitation_handler,
        })
    }

    async fn initialize(
        &self,
        params: InitializeRequestParams,
        timeout: Option<Duration>,
    ) -> Result<InitializeResult> {
        let handler = LoggingClientHandler::new(params, self.elicitation_handler.clone());

        let (transport_future, service_label) = {
            let mut guard = self.state.lock().await;
            match &mut *guard {
                ClientState::Connecting { transport } => match transport.take() {
                    Some(PendingTransport::ChildProcess(transport)) => (
                        service::serve_client(handler.clone(), transport).boxed(),
                        "stdio",
                    ),
                    Some(PendingTransport::StreamableHttp(transport)) => (
                        service::serve_client(handler.clone(), transport).boxed(),
                        "http",
                    ),
                    None => {
                        return Err(anyhow!(
                            "MCP client for {} already initializing",
                            handler.provider_name()
                        ));
                    }
                },
                ClientState::Ready { .. } => {
                    return Err(anyhow!(
                        "MCP client for {} already initialized",
                        handler.provider_name()
                    ));
                }
                ClientState::Stopped => return Err(anyhow!("MCP client has been shut down")),
            }
        };

        let service = match timeout {
            Some(duration) => time::timeout(duration, transport_future)
                .await
                .with_context(|| {
                    format!("Timed out establishing {service_label} MCP transport")
                })??,
            None => transport_future.await?,
        };

        let initialize_result_rmcp = service
            .peer()
            .peer_info()
            .ok_or_else(|| anyhow!("Handshake succeeded but server info missing"))?;
        let initialize_result = convert_to_mcp(initialize_result_rmcp)?;

        let mut guard = self.state.lock().await;
        *guard = ClientState::Ready {
            service: Arc::new(service),
        };

        Ok(initialize_result)
    }

    async fn list_all_tools(&self, timeout: Option<Duration>) -> Result<Vec<Tool>> {
        let service = self.service().await?;
        let rmcp_future = service.peer().list_all_tools();
        let rmcp_tools = run_with_timeout(rmcp_future, timeout, "tools/list").await?;

        rmcp_tools
            .into_iter()
            .map(|tool| convert_to_mcp::<_, Tool>(tool))
            .collect::<Result<Vec<_>>>()
            .context("Failed to convert MCP tool list")
    }

    async fn list_all_prompts(&self, timeout: Option<Duration>) -> Result<Vec<Prompt>> {
        let service = self.service().await?;
        let rmcp_future = service.peer().list_all_prompts();
        let rmcp_prompts = run_with_timeout(rmcp_future, timeout, "prompts/list").await?;

        rmcp_prompts
            .into_iter()
            .map(|prompt| convert_to_mcp::<_, Prompt>(prompt))
            .collect::<Result<Vec<_>>>()
            .context("Failed to convert MCP prompt list")
    }

    async fn list_all_resources(&self, timeout: Option<Duration>) -> Result<Vec<Resource>> {
        let service = self.service().await?;
        let rmcp_future = service.peer().list_all_resources();
        let rmcp_resources = run_with_timeout(rmcp_future, timeout, "resources/list").await?;

        rmcp_resources
            .into_iter()
            .map(|resource| convert_to_mcp::<_, Resource>(resource))
            .collect::<Result<Vec<_>>>()
            .context("Failed to convert MCP resource list")
    }

    async fn call_tool(
        &self,
        params: CallToolRequestParams,
        timeout: Option<Duration>,
    ) -> Result<CallToolResult> {
        let service = self.service().await?;
        let rmcp_params: rmcp::model::CallToolRequestParam = convert_to_rmcp(params)?;
        let rmcp_result =
            run_with_timeout(service.call_tool(rmcp_params), timeout, "tools/call").await?;
        convert_call_tool_result(rmcp_result)
    }

    async fn read_resource(
        &self,
        params: ReadResourceRequestParams,
        timeout: Option<Duration>,
    ) -> Result<ReadResourceResult> {
        let service = self.service().await?;
        let rmcp_params: rmcp::model::ReadResourceRequestParam = convert_to_rmcp(params)?;
        let rmcp_result = run_with_timeout(
            service.peer().read_resource(rmcp_params),
            timeout,
            "resources/read",
        )
        .await?;
        convert_to_mcp(rmcp_result).context("Failed to convert MCP resource contents")
    }

    async fn get_prompt(
        &self,
        params: GetPromptRequestParams,
        timeout: Option<Duration>,
    ) -> Result<GetPromptResult> {
        let service = self.service().await?;
        let rmcp_params: rmcp::model::GetPromptRequestParam = convert_to_rmcp(params)?;
        let rmcp_result = run_with_timeout(
            service.peer().get_prompt(rmcp_params),
            timeout,
            "prompts/get",
        )
        .await?;
        convert_to_mcp(rmcp_result).context("Failed to convert MCP prompt result")
    }

    async fn shutdown(&self) -> Result<()> {
        let mut guard = self.state.lock().await;
        let state = std::mem::replace(&mut *guard, ClientState::Stopped);
        drop(guard);

        match state {
            ClientState::Ready { service } => {
                service.cancellation_token().cancel();
                Ok(())
            }
            ClientState::Connecting { mut transport } => {
                drop(transport.take());
                Ok(())
            }
            ClientState::Stopped => Ok(()),
        }
    }

    async fn service(&self) -> Result<Arc<RunningService<RoleClient, LoggingClientHandler>>> {
        let guard = self.state.lock().await;
        match &*guard {
            ClientState::Ready { service } => Ok(service.clone()),
            ClientState::Connecting { .. } => Err(anyhow!("MCP client not initialized")),
            ClientState::Stopped => Err(anyhow!("MCP client has been shut down")),
        }
    }
}

#[derive(Clone)]
struct LoggingClientHandler {
    provider: String,
    initialize_params: InitializeRequestParams,
    elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
}

impl LoggingClientHandler {
    fn new(
        params: InitializeRequestParams,
        elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
    ) -> Self {
        let provider = params.client_info.name.clone();
        Self {
            provider,
            initialize_params: params,
            elicitation_handler,
        }
    }

    fn provider_name(&self) -> &str {
        &self.provider
    }

    fn handle_logging(&self, params: LoggingMessageNotificationParam) {
        let logger = params.logger.unwrap_or_else(|| "".to_string());
        let summary = params
            .data
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| params.data.to_string());

        match params.level {
            LoggingLevel::Debug => debug!(
                provider = self.provider.as_str(),
                logger = logger.as_str(),
                summary = %summary,
                payload = ?params.data,
                "MCP provider log"
            ),
            LoggingLevel::Info | LoggingLevel::Notice => info!(
                provider = self.provider.as_str(),
                logger = logger.as_str(),
                summary = %summary,
                payload = ?params.data,
                "MCP provider log"
            ),
            LoggingLevel::Warning => warn!(
                provider = self.provider.as_str(),
                logger = logger.as_str(),
                summary = %summary,
                payload = ?params.data,
                "MCP provider warning"
            ),
            LoggingLevel::Error
            | LoggingLevel::Critical
            | LoggingLevel::Alert
            | LoggingLevel::Emergency => error!(
                provider = self.provider.as_str(),
                logger = logger.as_str(),
                summary = %summary,
                payload = ?params.data,
                "MCP provider error"
            ),
        }
    }
}

impl ClientHandler for LoggingClientHandler {
    fn create_elicitation(
        &self,
        request: CreateElicitationRequestParam,
        _context: RequestContext<RoleClient>,
    ) -> impl std::future::Future<
        Output = Result<rmcp::model::CreateElicitationResult, rmcp::ErrorData>,
    > + Send
    + '_ {
        let provider = self.provider.clone();
        let handler = self.elicitation_handler.clone();
        async move {
            let default_response = rmcp::model::CreateElicitationResult {
                action: ElicitationAction::Decline,
                content: None,
            };

            if let Some(handler) = handler {
                let schema = Value::Object(request.requested_schema.clone());
                let message = request.message.clone();
                let payload = McpElicitationRequest {
                    message: message.clone(),
                    requested_schema: schema,
                };

                match handler.handle_elicitation(&provider, payload).await {
                    Ok(response) => {
                        info!(
                            provider = provider.as_str(),
                            message = message.as_str(),
                            action = ?response.action,
                            "MCP provider elicitation handled"
                        );
                        return Ok(rmcp::model::CreateElicitationResult {
                            action: response.action,
                            content: response.content,
                        });
                    }
                    Err(err) => {
                        warn!(
                            provider = provider.as_str(),
                            message = message.as_str(),
                            error = %err,
                            "Failed to process MCP elicitation; declining"
                        );
                    }
                }
            } else {
                info!(
                    provider = provider.as_str(),
                    message = request.message.as_str(),
                    "MCP provider requested elicitation but no handler configured; declining"
                );
            }

            Ok(default_response)
        }
    }

    fn list_roots(
        &self,
        _context: RequestContext<RoleClient>,
    ) -> impl std::future::Future<Output = Result<ListRootsResult, rmcp::ErrorData>> + Send + '_
    {
        let provider = self.provider.clone();
        async move {
            let mut roots = Vec::new();
            match std::env::current_dir() {
                Ok(dir) => {
                    if let Some(uri) = directory_to_file_uri(&dir) {
                        roots.push(Root {
                            name: Some("workspace".to_string()),
                            uri,
                        });
                    } else {
                        warn!(
                            provider = provider.as_str(),
                            path = %dir.display(),
                            "Failed to convert workspace directory to file URI for MCP roots"
                        );
                    }
                }
                Err(err) => {
                    warn!(
                        provider = provider.as_str(),
                        error = %err,
                        "Failed to resolve current directory for MCP roots"
                    );
                }
            }

            Ok(ListRootsResult { roots })
        }
    }

    fn on_cancelled(
        &self,
        params: CancelledNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        debug!(
            provider = self.provider.as_str(),
            request_id = %params.request_id,
            reason = ?params.reason,
            "MCP provider cancelled request"
        );
        async move {}
    }

    fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        info!(
            provider = self.provider.as_str(),
            progress_token = ?params.progress_token,
            progress = params.progress,
            total = ?params.total,
            message = ?params.message,
            "MCP provider progress update"
        );
        async move {}
    }

    fn on_logging_message(
        &self,
        params: LoggingMessageNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        self.handle_logging(params);
        async move {}
    }

    fn on_resource_updated(
        &self,
        params: ResourceUpdatedNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        info!(
            provider = self.provider.as_str(),
            uri = params.uri.as_str(),
            "MCP resource updated"
        );
        async move {}
    }

    fn on_resource_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        info!(
            provider = self.provider.as_str(),
            "MCP provider reported resource list change"
        );
        async move {}
    }

    fn on_tool_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        info!(
            provider = self.provider.as_str(),
            "MCP provider reported tool list change"
        );
        async move {}
    }

    fn on_prompt_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        info!(
            provider = self.provider.as_str(),
            "MCP provider reported prompt list change"
        );
        async move {}
    }

    fn get_info(&self) -> rmcp::model::ClientInfo {
        convert_to_rmcp(self.initialize_params.clone())
            .expect("initialize params conversion should not fail")
    }
}

fn directory_to_file_uri(path: &Path) -> Option<String> {
    Url::from_directory_path(path)
        .ok()
        .map(|url| url.to_string())
}

async fn run_with_timeout<F, T>(fut: F, timeout: Option<Duration>, label: &str) -> Result<T>
where
    F: std::future::Future<Output = Result<T, rmcp::service::ServiceError>>,
{
    if let Some(duration) = timeout {
        let result = time::timeout(duration, fut)
            .await
            .with_context(|| anyhow!("Timed out awaiting {label} after {duration:?}"))?;
        result.map_err(|err| anyhow!("{label} failed: {err}"))
    } else {
        fut.await.map_err(|err| anyhow!("{label} failed: {err}"))
    }
}

fn convert_call_tool_result(result: rmcp::model::CallToolResult) -> Result<CallToolResult> {
    let mut value = serde_json::to_value(result)?;
    if let Some(obj) = value.as_object_mut() {
        let missing_or_null = obj.get("content").map(Value::is_null).unwrap_or(true);
        if missing_or_null {
            obj.insert("content".to_string(), Value::Array(Vec::new()));
        }
    }
    serde_json::from_value(value).context("Failed to convert call tool result")
}

fn convert_to_rmcp<T, U>(value: T) -> Result<U>
where
    T: serde::Serialize,
    U: serde::de::DeserializeOwned,
{
    let json = serde_json::to_value(value)?;
    serde_json::from_value(json).map_err(|err| anyhow!(err))
}

fn convert_to_mcp<T, U>(value: T) -> Result<U>
where
    T: serde::Serialize,
    U: serde::de::DeserializeOwned,
{
    let json = serde_json::to_value(value)?;
    serde_json::from_value(json).map_err(|err| anyhow!(err))
}

fn create_env_for_mcp_server(
    extra_env: Option<HashMap<String, String>>,
) -> HashMap<String, String> {
    DEFAULT_ENV_VARS
        .iter()
        .filter_map(|var| {
            std::env::var(var)
                .ok()
                .map(|value| (var.to_string(), value))
        })
        .chain(extra_env.unwrap_or_default())
        .collect()
}

#[cfg(unix)]
const DEFAULT_ENV_VARS: &[&str] = &[
    "HOME",
    "LOGNAME",
    "PATH",
    "SHELL",
    "USER",
    "__CF_USER_TEXT_ENCODING",
    "LANG",
    "LC_ALL",
    "TERM",
    "TMPDIR",
    "TZ",
];

#[cfg(windows)]
const DEFAULT_ENV_VARS: &[&str] = &[
    "PATH",
    "PATHEXT",
    "USERNAME",
    "USERDOMAIN",
    "USERPROFILE",
    "TEMP",
    "TMP",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::mcp::{McpStdioServerConfig, McpTransportConfig};
    use serde_json::json;

    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            // SAFETY: Tests provide well-formed UTF-8 values and restore the
            // original value (if any) before dropping the guard, matching the
            // documented requirements for manipulating the process
            // environment.
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(ref original) = self.original {
                // SAFETY: Restores the previous UTF-8 environment value that
                // existed when the guard was created.
                unsafe {
                    std::env::set_var(self.key, original);
                }
            } else {
                // SAFETY: Removing the variable is safe because the guard is
                // the only code path mutating it during the test's lifetime.
                unsafe {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    #[test]
    fn schema_detection_handles_required_entries() {
        let schema = json!({
            "type": "object",
            "required": [TIMEZONE_ARGUMENT],
            "properties": {
                TIMEZONE_ARGUMENT: { "type": "string" }
            }
        });

        assert!(schema_requires_field(&schema, TIMEZONE_ARGUMENT));
        assert!(!schema_requires_field(&schema, "location"));
    }

    #[test]
    fn ensure_timezone_injects_from_override_env() {
        let _guard = EnvGuard::set(LOCAL_TIMEZONE_ENV_VAR, "Etc/UTC");
        let mut arguments = Map::new();

        ensure_timezone_argument(&mut arguments, true).unwrap();

        assert_eq!(
            arguments.get(TIMEZONE_ARGUMENT).and_then(Value::as_str),
            Some("Etc/UTC")
        );
    }

    #[test]
    fn ensure_timezone_does_not_override_existing_value() {
        let mut arguments = Map::new();
        arguments.insert(
            TIMEZONE_ARGUMENT.to_string(),
            Value::String("America/New_York".to_string()),
        );

        ensure_timezone_argument(&mut arguments, true).unwrap();

        assert_eq!(
            arguments.get(TIMEZONE_ARGUMENT).and_then(Value::as_str),
            Some("America/New_York")
        );
    }

    #[tokio::test]
    async fn convert_to_rmcp_round_trip() {
        let params = InitializeRequestParams {
            capabilities: ClientCapabilities {
                roots: Some(ClientCapabilitiesRoots {
                    list_changed: Some(true),
                }),
                ..Default::default()
            },
            client_info: Implementation {
                name: "vtcode".to_string(),
                version: "1.0".to_string(),
            },
            protocol_version: mcp_types::MCP_SCHEMA_VERSION.to_string(),
        };

        let converted: rmcp::model::InitializeRequestParam =
            convert_to_rmcp(params.clone()).unwrap();
        let round_trip: InitializeRequestParams = convert_to_mcp(converted).unwrap();
        assert_eq!(round_trip.client_info.name, "vtcode");
    }

    #[tokio::test]
    async fn provider_max_concurrency_defaults_to_one() {
        let config = McpProviderConfig {
            name: "test".into(),
            transport: McpTransportConfig::Stdio(McpStdioServerConfig {
                command: "cat".into(),
                args: vec![],
                working_directory: None,
            }),
            env: HashMap::new(),
            enabled: true,
            max_concurrent_requests: 0,
        };

        let provider = McpProvider::connect(config, None).await.unwrap();
        assert_eq!(provider.semaphore.available_permits(), 1);
    }

    #[test]
    fn directory_to_file_uri_generates_file_scheme() {
        let temp_dir = std::env::temp_dir();
        let uri = super::directory_to_file_uri(temp_dir.as_path())
            .expect("should create file uri for temp directory");
        assert!(uri.starts_with("file://"));
    }
}
