use super::*;
use serde_json::json;
use std::path::Path;

#[test]
fn lookup_trims_and_rejects_blanks() {
    let args = json!({ "path": "  src/main.rs  ", "blank": "   ", "num": 7 });
    assert_eq!(lookup_string(&args, "path").as_deref(), Some("src/main.rs"));
    assert_eq!(lookup_string(&args, "blank"), None);
    assert_eq!(lookup_string(&args, "num"), None);
    assert_eq!(lookup_string(&args, "missing"), None);
    // Multi-key form scans in order and skips blanks for the first usable value.
    let args = json!({ "a": "  ", "b": " second " });
    assert_eq!(lookup_first_string(&args, &["a", "b"]).as_deref(), Some("second"));
    assert_eq!(lookup_first_string(&args, &["a", "missing"]), None);
}

#[test]
fn relativize_matches_workspace_membership() {
    let root = Path::new("/ws");
    assert_eq!(relativize_to_workspace("/ws/src/main.rs", Some(root)), "src/main.rs");
    // Asymmetric: outside the root, the root itself, and relative inputs survive verbatim.
    assert_eq!(relativize_to_workspace("/elsewhere/a.rs", Some(root)), "/elsewhere/a.rs");
    assert_eq!(relativize_to_workspace("/ws", Some(root)), "/ws");
    assert_eq!(relativize_to_workspace("src/main.rs", Some(root)), "src/main.rs");
    assert_eq!(relativize_to_workspace("/ws/src/main.rs", None), "/ws/src/main.rs");
}

#[test]
fn humanize_key_capitalizes_first_letter_only() {
    assert_eq!(humanize_key("timeout_secs"), "Timeout secs");
    assert_eq!(humanize_key("url"), "Url");
    assert_eq!(humanize_key(""), "");
}

#[test]
fn humanize_tool_name_splits_word_boundaries() {
    for (raw, pretty) in [
        ("diagram_render", "Diagram Render"),
        ("DiagramCommand", "Diagram Command"),
        ("diagramRender", "Diagram Render"),
        ("read_file", "Read File"),
        ("mcp__server__diagram_render", "Diagram Render"),
        ("mcp::server::read_file", "Read File"),
    ] {
        assert_eq!(humanize_tool_name(raw), pretty, "{raw} prettifies");
    }
    // Asymmetric: acronyms and versions keep casing, wordless names echo.
    assert_eq!(humanize_tool_name("MCPTool"), "MCPTool");
    assert_eq!(humanize_tool_name("task_tracker_v2"), "Task Tracker V2");
    assert_eq!(humanize_tool_name("___"), "___");
    assert_eq!(humanize_tool_name(""), "");
}

#[test]
fn strip_mcp_prefix_is_total() {
    assert_eq!(strip_mcp_prefix("mcp__server__tool"), "tool");
    assert_eq!(strip_mcp_prefix("mcp::provider::tool"), "tool");
    assert_eq!(strip_mcp_prefix("mcp_fetch"), "fetch");
    assert_eq!(strip_mcp_prefix("fetch"), "fetch");
}

#[test]
fn mcp_tool_display_name_handles_all_spellings() {
    assert_eq!(mcp_tool_display_name("mcp__server__tool"), Some("tool"));
    assert_eq!(mcp_tool_display_name("mcp__tool"), Some("tool"));
    assert_eq!(mcp_tool_display_name("mcp::provider::tool"), Some("tool"));
    assert_eq!(mcp_tool_display_name("mcp_fetch"), Some("fetch"));
    assert_eq!(mcp_tool_display_name("fetch"), None);
}

#[test]
fn summarize_list_shows_two_plus_overflow() {
    assert_eq!(summarize_list(&[]), "");
    assert_eq!(summarize_list(&["a".to_string()]), "a");
    assert_eq!(summarize_list(&["a".to_string(), "b".to_string()]), "a, b");
    assert_eq!(summarize_list(&["a".to_string(), "b".to_string(), "c".to_string()]), "a, b +1 more");
}

