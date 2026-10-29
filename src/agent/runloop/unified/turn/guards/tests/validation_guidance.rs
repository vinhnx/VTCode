use super::*;

#[test]
fn readonly_signature_handles_file_and_code_search_signatures() {
    assert!(is_readonly_signature(r#"read file:{"path":"README.md"}"#));
    assert!(is_readonly_signature(
        r#"code_search:{"query":"LLMStreamEvent","path":"crates/codegen/vtcode-core/src/llm/providers/anthropic/api.rs"}"#
    ));
}
#[test]
fn readonly_signature_treats_safe_exec_runs_as_readonly() {
    assert!(is_readonly_signature(r#"command_session:{"action":"run","command":"cargo check"}"#));
}
#[test]
fn readonly_signature_fast_path_accepts_ro_tag() {
    assert!(is_readonly_signature("code_search:ro:{\"query\":\"Widget\"}"));
}
#[test]
fn readonly_signature_fast_path_rejects_rw_tag() {
    assert!(!is_readonly_signature("file_operation:rw:len42-fnv1234abcd"));
}
#[test]
fn validate_edit_file_args_accepts_legacy_old_new_string_keys() {
    let args = json!({
        "path": "src/lib.rs",
        "old_string": "before",
        "new_string": "after"
    });

    assert!(validate_tool_args_security(tool_names::EDIT_FILE, &args, None, None).is_none());
}
#[test]
fn validate_edit_file_args_still_rejects_when_replacements_missing() {
    let args = json!({
        "path": "src/lib.rs"
    });

    let failures = validate_tool_args_security(tool_names::EDIT_FILE, &args, None, None).unwrap();
    assert!(failures.iter().any(|msg| msg.contains("old_str")));
    assert!(failures.iter().any(|msg| msg.contains("new_str")));
}
#[test]
fn validate_command_session_args_without_registry_reports_single_missing_command() {
    let failures = validate_tool_args_security(tool_names::UNIFIED_EXEC, &json!({"action": "run"}), None, None)
        .expect("missing command should fail");

    assert_eq!(failures, vec!["Missing required argument: command".to_string()]);
}
#[test]
fn validate_command_session_args_without_registry_rejects_missing_action() {
    let failures = validate_tool_args_security(tool_names::UNIFIED_EXEC, &json!({}), None, None)
        .expect("missing action should fail");

    assert_eq!(
        failures,
        vec!["Invalid arguments: missing action; provide `action` or inferable exec arguments".to_string()]
    );
}
#[test]
fn navigation_loop_guidance_mentions_task_tracker_in_planning_workflow() {
    let guidance = navigation_loop_guidance(true, 1);
    assert!(guidance.contains("task_tracker"));
}
#[test]
fn navigation_loop_guidance_uses_generic_text_outside_planning_workflow() {
    let guidance = navigation_loop_guidance(false, 1);
    assert!(guidance.contains("read/search"));
    assert!(!guidance.contains("task_tracker"));
}
#[test]
fn navigation_loop_guidance_escalates_on_repetition() {
    let guidance = navigation_loop_guidance(false, 2);
    assert!(guidance.contains("navigation-loop guard has fired again"));
    assert!(guidance.contains("one blocking question"));
    assert_ne!(guidance, navigation_loop_guidance(false, 1));
}
#[test]
fn balancer_recovery_continues_and_preserves_mutation_pressure() {
    let mut tracker = LoopTracker::new();
    let sig = r#"command_session:{"action":"run","command":"cargo test"}"#.to_string();
    tracker.record(sig.clone());
    tracker.record(sig.clone());
    tracker.record(sig);
    tracker.consecutive_mutations = 3;
    tracker.consecutive_navigations = 5;

    let outcome = apply_balancer_recovery(&mut tracker);

    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert_eq!(tracker.max_count_filtered(|_| false), 0);
    assert_eq!(tracker.consecutive_mutations, 3);
    assert_eq!(tracker.consecutive_navigations, 0);
}
