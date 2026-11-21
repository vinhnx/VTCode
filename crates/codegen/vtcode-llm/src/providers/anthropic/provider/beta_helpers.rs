use super::*;

impl AnthropicProvider {
    pub(super) fn requires_advanced_tool_use_beta(&self, request: &LLMRequest) -> bool {
        request.tools.as_ref().is_some_and(|tools| {
            tools.iter().any(|tool| {
                (tool.is_tool_search() || tool.defer_loading.unwrap_or(false))
                    || tool.allowed_callers.as_ref().is_some_and(|callers| !callers.is_empty())
                    || tool.input_examples.as_ref().is_some_and(|examples| !examples.is_empty())
            })
        })
    }

    pub(super) fn code_execution_betas(&self, request: &LLMRequest) -> Vec<String> {
        request
            .tools
            .as_ref()
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(|tool| {
                        tool.is_anthropic_code_execution()
                            .then(|| code_execution_beta_name(&tool.tool_type))
                            .flatten()
                    })
                    .fold(Vec::new(), |mut betas, beta| {
                        if !betas.contains(&beta) {
                            betas.push(beta);
                        }
                        betas
                    })
            })
            .unwrap_or_default()
    }

    pub(super) fn context_management_betas(&self, request: &LLMRequest) -> Vec<&'static str> {
        let mut betas = Vec::new();

        if request
            .tools
            .as_ref()
            .is_some_and(|tools| tools.iter().any(ToolDefinition::is_anthropic_memory_tool))
        {
            betas.push(ANTHROPIC_CONTEXT_MANAGEMENT_BETA);
        }

        if let Some(context_management) = request.context_management.as_ref() {
            if uses_anthropic_compaction(context_management) {
                betas.push(ANTHROPIC_COMPACT_BETA);
            }

            if uses_anthropic_context_edits(context_management) && !betas.contains(&ANTHROPIC_CONTEXT_MANAGEMENT_BETA) {
                betas.push(ANTHROPIC_CONTEXT_MANAGEMENT_BETA);
            }
        }

        betas
    }

    /// Whether the advisor server-side tool should be sent for this request.
    ///
    /// Delegates to the request builder's `resolve_advisor_tool` so the beta
    /// header and the injected tool are gated by the exact same logic.
    pub(super) fn advisor_enabled_for_request(&self, request: &LLMRequest) -> bool {
        let executor = capabilities::resolve_model_name(&request.model, &self.model);
        request_builder::resolve_advisor_tool(executor, &self.anthropic_config.advisor).is_some()
    }
}
