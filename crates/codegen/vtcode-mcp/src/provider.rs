use anyhow::{Context, Result, anyhow};
use arc_swap::ArcSwap;
use hashbrown::HashMap;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, GetPromptRequestParams, InitializeRequestParams, Prompt,
    ReadResourceRequestParams, Resource, ServerPeerInfo, Tool,
};
use rmcp::service::ClientLifecycleMode;
use serde_json::{Map, Value};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};
use tracing::{Instrument, Span, warn};
use url::Url;

use super::rmcp_client::{auto_lifecycle_mode, latest_protocol_version};
use super::{LATEST_PROTOCOL_VERSION, SUPPORTED_PROTOCOL_VERSIONS};

use vtcode_config::auth::McpOAuthService;
use vtcode_config::mcp::{McpAllowListConfig, McpHttpHandshakeMode, McpProviderConfig, McpTransportConfig};
use vtcode_utility_tool_specs::parse_mcp_tool;

use super::sandbox_context::resolve_provider_sandbox_context;
use super::{McpClient, McpSandboxContext, RmcpClient};
use super::{
    McpElicitationHandler, McpPromptDetail, McpPromptInfo, McpResourceData, McpResourceInfo, McpToolInfo,
    TIMEZONE_ARGUMENT, apply_ui_extension_gate, build_headers, ensure_timezone_argument, schema_requires_field,
};

pub struct McpProvider {
    pub(super) name: String,
    #[expect(
        dead_code,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    protocol_version: String,
    client: ArcSwap<RmcpClient>,
    /// Stored config so we can reconnect after disconnection.
    config: McpProviderConfig,
    /// Stored elicitation handler for reconnection.
    elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
    /// Whether to advertise the MCP Apps UI extension, retained for reconnects.
    advertise_ui_apps: bool,
    /// Sandbox inherited by stdio launches and reconnects.
    sandbox_context: Option<McpSandboxContext>,
    pub(crate) semaphore: Arc<Semaphore>,
    caches: Mutex<ProviderCaches>,
    initialize_result: Mutex<Option<ServerPeerInfo>>,
}

#[derive(Default)]
struct ProviderCaches {
    tools: Option<Arc<Vec<McpToolInfo>>>,
    resources: Option<Arc<Vec<McpResourceInfo>>>,
    prompts: Option<Arc<Vec<McpPromptInfo>>>,
}

impl McpProvider {
    pub(super) async fn connect(
        config: McpProviderConfig,
        elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
        sandbox_context: Option<McpSandboxContext>,
        advertise_ui_apps: bool,
    ) -> Result<Self> {
        if config.name.trim().is_empty() {
            return Err(anyhow!("MCP provider name cannot be empty"));
        }

        let sandbox_context = resolve_provider_sandbox_context(sandbox_context, config.sandbox.as_ref())
            .with_context(|| format!("MCP provider '{}' has unsatisfiable sandbox overrides", config.name))?;

        let max_requests = std::cmp::max(1, config.max_concurrent_requests);

        let (client, protocol_version) = match &config.transport {
            McpTransportConfig::Stdio(stdio) => {
                let program = OsString::from(&stdio.command);
                let args: Vec<OsString> = stdio.args.iter().map(OsString::from).collect();
                let working_dir = stdio.working_directory.as_ref().map(PathBuf::from);
                let env: HashMap<OsString, OsString> = config
                    .env
                    .iter()
                    .map(|(key, value)| (OsString::from(key), OsString::from(value)))
                    .collect();
                let client = RmcpClient::new_stdio_client(
                    config.name.clone(),
                    program,
                    args,
                    working_dir,
                    Some(env),
                    elicitation_handler.clone(),
                    sandbox_context.clone(),
                )
                .await?;
                (client, LATEST_PROTOCOL_VERSION.to_string())
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

                let bearer_token = if let Some(oauth) = http.oauth.as_ref() {
                    McpOAuthService::new()
                        .resolve_access_token_for_resource(&config.name, oauth, &http.endpoint)
                        .await?
                        .ok_or_else(|| {
                            anyhow!(
                                "MCP HTTP provider '{}' requires OAuth login. Run `vtcode mcp login {}`.",
                                config.name,
                                config.name
                            )
                        })
                        .map(Some)?
                } else {
                    match http.api_key_env.as_ref() {
                        Some(var) => Some(
                            std::env::var(var)
                                .with_context(|| format!("Missing MCP API key environment variable: {var}"))?,
                        ),
                        None => None,
                    }
                };

                let headers = build_headers(&http.http_headers, &http.env_http_headers);
                let client = RmcpClient::new_streamable_http_client(
                    config.name.clone(),
                    &http.endpoint,
                    bearer_token,
                    headers,
                    elicitation_handler.clone(),
                )
                .await?;
                (client, http.protocol_version.clone())
            }
        };

        Ok(Self {
            name: config.name.clone(),
            protocol_version,
            client: ArcSwap::from_pointee(client),
            config,
            elicitation_handler,
            advertise_ui_apps,
            sandbox_context,
            semaphore: Arc::new(Semaphore::new(max_requests)),
            caches: Mutex::new(ProviderCaches::default()),
            initialize_result: Mutex::new(None),
        })
    }

