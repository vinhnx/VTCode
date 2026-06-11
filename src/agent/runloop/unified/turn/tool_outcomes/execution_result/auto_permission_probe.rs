use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::auto_permission::{ProbeWarning, probe_tool_output};
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

async fn auto_permission_probe_warning(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    content_for_model: &str,
) -> Option<ProbeWarning> {
    if !ctx.full_auto || ctx.is_planning_active() {
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
            tracing::warn!(tool = %tool_name, error = %err, "auto permission review prompt probe failed");
            None
        }
    }
}

fn append_probe_warning(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    probe_warning: ProbeWarning,
) -> Result<()> {
    tracing::trace!(tool = %tool_name, probe_hit = true, "auto permission review prompt probe flagged tool output");
    ctx.working_history
        .push(vtcode_core::llm::provider::Message::system(
            probe_warning.warning.clone(),
        ));
    ctx.renderer.line(
        MessageStyle::Warning,
        "Auto permission review flagged the latest tool output as suspicious prompt injection.",
    )?;
    Ok(())
}

pub(super) async fn push_tool_response_with_auto_permission_probe(
    t_ctx: &mut super::super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
    content_for_model: String,
) -> Result<()> {
    let probe_warning =
        auto_permission_probe_warning(t_ctx.ctx, tool_name, &content_for_model).await;
    t_ctx
        .ctx
        .push_tool_response(tool_call_id, content_for_model);
    if let Some(probe_warning) = probe_warning {
        append_probe_warning(t_ctx.ctx, tool_name, probe_warning)?;
    }
    Ok(())
}