#[test]
fn verbose_keys_match_command_and_content_keys() {
    for key in [
        "command",
        "cmd",
        "raw_command",
        "bash_command",
        "old_str",
        "new_str",
        "content",
        "new_content",
        "text",
        "patch",
        "code",
    ] {
        assert!(is_verbose_key(key), "{key} is verbose");
    }
    assert!(!is_verbose_key("path"));
    assert!(!is_verbose_key("pattern"));
}

#[test]
fn is_noise_param_matches_expected_keys() {
    assert!(is_noise_param("timeout_secs"));
    assert!(is_noise_param("max_bytes"));
    assert!(is_noise_param("debug_query"));
    assert!(is_noise_param("strictness"));
    assert!(is_noise_param("case_sensitive"));
    assert!(is_noise_param("context_lines"));
    assert!(is_noise_param("shell"));
    assert!(is_noise_param("sandbox_permissions"));
    assert!(is_noise_param("action")); // Redundant with summary headline
    // Output caps are plumbing like the size limits above.
    assert!(is_noise_param("max_output_tokens"));
    assert!(is_noise_param("max_tokens"));
    // Session plumbing is folded by `exec_session_param_detail` instead.
    assert!(!is_noise_param("session_id"));
    assert!(!is_noise_param("chars"));
    assert!(!is_noise_param("wait_timeout_seconds"));
    // Read file params should pass through (not noise)
    assert!(!is_noise_param("offset"));
    assert!(!is_noise_param("limit"));
    assert!(!is_noise_param("head_lines"));
    assert!(!is_noise_param("tail_lines"));
    assert!(!is_noise_param("start_line"));
    assert!(!is_noise_param("end_line"));
    // Meaningful params should pass through
    assert!(!is_noise_param("pattern"));
    assert!(!is_noise_param("path"));
    assert!(!is_noise_param("mode"));
}

#[test]
fn collect_param_details_skips_noise_params() {
    let args = json!({
        "action": "grep",
        "pattern": "agent loop",
        "strictness": "relaxed",
        "debug_query": "pattern",
        "detail_level": "full",
        "max_results": 20,
        "context_lines": 2,
        "scope": "repo",
        "max_bytes": 6000,
        "timeout_secs": 120
    });
    let details = collect_param_details(&args, &["pattern"], None);
    // Only detail_level, max_results, and scope should remain;
    // pattern is skipped (headline-consumed), noise params (including action) are skipped.
    for detail in &details {
        assert!(
            !detail.contains("Timeout")
                && !detail.contains("Max bytes")
                && !detail.contains("Debug query")
                && !detail.contains("Strictness")
                && !detail.contains("Context lines")
                && !detail.contains("Action"),
            "Noise param leaked through: {detail}"
        );
    }
}

#[test]
fn collect_param_details_skips_zero_numbers() {
    let args = json!({
        "action": "read",
        "path": "src/main.rs",
        "start_line": 1,
        "end_line": 200,
        "offset": 0,
        "limit": 0
    });
    let details = collect_param_details(&args, &["path"], None);
    for detail in &details {
        assert!(
            !detail.contains("Offset") && !detail.contains("Limit"),
            "Zero-valued param leaked through: {detail}"
        );
    }
    assert!(details.iter().any(|d| d.contains("Start line: 1")));
    assert!(details.iter().any(|d| d.contains("End line: 200")));
}

#[test]
fn collect_param_details_empty_skipped_shows_all_displayable() {
    let args = json!({ "path": "src/main.rs", "pattern": "TODO" });
    let details = collect_param_details(&args, &[], None);
    assert!(details.iter().any(|d| d.contains("Path: src/main.rs")), "{details:?}");
    assert!(details.iter().any(|d| d.contains("Pattern: TODO")), "{details:?}");
}

