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

pub(super) fn emit_permission_requested(
    harness_emitter: Option<&HarnessEventEmitter>,
    tool_name: &str,
    tool_call_id: Option<&str>,
) {
    let Some(emitter) = harness_emitter else { return };
    let _ = emitter.emit(ThreadEvent::PermissionRequested(vtcode_core::exec::events::PermissionRequestedEvent {
        tool_name: tool_name.to_string(),
        tool_call_id: tool_call_id.map(str::to_owned),
    }));
}

pub(super) fn emit_permission_resolved(
    harness_emitter: Option<&HarnessEventEmitter>,
    tool_name: &str,
    tool_call_id: Option<&str>,
    decision: PermissionDecision,
    wait_ms: u64,
) {
    let Some(emitter) = harness_emitter else { return };
    let _ = emitter.emit(ThreadEvent::PermissionResolved(vtcode_core::exec::events::PermissionResolvedEvent {
        tool_name: tool_name.to_string(),
        tool_call_id: tool_call_id.map(str::to_owned),
        decision,
        wait_ms,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::fs;

    #[test]
    fn emitted_permission_events_keep_tool_call_id() {
        let temp = tempfile::tempdir().expect("temp directory");
        let path = temp.path().join("events.jsonl");
        let emitter = HarnessEventEmitter::new(path.clone()).expect("event emitter");

        emit_permission_requested(Some(&emitter), "web_fetch", Some("call-1"));
        emit_permission_resolved(Some(&emitter), "web_fetch", Some("call-1"), PermissionDecision::Allow, 1);

        let lines = fs::read_to_string(path)
            .expect("event log")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("versioned event"))
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["event"]["tool_call_id"], "call-1");
        assert_eq!(lines[1]["event"]["tool_call_id"], "call-1");
    }
}
