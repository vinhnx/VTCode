use super::{
    CodexMcpStartupStatus, CodexMcpStartupTracker, MCP_SERVER_STATUS_UPDATED_METHOD, ServerEvent,
    archive_save_warning_message, codex_experimental_features_enabled, finalize_archive, normalize_planning_input,
    parse_mcp_startup_notification, persist_archive_progress,
};
use anyhow::anyhow;
use serde_json::json;
use vtcode_core::llm::provider::MessageRole;
use vtcode_core::prompts::system::PLANNING_WORKFLOW_IMPLEMENTATION_PROMPT;
use vtcode_core::utils::session_archive::SessionMessage;

#[test]
fn tracker_emits_immediate_failure_and_settled_summary() {
    let mut tracker = CodexMcpStartupTracker::new(Some(["alpha".to_string(), "beta".to_string()]));

    assert!(
        tracker
            .record_update("alpha".to_string(), CodexMcpStartupStatus::Starting)
            .is_empty()
    );

    let alpha_failure = tracker.record_update(
        "alpha".to_string(),
        CodexMcpStartupStatus::Failed {
            error: "MCP client for `alpha` failed to start: handshake failed".to_string(),
        },
    );
    assert_eq!(alpha_failure, vec!["MCP client for `alpha` failed to start: handshake failed".to_string()]);

    assert!(
        tracker
            .record_update("beta".to_string(), CodexMcpStartupStatus::Starting)
            .is_empty()
    );

    let settled = tracker.record_update("beta".to_string(), CodexMcpStartupStatus::Ready);
    assert_eq!(settled, vec!["MCP startup incomplete (failed: alpha)".to_string()]);
}

#[test]
fn tracker_ignores_stale_terminal_updates_after_finish() {
    let mut tracker = CodexMcpStartupTracker::new(Some(["alpha".to_string()]));
    let _ = tracker.record_update("alpha".to_string(), CodexMcpStartupStatus::Starting);
    let _ = tracker.record_update("alpha".to_string(), CodexMcpStartupStatus::Ready);

    assert!(
        tracker
            .record_update("alpha".to_string(), CodexMcpStartupStatus::Ready)
            .is_empty()
    );
}

#[test]
fn tracker_resets_when_next_round_starts() {
    let mut tracker = CodexMcpStartupTracker::new(Some(["alpha".to_string()]));
    let _ = tracker.record_update("alpha".to_string(), CodexMcpStartupStatus::Starting);
    let _ = tracker.record_update("alpha".to_string(), CodexMcpStartupStatus::Ready);

    assert!(
        tracker
            .record_update("alpha".to_string(), CodexMcpStartupStatus::Starting)
            .is_empty()
    );

    let next_round = tracker.record_update(
        "alpha".to_string(),
        CodexMcpStartupStatus::Failed {
            error: "MCP client for `alpha` failed to start".to_string(),
        },
    );
    assert_eq!(
        next_round,
        vec![
            "MCP client for `alpha` failed to start".to_string(),
            "MCP startup incomplete (failed: alpha)".to_string()
        ]
    );
}

#[test]
fn finish_after_lag_marks_missing_expected_servers_interrupted() {
    let mut tracker = CodexMcpStartupTracker::new(Some(["alpha".to_string(), "beta".to_string()]));
    let _ = tracker.record_update("alpha".to_string(), CodexMcpStartupStatus::Starting);
    let _ = tracker.record_update(
        "alpha".to_string(),
        CodexMcpStartupStatus::Failed {
            error: "MCP client for `alpha` failed to start".to_string(),
        },
    );

    let lagged = tracker.finish_after_lag();
    assert_eq!(
        lagged,
        vec![
            "MCP startup interrupted. The following servers were not initialized: beta".to_string(),
            "MCP startup incomplete (failed: alpha)".to_string()
        ]
    );
}

