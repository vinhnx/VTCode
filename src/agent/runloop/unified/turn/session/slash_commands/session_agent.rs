use anyhow::Result;
use vtcode_config::SubagentSpec;
use vtcode_core::session_agent::{ActiveSessionAgentState, SessionAgentResolutionError};
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::slash_commands::SessionAgentCommandAction;

use super::{SlashCommandContext, SlashCommandControl};

pub(crate) async fn handle_manage_session_agent(
    mut ctx: SlashCommandContext<'_>,
    action: SessionAgentCommandAction,
) -> Result<SlashCommandControl> {
    match action {
        SessionAgentCommandAction::List => list_session_agents(&mut ctx).await?,
        SessionAgentCommandAction::Select { name } => select_session_agent(&mut ctx, &name).await?,
        SessionAgentCommandAction::Clear => clear_session_agent(&mut ctx)?,
    }

    Ok(SlashCommandControl::Continue)
}

async fn list_session_agents(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    let specs = effective_session_agent_specs(ctx).await?;
    let active_name = ctx
        .active_session_agent
        .active()
        .map(|agent| agent.identity.name.as_str());

    let current = active_name.unwrap_or("base");
    ctx.renderer
        .line(MessageStyle::Info, &format!("Session agent: {current}"))?;

    if specs.is_empty() {
        ctx.renderer
            .line(MessageStyle::Info, "No session-agent specs are available.")?;
        return Ok(());
    }

    ctx.renderer
        .line(MessageStyle::Info, "Available session agents:")?;
    for spec in &specs {
        let marker = if active_name == Some(spec.name.as_str()) {
            "*"
        } else {
            " "
        };
        let aliases = if spec.aliases.is_empty() {
            String::new()
        } else {
            format!(" (aliases: {})", spec.aliases.join(", "))
        };
        ctx.renderer.line(
            MessageStyle::Output,
            &format!("{marker} {}{aliases}", spec.name),
        )?;
    }

    Ok(())
}

async fn select_session_agent(ctx: &mut SlashCommandContext<'_>, name: &str) -> Result<()> {
    let specs = effective_session_agent_specs(ctx).await?;
    match select_session_agent_display_name(ctx.active_session_agent, &specs, name) {
        Ok(display_name) => {
            set_header_session_agent(ctx, Some(display_name.clone()));
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Session agent switched to {display_name}."),
            )?;
        }
        Err(SessionAgentResolutionError::UnknownAgent { requested }) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Unknown session agent '{requested}'."),
            )?;
        }
    }

    Ok(())
}

fn clear_session_agent(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    ctx.active_session_agent.clear();
    set_header_session_agent(ctx, None);
    ctx.renderer
        .line(MessageStyle::Info, "Session agent reset to base session.")?;
    Ok(())
}

async fn effective_session_agent_specs(ctx: &SlashCommandContext<'_>) -> Result<Vec<SubagentSpec>> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        return Ok(Vec::new());
    };
    Ok(controller.effective_specs().await)
}

fn set_header_session_agent(ctx: &mut SlashCommandContext<'_>, name: Option<String>) {
    ctx.header_context.session_agent = name.clone();
    ctx.handle.set_session_agent(name);
}

fn select_session_agent_display_name(
    state: &mut ActiveSessionAgentState,
    specs: &[SubagentSpec],
    name: &str,
) -> Result<String, SessionAgentResolutionError> {
    Ok(state.select_from_specs(specs, name)?.display_name.clone())
}

#[cfg(test)]
mod tests {
    use vtcode_config::{SubagentSource, SubagentSpec};

    use super::*;

    #[test]
    fn select_session_agent_display_name_uses_alias_matching() {
        let mut state = ActiveSessionAgentState::default();
        let mut spec = test_spec("reviewer");
        spec.aliases = vec!["critic".to_string()];

        let selected =
            select_session_agent_display_name(&mut state, &[spec], "CRITIC").expect("selected");

        assert_eq!(selected, "reviewer");
        assert_eq!(
            state.active().map(|agent| agent.identity.name.as_str()),
            Some("reviewer")
        );
    }

    #[test]
    fn select_session_agent_display_name_preserves_active_on_unknown() {
        let mut state = ActiveSessionAgentState::default();
        let spec = test_spec("planner");
        select_session_agent_display_name(&mut state, std::slice::from_ref(&spec), "planner")
            .expect("initial selection");

        let err = select_session_agent_display_name(&mut state, &[spec], "missing")
            .expect_err("unknown agent");

        assert_eq!(
            err,
            SessionAgentResolutionError::UnknownAgent {
                requested: "missing".to_string()
            }
        );
        assert_eq!(
            state.active().map(|agent| agent.identity.name.as_str()),
            Some("planner")
        );
    }

    fn test_spec(name: &str) -> SubagentSpec {
        SubagentSpec {
            name: name.to_string(),
            description: format!("{name} description"),
            prompt: format!("{name} prompt"),
            tools: None,
            disallowed_tools: Vec::new(),
            permission_mode: None,
            model: None,
            color: None,
            reasoning_effort: None,
            source: SubagentSource::ProjectVtcode,
            file_path: None,
            warnings: Vec::new(),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            max_turns: None,
            initial_prompt: None,
            nickname_candidates: Vec::new(),
            memory: None,
            isolation: None,
            aliases: Vec::new(),
        }
    }
}
