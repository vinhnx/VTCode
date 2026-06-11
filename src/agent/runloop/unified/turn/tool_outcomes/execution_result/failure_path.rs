use vtcode_core::core::agent::error_recovery::ErrorType as RecoveryErrorType;
use vtcode_core::notifications::notify_tool_failure;
use vtcode_core::tools::registry::ToolExecutionError;

use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

use super::auto_permission_probe::push_tool_response_with_auto_permission_probe;

pub(super) async fn notify_structured_failure(
    tool_name: &str,
    user_msg: &str,
    notification_kind: Option<&'static str>,
) {
    if let Err(err) = notify_tool_failure(tool_name, user_msg, notification_kind).await {
        let notification_label = notification_kind.unwrap_or("failure");
        tracing::debug!(
            tool = %tool_name,
            error = %err,
            notification = notification_label,
            "Failed to emit tool failure notification"
        );
    }
}

pub(super) fn log_structured_failure(
    tool_name: &str,
    error: &ToolExecutionError,
    hint: Option<&str>,
    log_message: &'static str,
) {
    if let Some(hint) = hint {
        tracing::debug!(
            tool = %tool_name,
            category = ?error.category,
            retryable = error.retryable,
            partial_state_possible = error.partial_state_possible,
            hint = %hint,
            error = %error.message,
            "{log_message}"
        );
    } else {
        tracing::debug!(
            tool = %tool_name,
            category = ?error.category,
            retryable = error.retryable,
            partial_state_possible = error.partial_state_possible,
            error = %error.message,
            "{log_message}"
        );
    }
}

pub(super) async fn record_recovery_tool_error(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    error: &ToolExecutionError,
    error_type: RecoveryErrorType,
) {
    ctx.record_recovery_tool_error(tool_name, error, error_type)
        .await;
}

pub(super) async fn finalize_failed_tool_response(
    t_ctx: &mut super::super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    error: &ToolExecutionError,
    failure_kind: &'static str,
) {
    push_tool_error_response(
        t_ctx,
        tool_call_id,
        tool_name,
        args_val,
        error.message.as_str(),
        failure_kind,
        Some(error),
    )
    .await;

    super::record_request_user_input_interview_result(t_ctx.ctx, tool_name, None);
}

async fn push_tool_error_response(
    t_ctx: &mut super::super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    error_msg: &str,
    failure_kind: &'static str,
    structured_error: Option<&ToolExecutionError>,
) {
    let (fallback_tool, fallback_tool_args) = if let Some((tool, args)) =
        super::super::error_handling::fallback_from_error(tool_name, error_msg, Some(args_val))
    {
        (Some(tool), Some(args))
    } else {
        let fallback = t_ctx
            .ctx
            .tool_registry
            .suggest_fallback_tool(tool_name)
            .await;
        (fallback, None)
    };

    let error_content = match structured_error {
        Some(error) => super::super::error_handling::build_structured_error_content(
            error,
            fallback_tool,
            fallback_tool_args,
            failure_kind,
        ),
        None => super::build_error_content(
            error_msg.to_string(),
            fallback_tool,
            fallback_tool_args,
            failure_kind,
        ),
    };
    let serialized = error_content.to_string();
    if let Err(err) =
        push_tool_response_with_auto_permission_probe(t_ctx, tool_call_id, tool_name, serialized)
            .await
    {
        tracing::warn!(tool = %tool_name, error = %err, "failed to push probed tool error response");
    }
}