#[test]
fn exec_session_param_detail_keeps_session_and_wait_only() {
    let args = json!({
        "session_id": "run-2d5752f2",
        "chars": "y\n",
        "yield_time_ms": 1000,
        "wait_timeout_seconds": 600,
        "max_output_tokens": 4000,
        "max_tokens": 4000
    });
    // Token caps, the yield window, and the raw stdin payload must not each
    // become their own tree row.
    assert_eq!(exec_session_param_detail(&args).as_deref(), Some("Session run-2d5752f2 · wait up to 600s"));
}

#[test]
fn exec_session_param_detail_accepts_timeout_alias() {
    let args = json!({ "session_id": "run-abc", "timeout_seconds": 30 });
    assert_eq!(exec_session_param_detail(&args).as_deref(), Some("Session run-abc · wait up to 30s"));
}

#[test]
fn exec_session_param_detail_skips_zero_and_missing_wait() {
    let args = json!({ "session_id": "run-abc", "wait_timeout_seconds": 0 });
    assert_eq!(exec_session_param_detail(&args).as_deref(), Some("Session run-abc"));
}

#[test]
fn exec_session_param_detail_none_without_session_or_wait() {
    assert!(exec_session_param_detail(&json!({ "chars": "y\n", "max_output_tokens": 4000 })).is_none());
    assert!(exec_session_param_detail(&json!({ "path": "AGENTS.md" })).is_none());
}

#[test]
fn describe_list_files_phrases_location_then_patterns() {
    assert_eq!(
        describe_list_files(&json!({ "path": "checkpoints" }), None),
        Some(("List files in checkpoints".to_string(), &["path"][..]))
    );
    assert_eq!(
        describe_list_files(&json!({ "path": "." }), None),
        Some(("List files in workspace root".to_string(), &["path"][..]))
    );
    assert_eq!(
        describe_list_files(&json!({ "name_pattern": "*.rs" }), None),
        Some(("Find files named *.rs".to_string(), &["name_pattern"][..]))
    );
    assert_eq!(
        describe_list_files(&json!({ "content_pattern": "TODO" }), None),
        Some(("Search files for TODO".to_string(), &["content_pattern"][..]))
    );
    // Location wins over patterns when both are present.
    assert_eq!(
        describe_list_files(&json!({ "path": "src", "name_pattern": "*.rs" }), None)
            .as_ref()
            .map(|(title, _)| title.as_str()),
        Some("List files in src")
    );
    assert_eq!(describe_list_files(&json!({ "other": 1 }), None), None);
    // Workspace relativization applies to headline paths.
    let root = Path::new("/ws");
    assert_eq!(
        describe_list_files(&json!({ "path": "/ws/checkpoints" }), Some(root))
            .as_ref()
            .map(|(title, _)| title.as_str()),
        Some("List files in checkpoints")
    );
}

#[test]
fn describe_grep_file_requires_pattern_and_adds_scope() {
    assert_eq!(
        describe_grep_file(&json!({ "pattern": "TODO" }), None),
        Some(("Grep TODO".to_string(), &["pattern"][..]))
    );
    assert_eq!(
        describe_grep_file(&json!({ "pattern": "TODO", "path": "src/main.rs" }), None),
        Some(("Grep TODO in src/main.rs".to_string(), &["pattern", "path"][..]))
    );
    assert_eq!(describe_grep_file(&json!({ "path": "src/main.rs" }), None), None);
    assert_eq!(describe_grep_file(&json!({}), None), None);
}

#[test]
fn describe_code_search_marks_query_used() {
    let args = json!({ "query": "agent loop implementation", "max_results": 15 });
    let (summary, used) = describe_code_search(&args).expect("code search summary");
    assert_eq!(summary, "Search code for agent loop implementation");
    assert!(used.contains(&"query"));
    assert!(!used.contains(&"max_results"));
}

#[test]
fn describe_code_search_accepts_query_aliases() {
    // A valid search must never degrade to a generic header when the
    // query arrives under an alias key. The used set must record the
    // actual key so the value is not duplicated as a detail line.
    for key in ["pattern", "q", "text"] {
        let args = json!({ key: "core agent loop", "file_types": ["rs"] });
        let (summary, used) = describe_code_search(&args).expect("alias query should describe");
        assert_eq!(summary, "Search code for core agent loop");
        assert!(used.contains(&key), "used should record the matched key, got {used:?}");
    }
}

