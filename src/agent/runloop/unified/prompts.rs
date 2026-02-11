use std::path::Path;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::types::SystemPromptMode;
use vtcode_core::prompts::PromptContext;
use vtcode_core::prompts::system::{
    compose_system_instruction_text, default_system_prompt, minimal_system_prompt,
};

fn fallback_base_system_prompt(vt_cfg: Option<&vtcode_core::config::VTCodeConfig>) -> &'static str {
    match vt_cfg.map(|cfg| cfg.agent.system_prompt_mode) {
        Some(SystemPromptMode::Minimal) => minimal_system_prompt(),
        _ => default_system_prompt(),
    }
}

pub(crate) async fn read_system_prompt(
    workspace: &Path,
    session_addendum: Option<&str>,
    available_tools: &[String],
) -> String {
    // Build PromptContext with available information (workspace, current directory, tools)
    let mut prompt_context = PromptContext {
        workspace: Some(workspace.to_path_buf()),
        skip_standard_instructions: false,
        ..Default::default()
    };

    // Set current working directory
    if let Ok(cwd) = std::env::current_dir() {
        prompt_context.set_current_directory(cwd);
    }

    // Populate available tools so dynamic tool-aware guidelines match runtime capabilities.
    for tool in available_tools {
        prompt_context.add_tool(tool.clone());
    }
    if !prompt_context.available_tools.is_empty() {
        prompt_context.infer_capability_level();
    }

    // Load configuration
    let vt_cfg = ConfigManager::load_from_workspace(workspace)
        .ok()
        .map(|manager| manager.config().clone());

    // Use the new compose_system_instruction_text with enhancements
    let mut prompt =
        compose_system_instruction_text(workspace, vt_cfg.as_ref(), Some(&prompt_context)).await;

    // Fallback prompt if composition fails (should rarely happen)
    // Use centralized vtcode-core prompt variants to preserve safety/loop/plan guidance.
    if prompt.is_empty() {
        prompt = fallback_base_system_prompt(vt_cfg.as_ref()).to_string();
    }

    if let Some(addendum) = session_addendum {
        let trimmed = addendum.trim();
        if !trimmed.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(trimmed);
        }
    }

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::config::VTCodeConfig;
    use vtcode_core::config::types::SystemPromptMode;

    #[test]
    fn test_fallback_base_system_prompt_defaults_to_default() {
        assert_eq!(fallback_base_system_prompt(None), default_system_prompt());
    }

    #[test]
    fn test_fallback_base_system_prompt_uses_minimal_mode() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Minimal;

        assert_eq!(
            fallback_base_system_prompt(Some(&config)),
            minimal_system_prompt()
        );
    }
}