#[test]
fn parse_mcp_startup_notification_reads_failed_update() {
    let event = ServerEvent {
        method: MCP_SERVER_STATUS_UPDATED_METHOD.to_string(),
        params: json!({
            "name": "alpha",
            "status": "failed",
            "error": "MCP client for `alpha` failed to start: handshake failed"
        }),
        id: None,
    };

    let parsed = parse_mcp_startup_notification(&event);
    assert_eq!(
        parsed,
        Some((
            "alpha".to_string(),
            CodexMcpStartupStatus::Failed {
                error: "MCP client for `alpha` failed to start: handshake failed".to_string(),
            },
        ))
    );
}

#[test]
fn normalize_planning_input_switches_to_implementation_prompt() {
    let mut planning_active = true;
    let (user_input, turn_input) = normalize_planning_input("implement", &mut planning_active);

    assert_eq!(user_input, "implement");
    assert_eq!(turn_input, PLANNING_WORKFLOW_IMPLEMENTATION_PROMPT);
    assert!(!planning_active);
}

#[test]
fn normalize_planning_input_preserves_continue_user_prompt() {
    let mut planning_active = true;
    let (user_input, turn_input) = normalize_planning_input("continue", &mut planning_active);

    assert_eq!(user_input, "continue");
    assert_eq!(turn_input, PLANNING_WORKFLOW_IMPLEMENTATION_PROMPT);
    assert!(!planning_active);
}

#[test]
fn normalize_planning_input_recognizes_approval_aliases() {
    // Regression: the bridge previously only knew `implement|continue|go|
    // start|yes`, so a Codex server user typing `approve`/`lgtm`/`ship it`/
    // `implement now` stayed stuck in plan mode. These now derive from
    // `vtcode_core::planning` and must clear the flag + rewrite to the
    // implementation prompt.
    for alias in [
        "approve",
        "approved",
        "lgtm",
        "accepted",
        "ship it",
        "implement now",
        "start implementing",
        "execute the plan",
    ] {
        let mut planning_active = true;
        let (user_input, turn_input) = normalize_planning_input(alias, &mut planning_active);
        assert_eq!(user_input, alias, "user_input for {alias}");
        assert_eq!(turn_input, PLANNING_WORKFLOW_IMPLEMENTATION_PROMPT, "turn_input rewrite for {alias}");
        assert!(!planning_active, "flag cleared for {alias}");
    }
}

#[test]
fn normalize_planning_input_exit_planning_workflow_clears_flag_without_rewrite() {
    // "exit planning workflow" clears the planning-active flag (it is a
    // flag-clearing alias) but is *not* an implementation intent, so it must
    // pass through verbatim rather than being rewritten to the
    // implementation prompt. Both alias sets now derive from
    // `vtcode_core::planning`.
    let mut planning_active = true;
    let (user_input, turn_input) = normalize_planning_input("exit planning workflow", &mut planning_active);

    assert_eq!(user_input, "exit planning workflow");
    assert_eq!(turn_input, "exit planning workflow");
    assert!(!planning_active);
}

#[test]
fn normalize_planning_input_leaves_unrelated_input_untouched() {
    let mut planning_active = true;
    let (user_input, turn_input) = normalize_planning_input("add a login page", &mut planning_active);

    assert_eq!(user_input, "add a login page");
    assert_eq!(turn_input, "add a login page");
    assert!(planning_active);
}

#[test]
fn archive_save_warning_message_mentions_context_and_continuation() {
    let warning = archive_save_warning_message("after turn completion", &anyhow!("disk full while writing archive"));

    assert!(warning.contains("after turn completion"));
    assert!(warning.contains("VT Code will continue"));
    assert!(warning.contains("disk full while writing archive"));
}

#[test]
fn persist_archive_progress_without_archive_is_noop() {
    let messages = vec![SessionMessage::new(MessageRole::User, "hello")];
    assert!(persist_archive_progress(None, &messages, 1).is_none());
}

#[test]
fn finalize_archive_without_archive_is_noop() {
    let messages = vec![SessionMessage::new(MessageRole::Assistant, "done")];
    assert!(finalize_archive(None, messages).is_none());
}

#[test]
fn codex_experimental_features_default_to_disabled_without_vt_config() {
    assert!(!codex_experimental_features_enabled(None));
}
