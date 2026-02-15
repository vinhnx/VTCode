use anyhow::Result;
use vtcode_core::agent_teams::{TeamRole, TeamStorage, TeammateConfig};
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::slash_commands::TeamCommandAction;
use crate::agent::runloop::unified::state::ModelPickerTarget;
use crate::agent::runloop::unified::team_state::TeamState;

use super::{SlashCommandContext, SlashCommandControl};
#[path = "team_tasks.rs"]
mod team_tasks;

pub async fn handle_manage_teams(
    mut ctx: SlashCommandContext<'_>,
    action: TeamCommandAction,
) -> Result<SlashCommandControl> {
    if !super::agent_teams_enabled(ctx.vt_cfg) {
        ctx.renderer.line(
            MessageStyle::Info,
            "Agent teams are disabled. Enable [agent_teams] enabled = true or set VTCODE_EXPERIMENTAL_AGENT_TEAMS=1.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    match action {
        TeamCommandAction::Help => {
            super::render_team_usage(ctx.renderer)?;
            return Ok(SlashCommandControl::Continue);
        }
        TeamCommandAction::Model => {
            ctx.session_stats.model_picker_target = ModelPickerTarget::TeamDefault;
            return super::ui::start_model_picker(ctx).await;
        }
        TeamCommandAction::Stop => {
            if let Some(team) = ctx.session_stats.team_state.as_ref() {
                let teammate_names: Vec<String> = team.teammate_names();
                let team_ref = ctx.session_stats.team_state.as_ref().unwrap();
                for name in &teammate_names {
                    let proto = vtcode_core::agent_teams::TeamProtocolMessage {
                        r#type: vtcode_core::agent_teams::TeamProtocolType::ShutdownRequest,
                        details: None,
                    };
                    if let Err(err) = team_ref.send_protocol(name, "lead", proto, None).await {
                        tracing::warn!("Failed to send shutdown to {}: {}", name, err);
                    }
                }
            }
            // Shutdown in-process runners
            if let Some(runner) = ctx.session_stats.in_process_runner.as_ref() {
                runner.shutdown_all();
            }
            ctx.session_stats.in_process_runner = None;
            ctx.session_stats.team_state = None;
            ctx.session_stats.team_context = None;
            ctx.session_stats.delegate_mode = false;
            ctx.renderer.line(MessageStyle::Info, "Team stopped.")?;
            return Ok(SlashCommandControl::Continue);
        }
        _ => {}
    }

    match action {
        TeamCommandAction::Start {
            name,
            count,
            subagent_type,
            model,
        } => {
            if matches!(
                ctx.session_stats
                    .team_context
                    .as_ref()
                    .map(|context| context.role),
                Some(TeamRole::Teammate)
            ) {
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Teammate sessions cannot start new teams.",
                )?;
                return Ok(SlashCommandControl::Continue);
            }

            if ctx.session_stats.team_state.is_some() {
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Team already running. Use /team stop before starting a new team.",
                )?;
                return Ok(SlashCommandControl::Continue);
            }

            let team_name = name.unwrap_or_else(|| "team".to_string());
            let storage = TeamStorage::from_config(ctx.vt_cfg.as_ref()).await?;
            if storage.load_team_config(&team_name).await?.is_some() {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Team '{}' already exists.", team_name),
                )?;
                return Ok(SlashCommandControl::Continue);
            }

            let default_subagent = subagent_type.unwrap_or_else(|| "general".to_string());
            let desired_count = count.unwrap_or(3);
            if desired_count == 0 {
                ctx.renderer
                    .line(MessageStyle::Error, "Team size must be at least 1.")?;
                return Ok(SlashCommandControl::Continue);
            }

            let max_teammates = super::resolve_max_teammates(ctx.vt_cfg);
            if desired_count > max_teammates {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!(
                        "Team size {} exceeds max_teammates {}.",
                        desired_count, max_teammates
                    ),
                )?;
                return Ok(SlashCommandControl::Continue);
            }

            let mut teammates = Vec::new();
            let default_model = super::resolve_default_team_model(ctx.vt_cfg, model);
            for idx in 1..=desired_count {
                teammates.push(TeammateConfig {
                    name: format!("teammate-{}", idx),
                    subagent_type: default_subagent.clone(),
                    model: default_model.clone(),
                    session_id: None,
                });
            }

            let mut team = TeamState::create(
                storage.clone(),
                team_name.clone(),
                default_subagent.clone(),
                teammates,
            )
            .await?;
            if let Some(first) = team.config.teammates.first() {
                team.set_active_teammate(Some(first.name.clone())).await?;
            }

            ctx.session_stats.team_state = Some(team);
            ctx.session_stats.team_context = Some(vtcode_core::agent_teams::TeamContext {
                team_name: team_name.clone(),
                role: TeamRole::Lead,
                teammate_name: None,
                mode: super::resolve_teammate_mode(ctx.vt_cfg),
            });

            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Team '{}' started with {} teammates (default: {}).",
                    team_name, desired_count, default_subagent
                ),
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "Use /team task add <description> to queue work.",
            )?;

            let mode = super::resolve_teammate_mode(ctx.vt_cfg);
            if mode == vtcode_config::agent_teams::TeammateMode::Tmux {
                if let Err(err) = crate::agent::runloop::unified::team_tmux::spawn_tmux_teammates(
                    &team_name,
                    ctx.config.workspace.as_path(),
                    ctx.session_stats.team_state.as_ref().unwrap(),
                ) {
                    ctx.renderer
                        .line(MessageStyle::Error, &format!("TMUX spawn failed: {}", err))?;
                }
            } else if mode == vtcode_config::agent_teams::TeammateMode::InProcess {
                let team = ctx.session_stats.team_state.as_ref().unwrap();
                let mut runner =
                    vtcode_core::agent_teams::InProcessTeamRunner::new(team_name.clone());
                for tm in &team.config.teammates {
                    let spawn_cfg = vtcode_core::agent_teams::TeammateSpawnConfig {
                        teammate: tm.clone(),
                        team_name: team_name.clone(),
                        api_key: ctx.config.api_key.clone(),
                        poll_interval: std::time::Duration::from_millis(500),
                        vt_cfg: ctx.vt_cfg.clone(),
                    };
                    if let Err(err) = runner.spawn_teammate(spawn_cfg) {
                        ctx.renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to spawn in-process '{}': {}", tm.name, err),
                        )?;
                    }
                }
                ctx.session_stats.in_process_runner = Some(runner);
            }

            Ok(SlashCommandControl::Continue)
        }
        TeamCommandAction::Add {
            name,
            subagent_type,
            model,
        } => {
            let max_teammates = super::resolve_max_teammates(ctx.vt_cfg);
            if !super::ensure_team_state(&mut ctx).await? {
                ctx.renderer
                    .line(MessageStyle::Info, "No active team. Use /team start.")?;
                return Ok(SlashCommandControl::Continue);
            }

            let (team_name, default_subagent, teammate_count) = {
                let team = ctx.session_stats.team_state.as_ref().expect("team state");
                (
                    team.config.name.clone(),
                    team.config.default_subagent.clone(),
                    team.config.teammates.len(),
                )
            };

            if teammate_count >= max_teammates {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Team already at max_teammates {}.", max_teammates),
                )?;
                return Ok(SlashCommandControl::Continue);
            }

            let subagent = subagent_type.unwrap_or(default_subagent);
            let default_model = super::resolve_default_team_model(ctx.vt_cfg, model);
            {
                let team = ctx.session_stats.team_state.as_mut().expect("team state");
                team.add_teammate(name.clone(), subagent.clone(), default_model.clone())
                    .await?;
            }
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Teammate '{}' added ({}).", name, subagent),
            )?;

            if super::resolve_teammate_mode(ctx.vt_cfg)
                == vtcode_config::agent_teams::TeammateMode::Tmux
            {
                if let Err(err) = crate::agent::runloop::unified::team_tmux::spawn_tmux_teammate(
                    &team_name,
                    ctx.config.workspace.as_path(),
                    &name,
                    default_model.as_deref(),
                ) {
                    ctx.renderer
                        .line(MessageStyle::Error, &format!("TMUX spawn failed: {}", err))?;
                }
            }

            Ok(SlashCommandControl::Continue)
        }
        TeamCommandAction::Remove { name } => {
            if !super::ensure_team_state(&mut ctx).await? {
                ctx.renderer
                    .line(MessageStyle::Info, "No active team. Use /team start.")?;
                return Ok(SlashCommandControl::Continue);
            }
            {
                let team = ctx.session_stats.team_state.as_mut().expect("team state");
                team.remove_teammate(&name).await?;
            }
            ctx.renderer
                .line(MessageStyle::Info, &format!("Teammate '{}' removed.", name))?;
            Ok(SlashCommandControl::Continue)
        }
        TeamCommandAction::TaskAdd {
            description,
            depends_on,
        } => team_tasks::handle_task_add(&mut ctx, description, depends_on).await,
        TeamCommandAction::TaskClaim { task_id } => {
            team_tasks::handle_task_claim(&mut ctx, task_id).await
        }
        TeamCommandAction::TaskComplete {
            task_id,
            success,
            summary,
        } => team_tasks::handle_task_complete(&mut ctx, task_id, success, summary).await,
        TeamCommandAction::Tasks => team_tasks::handle_tasks(&mut ctx).await,
        TeamCommandAction::Teammates => {
            if !super::ensure_team_state(&mut ctx).await? {
                ctx.renderer
                    .line(MessageStyle::Info, "No active team. Use /team start.")?;
                return Ok(SlashCommandControl::Continue);
            }
            let (teammates, active) = {
                let team = ctx.session_stats.team_state.as_ref().expect("team state");
                (
                    team.config.teammates.clone(),
                    team.active_teammate().map(|name| name.to_string()),
                )
            };
            if teammates.is_empty() {
                ctx.renderer.line(MessageStyle::Info, "No teammates yet.")?;
                return Ok(SlashCommandControl::Continue);
            }
            ctx.renderer.line(MessageStyle::Info, "Teammates:")?;
            let active = active.as_deref();
            for teammate in &teammates {
                let model = teammate.model.as_deref().unwrap_or("default");
                let active_marker = if active == Some(teammate.name.as_str()) {
                    " (active)"
                } else {
                    ""
                };
                ctx.renderer.line(
                    MessageStyle::Output,
                    &format!(
                        "  {} ({}, model: {}){}",
                        teammate.name, teammate.subagent_type, model, active_marker
                    ),
                )?;
            }
            Ok(SlashCommandControl::Continue)
        }
        TeamCommandAction::Assign { task_id, teammate } => {
            team_tasks::handle_assign(&mut ctx, task_id, teammate).await
        }
        TeamCommandAction::Message { recipient, message } => {
            let sender = super::current_sender(&ctx);
            if !super::ensure_team_state(&mut ctx).await? {
                ctx.renderer
                    .line(MessageStyle::Info, "No active team. Use /team start.")?;
                return Ok(SlashCommandControl::Continue);
            }
            {
                let team = ctx.session_stats.team_state.as_mut().expect("team state");
                team.send_message(&recipient, &sender, message, None)
                    .await?;
            }
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Message sent to {}.", recipient),
            )?;
            Ok(SlashCommandControl::Continue)
        }
        TeamCommandAction::Broadcast { message } => {
            if matches!(
                ctx.session_stats
                    .team_context
                    .as_ref()
                    .map(|context| context.role),
                Some(TeamRole::Teammate)
            ) {
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Broadcast is only available to the lead.",
                )?;
                return Ok(SlashCommandControl::Continue);
            }

            let sender = super::current_sender(&ctx);
            if !super::ensure_team_state(&mut ctx).await? {
                ctx.renderer
                    .line(MessageStyle::Info, "No active team. Use /team start.")?;
                return Ok(SlashCommandControl::Continue);
            }
            let teammate_names = {
                let team = ctx.session_stats.team_state.as_ref().expect("team state");
                team.config
                    .teammates
                    .iter()
                    .map(|teammate| teammate.name.clone())
                    .collect::<Vec<_>>()
            };
            {
                let team = ctx.session_stats.team_state.as_mut().expect("team state");
                for teammate in &teammate_names {
                    team.send_message(teammate, &sender, message.clone(), None)
                        .await?;
                }
            }
            ctx.renderer.line(MessageStyle::Info, "Broadcast sent.")?;
            Ok(SlashCommandControl::Continue)
        }
        TeamCommandAction::Help | TeamCommandAction::Model | TeamCommandAction::Stop => {
            Ok(SlashCommandControl::Continue)
        }
    }
}