    pub(super) fn invalidate_caches(&self) {
        if let Ok(mut caches) = self.caches.try_lock() {
            caches.tools = None;
            caches.resources = None;
            caches.prompts = None;
        }
    }

    pub(super) async fn initialize(
        &self,
        params: InitializeRequestParams,
        startup_timeout: Option<Duration>,
        _tool_timeout: Option<Duration>,
        _allowlist: &McpAllowListConfig,
    ) -> Result<()> {
        let client = self.client.load_full();
        let result = client.initialize(params, startup_timeout, self.handshake_lifecycle()).await?;

        let protocol_version_str = result.protocol_version.to_string();
        if !SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .any(|supported| *supported == protocol_version_str)
        {
            return Err(anyhow!(
                "MCP server for '{}' negotiated unsupported protocol version '{}'",
                self.name,
                protocol_version_str
            ));
        }

        *self.initialize_result.lock().await = Some(result);
        Ok(())
    }

    /// Lifecycle mode for the rmcp handshake, derived from the transport.
    ///
    /// stdio always probes `server/discover` first (`Auto`) since a local
    /// child process answers immediately either way. HTTP defaults to the
    /// direct legacy handshake so legacy-only servers (e.g. DeepWiki) are not
    /// penalized by the discover-timeout fallback; opt in to `Auto` per
    /// provider via `handshake = "auto"` for modern servers.
    fn handshake_lifecycle(&self) -> ClientLifecycleMode {
        match &self.config.transport {
            McpTransportConfig::Stdio(_) => auto_lifecycle_mode(),
            McpTransportConfig::Http(http) => match http.handshake {
                McpHttpHandshakeMode::Legacy => ClientLifecycleMode::Initialize,
                McpHttpHandshakeMode::Auto => auto_lifecycle_mode(),
            },
        }
    }

    /// Protocol version negotiated during the last successful handshake, if any.
    pub(super) async fn negotiated_protocol_version(&self) -> Option<String> {
        self.initialize_result
            .lock()
            .await
            .as_ref()
            .map(|info| info.protocol_version.to_string())
    }

