use super::config::SystemPromptConfig;
use super::context::PromptContext;
use super::templates::PromptTemplates;

/// System prompt generator
pub struct SystemPromptGenerator<'a> {
    config: &'a SystemPromptConfig,
    context: &'a PromptContext,
}

impl<'a> SystemPromptGenerator<'a> {
    pub fn new(config: &'a SystemPromptConfig, context: &'a PromptContext) -> Self {
        Self { config, context }
    }

    /// Generate complete system prompt
    pub fn generate(&self) -> String {
        let mut prompt_parts = Vec::new();

        // Base system prompt
        prompt_parts.push(PromptTemplates::base_system_prompt().to_string());

        // Custom instruction if provided
        if let Some(custom) = &self.config.custom_instruction {
            prompt_parts.push(custom.clone());
        }

        // Personality
        prompt_parts
            .push(PromptTemplates::personality_prompt(&self.config.personality).to_string());

        // Response style
        prompt_parts
            .push(PromptTemplates::response_style_prompt(&self.config.response_style).to_string());

        // Tool usage if enabled
        if self.config.include_tools && !self.context.available_tools.is_empty() {
            prompt_parts.push(PromptTemplates::tool_usage_prompt().to_string());
            prompt_parts.push(format!(
                "Available tools: {}",
                self.context.available_tools.join(", ")
            ));
        }

        // Workspace context if enabled
        if self.config.include_workspace {
            if let Some(workspace) = &self.context.workspace {
                prompt_parts.push(PromptTemplates::workspace_context_prompt().to_string());
                prompt_parts.push(format!("Current workspace: {}", workspace.display()));
            }

            if !self.context.languages.is_empty() {
                prompt_parts.push(format!(
                    "Detected languages: {}",
                    self.context.languages.join(", ")
                ));
            }

            if let Some(project_type) = &self.context.project_type {
                prompt_parts.push(format!("Project type: {}", project_type));
            }
        }

        // Safety guidelines
        prompt_parts.push(PromptTemplates::safety_guidelines_prompt().to_string());

        prompt_parts.join("\n\n")
    }
}

/// Generate system instruction with configuration (backward compatibility function)
pub fn generate_system_instruction_with_config(
    config: &SystemPromptConfig,
    context: &PromptContext,
) -> String {
    let generator = SystemPromptGenerator::new(config, context);
    generator.generate()
}
