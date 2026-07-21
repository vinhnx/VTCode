//! Prompt environment and interaction rendering helpers.

use crate::config::VTCodeConfig;
use crate::prompts::context::PromptContext;
use crate::prompts::temporal::generate_temporal_context;

/// Assemble the environment addenda section for the system prompt.
pub fn render_environment_addenda(
    vtcode_config: Option<&VTCodeConfig>,
    prompt_context: Option<&PromptContext>,
) -> Option<String> {
    let mut lines = Vec::new();

    if let Some(ctx) = prompt_context
        && !ctx.languages.is_empty()
    {
        lines.push(format!("- Languages: {}. Match structural-search `lang` when needed.", ctx.languages.join(", ")));
    }

    if let Some(cfg) = vtcode_config {
        if let Some(interaction_line) = render_interaction_addendum(cfg) {
            lines.push(interaction_line);
        }

        if cfg.mcp.enabled {
            lines.push("- Sources: prefer MCP before external fetches when available.".to_string());
        }

        if cfg.agent.include_temporal_context && !cfg.prompt_cache.cache_friendly_prompt_shaping {
            lines.push(
                generate_temporal_context(cfg.agent.temporal_context_use_utc)
                    .trim()
                    .replacen("Current date and time", "- Time", 1)
                    .to_string(),
            );
        }

        if cfg.agent.include_working_directory
            && let Some(ctx) = prompt_context
            && let Some(cwd) = &ctx.current_directory
        {
            lines.push(format!("- Working directory: {}", cwd.display()));
        }
    }

    if lines.is_empty() {
        None
    } else {
        Some(format!("## Environment\n{}", lines.join("\n")))
    }
}

/// Render the interaction addendum line based on HITL and ask_questions config.
fn render_interaction_addendum(cfg: &VTCodeConfig) -> Option<String> {
    match (cfg.security.human_in_the_loop, cfg.chat.ask_questions.enabled) {
        (true, true) => None,
        (true, false) => Some(
            "- Interaction: approval may gate sensitive actions; no `request_user_input`, so make reasonable assumptions unless Planning workflow needs follow-up.".to_string(),
        ),
        (false, true) => Some(
            "- Interaction: approval reduced by config; use `request_user_input` for material blockers.".to_string(),
        ),
        (false, false) => Some(
            "- Interaction: approval reduced by config; no `request_user_input`, so make reasonable assumptions unless Planning workflow needs follow-up.".to_string(),
        ),
    }
}
