use serde_json::json;
use vtcode_core::config::constants::tools;

use super::{
    ToolPermissionPromptKind, build_permission_context_lines, build_tool_permission_options, cancelled_prompt_decision,
    compact_justification_lines, extract_shell_approval_command_prefix_words, extract_shell_approval_justification,
    extract_shell_approval_scope_signature, extract_shell_command_text, extract_shell_persistent_approval_prefix_rule,
    format_command_preview_lines, lowercase_first_action, push_capped_context_line,
    render_shell_persistent_approval_prefix_entry, shell_allows_persistent_decisions, shell_command_preview_lines,
    shell_environment_row, shell_permission_cache_suffix, tool_permission_header_lines, tool_permission_prompt_kind,
    tool_permission_title, truncate_arg_preview,
};
use crate::agent::runloop::unified::tool_routing::shell_approval::PersistentApprovalTarget;

#[test]
fn command_session_extracts_command_when_action_is_run() {
    let args = json!({
        "action": "run",
        "command": "cargo check"
    });
    let command = extract_shell_command_text(tools::UNIFIED_EXEC, Some(&args));
    assert_eq!(command.as_deref(), Some("cargo check"));
}

#[test]
fn command_session_extracts_cmd_alias_when_action_is_inferred() {
    let args = json!({
        "cmd": "cargo check"
    });
    let command = extract_shell_command_text(tools::UNIFIED_EXEC, Some(&args));
    assert_eq!(command.as_deref(), Some("cargo check"));
}

#[test]
fn command_session_extracts_indexed_command_parts_when_action_is_inferred() {
    let args = json!({
        "command.0": "cargo",
        "command.1": "check"
    });
    let command = extract_shell_command_text(tools::UNIFIED_EXEC, Some(&args));
    assert_eq!(command.as_deref(), Some("cargo check"));
}

#[test]
fn command_session_ignores_non_run_actions() {
    let args = json!({
        "action": "poll",
        "session_id": "run-123"
    });
    let command = extract_shell_command_text(tools::UNIFIED_EXEC, Some(&args));
    assert_eq!(command, None);
}

#[test]
fn arg_preview_truncates_unicode_safely() {
    let value = "an’t ".repeat(20);
    let truncated = truncate_arg_preview(&value);
    assert!(truncated.ends_with("..."));
    assert!(truncated.chars().count() <= 60);
}

#[test]
fn shell_runs_disable_persistent_decisions() {
    let args = json!({
        "action": "run",
        "command": "echo hi",
        "sandbox_permissions": "with_additional_permissions"
    });
    assert!(!shell_allows_persistent_decisions(tools::UNIFIED_EXEC, Some(&args)));
}

#[test]
fn non_shell_command_session_actions_keep_persistent_decisions() {
    let args = json!({
        "action": "poll",
        "session_id": "run-123",
        "sandbox_permissions": "with_additional_permissions"
    });
    assert!(shell_allows_persistent_decisions(tools::UNIFIED_EXEC, Some(&args)));
}

#[test]
fn cache_suffix_includes_permissions_for_shell_run() {
    let plain = json!({
        "command": "echo hi"
    });
    let with_permissions = json!({
        "command": "echo hi",
        "sandbox_permissions": "with_additional_permissions",
        "additional_permissions": {
            "fs_write": ["/tmp/demo.txt"]
        }
    });

    let plain_key = shell_permission_cache_suffix("shell", Some(&plain));
    let permissioned_key = shell_permission_cache_suffix("shell", Some(&with_permissions));

    assert_ne!(plain_key, permissioned_key);
    assert!(
        permissioned_key
            .as_deref()
            .unwrap_or_default()
            .contains("with_additional_permissions")
    );
}

#[test]
fn cache_suffix_normalizes_use_default_with_additional_permissions() {
    let args = json!({
        "command": "echo hi",
        "sandbox_permissions": "use_default",
        "additional_permissions": {
            "fs_write": ["/tmp/demo.txt"]
        }
    });

    let permissioned_key = shell_permission_cache_suffix("shell", Some(&args));

    assert_eq!(
        permissioned_key.as_deref(),
        Some(
            "echo hi|sandbox_permissions=\"with_additional_permissions\"|additional_permissions={\"fs_write\":[\"/tmp/demo.txt\"]}"
        )
    );
}

