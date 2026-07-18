use anyhow::Result;
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::exec::events::PermissionDecision;
use vtcode_core::tool_policy::ToolPolicy;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::InlineHandle;

use super::{
    HitlDecision, ToolPermissionFlow,
    permission_events::{emit_permission_requested, emit_permission_resolved},
    permission_prompt::prompt_policy_denied_tool,
};
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::turn::tool_outcomes::error_handling::tool_denial_diagnostic;

/// Handle the policy-denied path: prompt the user with "Enable" option
/// and either update the policy to Allow or return Denied.
pub(super) async fn handle_policy_denied<S: UiSession + ?Sized>(
    tool_registry: &ToolRegistry,
    tool_name: &str,
    tool_permission_cache: Option<&std::sync::Arc<tokio::sync::RwLock<vtcode_core::acp::ToolPermissionCache>>>,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    ctrl_c_state: &std::sync::Arc<CtrlCState>,
    ctrl_c_notify: &std::sync::Arc<tokio::sync::Notify>,
    session: &mut S,
    skip_confirmations: bool,
    harness_emitter: Option<&HarnessEventEmitter>,
) -> Result<Option<ToolPermissionFlow>> {
    if skip_confirmations {
        return Ok(Some(ToolPermissionFlow::Denied));
    }

    let diagnostic = tool_denial_diagnostic(tool_name);
    let permission_start = std::time::Instant::now();
    emit_permission_requested(harness_emitter, tool_name);
    let decision =
        match prompt_policy_denied_tool(tool_name, diagnostic, renderer, handle, ctrl_c_state, ctrl_c_notify, session)
            .await
        {
            Ok(d) => d,
            Err(e) => {
                let wait_ms = permission_start.elapsed().as_millis() as u64;
                emit_permission_resolved(harness_emitter, tool_name, PermissionDecision::Cancelled, wait_ms);
                return Err(e);
            }
        };
    let wait_ms = permission_start.elapsed().as_millis() as u64;
    emit_permission_resolved(
        harness_emitter,
        tool_name,
        super::permission_events::map_hitl_to_permission_decision(&decision),
        wait_ms,
    );

    match decision {
        HitlDecision::Enable => {
            if let Err(err) = tool_registry.set_tool_policy(tool_name, ToolPolicy::Allow).await {
                tracing::warn!("Failed to update tool policy for '{}': {}", tool_name, err);
            }
            if let Some(cache) = tool_permission_cache {
                let mut perm_cache = cache.write().await;
                perm_cache.invalidate(tool_name);
            }
            Ok(None)
        }
        _ => Ok(Some(ToolPermissionFlow::Denied)),
    }
}
