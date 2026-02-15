use anyhow::Result;
use vtcode_core::agent_teams::TeamRole;
use vtcode_core::utils::ansi::MessageStyle;

use super::super::{SlashCommandContext, SlashCommandControl};

pub async fn handle_task_add(
    ctx: &mut SlashCommandContext<'_>,
    description: String,
    depends_on: Vec<u64>,
) -> Result<SlashCommandControl> {
    if !super::super::ensure_team_state(ctx).await? {
        ctx.renderer
            .line(MessageStyle::Info, "No active team. Use /team start.")?;
        return Ok(SlashCommandControl::Continue);
    }
    let id = {
        let team = ctx.session_stats.team_state.as_mut().expect("team state");
        team.add_task(description, depends_on).await?
    };
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Task #{} added. Use /team assign {} <teammate>.", id, id),
    )?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_task_claim(
    ctx: &mut SlashCommandContext<'_>,
    task_id: u64,
) -> Result<SlashCommandControl> {
    if !super::super::ensure_team_state(ctx).await? {
        ctx.renderer
            .line(MessageStyle::Info, "No active team. Use /team start.")?;
        return Ok(SlashCommandControl::Continue);
    }
    let teammate_name = match ctx.session_stats.team_context.as_ref() {
        Some(context) if context.role == TeamRole::Teammate => context
            .teammate_name
            .clone()
            .unwrap_or_else(|| "teammate".to_string()),
        _ => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Task claim is only available to teammates.",
            )?;
            return Ok(SlashCommandControl::Continue);
        }
    };
    {
        let team = ctx.session_stats.team_state.as_mut().expect("team state");
        team.claim_task(task_id, &teammate_name).await?;
    }
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Task #{} claimed by {}.", task_id, teammate_name),
    )?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_task_complete(
    ctx: &mut SlashCommandContext<'_>,
    task_id: u64,
    success: bool,
    summary: Option<String>,
) -> Result<SlashCommandControl> {
    if !super::super::ensure_team_state(ctx).await? {
        ctx.renderer
            .line(MessageStyle::Info, "No active team. Use /team start.")?;
        return Ok(SlashCommandControl::Continue);
    }

    let sender = super::super::current_sender(ctx);
    let (assigned_to, details, team_name, tasks_snapshot) = {
        let team = ctx.session_stats.team_state.as_mut().expect("team state");
        let (assigned_to, details) = team
            .complete_task(task_id, success, summary.clone())
            .await?;
        (
            assigned_to,
            details,
            team.config.name.clone(),
            team.tasks.clone(),
        )
    };

    if let Some(hooks) = ctx.lifecycle_hooks {
        let status = if success { "completed" } else { "failed" };
        if let Some(details) = details.as_ref() {
            let _ = hooks
                .run_task_completion("team_task", status, Some(details))
                .await;
        } else {
            let _ = hooks.run_task_completion("team_task", status, None).await;
        }
    }

    if let Some(assigned) = assigned_to.as_deref() {
        if super::super::is_teammate_idle(&tasks_snapshot, assigned)
            && let Some(hooks) = ctx.lifecycle_hooks
        {
            let details = serde_json::json!({
                "teammate": assigned,
                "team": team_name,
            });
            let _ = hooks.run_teammate_idle(assigned, Some(&details)).await;
        }
        {
            let team = ctx.session_stats.team_state.as_ref().expect("team state");
            let proto = vtcode_core::agent_teams::TeamProtocolMessage {
                r#type: vtcode_core::agent_teams::TeamProtocolType::IdleNotification,
                details: Some(serde_json::json!({ "reason": "available" })),
            };
            let _ = team.send_protocol("lead", assigned, proto, None).await;
        }
    }

    let summary_text = summary.unwrap_or_else(|| "No summary provided".to_string());
    if sender != "lead" {
        {
            let team = ctx.session_stats.team_state.as_mut().expect("team state");
            team.send_message(
                "lead",
                &sender,
                format!(
                    "Task #{} {}. Summary: {}",
                    task_id,
                    if success { "completed" } else { "failed" },
                    summary_text
                ),
                Some(task_id),
            )
            .await?;
        }
    }

    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Task #{} marked {}.",
            task_id,
            if success { "completed" } else { "failed" }
        ),
    )?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_tasks(ctx: &mut SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if !super::super::ensure_team_state(ctx).await? {
        ctx.renderer
            .line(MessageStyle::Info, "No active team. Use /team start.")?;
        return Ok(SlashCommandControl::Continue);
    }
    let tasks_snapshot = {
        let team = ctx.session_stats.team_state.as_mut().expect("team state");
        team.reload_tasks().await?;
        team.tasks.clone()
    };
    if tasks_snapshot.tasks.is_empty() {
        ctx.renderer.line(MessageStyle::Info, "No tasks yet.")?;
        return Ok(SlashCommandControl::Continue);
    }
    ctx.renderer.line(MessageStyle::Info, "Team tasks:")?;
    for task in &tasks_snapshot.tasks {
        let status = match task.status {
            vtcode_core::agent_teams::TeamTaskStatus::Pending => "pending",
            vtcode_core::agent_teams::TeamTaskStatus::InProgress => "in_progress",
            vtcode_core::agent_teams::TeamTaskStatus::Completed => "completed",
            vtcode_core::agent_teams::TeamTaskStatus::Failed => "failed",
        };
        let assignee = task.assigned_to.as_deref().unwrap_or("unassigned");
        let deps = if task.depends_on.is_empty() {
            "deps: none".to_string()
        } else {
            format!(
                "deps: {}",
                task.depends_on
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        ctx.renderer.line(
            MessageStyle::Output,
            &format!(
                "  #{} [{}] {} (assigned: {}, {})",
                task.id, status, task.description, assignee, deps
            ),
        )?;
        if matches!(
            task.status,
            vtcode_core::agent_teams::TeamTaskStatus::Completed
                | vtcode_core::agent_teams::TeamTaskStatus::Failed
        ) && let Some(summary) = task.result_summary.as_deref()
            && !summary.trim().is_empty()
        {
            ctx.renderer.line(
                MessageStyle::Output,
                &format!("    summary: {}", summary.trim()),
            )?;
        }
    }
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_assign(
    ctx: &mut SlashCommandContext<'_>,
    task_id: u64,
    teammate: String,
) -> Result<SlashCommandControl> {
    if matches!(
        ctx.session_stats
            .team_context
            .as_ref()
            .map(|context| context.role),
        Some(TeamRole::Teammate)
    ) {
        ctx.renderer.line(
            MessageStyle::Info,
            "Task assignment is only available to the lead.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    if !super::super::ensure_team_state(ctx).await? {
        ctx.renderer
            .line(MessageStyle::Info, "No active team. Use /team start.")?;
        return Ok(SlashCommandControl::Continue);
    }

    let has_teammate = {
        let team = ctx.session_stats.team_state.as_ref().expect("team state");
        team.config
            .teammates
            .iter()
            .any(|entry| entry.name == teammate)
    };
    if !has_teammate {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Teammate '{}' not found.", teammate),
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    {
        let team = ctx.session_stats.team_state.as_mut().expect("team state");
        team.assign_task(task_id, &teammate).await?;
        let task_desc = team
            .tasks
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .map(|task| task.description.clone())
            .unwrap_or_else(|| "Task".to_string());
        team.send_message(
            &teammate,
            "lead",
            format!("Task #{}: {}", task_id, task_desc),
            Some(task_id),
        )
        .await?;
        let proto = vtcode_core::agent_teams::TeamProtocolMessage {
            r#type: vtcode_core::agent_teams::TeamProtocolType::TaskUpdate,
            details: Some(serde_json::json!({
                "task_id": task_id,
                "action": "assigned",
                "teammate": teammate,
            })),
        };
        let _ = team
            .send_protocol("lead", "system", proto, Some(task_id))
            .await;
    };
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Task #{} assigned to {}.", task_id, teammate),
    )?;
    Ok(SlashCommandControl::Continue)
}