#[test]
fn cache_suffix_ignores_empty_additional_permissions() {
    let args = json!({
        "command": "echo hi",
        "sandbox_permissions": "use_default",
        "additional_permissions": {
            "fs_read": [],
            "fs_write": []
        }
    });

    let permissioned_key = shell_permission_cache_suffix("shell", Some(&args));

    assert_eq!(permissioned_key.as_deref(), Some("echo hi"));
}

#[test]
fn shell_approval_justification_is_extracted_for_run_actions() {
    let args = json!({
        "action": "run",
        "command": "cargo build",
        "justification": "Do you want to build the project outside the sandbox?"
    });

    let justification = extract_shell_approval_justification(tools::UNIFIED_EXEC, Some(&args));
    assert_eq!(justification.as_deref(), Some("Do you want to build the project outside the sandbox?"));
}

#[test]
fn shell_approval_justification_ignores_non_run_actions() {
    let args = json!({
        "action": "poll",
        "session_id": "run-123",
        "justification": "ignored"
    });

    let justification = extract_shell_approval_justification(tools::UNIFIED_EXEC, Some(&args));
    assert_eq!(justification, None);
}

#[test]
fn shell_persistent_prefix_rule_requires_matching_command_prefix() {
    let args = json!({
        "action": "run",
        "command": "cargo test -p vtcode",
        "prefix_rule": ["cargo", "build"]
    });

    let prefix_rule = extract_shell_persistent_approval_prefix_rule(tools::UNIFIED_EXEC, Some(&args));
    assert_eq!(prefix_rule, None);
}

#[test]
fn shell_persistent_prefix_rule_rejects_compound_commands() {
    let args = json!({
        "action": "run",
        "command": "cargo test && cargo fmt",
        "prefix_rule": ["cargo", "test"]
    });

    let prefix_rule = extract_shell_persistent_approval_prefix_rule(tools::UNIFIED_EXEC, Some(&args));
    assert_eq!(prefix_rule, None);
}

#[test]
fn shell_approval_prefix_text_normalizes_simple_commands() {
    let args = json!({
        "action": "run",
        "command": ["cargo", "test", "-p", "vtcode"]
    });

    let command = extract_shell_approval_command_prefix_words(tools::UNIFIED_EXEC, Some(&args));
    assert_eq!(
        command,
        Some(vec![
            "cargo".to_string(),
            "test".to_string(),
            "-p".to_string(),
            "vtcode".to_string()
        ])
    );
}

#[test]
fn shell_approval_scope_signature_tracks_requested_permissions() {
    let args = json!({
        "action": "run",
        "command": "cargo test",
        "sandbox_permissions": "require_escalated"
    });

    let scope = extract_shell_approval_scope_signature(tools::UNIFIED_EXEC, Some(&args));
    assert_eq!(scope.as_deref(), Some("sandbox_permissions=\"require_escalated\"|additional_permissions=null"));
}

#[test]
fn shell_approval_scope_signature_normalizes_implicit_additional_permissions() {
    let args = json!({
        "action": "run",
        "command": "cargo test",
        "sandbox_permissions": "use_default",
        "additional_permissions": {
            "fs_write": ["/tmp/demo.txt"]
        }
    });

    let scope = extract_shell_approval_scope_signature(tools::UNIFIED_EXEC, Some(&args));
    assert_eq!(
        scope.as_deref(),
        Some(
            "sandbox_permissions=\"with_additional_permissions\"|additional_permissions={\"fs_write\":[\"/tmp/demo.txt\"]}"
        )
    );
}

#[test]
fn shell_persistent_approval_entry_includes_scope() {
    let args = json!({
        "action": "run",
        "command": "cargo test -p vtcode",
        "prefix_rule": ["cargo", "test"],
        "sandbox_permissions": "require_escalated"
    });

    let entry = render_shell_persistent_approval_prefix_entry(
        tools::UNIFIED_EXEC,
        Some(&args),
        &["cargo".to_string(), "test".to_string()],
    );
    assert_eq!(
        entry.as_deref(),
        Some("cargo test|sandbox_permissions=\"require_escalated\"|additional_permissions=null")
    );
}

