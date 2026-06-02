//! Verifies `vtcode_core::safety::hitl` is reachable. This is a regression
//! guard against the `safety.rs` + `safety/` ambiguity that previously made
//! the human-in-the-loop submodule silently unbuilt.

#![allow(dead_code)]

#[test]
fn safety_hitl_module_is_reachable() {
    use vtcode_core::safety::hitl::{
        HitlAuditTrail, HitlEvent, HitlGate, HitlPolicy, HitlStatistics, OversightDecision,
    };

    let _: OversightDecision = OversightDecision::Allow;
    let _: Option<HitlGate> = None;
    let _: Option<HitlPolicy> = None;
    let _: Option<HitlAuditTrail> = None;
    let _: Option<HitlStatistics> = None;
    let _: Option<HitlEvent> = None;
}
