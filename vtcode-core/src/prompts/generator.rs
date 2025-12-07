use super::config::SystemPromptConfig;
use super::context::PromptContext;
use super::system_prompt_cache::PROMPT_CACHE;
use super::templates::PromptTemplates;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as FmtWrite;
use std::hash::{Hash, Hasher};

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
            }};
        }

        // Base system prompt
        append!(PromptTemplates::base_system_prompt());
        first = false;

        // Custom instruction if provided (borrowed, avoid clone)
        if let Some(custom) = self.config.custom_instruction.as_deref() {
            append!(custom);
            first = false;
        }

        // Personality and response style (static &'static str from templates)
        append!(PromptTemplates::personality_prompt(
            &self.config.personality
        ));
        first = false;
        append!(PromptTemplates::response_style_prompt(
            &self.config.response_style
        ));
        first = false;

        // Tool usage if enabled
        if self.config.include_tools && !self.context.available_tools.is_empty() {
            append!(PromptTemplates::tool_usage_prompt());
            let mut tools = self.context.available_tools.clone();
            tools.sort();
            tools.dedup();
            let tools = tools.join(", ");
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
                let mut langs = self.context.languages.clone();
                langs.sort();
                langs.dedup();
                let langs = langs.join(", ");
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
    let cache_key = cache_key(config, context);
    PROMPT_CACHE.get_or_insert_with(&cache_key, || {
        let generator = SystemPromptGenerator::new(config, context);
        generator.generate()
    })
}

fn cache_key(config: &SystemPromptConfig, context: &PromptContext) -> String {
    let mut hasher = DefaultHasher::new();

    config.verbose.hash(&mut hasher);
    config.include_tools.hash(&mut hasher);
    config.include_workspace.hash(&mut hasher);
    config.personality.hash(&mut hasher);
    config.response_style.hash(&mut hasher);
    if let Some(custom) = &config.custom_instruction {
        custom.hash(&mut hasher);
    }

    if let Some(workspace) = &context.workspace {
        workspace.hash(&mut hasher);
    }

    let mut languages = context.languages.clone();
    languages.sort();
    languages.dedup();
    languages.hash(&mut hasher);

    let mut tools = context.available_tools.clone();
    tools.sort();
    tools.dedup();
    tools.hash(&mut hasher);

    if let Some(project_type) = &context.project_type {
        project_type.hash(&mut hasher);
    }

    if let Some(preferences) = &context.user_preferences {
        preferences.preferred_languages.hash(&mut hasher);
        if let Some(style) = &preferences.coding_style {
            style.hash(&mut hasher);
        }
        preferences.preferred_frameworks.hash(&mut hasher);
    }

    format!("system_prompt:{:x}", hasher.finish())
}