#[test]
fn canonical_mcp_tools_use_mcp_prompt_kind() {
    assert_eq!(tool_permission_prompt_kind("mcp::calendar::list_events"), ToolPermissionPromptKind::Mcp);
}

#[test]
fn model_visible_mcp_tools_use_mcp_prompt_kind() {
    assert_eq!(tool_permission_prompt_kind("mcp__calendar__list_events"), ToolPermissionPromptKind::Mcp);
}

#[test]
fn non_mcp_tools_keep_standard_prompt_kind() {
    assert_eq!(tool_permission_prompt_kind(tools::UNIFIED_EXEC), ToolPermissionPromptKind::Standard);
}

#[test]
fn mcp_prompt_uses_cancel_without_persistent_deny() {
    let titles =
        build_tool_permission_options(ToolPermissionPromptKind::Mcp, Some(&PersistentApprovalTarget::ToolLevel))
            .into_iter()
            .map(|item| item.title)
            .collect::<Vec<_>>();
    assert!(titles.iter().any(|title| title == "Cancel"));
    assert!(!titles.iter().any(|title| title == "Always Deny"));
}

#[test]
fn standard_prompt_keeps_persistent_deny() {
    let titles =
        build_tool_permission_options(ToolPermissionPromptKind::Standard, Some(&PersistentApprovalTarget::ToolLevel))
            .into_iter()
            .map(|item| item.title)
            .collect::<Vec<_>>();
    assert!(titles.iter().any(|title| title == "Deny Once"));
    assert!(titles.iter().any(|title| title == "Always Deny"));
}

#[test]
fn always_rows_carry_permanence_badges() {
    let items =
        build_tool_permission_options(ToolPermissionPromptKind::Standard, Some(&PersistentApprovalTarget::ToolLevel));
    let badge_for = |title: &str| {
        items
            .iter()
            .find(|item| item.title == title)
            .unwrap_or_else(|| panic!("{title} row must exist"))
            .badge
            .clone()
    };
    assert_eq!(badge_for("Always approve").as_deref(), Some("Permanent"));
    assert_eq!(badge_for("Always Deny").as_deref(), Some("Persistent"));
    assert_eq!(badge_for("Allow for Session").as_deref(), Some("Session"));
    assert_eq!(badge_for("Approve Once"), None, "one-shot rows must not claim permanence");
    assert_eq!(badge_for("Deny Once"), None);
}

#[test]
fn shell_prompt_offers_policy_cache_option_for_exact_invocation() {
    let titles = build_tool_permission_options(
        ToolPermissionPromptKind::Standard,
        Some(&PersistentApprovalTarget::ExactInvocation {
            display_label: "command `cargo clippy`".to_string(),
        }),
    )
    .into_iter()
    .map(|item| item.title)
    .collect::<Vec<_>>();
    assert!(titles.iter().any(|title| title == "Always approve"));
    assert!(!titles.iter().any(|title| title == "Always Deny"));
}

#[test]
fn shell_prompt_labels_learned_family_permanent_option() {
    let items = build_tool_permission_options(
        ToolPermissionPromptKind::Standard,
        Some(&PersistentApprovalTarget::LearnedPattern {
            key: "shell-pattern:awk README.md|sandbox_permissions=\"use_default\"|additional_permissions=null"
                .to_string(),
            display_label: "safe `awk` reads under `README.md`".to_string(),
        }),
    );
    let permanent = items
        .iter()
        .find(|item| item.title == "Always approve")
        .expect("permanent option");
    assert_eq!(permanent.subtitle.as_deref(), Some("Remember safe `awk` reads under `README.md` in this workspace"));
    assert_eq!(permanent.badge.as_deref(), Some("Permanent"));
}

#[test]
fn shell_header_omits_intro_because_title_asks_the_question() {
    let lines = tool_permission_header_lines("exec_command", "python3 -c 'truncated…'", true, None);
    assert!(lines.is_empty(), "shell intro lives in the overlay title, got: {lines:?}");
}

#[test]
fn shell_header_keeps_source_row_without_intro() {
    let lines = tool_permission_header_lines("exec_command", "cargo test", true, Some("agent-1"));
    assert_eq!(lines, vec!["Requested from: agent-1"]);
}

