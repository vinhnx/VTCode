use super::*;

#[test]
fn preflight_projection_preserves_other_planning_synthesis_directives() {
    let planning = crate::agent::runloop::unified::turn::tool_outcomes::handlers::PLANNING_TOOL_FREE_RECOVERY_DIRECTIVE;
    let mut history = vec![
        uni::Message::tool_response("policy".into(), r#"{"failure_kind":"policy"}"#.into()),
        uni::Message::system(planning.into()),
        uni::Message::assistant("new batch".into()),
        uni::Message::tool_response("preflight".into(), r#"{"failure_kind":"preflight_circuit_breaker"}"#.into()),
        uni::Message::system(planning.into()),
    ];
    let canonical = history.clone();
    super::super::omit_expired_preflight_directives(&mut history);
    assert_eq!(history, canonical[..4]);
    assert_eq!(canonical.len(), 5);
}
#[test]
fn preflight_fixture_projection_removes_expired_bytes_without_changing_archive() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tool_outcomes/handlers/tests/fixtures/tracker_preflight_session.json"
    ))
    .unwrap();
    let canonical: Vec<_> = fixture["legacy_preflight_directives"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| uni::Message::system(value.as_str().unwrap().to_string()))
        .collect();
    assert_eq!(canonical.len(), 6);
    let bytes_before: usize = canonical.iter().map(|message| message.content.as_text().len()).sum();
    let mut projected = canonical.clone();
    super::super::omit_expired_preflight_directives(&mut projected);
    assert!(projected.is_empty());
    assert_eq!(canonical.len(), 6);
    assert_eq!(bytes_before, 2298);
}
#[test]
fn preflight_projection_omits_only_identifiable_legacy_system_directives() {
    let legacy = crate::agent::runloop::unified::turn::tool_outcomes::handlers::PREFLIGHT_CIRCUIT_RECOVERY_DIRECTIVE;
    let canonical = vec![
        uni::Message::system(legacy.to_string()),
        uni::Message::user(legacy.to_string()),
        uni::Message::system("Policy: tools are disabled until approval".to_string()),
    ];
    let archive = canonical.clone();
    let mut projected = canonical.clone();
    super::super::omit_expired_preflight_directives(&mut projected);
    assert_eq!(projected, canonical[1..]);
    assert_eq!(canonical, archive);
    projected.push(uni::Message::turn_scoped_system(super::super::recovery_mode_directive(
        false,
        Some("preflight validation circuit breaker"),
    )));
    assert!(
        projected
            .last()
            .unwrap()
            .content
            .as_text()
            .contains("do_not_emit_tool_calls: true")
    );
    super::super::omit_expired_preflight_directives(&mut projected);
    assert_eq!(projected.len(), 3, "active request-only synthesis must survive");
}
#[test]
fn stable_prefix_hash_ignores_runtime_only_changes() {
    let first =
        "Static prefix\n## Skills\n- rust-skills\n[Runtime Context]\n- Time (UTC): 2026-03-22T00:00:00Z\n- retries: 1";
    let second =
        "Static prefix\n## Skills\n- rust-skills\n[Runtime Context]\n- Time (UTC): 2026-03-23T00:00:00Z\n- retries: 4";

    assert_eq!(stable_system_prefix_hash(first), stable_system_prefix_hash(second));
}
#[test]
fn collapsed_output_notice_is_provider_neutral_for_collapsed_tool_turns() {
    let history = vec![uni::Message::tool_response("toolu_1".to_string(), "output".to_string())];

    assert!(super::super::should_add_collapsed_tool_output_notice(
        true,
        ToolDisplayMode::Compact,
        Some(ToolOutputMode::Full),
        &history,
    ));
    assert!(super::super::should_add_collapsed_tool_output_notice(
        false,
        ToolDisplayMode::Expanded,
        Some(ToolOutputMode::Compact),
        &history,
    ));
    assert!(!super::super::should_add_collapsed_tool_output_notice(
        true,
        ToolDisplayMode::Expanded,
        Some(ToolOutputMode::Full),
        &history,
    ));
    let command_history = vec![uni::Message::tool_response_with_origin(
        "toolu_2".to_string(),
        "output".to_string(),
        "exec_command".to_string(),
    )];
    assert!(super::super::should_add_collapsed_tool_output_notice(
        true,
        ToolDisplayMode::Expanded,
        Some(ToolOutputMode::Full),
        &command_history,
    ));
    assert!(super::super::should_add_collapsed_tool_output_notice(
        true,
        ToolDisplayMode::Compact,
        Some(ToolOutputMode::Compact),
        &history,
    ));

    let history_with_system_directive = vec![
        history[0].clone(),
        uni::Message::system("auto-permission review warning".to_string()),
    ];
    assert!(super::super::should_add_collapsed_tool_output_notice(
        true,
        ToolDisplayMode::Expanded,
        Some(ToolOutputMode::Compact),
        &history_with_system_directive,
    ));
}
