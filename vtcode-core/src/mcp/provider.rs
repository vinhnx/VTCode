use anyhow::{Context, Result, anyhow};
use mcp_types::{
    CallToolRequestParams, CallToolResult, GetPromptRequestParams, InitializeRequestParams,
    InitializeResult, Prompt, ReadResourceRequestParams, Resource, SUPPORTED_PROTOCOL_VERSIONS,
    Tool,
};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};
use tracing::warn;

use crate::config::mcp::{McpAllowListConfig, McpProviderConfig, McpTransportConfig};

use super::{
    McpElicitationHandler, McpPromptDetail, McpPromptInfo, McpResourceData, McpResourceInfo,
    McpToolInfo, TIMEZONE_ARGUMENT, build_headers, ensure_timezone_argument,
    schema_requires_field,
};
use super::{McpClient, RmcpClient};

pub struct McpProvider {
    pub(super) name: String,
    pub(super) protocol_version: String,
    client: Arc<RmcpClient>,
    semaphore: Arc<Semaphore>,
    tools_cache: Mutex<Option<Vec<McpToolInfo>>>,
    resources_cache: Mutex<Option<Vec<McpResourceInfo>>>,
    prompts_cache: Mutex<Option<Vec<McpPromptInfo>>>,
    initialize_result: Mutex<Option<InitializeResult>>,
}

impl McpProvider {
    pub(super) async fn connect(
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
                    config.name.clone(),
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

    pub(super) fn invalidate_caches(&self) {
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

    pub(super) async fn initialize(
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

    pub(super) async fn list_tools(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpToolInfo>> {
        if let Some(cache) = &*self.tools_cache.lock().await {
            return Ok(cache.clone());
        }

        self.refresh_tools(allowlist, timeout).await
    }

    pub(super) async fn refresh_tools(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpToolInfo>> {
        let tools = self.client.list_all_tools(timeout).await?;
        let filtered = self.filter_tools(tools, allowlist);
        *self.tools_cache.lock().await = Some(filtered.clone());
        Ok(filtered)
    }

    pub(super) async fn has_tool(
        &self,
        tool_name: &str,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        let tools = self.list_tools(allowlist, timeout).await?;
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
        if let Some(tools) = &*self.tools_cache.lock().await
            && let Some(tool) = tools.iter().find(|tool| tool.name == tool_name)
        {
            return Ok(schema_requires_field(&tool.input_schema, field));
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

    pub(super) async fn list_resources(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpResourceInfo>> {
        if let Some(cache) = &*self.resources_cache.lock().await {
            return Ok(cache.clone());
        }

        self.refresh_resources(allowlist, timeout).await
    }

    pub(super) async fn refresh_resources(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpResourceInfo>> {
        let resources = self.client.list_all_resources(timeout).await?;
        let filtered = self.filter_resources(resources, allowlist);
        *self.resources_cache.lock().await = Some(filtered.clone());
        Ok(filtered)
    }

    pub(super) async fn has_resource(
        &self,
        uri: &str,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        let resources = self.list_resources(allowlist, timeout).await?;
        Ok(resources.iter().any(|resource| resource.uri == uri))
    }

    pub(super) async fn read_resource(
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

    pub(super) async fn list_prompts(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpPromptInfo>> {
        if let Some(cache) = &*self.prompts_cache.lock().await {
            return Ok(cache.clone());
        }

        self.refresh_prompts(allowlist, timeout).await
    }

    pub(super) async fn refresh_prompts(
        &self,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<Vec<McpPromptInfo>> {
        let prompts = self.client.list_all_prompts(timeout).await?;
        let filtered = self.filter_prompts(prompts, allowlist);
        *self.prompts_cache.lock().await = Some(filtered.clone());
        Ok(filtered)
    }

    pub(super) async fn has_prompt(
        &self,
        prompt_name: &str,
        allowlist: &McpAllowListConfig,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        let prompts = self.list_prompts(allowlist, timeout).await?;
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

    pub(super) async fn cached_tools(&self) -> Option<Vec<McpToolInfo>> {
        self.tools_cache.lock().await.clone()
    }

    pub(super) async fn shutdown(&self) -> Result<()> {
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
