//! Prompt environment and interaction rendering helpers.

use crate::config::VTCodeConfig;
use crate::prompts::context::PromptContext;
use crate::prompts::temporal::generate_temporal_date_context;

/// Assemble the environment addenda section for the system prompt.
pub fn render_environment_addenda(
    vtcode_config: Option<&VTCodeConfig>,
    prompt_context: Option<&PromptContext>,
) -> Option<String> {
    let mut lines = Vec::new();

    if let Some(ctx) = prompt_context
        && !ctx.languages.is_empty()
    {
        // The language list tells the model which languages the workspace
        // uses. The `code_search` schema owns the filter parameter, so this
        // line names the languages and not the parameter.
        lines.push(format!("- Languages: {}.", ctx.languages.join(", ")));
    }

    if let Some(cfg) = vtcode_config {
        if let Some(interaction_line) = render_interaction_addendum(cfg) {
            lines.push(interaction_line);
        }

        if cfg.mcp.enabled {
            lines.push(
                "- Sources: prefer MCP (including Agent Plugin servers) before external fetches when available."
                    .to_string(),
            );
        }

        // The system-prompt cache freezes this value at segment/session start.
        // Do not move temporal context into a per-turn runtime appendix: doing
        // so changes the cache prefix on every request. Date-only (no clock
        // time) keeps the cached prefix stable all day; precise time belongs
        // in a `<system-reminder>` history message when a task needs it.
        if cfg.agent.include_temporal_context {
            let date_context = generate_temporal_date_context(cfg.agent.temporal_context_use_utc);
            let trimmed = date_context.trim();
            let line = if let Some(rest) = trimmed.strip_prefix("Current date (UTC):") {
                format!("- Date (UTC): {}", rest.trim())
            } else {
                trimmed.replacen("Current date:", "- Date:", 1).to_string()
            };
            lines.push(line);
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
    match cfg.chat.ask_questions.enabled {
        true => None,
        false => Some(
            "- Interaction: no `request_user_input`; make reasonable assumptions unless Planning workflow needs follow-up."
                .to_string(),
        ),
    }
}