#[test]
fn describe_fetch_action_shows_full_url_or_bare_verb() {
    assert_eq!(
        describe_fetch_action(&json!({ "url": "https://x.y/z" })),
        ("Fetch https://x.y/z".to_string(), &["url"][..])
    );
    assert_eq!(describe_fetch_action(&json!({})), ("Use Fetch".to_string(), &[][..]));
}

#[test]
fn preview_command_multiline_script_folds_in_content() {
    // Screenshot 2026-09-02: `• Ran python3 -c "` with `tur…ool_calls`
    // continuations. A prefix-only first line must fold in script lines so
    // the header shows real content instead of a dangling quote.
    let command = "python3 -c \"\nimport json\nwith open('.vtcode/checkpoints/turn_1032.json') as f:\n    pass\n\"";
    let preview = preview_command(command, 70);
    assert!(
        preview.starts_with("python3 -c \"import json"),
        "prefix-only line should fold in script, got: {preview}"
    );
    assert!(!preview.contains("tur…ool"), "mid-string ellipsis leaked: {preview}");
    assert_ne!(preview, "python3 -c \"");
}

#[test]
fn preview_command_unclosed_first_line_appends_continuation() {
    // `python3 -c "import json` (unclosed, partial content) must also pull
    // the next line instead of rendering a dangling open quote.
    let command = "python3 -c \"import json\nwith open('a.json') as f: pass\"";
    let preview = preview_command(command, 70);
    assert!(preview.contains("import json"), "got: {preview}");
    assert!(preview.contains("with open"), "got: {preview}");
}

#[test]
fn preview_command_windows_style_runner_folds_in_content() {
    // Windows-style separators must still be recognized as script runners
    // so the header folds in the script instead of a dangling
    // `python.exe -c "` first line.
    let command = "C:\\Python\\python.exe -c \"\nimport json\nprint('hi')\n\"";
    let preview = preview_command(command, 70);
    assert!(
        preview.starts_with("C:\\Python\\python.exe -c \"import json"),
        "windows runner should fold in script, got: {preview}"
    );
}

#[test]
fn preview_command_plain_multiline_keeps_first_line() {
    // Ordinary multi-line shell still previews only the first line.
    assert_eq!(preview_command("ls -R\npwd", 70), "ls -R");
}

#[test]
fn preview_command_ignores_contractions_and_non_runners() {
    // `echo don't` has an apostrophe but no runner flag: never fold.
    assert_eq!(preview_command("echo don't\nnext-line", 70), "echo don't");
    // `grep -c` / `--code-review` are substring traps, not script runners.
    assert_eq!(preview_command("grep -c 'pat\nnext-line", 70), "grep -c 'pat");
    assert_eq!(preview_command("tool --code-review\nnext-line", 70), "tool --code-review");
    // Balanced runner script with a contraction inside stays single-line.
    assert_eq!(preview_command("python3 -c \"print('hi') # don't\"", 70), "python3 -c \"print('hi') # don't\"");
}

#[test]
fn preview_command_long_command_head_truncates_at_word_boundary() {
    // 30-char budget: the 29-char cut lands inside "epsilon", but the last
    // space (index 27) is past half the budget, so it is honored.
    let command = "echo alpha beta gamma delta epsilon zeta eta theta iota";
    assert_eq!(preview_command(command, 30), "echo alpha beta gamma delta…");
}

#[test]
fn preview_full_command_never_truncates() {
    // Transcript surfaces (expanded `• Ran`, live PTY headers) show the
    // command in full; only compact previews truncate. Exact screenshot
    // bytes (`||`, `\.backup`) round-trip unchanged.
    let command = "grep -rn \"@vinhnx/vtcode|npm install -g||npx @vinhnx\" docs | grep -v node_modules | grep -v package-lock | grep -v \"\\.backup\"";
    let full = preview_full_command(command);
    assert_eq!(full, command);
    assert!(!full.contains('…'));
}

