use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::auto_mode::{ProbeWarning, probe_tool_output};
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

async fn auto_mode_probe_warning(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    content_for_model: &str,
) -> Option<ProbeWarning> {
    if !ctx.vt_cfg.is_some_and(|cfg| {
        cfg.permissions.default_mode == vtcode_core::config::PermissionMode::Auto
    }) || !ctx.session_stats.is_autonomous_mode()
    {
        return None;
    }

    let permissions = ctx.vt_cfg.map(|cfg| cfg.permissions.clone())?;
    let working_history = ctx.working_history.clone();
    match probe_tool_output(
        ctx.provider_client.as_mut(),
        ctx.config,
        ctx.vt_cfg,
        &permissions,
        &working_history,
        content_for_model,
    )
    .await
    {
        Ok(warning) => warning,
        Err(err) => {
            tracing::warn!(tool = %tool_name, error = %err, "auto mode prompt probe failed");
            None
        }
    }
}

fn append_probe_warning(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    probe_warning: ProbeWarning,
) -> Result<()> {
    tracing::trace!(tool = %tool_name, probe_hit = true, "auto mode prompt probe flagged tool output");
    ctx.working_history
        .push(vtcode_core::llm::provider::Message::system(
            probe_warning.warning.clone(),
        ));
    ctx.renderer.line(
        MessageStyle::Warning,
        "Auto mode flagged the latest tool output as suspicious prompt injection.",
    )?;
    Ok(())
}

pub(super) async fn push_tool_response_with_auto_mode_probe(
    t_ctx: &mut super::super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
    content_for_model: String,
) -> Result<()> {
    let probe_warning = auto_mode_probe_warning(t_ctx.ctx, tool_name, &content_for_model).await;
    t_ctx
        .ctx
        .push_tool_response(tool_call_id, content_for_model);
    if let Some(probe_warning) = probe_warning {
        append_probe_warning(t_ctx.ctx, tool_name, probe_warning)?;
    }
    Ok(())
}
