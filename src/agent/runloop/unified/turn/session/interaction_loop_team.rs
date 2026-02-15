use anyhow::Result;
use vtcode_core::agent_teams::TeamRole;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::inline_events::TeamSwitchDirection;
use crate::agent::runloop::unified::state::SessionStats;

use super::interaction_loop::InteractionLoopContext;

pub(super) fn direct_message_target(session_stats: &SessionStats) -> Option<String> {
    let context = session_stats.team_context.as_ref()?;
    if context.role != TeamRole::Lead {
        return None;
    }
    session_stats
        .team_state
        .as_ref()
        .and_then(|team| team.active_teammate())
        .map(|name| name.to_string())
}

pub(super) async fn handle_team_switch(
    ctx: &mut InteractionLoopContext<'_>,
    direction: TeamSwitchDirection,
) -> Result<()> {
    let role = ctx.session_stats.team_context.as_ref().map(|ctx| ctx.role);
    if matches!(role, Some(TeamRole::Teammate)) {
        ctx.renderer.line(
            MessageStyle::Info,
            "Active teammate selection is only available to the lead.",
        )?;
        return Ok(());
    }

    let Some(team) = ctx.session_stats.team_state.as_mut() else {
        ctx.renderer
            .line(MessageStyle::Info, "No active team. Use /team start.")?;
        return Ok(());
    };

    let mut options = Vec::new();
    options.push(None);
    for name in team.teammate_names() {
        options.push(Some(name));
    }

    if options.len() <= 1 {
        ctx.renderer
            .line(MessageStyle::Info, "No teammates to select.")?;
        return Ok(());
    }

    let current = team.active_teammate().map(|name| name.to_string());
    let current_idx = options
        .iter()
        .position(|entry| entry.as_deref() == current.as_deref())
        .unwrap_or(0);

    let next_idx = match direction {
        TeamSwitchDirection::Next => (current_idx + 1) % options.len(),
        TeamSwitchDirection::Previous => {
            if current_idx == 0 {
                options.len() - 1
            } else {
                current_idx - 1
            }
        }
    };

    let next = options[next_idx].clone();
    team.set_active_teammate(next.clone()).await?;
    let label = next.as_deref().unwrap_or("lead");
    ctx.renderer
        .line(MessageStyle::Info, &format!("Active teammate: {}.", label))?;

    Ok(())
}

pub(super) async fn poll_team_mailbox(ctx: &mut InteractionLoopContext<'_>) -> Result<()> {
    let team_context = match ctx.session_stats.team_context.as_ref() {
        Some(context) => context.clone(),
        None => return Ok(()),
    };

    if ctx.session_stats.team_state.is_none() {
        let storage =
            vtcode_core::agent_teams::TeamStorage::from_config(ctx.vt_cfg.as_ref()).await?;
        match crate::agent::runloop::unified::team_state::TeamState::load(
            storage,
            &team_context.team_name,
        )
        .await
        {
            Ok(mut team) => {
                let r = match team_context.role {
                    TeamRole::Lead => "lead",
                    TeamRole::Teammate => {
                        team_context.teammate_name.as_deref().unwrap_or("teammate")
                    }
                };
                let _ = team.load_persisted_offset(r).await;
                ctx.session_stats.team_state = Some(team);
            }
            Err(err) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to load team '{}': {}", team_context.team_name, err),
                )?;
                ctx.session_stats.team_context = None;
                return Ok(());
            }
        }
    }

    let recipient = match team_context.role {
        TeamRole::Lead => "lead".to_string(),
        TeamRole::Teammate => team_context
            .teammate_name
            .clone()
            .unwrap_or_else(|| "teammate".to_string()),
    };

    let Some(team) = ctx.session_stats.team_state.as_mut() else {
        return Ok(());
    };
    team.reload_tasks().await?;

    let messages = team.read_mailbox(&recipient).await?;
    for message in &messages {
        if let Some(proto) = &message.protocol {
            let injected = handle_team_protocol(ctx, &message.sender, proto)?;
            if let Some(text) = injected {
                ctx.conversation_history.push(uni::Message::system(text));
            }
            continue;
        }

        let text = message.content.as_deref().unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }

        let mut header = format!("Team message from {}", message.sender);
        if let Some(task_id) = message.task_id {
            header.push_str(&format!(" (task #{})", task_id));
        }
        ctx.renderer.line(MessageStyle::Info, &header)?;
        ctx.renderer.line(MessageStyle::Output, text)?;

        let injected = if let Some(task_id) = message.task_id {
            format!(
                "[Team message from {} re task #{}]\n{}",
                message.sender, task_id, text
            )
        } else {
            format!("[Team message from {}]\n{}", message.sender, text)
        };
        ctx.conversation_history.push(uni::Message::user(injected));
    }

    Ok(())
}

fn handle_team_protocol(
    ctx: &mut InteractionLoopContext<'_>,
    sender: &str,
    proto: &vtcode_core::agent_teams::TeamProtocolMessage,
) -> Result<Option<String>> {
    use vtcode_core::agent_teams::TeamProtocolType;
    let inject = match proto.r#type {
        TeamProtocolType::IdleNotification => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Teammate '{}' is now idle.", sender),
            )?;
            Some(format!(
                "[vtcode:team_protocol] Teammate '{}' is idle and available for new tasks.",
                sender
            ))
        }
        TeamProtocolType::ShutdownRequest => {
            ctx.renderer.line(
                MessageStyle::Warning,
                &format!("Teammate '{}' requested shutdown.", sender),
            )?;
            Some(format!(
                "[vtcode:team_protocol] Teammate '{}' requested shutdown.",
                sender
            ))
        }
        TeamProtocolType::ShutdownApproved => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Shutdown approved for '{}'.", sender),
            )?;
            Some(format!(
                "[vtcode:team_protocol] Shutdown approved for '{}'.",
                sender
            ))
        }
        TeamProtocolType::TaskUpdate => {
            let detail = proto
                .details
                .as_ref()
                .map(|d| d.to_string())
                .unwrap_or_default();
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Task update from '{}': {}", sender, detail),
            )?;
            None
        }
    };
    Ok(inject)
}
