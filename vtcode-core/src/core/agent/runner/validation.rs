use super::AgentRunner;
use crate::config::types::CapabilityLevel;
use crate::core::agent::harness_kernel::SessionToolCatalogSnapshot;
use crate::llm::provider::{LLMRequest, ToolDefinition};
use crate::tools::handlers::{SessionSurface, SessionToolsConfig, ToolModelCapabilities};
use anyhow::{Result, anyhow};
use hashbrown::HashSet;

impl AgentRunner {
    pub(super) async fn build_exposed_tool_snapshot(&self) -> Result<SessionToolCatalogSnapshot> {
        let config = SessionToolsConfig {
            surface: SessionSurface::AgentRunner,
            capability_level: CapabilityLevel::CodeSearch,
            documentation_mode: self.config().agent.tool_documentation_mode,
            planning_active: self.tool_registry.is_planning_active(),
            request_user_input_enabled: false,
            model_capabilities: ToolModelCapabilities::for_model_name(&self.model),
            deferred_tool_policy: crate::tools::handlers::deferred_tool_policy_for_runtime(
                crate::llm::factory::infer_provider(
                    Some(&self.config().agent.provider),
                    &self.model,
                ),
                self.provider_client
                    .supports_responses_compaction(&self.model),
                Some(self.config()),
            ),
            anthropic_native_memory_enabled:
                crate::tools::handlers::anthropic_native_memory_enabled_for_runtime(
                    crate::llm::factory::infer_provider(
                        Some(&self.config().agent.provider),
                        &self.model,
                    ),
                    &self.model,
                    Some(self.config()),
                ),
        };

        let definitions = self.tool_registry.model_tools(config).await;
        let mut exposed = Vec::with_capacity(definitions.len());
        for tool in definitions {
            if self.is_tool_exposed(tool.function_name()).await {
                exposed.push(tool);
            }
        }

        Ok(self.tool_registry.tool_catalog_state().snapshot_for_defs(
            exposed,
            self.tool_registry.is_planning_active(),
            false,
        ))
    }

    pub(super) async fn build_exposed_tool_definitions(&self) -> Result<Vec<ToolDefinition>> {
        let snapshot = self.build_exposed_tool_snapshot().await?;
        Ok(snapshot
            .snapshot
            .as_ref()
            .map(|defs| defs.as_ref().clone())
            .unwrap_or_default())
    }

    /// Build universal ToolDefinitions for the current agent.
    pub(crate) async fn build_universal_tools(&self) -> Result<Vec<ToolDefinition>> {
        if let Some(definitions) = self.tool_definitions_override.read().clone() {
            return Ok(definitions);
        }
        self.build_exposed_tool_definitions().await
    }

    pub(crate) async fn build_universal_tool_snapshot(&self) -> Result<SessionToolCatalogSnapshot> {
        if let Some(definitions) = self.tool_definitions_override.read().clone() {
            return Ok(self.tool_registry.tool_catalog_state().snapshot_for_defs(
                definitions,
                self.tool_registry.is_planning_active(),
                false,
            ));
        }
        self.build_exposed_tool_snapshot().await
    }

    /// Validate LLM request before sending to provider.
    ///
    /// Cheap pre-flight gate: catches malformed requests (empty system prompt,
    /// no messages, duplicate tool names, or required schema fields missing
    /// from properties) before paying for an API round-trip. Borrows-only;
    /// no allocations beyond the pre-sized name set.
    ///
    /// # Errors
    ///
    /// Returns the first invariant violation encountered.
    pub(super) fn validate_llm_request(&self, request: &LLMRequest) -> Result<()> {
        if request
            .system_prompt
            .as_ref()
            .is_none_or(|s| s.trim().is_empty())
        {
            return Err(anyhow!("system prompt cannot be empty"));
        }
        if request.messages.is_empty() {
            return Err(anyhow!("message history cannot be empty"));
        }
        if request.model.trim().is_empty() {
            return Err(anyhow!("model identifier cannot be empty"));
        }

        if let Some(tools) = request.tools.as_deref() {
            let mut seen: HashSet<&str> = HashSet::with_capacity(tools.len());
            for tool in tools {
                validate_tool_definition(tool, &mut seen)?;
            }
        }

        Ok(())
    }
}

/// Validate a single tool definition for schema correctness.
///
/// Borrows the tool name into `seen` to detect duplicates without cloning.
fn validate_tool_definition<'a>(
    tool: &'a ToolDefinition,
    seen: &mut HashSet<&'a str>,
) -> Result<()> {
    let Some(func) = tool.function.as_ref() else {
        return Ok(());
    };

    let name = func.name.as_str();
    if name.trim().is_empty() {
        return Err(anyhow!("tool function name cannot be empty"));
    }
    if !seen.insert(name) {
        return Err(anyhow!("duplicate tool name: {name}"));
    }

    if let Some(obj) = func.parameters.as_object()
        && let Some(required_arr) = obj.get("required").and_then(|v| v.as_array())
        && let Some(props) = obj.get("properties").and_then(|p| p.as_object())
    {
        for req in required_arr.iter().filter_map(|v| v.as_str()) {
            if !props.contains_key(req) {
                return Err(anyhow!(
                    "tool '{name}' has required field '{req}' not in properties"
                ));
            }
        }
    }
    Ok(())
}