#[test]
fn permission_title_asks_the_question_only_for_shell_commands() {
    assert_eq!(tool_permission_title(true), "Would you like to run the following command?");
    assert_eq!(tool_permission_title(false), "Tool Permission Required");
}

#[test]
fn header_keeps_action_summary_for_diff_preview_file_identity() {
    let lines = tool_permission_header_lines("edit_file", "Edit file src/main.rs", false, None);
    assert_eq!(lines, vec!["The agent wants to edit file src/main.rs and needs your approval."]);
}

#[test]
fn header_lists_source_above_action_summary() {
    let lines = tool_permission_header_lines("edit_file", "Edit file src/main.rs", false, Some("agent-1"));
    assert_eq!(
        lines,
        vec![
            "The agent wants to edit file src/main.rs and needs your approval.",
            "Requested from: agent-1"
        ]
    );
}

#[test]
fn header_falls_back_to_tool_name_when_action_is_empty() {
    let lines = tool_permission_header_lines("exec_command", "   ", false, None);
    assert_eq!(lines, vec!["The agent wants to use exec_command and needs your approval."]);
}

#[test]
fn header_moves_mcp_prefix_to_trailing_via_mcp() {
    let lines = tool_permission_header_lines("mcp__calendar__list_events", "MCP Search code for docs", false, None);
    assert_eq!(lines, vec!["The agent wants to search code for docs via MCP and needs your approval."]);
}

#[test]
fn lowercase_first_action_preserves_leading_acronyms() {
    assert_eq!(lowercase_first_action("Edit file src/main.rs"), "edit file src/main.rs");
    assert_eq!(lowercase_first_action("MCP Search code"), "MCP Search code");
    assert_eq!(lowercase_first_action(""), "");
}

#[test]
fn permission_context_lines_fall_back_without_any_reason() {
    let lines = build_permission_context_lines(None, None, None);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("No additional details were provided."));
}

#[test]
fn permission_context_lines_fall_back_when_reason_is_blank() {
    let lines = build_permission_context_lines(Some("   \n  "), None, None);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("No additional details were provided."));
}

#[test]
fn permission_context_lines_dedupes_shell_goal_with_justification_goal() {
    let just = vtcode_core::tools::ToolJustification {
        tool_name: "exec_command".to_string(),
        reason: "List the workspace".to_string(),
        expected_outcome: None,
        risk_level: "Low".to_string(),
        timestamp: "now".to_string(),
    };
    let lines = build_permission_context_lines(None, Some("List the workspace"), Some(&just));
    let goals = lines.iter().filter(|line| line.starts_with(super::AGENT_GOAL_LABEL)).count();
    assert_eq!(goals, 1, "shell and ledger goals must not duplicate: {lines:?}");
    assert!(lines.iter().any(|line| line == "Risk: Low"));
}

#[test]
fn permission_context_lines_keeps_risk_when_goal_comes_from_approval_reason() {
    let just = vtcode_core::tools::ToolJustification {
        tool_name: "exec_command".to_string(),
        reason: "Ledger goal".to_string(),
        expected_outcome: None,
        risk_level: "High".to_string(),
        timestamp: "now".to_string(),
    };
    let lines = build_permission_context_lines(Some("Explicit reason"), None, Some(&just));
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with(&format!("{}: Explicit reason", super::AGENT_GOAL_LABEL)));
    assert_eq!(lines[1], "Risk: High");
}

#[test]
fn mcp_prompt_cancellation_is_non_persistent() {
    assert_eq!(cancelled_prompt_decision(ToolPermissionPromptKind::Mcp), super::HitlDecision::DeniedOnce);
}

#[test]
fn capped_context_line_keeps_first_logical_row_only() {
    let mut lines = Vec::new();
    push_capped_context_line(&mut lines, "Reason", "  \nFirst reason\nSecond reason\n", 160);
    assert_eq!(lines, vec!["Reason: First reason"]);

    let mut blank = Vec::new();
    push_capped_context_line(&mut blank, "Reason", "   \n  ", 160);
    assert!(blank.is_empty());
}

