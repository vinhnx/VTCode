use super::AgentRunner;
use crate::config::types::CapabilityLevel;
use crate::core::agent::harness_kernel::SessionToolCatalogSnapshot;
use crate::llm::provider::{LLMRequest, ToolDefinition};
use crate::tools::handlers::{SessionSurface, SessionToolsConfig, ToolModelCapabilities};
use anyhow::{Result, anyhow};
use hashbrown::HashSet;

impl AgentRunner {
    fn session_tools_config_for_snapshot(
        &self,
        planning_active: bool,
        request_user_input_enabled: bool,
    ) -> SessionToolsConfig {
        let provider = crate::llm::factory::infer_provider(Some(&self.config().agent.provider), &self.model);
        SessionToolsConfig {
            surface: SessionSurface::AgentRunner,
            capability_level: CapabilityLevel::CodeSearch,
            documentation_mode: self.config().agent.tool_documentation_mode,
            planning_active,
            request_user_input_enabled,
            model_capabilities: ToolModelCapabilities::for_model_name(&self.model),
            deferred_tool_policy: crate::tools::handlers::deferred_tool_policy_for_runtime(
                provider,
                self.provider_client.supports_responses_compaction(&self.model),
                Some(self.config()),
            ),
            anthropic_native_memory_enabled: crate::tools::handlers::anthropic_native_memory_enabled_for_runtime(
                provider,
                &self.model,
                Some(self.config()),
            ),
            tool_profile: self.config().tools.profile,
        }
    }

    async fn build_exposed_tool_snapshot(&self) -> Result<SessionToolCatalogSnapshot> {
        let planning_active = self.tool_registry.is_planning_active();
        let request_user_input_enabled = false;
        // Keep definitions stable across runtime mode transitions by retaining
        // both projections. Use the planning projection first so multi-action
        // schemas stay masked to their planning-safe shape when a tool exists
        // in both modes. Active names still follow the current profile, mode,
        // permissions, and allow-list.
        let mut config = self.session_tools_config_for_snapshot(true, true);
        let mut definitions = self.tool_registry.model_tools(config.clone()).await;
        let mut definition_names: HashSet<String> =
            definitions.iter().map(|tool| tool.function_name().to_string()).collect();
        config.planning_active = false;
        for tool in self.tool_registry.model_tools(config.clone()).await {
            if definition_names.insert(tool.function_name().to_string()) {
                definitions.push(tool);
            }
        }
        config.planning_active = planning_active;
        config.request_user_input_enabled = request_user_input_enabled;
        let runtime_tools = self.tool_registry.model_tools(config).await;
        if let Some(runtime_task_tracker) = runtime_tools
            .iter()
            .find(|tool| tool.function_name() == crate::config::constants::tools::TASK_TRACKER)
        {
            if let Some(existing) = definitions
                .iter_mut()
                .find(|tool| tool.function_name() == crate::config::constants::tools::TASK_TRACKER)
            {
                *existing = runtime_task_tracker.clone();
            } else {
                definitions.push(runtime_task_tracker.clone());
            }
        }
        let mut runtime_tool_names: HashSet<String> =
            runtime_tools.into_iter().map(|tool| tool.function_name().to_string()).collect();
        // Stale-allowlist detection must run against the actually emitted
        // current-mode tools, captured before the allowlist entries
        // themselves are merged in below (matching after the merge would
        // trivially succeed and the drift would stay silent).
        let emitted_runtime_names = runtime_tool_names.clone();
        let full_auto_allowlist = self.tool_registry.current_full_auto_allowlist().await;
        if let Some(allowlist) = full_auto_allowlist.as_ref() {
            runtime_tool_names.extend(allowlist.iter().cloned());
        }
        let supports_active_name_restriction = self.provider_client.supports_native_allowed_tools(&self.model);
        let (mut active_tool_names, mut exposed_definitions) = self
            .advertise_snapshot_tools(&definitions, &runtime_tool_names, supports_active_name_restriction, true)
            .await;
        // Stale-allowlist fallback (mirrors the interactive tool_shaping
        // guard): a non-empty full-auto allowlist whose entries match no
        // emitted tool is pure config drift, so degrade to the unrestricted
        // catalog for ADVERTISEMENT rather than handing the model a silent
        // zero-tool catalog (fatal on routes without native allowed-tools,
        // e.g. local models). An intentionally empty allowlist ("no tools
        // allowed") and permission-denial collapses are left untouched, and
        // execution dispatch keeps enforcing the allowlist at call time.
        if exposed_definitions.is_empty()
            && !definitions.is_empty()
            && let Some(allowlist) = full_auto_allowlist.as_ref()
            && !allowlist.is_empty()
            && !allowlist.iter().any(|entry| emitted_runtime_names.contains(entry.as_str()))
        {
            tracing::warn!(
                allowed_tools = ?allowlist,
                "automation.full_auto.allowed_tools matched no emitted tool; falling back to the full catalog for advertisement (execution still enforces the allowlist)"
            );
            (active_tool_names, exposed_definitions) = self
                .advertise_snapshot_tools(&definitions, &runtime_tool_names, supports_active_name_restriction, false)
                .await;
        }

        Ok(self
            .tool_registry
            .tool_catalog_state()
            .snapshot_for_stable_defs_with_active_names(
                exposed_definitions,
                active_tool_names,
                planning_active,
                request_user_input_enabled,
            ))
    }

    /// Partition `definitions` into advertised names and wire definitions.
    ///
    /// Extracted so `build_exposed_tool_snapshot` can re-run advertisement
    /// with the full-auto allowlist suspended for the stale-allowlist
    /// fallback without duplicating the permit/active/native gating.
    async fn advertise_snapshot_tools(
        &self,
        definitions: &[ToolDefinition],
        runtime_tool_names: &HashSet<String>,
        supports_active_name_restriction: bool,
        honor_full_auto_deny: bool,
    ) -> (Vec<String>, Vec<ToolDefinition>) {
        let mut active_tool_names = Vec::new();
        let mut exposed_definitions = Vec::new();
        for tool in definitions {
            let tool_name = tool.function_name().to_string();
            if !self
                .tool_permitted_for_advertisement_inner(tool_name.as_str(), honor_full_auto_deny)
                .await
            {
                continue;
            }
            let active = runtime_tool_names.contains(&tool_name)
                && self.tool_exposed_inner(tool_name.as_str(), honor_full_auto_deny).await;
            if active {
                active_tool_names.push(tool_name);
            }
            if supports_active_name_restriction || active {
                exposed_definitions.push(tool.clone());
            }
        }
        (active_tool_names, exposed_definitions)
    }

    async fn build_exposed_tool_definitions(&self) -> Result<Vec<ToolDefinition>> {
        let snapshot = self.build_exposed_tool_snapshot().await?;
        let active_tool_names: HashSet<&str> = snapshot.active_tool_names.iter().map(String::as_str).collect();
        Ok(snapshot
            .snapshot
            .as_ref()
            .map(|defs| {
                defs.iter()
                    .filter(|tool| active_tool_names.contains(tool.function_name()))
                    .cloned()
                    .collect()
            })
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
        if request.system_prompt.as_ref().is_none_or(|s| s.trim().is_empty()) {
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
fn validate_tool_definition<'a>(tool: &'a ToolDefinition, seen: &mut HashSet<&'a str>) -> Result<()> {
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
                return Err(anyhow!("tool '{name}' has required field '{req}' not in properties"));
            }
        }
    }
    Ok(())
}