    pub(super) async fn list_tools(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpToolInfo>> {
        Ok(self.list_tools_shared(allowlist, timeout).await?.as_ref().clone())
    }

    async fn list_tools_shared(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Arc<Vec<McpToolInfo>>> {
        let mut caches = self.caches.lock().await;
        if self.client.load_full().take_tool_list_changed() {
            caches.tools = None;
        }

        if let Some(cache) = &caches.tools {
            return Ok(Arc::clone(cache));
        }
        drop(caches);

        self.refresh_tools_shared(allowlist, timeout).await
    }

    pub(super) async fn refresh_tools(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpToolInfo>> {
        Ok(self.refresh_tools_shared(allowlist, timeout).await?.as_ref().clone())
    }

    async fn refresh_tools_shared(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Arc<Vec<McpToolInfo>>> {
        let client = self.client.load_full();
        let tools = client.list_all_tools(timeout).await?;
        let filtered = Arc::new(self.filter_tools(tools, allowlist));
        self.caches.lock().await.tools = Some(Arc::clone(&filtered));
        Ok(filtered)
    }

    pub(super) async fn has_tool(
        &self,
        tool_name: &str,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        let tools = self.list_tools_shared(allowlist, timeout).await?;
        Ok(tools.iter().any(|tool| tool.name == tool_name))
    }

    pub(super) async fn call_tool(
        &self,
        tool_name: &str,
        args: &Value,
        timeout: Option<Duration>,
        allowlist: &McpAllowListConfig,
    ) -> Result<CallToolResult> {
        if !allowlist.is_tool_allowed(&self.name, tool_name) {
            return Err(anyhow!("Tool '{}' is blocked by the MCP allow list for provider '{}'", tool_name, self.name));
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
                format!("failed to prepare arguments for MCP tool '{}' on provider '{}'", tool_name, self.name)
            })?;
        let params = CallToolRequestParams::new(tool_name.to_string()).with_arguments(arguments);
        let client = self.client.load_full();
        async move { client.call_tool(params, timeout).await }
            .instrument(mcp_tool_call_span(&self.name, tool_name, &self.config.transport))
            .await
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
        if let Some(tools) = &self.caches.lock().await.tools
            && let Some(tool) = tools.iter().find(|tool| tool.name == tool_name)
        {
            return Ok(schema_requires_field(&tool.input_schema, field));
        }

        match self.refresh_tools_shared(allowlist, timeout).await {
            Ok(tools) => Ok(tools
                .iter()
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

    pub(super) async fn list_resources(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpResourceInfo>> {
        Ok(self.list_resources_shared(allowlist, timeout).await?.as_ref().clone())
    }

    async fn list_resources_shared(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Arc<Vec<McpResourceInfo>>> {
        let mut caches = self.caches.lock().await;
        if self.client.load_full().take_resource_list_changed() {
            caches.resources = None;
        }

        if let Some(cache) = &caches.resources {
            return Ok(Arc::clone(cache));
        }
        drop(caches);

        self.refresh_resources_shared(allowlist, timeout).await
    }

    pub(super) async fn refresh_resources(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpResourceInfo>> {
        Ok(self.refresh_resources_shared(allowlist, timeout).await?.as_ref().clone())
    }

    async fn refresh_resources_shared(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Arc<Vec<McpResourceInfo>>> {
        let client = self.client.load_full();
        let resources = client.list_all_resources(timeout).await?;
        let filtered = Arc::new(self.filter_resources(resources, allowlist));
        self.caches.lock().await.resources = Some(Arc::clone(&filtered));
        Ok(filtered)
    }

    pub(super) async fn has_resource(
        &self,
        uri: &str,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        let resources = self.list_resources_shared(allowlist, timeout).await?;
        Ok(resources.iter().any(|resource| resource.uri == uri))
    }

    pub(super) async fn read_resource(
        &self,
        uri: &str,
        timeout: Option<Duration>,
        allowlist: &McpAllowListConfig,
    ) -> Result<McpResourceData> {
        if !allowlist.is_resource_allowed(&self.name, uri) {
            return Err(anyhow!("Resource '{}' is blocked by the MCP allow list for provider '{}'", uri, self.name));
        }

        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .context("Failed to acquire MCP request slot")?;
        let params = ReadResourceRequestParams::new(uri.to_string());
        let client = self.client.load_full();
        let result = client.read_resource(params, timeout).await?;
        Ok(McpResourceData {
            provider: self.name.clone(),
            uri: uri.to_string(),
            contents: result.contents,
            meta: Map::new(),
        })
    }

    pub(super) async fn list_prompts(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpPromptInfo>> {
        Ok(self.list_prompts_shared(allowlist, timeout).await?.as_ref().clone())
    }

    async fn list_prompts_shared(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Arc<Vec<McpPromptInfo>>> {
        let mut caches = self.caches.lock().await;
        if self.client.load_full().take_prompt_list_changed() {
            caches.prompts = None;
        }

        if let Some(cache) = &caches.prompts {
            return Ok(Arc::clone(cache));
        }
        drop(caches);

        self.refresh_prompts_shared(allowlist, timeout).await
    }

    pub(super) async fn refresh_prompts(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpPromptInfo>> {
        Ok(self.refresh_prompts_shared(allowlist, timeout).await?.as_ref().clone())
    }

    async fn refresh_prompts_shared(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Arc<Vec<McpPromptInfo>>> {
        let client = self.client.load_full();
        let prompts = client.list_all_prompts(timeout).await?;
        let filtered = Arc::new(self.filter_prompts(prompts, allowlist));
        self.caches.lock().await.prompts = Some(Arc::clone(&filtered));
        Ok(filtered)
    }

    pub(super) async fn has_prompt(
        &self,
        prompt_name: &str,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        let prompts = self.list_prompts_shared(allowlist, timeout).await?;
        Ok(prompts.iter().any(|prompt| prompt.name == prompt_name))
    }

    pub(super) async fn get_prompt(
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
        // Convert HashMap<String, String> to JsonObject (BTreeMap<String, Value>)
        let args_json: Map<String, Value> = arguments.into_iter().map(|(k, v)| (k, Value::String(v))).collect();

        let params = GetPromptRequestParams::new(prompt_name.to_string()).with_arguments(args_json);
        let client = self.client.load_full();
        let result = client.get_prompt(params, timeout).await?;
        Ok(McpPromptDetail {
            provider: self.name.clone(),
            name: prompt_name.to_string(),
            description: result.description,
            messages: result.messages,
            meta: Map::new(),
        })
    }

    /// Return the cached tool list as a shared handle.
    ///
    /// The cache already stores `Arc<Vec<McpToolInfo>>`; returning the `Arc`
    /// avoids deep-cloning every tool description and JSON schema on each hit.
    pub(super) async fn cached_tools_shared(&self) -> Option<Arc<Vec<McpToolInfo>>> {
        self.caches.lock().await.tools.as_ref().map(Arc::clone)
    }

    /// Return cached tools or refresh them, handing back the shared cache entry.
    ///
    /// Callers that only need to read/record the list can keep the `Arc` and
    /// skip the per-call deep clone that the owned-`Vec` return used to force.
    pub(super) async fn cached_tools_or_refresh_shared(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Arc<Vec<McpToolInfo>>> {
        if let Some(tools) = self.cached_tools_shared().await {
            return Ok(tools);
        }

        self.refresh_tools_shared(allowlist, timeout).await
    }

    pub(super) async fn shutdown(&self) -> Result<()> {
        let client = self.client.load_full();
        client.shutdown().await
    }

    /// Returns `true` when the underlying transport is still connected and responsive.
    pub(super) async fn is_healthy(&self) -> bool {
        let client = self.client.load_full();
        client.is_healthy().await
    }

    /// Attempt to re-establish the MCP connection using the stored configuration.
    ///
    /// This replaces the inner [`RmcpClient`] with a freshly connected one, then
    /// re-initialises the provider (tools/resources/prompts caches are invalidated).
    pub(super) async fn reconnect(
        &self,
        startup_timeout: Option<Duration>,
        tool_timeout: Option<Duration>,
        allowlist: &McpAllowListConfig,
    ) -> Result<()> {
        tracing::info!(provider = self.name.as_str(), "Attempting MCP reconnection");

        // Shut down the old client (best-effort).
        {
            let old = self.client.load_full();
            drop(old.shutdown().await);
        }

        // Create a fresh client from the stored config.
        let new_provider = McpProvider::connect(
            self.config.clone(),
            self.elicitation_handler.clone(),
            self.sandbox_context.clone(),
            self.advertise_ui_apps,
        )
        .await
        .with_context(|| format!("MCP reconnect failed for provider '{}'", self.name))?;

        // Swap inner client.
        {
            let new_client = new_provider.client.load_full();
            self.client.store(new_client);
        }

        // Invalidate all caches before re-initialisation.
        self.invalidate_caches();

        // Re-initialise (handshake + tool refresh).
        let init_params = build_reconnect_initialize_params(self.advertise_ui_apps);
        self.initialize(init_params, startup_timeout, tool_timeout, allowlist)
            .await
            .with_context(|| format!("MCP re-initialization failed for provider '{}'", self.name))?;

        tracing::info!(provider = self.name.as_str(), "MCP reconnection successful");
        Ok(())
    }

    fn filter_tools(&self, tools: Vec<Tool>, allowlist: &McpAllowListConfig) -> Vec<McpToolInfo> {
        filter_tools_sorted(&self.name, tools, allowlist)
    }

    fn filter_resources(&self, resources: Vec<Resource>, allowlist: &McpAllowListConfig) -> Vec<McpResourceInfo> {
        filter_resources_sorted(&self.name, resources, allowlist)
    }

    fn filter_prompts(&self, prompts: Vec<Prompt>, allowlist: &McpAllowListConfig) -> Vec<McpPromptInfo> {
        filter_prompts_sorted(&self.name, prompts, allowlist)
    }
}

/// Build initialize params for a provider reconnect, retaining the UI advertisement.
fn build_reconnect_initialize_params(advertise_ui_apps: bool) -> InitializeRequestParams {
    let mut capabilities = rmcp::model::ClientCapabilities::default();
    apply_ui_extension_gate(&mut capabilities, advertise_ui_apps);
    InitializeRequestParams::new(capabilities, super::utils::build_client_implementation())
        .with_protocol_version(latest_protocol_version())
}

// Servers may reorder lists between calls; a stable order keeps LLM prompt-cache prefixes valid.
fn filter_tools_sorted(provider: &str, tools: Vec<Tool>, allowlist: &McpAllowListConfig) -> Vec<McpToolInfo> {
    let mut filtered: Vec<McpToolInfo> = tools
        .into_iter()
        .filter(|tool| allowlist.is_tool_allowed(provider, &tool.name))
        .map(|tool| {
            let parsed = parse_mcp_tool(&tool);
            McpToolInfo {
                description: parsed.description,
                input_schema: parsed.input_schema,
                output_schema: parsed.output_schema,
                provider: provider.to_owned(),
                name: parsed.name,
                ui: parsed.ui,
                visibility: parsed.visibility,
            }
        })
        .collect();
    filtered.sort_by(|a, b| a.name.cmp(&b.name));
    filtered
}

fn filter_resources_sorted(
    provider: &str,
    resources: Vec<Resource>,
    allowlist: &McpAllowListConfig,
) -> Vec<McpResourceInfo> {
    let mut filtered: Vec<McpResourceInfo> = resources
        .into_iter()
        .filter(|resource| allowlist.is_resource_allowed(provider, &resource.uri))
        .map(|resource| McpResourceInfo {
            provider: provider.to_owned(),
            uri: resource.uri.clone(),
            name: resource.name.clone(),
            description: resource.description.clone(),
            mime_type: resource.mime_type.clone(),
            size: resource.size.map(|s| i64::try_from(s).unwrap_or(i64::MAX)),
        })
        .collect();
    filtered.sort_by(|a, b| a.uri.cmp(&b.uri));
    filtered
}

fn filter_prompts_sorted(provider: &str, prompts: Vec<Prompt>, allowlist: &McpAllowListConfig) -> Vec<McpPromptInfo> {
    let mut filtered: Vec<McpPromptInfo> = prompts
        .into_iter()
        .filter(|prompt| allowlist.is_prompt_allowed(provider, &prompt.name))
        .map(|prompt| McpPromptInfo {
            provider: provider.to_owned(),
            name: prompt.name.clone(),
            description: prompt.description.clone(),
            arguments: prompt.arguments.clone().unwrap_or_default(),
        })
        .collect();
    filtered.sort_by(|a, b| a.name.cmp(&b.name));
    filtered
}

fn mcp_tool_call_span(provider_name: &str, tool_name: &str, transport: &McpTransportConfig) -> Span {
    let (transport_label, server_address, server_port) = match transport {
        McpTransportConfig::Stdio(_) => ("stdio", String::new(), 0_u16),
        McpTransportConfig::Http(http) => {
            let (server_address, server_port) = Url::parse(&http.endpoint)
                .ok()
                .and_then(|url| {
                    url.host_str()
                        .map(|host| (host.to_string(), url.port_or_known_default().unwrap_or_default()))
                })
                .unwrap_or_default();
            ("streamable_http", server_address, server_port)
        }
    };

    tracing::info_span!(
        "mcp.tools.call",
        provider = provider_name,
        tool = tool_name,
        rpc_system = "jsonrpc",
        rpc_method = "tools/call",
        transport = transport_label,
        server_address = server_address.as_str(),
        server_port = server_port,
    )
}
#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
