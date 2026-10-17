use anyhow::Result;
use tracing::Instrument;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::auto_permission::{
    ProbeRuntime, ProbeWarning, probe_tool_output, recent_user_context,
};
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;
use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;

mod status;

async fn auto_permission_probe_warning(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    content_for_model: &str,
    batch_spinner: Option<&PlaceholderSpinner>,
) -> Option<ProbeWarning> {
    if ctx.is_planning_active()
        || ctx.ctrl_c_state.is_cancel_requested()
        || ctx.ctrl_c_state.is_cancel_handled()
        || ctx.ctrl_c_state.is_exit_requested()
    {
        return None;
    }
    let permissions = ctx.vt_cfg.map(|cfg| &cfg.permissions)?;
    if !ctx.full_auto
        && !(ctx.renderer.supports_inline_ui()
            && permissions.auto_permission.use_decisions_probe
            && ctx.provider_client.supports_decisions())
    {
        return None;
    }
    // Empty outputs carry nothing to probe and must not consume the
    // per-turn probe budget.
    if content_for_model.trim().is_empty() {
        return None;
    }
    if !ctx.harness_state.can_spend_auto_permission_probe_model_call() {
        tracing::debug!(tool = %tool_name, "auto permission review prompt probe budget exhausted for this turn");
        return None;
    }
    // The probe reads only the last two user messages: extract them up front
    // instead of cloning the whole conversation for every tool result.
    let user_context = recent_user_context(ctx.working_history);
    ctx.harness_state.record_auto_permission_probe_model_call();
    let status = ctx
        .renderer
        .supports_inline_ui()
        .then(|| status::ProbeStatus::new(ctx.handle, ctx.input_status_state, ctx.ctrl_c_state, batch_spinner));
    let span = tracing::info_span!(
        "tool_output_probe",
        session_id = crate::main_helpers::runtime_archive_session_id().as_deref().unwrap_or("unarchived"),
        turn_id = %ctx.harness_state.turn_id.0,
        full_auto = ctx.full_auto,
    );
    let mut runtime = ProbeRuntime {
        stats: ctx.session_stats,
        stop: ctx.ctrl_c_state,
        notify: ctx.ctrl_c_notify,
    };
    let result = probe_tool_output(
        ctx.provider_client.as_mut(),
        ctx.config,
        ctx.vt_cfg,
        permissions,
        &user_context,
        content_for_model,
        &mut runtime,
    )
    .instrument(span)
    .await;
    drop(status);
    if status::stopped(ctx.ctrl_c_state) {
        crate::agent::runloop::unified::status_line::clear_input_status(ctx.handle, ctx.input_status_state);
    }
    match result {
        Ok(warning) => warning,
        Err(error) => {
            tracing::warn!(tool = %tool_name, %error, "auto permission review prompt probe failed");
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
    let queued = ctx.harness_state.queue_auto_permission_probe_warning(probe_warning.warning);
    tracing::trace!(tool = %tool_name, queued, "queued auto permission review prompt probe warning");
    ctx.renderer.line(
        MessageStyle::Warning,
        "Auto permission review flagged the latest tool output as suspicious prompt injection.",
    )?;
    Ok(())
}

pub(super) fn flush_auto_permission_probe_warning(ctx: &mut TurnProcessingContext<'_>) {
    if let Some(warning) = ctx.harness_state.take_auto_permission_probe_warning() {
        ctx.push_system_message(warning);
    }
}

pub(super) async fn push_tool_response_with_auto_permission_probe(
    t_ctx: &mut super::super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
    content_for_model: String,
    batch_spinner: Option<&PlaceholderSpinner>,
) -> Result<()> {
    let probe_warning = auto_permission_probe_warning(t_ctx.ctx, tool_name, &content_for_model, batch_spinner).await;
    t_ctx.ctx.push_tool_response(tool_call_id, Some(tool_name), content_for_model);
    if let Some(probe_warning) = probe_warning {
        append_probe_warning(t_ctx.ctx, tool_name, probe_warning)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