#[test]
fn compact_justification_lines_keep_reason_and_risk_only() {
    let just = vtcode_core::tools::ToolJustification {
        tool_name: "exec_command".to_string(),
        reason: "Build the project\nwith more detail on a second line".to_string(),
        expected_outcome: Some("Binary compiles\nplus extra".to_string()),
        risk_level: "High".to_string(),
        timestamp: "now".to_string(),
    };
    let lines = compact_justification_lines(&just);
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("What the agent is trying to do: Build the project"));
    assert!(!lines[0].contains('\n'));
    assert!(!lines.iter().any(|line| line.starts_with("Expected:")));
    assert_eq!(lines[1], "Risk: High");
}

#[test]
fn compact_justification_lines_truncate_long_reason() {
    let just = vtcode_core::tools::ToolJustification {
        tool_name: "exec_command".to_string(),
        reason: "x".repeat(400),
        expected_outcome: None,
        risk_level: String::new(),
        timestamp: "now".to_string(),
    };
    let lines = compact_justification_lines(&just);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].chars().count() <= super::AGENT_GOAL_LABEL.len() + 2 + super::MAX_CONTEXT_LINE_CHARS);
}

#[test]
fn permanent_option_subtitle_truncates_long_command_label() {
    // Asymmetric head/tail: middle-truncation must keep the executable
    // prefix AND the dangerous destination suffix.
    let long_label = format!("command `python3 {}` --output ../../critical.txt", "x".repeat(200));
    let items = build_tool_permission_options(
        ToolPermissionPromptKind::Standard,
        Some(&PersistentApprovalTarget::ExactInvocation { display_label: long_label }),
    );
    let permanent = items
        .iter()
        .find(|item| item.title == "Always approve")
        .expect("permanent option");
    let subtitle = permanent.subtitle.as_deref().expect("subtitle");
    assert!(subtitle.contains('…'), "long label should be truncated, got: {subtitle}");
    assert!(subtitle.contains("python3"), "executable prefix missing from {subtitle:?}");
    assert!(subtitle.contains("../../critical.txt"), "dangerous suffix missing from {subtitle:?}");
    assert!(subtitle.chars().count() <= "Remember  in this workspace".len() + super::MAX_APPROVAL_LABEL_CHARS + 8);
}

#[test]
fn command_preview_distinguishes_same_prefix_different_dangerous_suffixes() {
    let shared = "python3 -c 'print(1)' --output ".to_string() + &"safe-prefix/".repeat(12);
    let first = format_command_preview_lines(vec![format!("{shared}report.txt")]);
    let second = format_command_preview_lines(vec![format!("{shared}../../critical.txt")]);
    assert_ne!(first, second);
    assert!(first[0].contains("report.txt"), "tail missing from {first:?}");
    assert!(second[0].contains("../../critical.txt"), "dangerous tail missing from {second:?}");
    assert!(
        !first[0].contains('…') && !second[0].contains('…'),
        "full command must not truncate: {first:?} {second:?}"
    );
    assert!(
        first[0].starts_with("`$ ") && second[0].starts_with("`$ "),
        "first row carries the shell marker: {first:?} {second:?}"
    );
}

#[test]
fn command_preview_preserves_short_unicode_command() {
    let preview = format_command_preview_lines(vec!["printf 'xin chào 世界'".to_string()]);
    assert_eq!(preview, vec!["`$ printf 'xin chào 世界'`"]);
}

#[test]
fn command_preview_shows_long_single_line_in_full_without_truncation() {
    // Screenshot 2026-09-27: `awk 'NR==FNR ...'` was middle-truncated in
    // the HITL modal, hiding the script body under review.
    let command = "awk 'NR==FNR {if ($0 ~ /^#+ /) {h=$0; sub(/^#+ /, \"\", h); gsub(/ +$/, \"\", h)}}' README.md README.md; echo 'anchor-check exit=$?'";
    let preview = format_command_preview_lines(vec![command.to_string()]);
    assert_eq!(preview.len(), 1);
    assert!(preview[0].starts_with("`$ awk"), "got: {:?}", preview[0]);
    assert!(!preview[0].contains('…'), "got: {:?}", preview[0]);
    for fragment in ["awk", "NR==FNR", "/^#+ /", "README.md", "anchor-check"] {
        assert!(preview[0].contains(fragment), "missing {fragment:?} in {:?}", preview[0]);
    }
}

