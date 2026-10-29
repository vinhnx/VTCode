use super::*;

#[test]
fn non_follow_up_resets_follow_up_tracking() {
    let mut stats = SessionStats::default();
    stats.mark_turn_stalled(true, Some("turn aborted".to_string()));
    let _ = stats.register_follow_up_prompt("continue");
    let _ = stats.register_follow_up_prompt("continue");
    assert!(stats.turn_stalled());
    assert_eq!(stats.turn_stall_reason(), Some("turn aborted"));

    assert_eq!(stats.register_follow_up_prompt("run tests and summarize"), FollowUpPromptAction::None);
    assert!(!stats.turn_stalled());
    assert_eq!(stats.turn_stall_reason(), None);
}
#[test]
fn follow_up_prompt_variants_are_detected() {
    let mut stats = SessionStats::default();
    assert_eq!(stats.register_follow_up_prompt("continue."), FollowUpPromptAction::None);
    assert_eq!(stats.register_follow_up_prompt("continue with your recommendation"), FollowUpPromptAction::None);
    assert_eq!(stats.register_follow_up_prompt("please continue"), FollowUpPromptAction::ForceConclusion);
}
#[test]
fn suppressed_follow_up_prompt_is_ignored_once() {
    let mut stats = SessionStats::default();
    stats.mark_turn_stalled(true, Some("turn blocked".to_string()));
    stats.suppress_next_follow_up_prompt();

    assert_eq!(stats.register_follow_up_prompt("continue"), FollowUpPromptAction::None);
    assert!(stats.turn_stalled());
    assert_eq!(stats.turn_stall_reason(), Some("turn blocked"));

    assert!(stats.register_follow_up_prompt("continue").is_stalled_recovery());
}
#[test]
fn suppressed_non_follow_up_still_clears_stall_state() {
    let mut stats = SessionStats::default();
    stats.mark_turn_stalled(true, Some("turn blocked".to_string()));
    stats.suppress_next_follow_up_prompt();

    assert_eq!(stats.register_follow_up_prompt("run tests and summarize"), FollowUpPromptAction::None);
    assert!(!stats.turn_stalled());
    assert_eq!(stats.turn_stall_reason(), None);
}
#[test]
fn follow_up_prompt_action_exposes_stall_reason() {
    let action = FollowUpPromptAction::RecoverFromStall { stall_reason: Some("blocked".to_string()) };

    assert!(action.should_force_autonomous_response());
    assert!(action.is_stalled_recovery());
    assert_eq!(action.stall_reason(), Some("blocked"));
}
#[test]
fn helper_detects_follow_up_variants() {
    assert!(is_follow_up_prompt_like("continue"));
    assert!(is_follow_up_prompt_like("continue."));
    assert!(is_follow_up_prompt_like("please continue"));
    assert!(is_follow_up_prompt_like("Continue autonomously from the last stalled turn. Stall reason: x."));
    assert!(!is_follow_up_prompt_like("run tests and summarize"));
}
#[test]
fn safe_mode_prompts_are_disabled_for_auto_permission() {
    assert!(!should_enforce_safe_mode_prompts(false, true, Some(WorkspaceTrustLevel::ToolsPolicy),));
}
#[test]
fn auto_permission_denials_trigger_prompt_fallback_after_threshold() {
    let mut stats = SessionStats::default();

    assert!(!stats.record_auto_permission_denial(
        AutoPermissionDenial {
            stage: "stage2",
            reason: "blocked".to_string(),
            matched_rule: Some("rule".to_string()),
            matched_exception: None,
        },
        3,
        20,
    ));
    assert!(!stats.auto_permission_prompt_fallback_active());

    assert!(!stats.record_auto_permission_denial(
        AutoPermissionDenial {
            stage: "stage2",
            reason: "blocked".to_string(),
            matched_rule: Some("rule".to_string()),
            matched_exception: None,
        },
        3,
        20,
    ));
    assert!(!stats.auto_permission_prompt_fallback_active());

    assert!(stats.record_auto_permission_denial(
        AutoPermissionDenial {
            stage: "stage2",
            reason: "blocked".to_string(),
            matched_rule: Some("rule".to_string()),
            matched_exception: None,
        },
        3,
        20,
    ));
    assert!(stats.auto_permission_prompt_fallback_active());
}
