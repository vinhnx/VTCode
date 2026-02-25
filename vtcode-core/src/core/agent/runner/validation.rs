use super::AgentRunner;
use crate::llm::provider::{FunctionDefinition, LLMRequest, ToolDefinition};
use anyhow::{Result, anyhow};
use std::collections::HashSet;

impl AgentRunner {
    /// Build universal ToolDefinitions for the current agent.
    pub(super) async fn build_universal_tools(&mut self) -> Result<Vec<ToolDefinition>> {
        let gemini_tools = self.build_agent_tools().await?;

        // Convert Gemini tools to universal ToolDefinition format
        let tools: Vec<ToolDefinition> = gemini_tools
            .into_iter()
            .flat_map(|tool| tool.function_declarations)
            .map(|decl| ToolDefinition {
                tool_type: "function".to_owned(),
                function: Some(FunctionDefinition {
                    name: decl.name,
                    description: decl.description,
                    parameters: decl.parameters,
                }),
                web_search: None,
                shell: None,
                grammar: None,
                strict: None,
                defer_loading: None,
            })
            .collect();

        Ok(crate::prompts::sort_tool_definitions(tools))
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
