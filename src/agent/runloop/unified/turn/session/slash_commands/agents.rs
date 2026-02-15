use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::slash_commands::AgentCommandAction;

use super::{SlashCommandContext, SlashCommandControl};

pub async fn handle_manage_agents(
    ctx: SlashCommandContext<'_>,
    action: AgentCommandAction,
) -> Result<SlashCommandControl> {
    match action {
        AgentCommandAction::List => {
            ctx.renderer
                .line(MessageStyle::Info, "Built-in Subagents")?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  explore         - Fast read-only codebase search (haiku)",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  plan            - Research specialist for planning mode (sonnet)",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  general         - Multi-step tasks with full capabilities (sonnet)",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  code-reviewer   - Code quality and security review",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  debugger        - Error investigation and fixes",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Custom Subagents (project: .vtcode/agents/ | user: ~/.vtcode/agents/)",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;

            // Load and display custom agents from .vtcode/agents/ and ~/.vtcode/agents/
            let mut custom_agents = Vec::new();

            // 1. Check project agents (.vtcode/agents/)
            let project_agents_dir = ctx.config.workspace.join(".vtcode/agents");
            if project_agents_dir.exists()
                && let Ok(entries) = std::fs::read_dir(project_agents_dir)
            {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file()
                        && path.extension().and_then(|s| s.to_str()) == Some("md")
                        && let Some(name) = path.file_stem().and_then(|s| s.to_str())
                    {
                        custom_agents.push(format!("  {: <15} - (project)", name));
                    }
                }
            }

            // 2. Check user agents (~/.vtcode/agents/)
            if let Some(home) = dirs::home_dir() {
                let user_agents_dir = home.join(".vtcode/agents");
                if user_agents_dir.exists()
                    && let Ok(entries) = std::fs::read_dir(user_agents_dir)
                {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file()
                            && path.extension().and_then(|s| s.to_str()) == Some("md")
                            && !custom_agents.iter().any(|a| {
                                a.contains(path.file_stem().and_then(|s| s.to_str()).unwrap_or(""))
                            })
                            && let Some(name) = path.file_stem().and_then(|s| s.to_str())
                        {
                            custom_agents.push(format!("  {: <15} - (user)", name));
                        }
                    }
                }
            }

            if custom_agents.is_empty() {
                ctx.renderer.line(
                    MessageStyle::Output,
                    "  Use /agents create to add a custom agent",
                )?;
            } else {
                for agent in custom_agents {
                    ctx.renderer.line(MessageStyle::Output, &agent)?;
                }
            }
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Info,
                "More info: https://code.claude.com/docs/en/sub-agents",
            )?;
            Ok(SlashCommandControl::Continue)
        }
        AgentCommandAction::Create => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Creating a new subagent interactively...",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "Use Claude to generate a subagent configuration:",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  > I need a subagent that [describe what it should do]",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer
                .line(MessageStyle::Output, "Or edit manually:")?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer
                .line(MessageStyle::Output, "  mkdir -p .vtcode/agents")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  Create a .md file with YAML frontmatter in that directory",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Info,
                "For format details: https://code.claude.com/docs/en/sub-agents#file-format",
            )?;
            Ok(SlashCommandControl::Continue)
        }
        AgentCommandAction::Edit(agent_name) => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Editing subagent: {}", agent_name),
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "Edit the agent configuration file manually:",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  Project agents:  .vtcode/agents/{}.md",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  User agents:     ~/.vtcode/agents/{}.md",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Or use /edit command to open in your editor",
            )?;
            Ok(SlashCommandControl::Continue)
        }
        AgentCommandAction::Delete(agent_name) => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Deleting subagent: {}", agent_name),
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer
                .line(MessageStyle::Output, "Remove the agent configuration file:")?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                &format!("  rm .vtcode/agents/{}.md", agent_name),
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                &format!("  # or ~/.vtcode/agents/{}.md for user agents", agent_name),
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Changes take effect on next session start",
            )?;
            Ok(SlashCommandControl::Continue)
        }
        AgentCommandAction::Help => {
            ctx.renderer
                .line(MessageStyle::Info, "Subagent Management")?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "Usage: /agents [list|create|edit|delete] [options]",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  /agents              List all available subagents",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  /agents create       Create a new subagent interactively",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  /agents edit NAME    Edit an existing subagent",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  /agents delete NAME  Delete a subagent",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Documentation: https://code.claude.com/docs/en/sub-agents",
            )?;
            Ok(SlashCommandControl::Continue)
        }
    }
}