#[test]
fn preview_command_keeps_half_budget_when_first_space_is_early() {
    // Cut lands inside "epsilon" with the only space at index 4, well under
    // half the 29-char budget: fall back to a hard cut instead of collapsing
    // the preview to "echo…".
    let command = "echo alpha-beta-gamma-delta-epsilon zeta";
    assert_eq!(preview_command(command, 30), "echo alpha-beta-gamma-delta-e…");
}

#[test]
fn preview_command_short_command_unchanged() {
    assert_eq!(preview_command("git status --short", 70), "git status --short");
    assert_eq!(preview_command("   ", 70), "");
    assert_eq!(preview_command("echo hi", 0), "");
}

#[test]
fn unified_exec_labels_route_followups_and_launches() {
    assert_eq!(unified_exec_action_label(&json!({"command": "ls"})), "Run command");
    assert_eq!(unified_exec_action_label(&json!({})), "Run command");
    assert_eq!(unified_exec_action_label(&json!({"code": "1"})), "Run code");
    assert_eq!(unified_exec_action_label(&json!({"session_id": "s", "action": "wait"})), "Wait for command session");
    assert_eq!(unified_exec_action_label(&json!({"session_id": "s", "action": "poll"})), "Read command session");
    assert_eq!(unified_exec_action_label(&json!({"session_id": "s", "action": "bogus"})), "Exec action");
    // Case-insensitive like the dispatcher.
    assert_eq!(unified_exec_action_label(&json!({"action": "WAIT"})), "Wait for command session");
}

#[test]
fn write_stdin_distinguishes_wait_send_and_poll() {
    assert_eq!(write_stdin_action_label(&json!({"session_id": "s", "action": "wait"})), "Wait for command session");
    assert_eq!(write_stdin_action_label(&json!({"session_id": "s", "chars": "y\n"})), "Send command input");
    assert_eq!(write_stdin_action_label(&json!({"session_id": "s"})), "Read command session");
    // Explicit write with empty/missing input polls instead of sending.
    assert_eq!(write_stdin_action_label(&json!({"action": "write", "chars": ""})), "Read command session");
    // Whitespace-only chars are still stdin bytes.
    assert_eq!(write_stdin_action_label(&json!({"chars": "   "})), "Send command input");
    assert!(!has_session_input(&json!({"chars": ""})));
    assert!(!has_session_input(&json!({})));
}

#[test]
fn harness_visibility_pins_every_kind() {
    use vtcode_exec_events::HarnessEventKind::*;
    // Routine telemetry stays out of the transcript …
    for kind in [
        ToolLatencyRecorded,
        PlanningStarted,
        PlanningCompleted,
        ContinuationStarted,
        ContinuationSkipped,
        EvaluationStarted,
        EvaluationPassed,
        RevisionStarted,
        VerificationStarted,
        VerificationPassed,
        SnapshotCreated,
        DelegatedAgentStatus,
        SessionMessageSent,
        SessionMessageReceived,
    ] {
        assert!(!harness_is_user_visible(&kind), "{kind:?} stays out of the transcript");
    }
    // … while anything the user can act on stays visible.
    for kind in [
        SessionMessageExpired,
        TurnBlocked,
        BlockedRecoveryStarted,
        BlockedRecoveryFinished,
        BlockedHandoffWritten,
        BlockedHandoffResolved,
        EvaluationFailed,
        EscalationTriggered,
        EscalationBypassed,
        VerificationFailed,
        ErrorRecovered,
        ToolRetryAttempted,
        SnapshotRestored,
        SessionToolLimitIncreased,
        ToolLoopLimitIncreased,
        BackgroundSubprocessCompleted,
    ] {
        assert!(harness_is_user_visible(&kind), "{kind:?} stays visible");
    }
}
