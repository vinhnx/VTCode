use vtcode_core::exec::events::{PermissionDecision, ThreadEvent};

use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;

pub(super) fn map_hitl_to_permission_decision(decision: &super::HitlDecision) -> PermissionDecision {
    match decision {
        super::HitlDecision::Approved
        | super::HitlDecision::ApprovedSession
        | super::HitlDecision::ApprovedPermanent
        | super::HitlDecision::Enable => PermissionDecision::Allow,
        super::HitlDecision::Denied | super::HitlDecision::DeniedOnce => PermissionDecision::Deny,
        super::HitlDecision::Interrupt | super::HitlDecision::Exit => PermissionDecision::Cancelled,
    }
}

pub(super) fn emit_permission_requested(harness_emitter: Option<&HarnessEventEmitter>, tool_name: &str) {
    let Some(emitter) = harness_emitter else { return };
    let _ = emitter.emit(ThreadEvent::PermissionRequested(vtcode_core::exec::events::PermissionRequestedEvent {
        tool_name: tool_name.to_string(),
    }));
}

pub(super) fn emit_permission_resolved(
    harness_emitter: Option<&HarnessEventEmitter>,
    tool_name: &str,
    decision: PermissionDecision,
    wait_ms: u64,
) {
    let Some(emitter) = harness_emitter else { return };
    let _ = emitter.emit(ThreadEvent::PermissionResolved(vtcode_core::exec::events::PermissionResolvedEvent {
        tool_name: tool_name.to_string(),
        decision,
        wait_ms,
    }));
}
