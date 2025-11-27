use super::config::SystemPromptConfig;
use super::context::PromptContext;
use super::templates::PromptTemplates;
use std::fmt::Write as FmtWrite;

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
        // Build prompt in-place into a single String to avoid many intermediate allocations
        let mut out = String::new();
        let mut first = true;

        // helper macro to append sections with a blank line separator
        macro_rules! append {
            ($s:expr) => {{
                if !first {
                    out.push_str("\n\n");
                }
                out.push_str($s);
                first = false;
            }};
        }

        // Base system prompt
        append!(PromptTemplates::base_system_prompt());

        // Custom instruction if provided (borrowed, avoid clone)
        if let Some(custom) = self.config.custom_instruction.as_deref() {
            append!(custom);
        }

        // Personality and response style (static &'static str from templates)
        append!(PromptTemplates::personality_prompt(
            &self.config.personality
        ));
        append!(PromptTemplates::response_style_prompt(
            &self.config.response_style
        ));

        // Tool usage if enabled
        if self.config.include_tools && !self.context.available_tools.is_empty() {
            append!(PromptTemplates::tool_usage_prompt());
            let tools = self.context.available_tools.join(", ");
            if !first {
                out.push_str("\n\n");
            }
            let _ = out.write_str("Available tools: ");
            let _ = out.write_str(&tools);
            first = false;
        }

        // Workspace context if enabled
        if self.config.include_workspace {
            if let Some(workspace) = &self.context.workspace {
                append!(PromptTemplates::workspace_context_prompt());
                if !first {
                    out.push_str("\n\n");
                }
                let _ = write!(out, "Current workspace: {}", workspace.display());
                first = false;
            }

            if !self.context.languages.is_empty() {
                let langs = self.context.languages.join(", ");
                if !first {
                    out.push_str("\n\n");
                }
                let _ = out.write_str("Detected languages: ");
                let _ = out.write_str(&langs);
                first = false;
            }

            if let Some(project_type) = &self.context.project_type {
                if !first {
                    out.push_str("\n\n");
                }
                let _ = out.write_str("Project type: ");
                let _ = out.write_str(project_type);
                first = false;
            }
        }

        // Safety guidelines
        append!(PromptTemplates::safety_guidelines_prompt());

        out
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