#[test]
fn multiline_command_preview_collapses_middle_but_keeps_full_lines() {
    // Viewport budget: 12 logical lines collapse to 5 head + marker +
    // 2 tail, but every shown line is complete. Each line exceeds the
    // old 120-char truncation cap, locking in no per-line truncation.
    let lines: Vec<String> = (1..=12).map(|line| format!("line {line} {}", "x".repeat(150))).collect();
    let preview = format_command_preview_lines(lines);
    assert_eq!(preview.len(), 8);
    assert_eq!(preview[0], format!("`$ line 1 {}`", "x".repeat(150)));
    assert_eq!(preview[4], format!("`line 5 {}`", "x".repeat(150)));
    assert_eq!(preview[5], "… +5 more lines (full command runs on approval)");
    assert_eq!(preview[6], format!("`line 11 {}`", "x".repeat(150)));
    assert_eq!(preview[7], format!("`line 12 {}`", "x".repeat(150)));
}

#[test]
fn command_preview_neutralizes_carriage_return_and_tab() {
    // A lone `\r` survives `str::lines`; without sanitization the
    // terminal could hide the suffix of the preview row under review.
    let preview = format_command_preview_lines(vec!["echo ok\r\trm -rf /tmp/review-target".to_string()]);
    assert_eq!(preview.len(), 1);
    assert!(
        preview[0].chars().all(|c| c != '\r' && c != '\t'),
        "control chars must be neutralized, got: {preview:?}"
    );
    assert!(preview[0].contains("rm -rf /tmp/review-target"), "suffix must stay visible: {preview:?}");
}

#[test]
fn command_preview_skips_blank_lines_to_save_viewport_rows() {
    let preview = format_command_preview_lines(vec![
        "echo alpha".to_string(),
        "   ".to_string(),
        String::new(),
        "echo beta".to_string(),
    ]);
    assert_eq!(preview, vec!["`$ echo alpha`", "`echo beta`"]);
}

#[test]
fn shell_environment_row_reports_requested_posture() {
    let run = |extra: serde_json::Value| {
        let mut args = serde_json::Map::new();
        args.insert("action".to_string(), json!("run"));
        args.insert("command".to_string(), json!("cargo check --locked"));
        if let serde_json::Value::Object(map) = extra {
            args.extend(map);
        }
        shell_environment_row(tools::UNIFIED_EXEC, Some(&serde_json::Value::Object(args)))
    };
    assert_eq!(run(json!({})).as_deref(), Some("Environment: default policy"));
    assert_eq!(
        run(json!({"sandbox_permissions": "require_escalated"})).as_deref(),
        Some("Environment: escalated privileges")
    );
    assert_eq!(run(json!({"sandbox_permissions": "bypass_sandbox"})).as_deref(), Some("Environment: no sandbox"));
    assert_eq!(
        run(json!({"additional_permissions": {"fs_write": ["/tmp/demo.txt"]}})).as_deref(),
        Some("Environment: default policy + extra grants")
    );
}

#[test]
fn shell_environment_row_is_absent_for_non_shell_tools() {
    assert!(
        shell_environment_row(tools::UNIFIED_EXEC, Some(&json!({"action": "poll", "session_id": "run-1"}))).is_none()
    );
    assert!(shell_environment_row("edit_file", Some(&json!({"path": "src/main.rs"}))).is_none());
}

#[test]
fn shell_command_preview_uses_canonical_extractor_for_aliases() {
    for (tool_name, args) in [
        ("exec_command", json!({"cmd": "cargo check --locked"})),
        (tools::UNIFIED_EXEC, json!({"action": "run", "command": "cargo nextest run"})),
        ("container.exec", json!({"command": "git status --short"})),
    ] {
        let preview = shell_command_preview_lines(tool_name, Some(&args)).expect("run alias has command preview");
        assert_eq!(preview.len(), 1);
        assert!(!preview[0].is_empty());
    }
    assert!(
        shell_command_preview_lines(tools::UNIFIED_EXEC, Some(&json!({"action": "poll", "session_id": "run-1"})))
            .is_none()
    );
}
