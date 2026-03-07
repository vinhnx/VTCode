use super::AgentRunner;
use crate::config::types::CapabilityLevel;
use crate::llm::provider::{LLMRequest, ToolDefinition};
use crate::tools::handlers::{SessionSurface, SessionToolsConfig, ToolModelCapabilities};
use anyhow::{Result, anyhow};
use hashbrown::HashSet;

impl AgentRunner {
    pub(super) async fn build_exposed_tool_definitions(&self) -> Result<Vec<ToolDefinition>> {
        let config = SessionToolsConfig {
            surface: SessionSurface::AgentRunner,
            capability_level: CapabilityLevel::CodeSearch,
            documentation_mode: self.config().agent.tool_documentation_mode,
            plan_mode: self.tool_registry.is_plan_mode(),
            request_user_input_enabled: false,
            model_capabilities: ToolModelCapabilities::for_model_name(&self.model),
        };

        let definitions = self.tool_registry.model_tools(config).await;
        let mut exposed = Vec::with_capacity(definitions.len());
        for tool in definitions {
            if self.is_tool_exposed(tool.function_name()).await {
                exposed.push(tool);
            }
        }

        Ok(exposed)
    }

    /// Build universal ToolDefinitions for the current agent.
    pub(super) async fn build_universal_tools(&self) -> Result<Vec<ToolDefinition>> {
        self.build_exposed_tool_definitions().await
    }

    /// Validate LLM request before sending to provider.
    /// Catches configuration errors early to avoid wasted API calls.
    #[allow(dead_code)]
    pub(super) fn validate_llm_request(&self, request: &LLMRequest) -> Result<()> {
        // Validate system prompt presence
        if request
            .system_prompt
            .as_ref()
            .is_none_or(|s| s.trim().is_empty())
        {
            return Err(anyhow!("System prompt cannot be empty"));
        }

        // Validate message history
        if request.messages.is_empty() {
            return Err(anyhow!("Message history cannot be empty"));
        }

        // Validate tools if present
        if let Some(tools) = &request.tools {
            let mut seen_names = HashSet::new();
            for tool in tools.iter() {
                self.validate_tool_definition(tool, &mut seen_names)?;
            }
        }

        // Validate model is specified
        if request.model.trim().is_empty() {
            return Err(anyhow!("Model identifier cannot be empty"));
        }

        Ok(())
    }

    /// Validate a single tool definition for schema correctness.
    #[allow(dead_code)]
    fn validate_tool_definition(
        &self,
        tool: &ToolDefinition,
        seen_names: &mut HashSet<String>,
    ) -> Result<()> {
        if let Some(func) = &tool.function {
            // Check name is not empty
            if func.name.trim().is_empty() {
                return Err(anyhow!("Tool function name cannot be empty"));
            }
            // Check for duplicate names
            if !seen_names.insert(func.name.clone()) {
                return Err(anyhow!("Duplicate tool name: {}", func.name));
            }
            // Validate parameters schema if it's an object
            if let Some(obj) = func.parameters.as_object()
                && let Some(required) = obj.get("required")
                && let Some(required_arr) = required.as_array()
                && let Some(props) = obj.get("properties").and_then(|p| p.as_object())
            {
                for req in required_arr {
                    if let Some(req_name) = req.as_str()
                        && !props.contains_key(req_name)
                    {
                        return Err(anyhow!(
                            "Tool '{}' has required field '{}' not in properties",
                            func.name,
                            req_name
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}
