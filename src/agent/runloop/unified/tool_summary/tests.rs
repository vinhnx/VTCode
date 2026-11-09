use serde_json::json;
use vtcode_commons::formatting::wrap_text_words;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::{InlineCommand, InlineHandle};

use super::{
    ToolSummaryRenderContext, build_tool_summary, describe_tool_action, prepare_summary_data, render_tool_call_summary,
    run_summary_is_placeholder,
};
use vtcode_core::tools::registry::labels::tool_action_label;

#[test]
fn build_tool_summary_formats_run_command_as_ran() {
    assert_eq!(build_tool_summary("Run command", "cargo check -p vtcode"), "Ran cargo check -p vtcode");
}

#[test]
fn build_tool_summary_collapses_use_prefix_with_word_order_drift() {
    assert_eq!(build_tool_summary("Search code", "Use Code search"), "Search code");
    assert_eq!(build_tool_summary("Read file", "Use read file"), "Read file");
    assert_eq!(
        build_tool_summary("Search code", "Use Code search workspace"),
        "Search code Use Code search workspace"
    );
}

#[test]
fn build_tool_summary_prefers_headline_over_generic_use_action() {
    // A generic "Use X" action label must never prefix a canonical
    // headline: prefer "Search code for …" over
    // "Use Code search Search code for …".
    assert_eq!(
        build_tool_summary("Use Code search", "Search code for core agent loop"),
        "Search code for core agent loop"
    );
    assert_eq!(build_tool_summary("Use Code search", "Search code"), "Search code");
}

#[test]
fn describe_tool_action_code_search_describes_query() {
    let (description, used_keys) = describe_tool_action(
        tool_names::CODE_SEARCH,
        &json!({
            "query": "core agent loop",
            "file_types": ["rs"],
            "max_results": 15
        }),
        None,
    );

    assert_eq!(description, "Search code for core agent loop");
    assert!(used_keys.contains("query"));
    assert!(!used_keys.contains("max_results"));
}

#[test]
fn describe_tool_action_handles_unified_exec_run_command() {
    let (description, used_keys) = describe_tool_action(
        tool_names::UNIFIED_EXEC,
        &json!({
            "action": "run",
            "command": "cargo check -p vtcode"
        }),
        None,
    );

    assert_eq!(description, "cargo check -p vtcode");
    assert!(used_keys.contains("command"));
}

#[test]
fn prepare_summary_data_folds_exec_session_params_into_one_row() {
    let args = json!({
        "session_id": "run-2d5752f2",
        "chars": "y\n",
        "wait_timeout_seconds": 600,
        "max_output_tokens": 4000,
        "max_tokens": 4000
    });
    let data = prepare_summary_data(tool_names::UNIFIED_EXEC, &args, None, Some("output"));

    // The four plumbing rows from the screenshot collapse to one row that
    // carries only the session identity and the wait deadline.
    assert_eq!(data.summary, "Send command input");
    assert_eq!(data.details, vec!["Session run-2d5752f2 · wait up to 600s".to_string()]);
    // The generic capture label beside a session row reads as a stutter.
    assert_eq!(data.stream_label, None);
}

#[test]
fn prepare_summary_data_uses_public_write_stdin_name() {
    // The legacy `write_stdin` name must collapse to the action label rather
    // than leaving a `Use write_stdin` tail after `Send command input`, and
    // its plumbing must not re-leak through the generic detail collector.
    let args = json!({
        "session_id": "run-abc",
        "chars": "y\n",
        "yield_time_ms": 1000,
        "wait_timeout_seconds": 600,
        "max_output_tokens": 4000,
        "max_tokens": 4000
    });
    let data = prepare_summary_data(tool_names::WRITE_STDIN, &args, None, Some("output"));

    assert_eq!(data.summary, "Send command input");
    assert_eq!(data.details, vec!["Session run-abc · wait up to 600s".to_string()]);
    assert_eq!(data.stream_label, None);
}

#[test]
fn prepare_summary_data_labels_pure_waits_as_waits() {
    // Screenshot 2026-10-02: two pure waits (no stdin payload) rendered as
    // `Send command input`. A wait with no input must read as a wait.
    for tool in [tool_names::WRITE_STDIN, tool_names::UNIFIED_EXEC] {
        let args = json!({
            "session_id": "run-6b54fe59",
            "action": "wait",
            "wait_timeout_seconds": 60
        });
        let data = prepare_summary_data(tool, &args, None, Some("output"));
        assert_eq!(data.summary, "Wait for command session", "tool: {tool}");
        assert_eq!(data.details, vec!["Session run-6b54fe59 · wait up to 60s".to_string()]);
    }
}

#[test]
fn prepare_summary_data_labels_session_poll_without_input_as_read() {
    // A session_id-only follow-up polls output; it must not claim to send.
    let args = json!({ "session_id": "run-abc" });
    let data = prepare_summary_data(tool_names::WRITE_STDIN, &args, None, Some("output"));
    assert_eq!(data.summary, "Read command session");
}

#[test]
fn prepare_summary_data_keeps_failure_label_on_session_calls() {
    // Dropping every session label would hide a failed read's signal, so
    // only the redundant capture label is suppressed.
    let args = json!({ "session_id": "run-abc", "action": "wait" });
    let data = prepare_summary_data(tool_names::UNIFIED_EXEC, &args, None, Some("error"));

    assert_eq!(data.stream_label.as_deref(), Some("error"));
}

#[test]
fn prepare_summary_data_keeps_stream_label_for_ordinary_tools() {
    let args = json!({ "path": "src/lib.rs", "start_line": 10, "offset": 0 });
    let data = prepare_summary_data(tool_names::READ_FILE, &args, None, Some("content"));

    assert_eq!(data.details, vec!["Start line: 10".to_string()]);
    assert_eq!(data.stream_label.as_deref(), Some("content"));
}

#[test]
fn prepare_summary_data_hides_output_caps_on_ordinary_tools() {
    // The cap is plumbing for every tool, not just session follow-ups. The
    // query is the search's content and shows; only the cap hides.
    let args = json!({ "query": "tool intent", "max_output_tokens": 200, "limit": 5 });
    let data = prepare_summary_data(tool_names::SEARCH_TOOLS, &args, None, None);
    assert_eq!(data.details, vec!["Query: tool intent".to_string(), "Limit: 5".to_string()]);
}

#[test]
fn describe_tool_action_labels_pty_session_readers() {
    let args = json!({ "session_id": "run-abc" });
    for (tool, label) in [
        (tool_names::SEND_PTY_INPUT, "Send command input"),
        (tool_names::READ_PTY_SESSION, "Read command session"),
    ] {
        let (headline, _) = describe_tool_action(tool, &args, None);
        assert_eq!(headline, label);
        assert_eq!(tool_action_label(tool, &args), label);
    }
}

#[test]
fn describe_tool_action_annotates_skill_doc_reads_with_skill_name() {
    let (description, used_keys) = describe_tool_action(
        tool_names::READ_FILE,
        &json!({
            "path": "/tmp/pr-babysitter/SKILL.md"
        }),
        None,
    );

    assert_eq!(description, "Read file /tmp/pr-babysitter/SKILL.md (pr-babysitter skill)");
    assert!(used_keys.contains("path"));
}

#[test]
fn describe_tool_action_annotates_unified_file_skill_doc_reads_with_skill_name() {
    let (description, used_keys) = describe_tool_action(
        tool_names::UNIFIED_FILE,
        &json!({
            "action": "read",
            "path": "skills/code-review-skill/SKILL.md"
        }),
        None,
    );

    assert_eq!(description, "Read file skills/code-review-skill/SKILL.md (code-review-skill skill)");
    assert!(used_keys.contains("path"));
}

#[test]
fn run_summary_placeholder_detection_catches_generic_labels() {
    assert!(run_summary_is_placeholder("Ran Use Unified exec"));
    assert!(run_summary_is_placeholder("Ran command"));
    assert!(run_summary_is_placeholder("Ran bash"));
    assert!(!run_summary_is_placeholder("Ran cargo check -p vtcode"));
}

#[test]
fn wrap_text_words_wraps_long_command_summary() {
    let text = "cargo test -p vtcode run_command_preview_ build_tool_summary_formats_run_command_as_ran";
    let wrapped = wrap_text_words(text, 62, 58);
    assert_eq!(wrapped.len(), 2);
    assert_eq!(wrapped[0], "cargo test -p vtcode run_command_preview_");
    assert_eq!(wrapped[1], "build_tool_summary_formats_run_command_as_ran");
}

#[test]
fn extract_first_patch_file_path_from_update() {
    let args = json!({
        "input": "*** Begin Patch\n*** Update File: src/main.rs\n@@ -1,3 +1,4 @@\n+use std::io;\n*** End Patch"
    });
    assert_eq!(super::extract_first_patch_file_path(&args), Some("src/main.rs".to_string()));
}

#[test]
fn extract_first_patch_file_path_from_add() {
    let args = json!({
        "patch": "*** Begin Patch\n*** Add File: new_file.txt\n+Hello\n*** End Patch"
    });
    assert_eq!(super::extract_first_patch_file_path(&args), Some("new_file.txt".to_string()));
}

#[test]
fn extract_first_patch_file_path_from_delete() {
    let args = json!({
        "input": "*** Begin Patch\n*** Delete File: old.txt\n*** End Patch"
    });
    assert_eq!(super::extract_first_patch_file_path(&args), Some("old.txt".to_string()));
}

#[test]
fn extract_first_patch_file_path_returns_first_of_multiple() {
    let args = json!({
        "input": "*** Begin Patch\n*** Update File: a.rs\n+line\n*** Update File: b.rs\n+line\n*** End Patch"
    });
    assert_eq!(super::extract_first_patch_file_path(&args), Some("a.rs".to_string()));
}

#[test]
fn extract_first_patch_file_path_returns_none_for_missing() {
    let args = json!({"input": "not a valid patch"});
    assert_eq!(super::extract_first_patch_file_path(&args), None);
}

#[test]
fn describe_tool_action_apply_patch_shows_file_path() {
    let (description, used_keys) = describe_tool_action(
        tool_names::APPLY_PATCH,
        &json!({
            "input": "*** Begin Patch\n*** Update File: src/lib.rs\n+line\n*** End Patch"
        }),
        None,
    );
    assert_eq!(description, "Apply patch to src/lib.rs");
    assert!(used_keys.contains("path"));
}

#[test]
fn describe_tool_action_apply_patch_falls_back_without_path() {
    let (description, used_keys) =
        describe_tool_action(tool_names::APPLY_PATCH, &json!({"input": "not a patch"}), None);
    assert_eq!(description, "Apply workspace patch");
    assert!(used_keys.is_empty());
}

#[test]
fn try_bash_grammar_ansi_highlights_cargo_command() {
    let result =
        super::try_bash_grammar_ansi("cargo check --locked -p vtcode", anstyle::Color::Ansi(anstyle::AnsiColor::White));
    assert!(result.is_some(), "cargo command should get bash grammar highlighting");
    let ansi = result.unwrap();
    // ANSI escape sequences present (ESC = 0x1b)
    assert!(ansi.contains('\u{1b}'), "output should contain ANSI escape codes");
    // Plain text is preserved when ANSI codes are stripped
    let stripped = vtcode_commons::ansi::strip_ansi_codes(&ansi);
    assert_eq!(stripped, "cargo check --locked -p vtcode");
}

#[test]
fn try_bash_grammar_ansi_highlights_sed_command() {
    let result = super::try_bash_grammar_ansi(
        "sed -n 178,190p src/main_helpers/bootstrap.rs",
        anstyle::Color::Ansi(anstyle::AnsiColor::White),
    );
    assert!(result.is_some(), "sed command should get bash grammar highlighting");
    let ansi = result.unwrap();
    assert!(ansi.contains('\u{1b}'));
    let stripped = vtcode_commons::ansi::strip_ansi_codes(&ansi);
    assert_eq!(stripped, "sed -n 178,190p src/main_helpers/bootstrap.rs");
}

#[test]
fn try_bash_grammar_ansi_highlights_rg_command() {
    let result =
        super::try_bash_grammar_ansi("rg -n use std::time::Instant", anstyle::Color::Ansi(anstyle::AnsiColor::White));
    assert!(result.is_some(), "rg command should get bash grammar highlighting");
    let ansi = result.unwrap();
    assert!(ansi.contains('\u{1b}'));
    let stripped = vtcode_commons::ansi::strip_ansi_codes(&ansi);
    assert_eq!(stripped, "rg -n use std::time::Instant");
}

#[test]
fn render_command_segment_falls_back_for_empty() {
    let result = super::render_command_segment(
        "",
        anstyle::Color::Ansi(anstyle::AnsiColor::Green),
        anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlack),
        true,
    );
    assert_eq!(result, "");
}

#[test]
fn render_command_segment_produces_highlighted_output() {
    // Force color output on. The color policy uses a Lazy initializer that
    // reads NO_COLOR from the env; we must trigger it first, then override.
    // (nextest runs each test in its own process, so this is safe.)
    let _ = vtcode_commons::color_policy::current_color_output_policy();
    vtcode_commons::color_policy::set_color_output_policy(vtcode_commons::color_policy::ColorOutputPolicy {
        enabled: true,
        source: vtcode_commons::color_policy::ColorOutputPolicySource::CliColorAlways,
    });

    let result = super::render_command_segment(
        "cargo check --locked -p vtcode",
        anstyle::Color::Ansi(anstyle::AnsiColor::Green),
        anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlack),
        true,
    );
    // Should contain ANSI escape codes from bash grammar highlighting
    assert!(result.contains('\u{1b}'), "command segment should be syntax-highlighted");
    let stripped = vtcode_commons::ansi::strip_ansi_codes(&result);
    assert_eq!(stripped.as_ref(), "cargo check --locked -p vtcode");
}

#[test]
fn inline_tool_summary_keeps_command_tokens_independently_styled() {
    let _ = vtcode_commons::color_policy::current_color_output_policy();
    vtcode_commons::color_policy::set_color_output_policy(vtcode_commons::color_policy::ColorOutputPolicy {
        enabled: true,
        source: vtcode_commons::color_policy::ColorOutputPolicySource::CliColorAlways,
    });

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    renderer.set_tool_display_mode(vtcode_core::config::ToolDisplayMode::Expanded);
    let context = ToolSummaryRenderContext { workspace_root: None };

    render_tool_call_summary(
        &mut renderer,
        tool_names::UNIFIED_EXEC,
        &json!({"action": "run", "command": "find src/agent/runloop -maxdepth 3 -type f -name *.rs | sort"}),
        None,
        &context,
        anstyle::Color::Ansi(anstyle::AnsiColor::Green),
    )
    .expect("summary rendering should succeed");

    let commands: Vec<_> = std::iter::from_fn(|| receiver.try_recv().ok()).collect();
    let summary_segments = commands
        .iter()
        .find_map(|command| match command {
            InlineCommand::AppendLine { segments, .. }
                if segments
                    .iter()
                    .map(|segment| segment.text.as_str())
                    .collect::<String>()
                    .contains("find") =>
            {
                Some(segments.as_slice())
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            let texts: Vec<_> = commands
                .iter()
                .filter_map(|command| match command {
                    InlineCommand::AppendLine { segments, .. } => {
                        Some(segments.iter().map(|segment| segment.text.as_str()).collect::<String>())
                    }
                    _ => None,
                })
                .collect();
            panic!("expected inline command summary, got {texts:?}");
        });
    let command_color = summary_segments
        .iter()
        .find(|segment| segment.text.contains("find"))
        .and_then(|segment| segment.style.color);
    let option_color = summary_segments
        .iter()
        .find(|segment| segment.text.contains("maxdepth"))
        .and_then(|segment| segment.style.color);

    assert_ne!(command_color, option_color);
}

#[test]
fn compact_tool_summary_keeps_each_call_visible() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    renderer.set_tool_display_mode(vtcode_core::config::ToolDisplayMode::Compact);
    let context = ToolSummaryRenderContext { workspace_root: None };

    for max_results in [30, 100] {
        render_tool_call_summary(
            &mut renderer,
            tool_names::LIST_FILES,
            &json!({"path": ".", "max_results": max_results}),
            None,
            &context,
            anstyle::Color::Ansi(anstyle::AnsiColor::Green),
        )
        .expect("summary should render");
    }

    let text = std::iter::from_fn(|| receiver.try_recv().ok())
        .filter_map(|command| match command {
            InlineCommand::AppendLine { segments, .. } => {
                Some(segments.into_iter().map(|s| s.text).collect::<String>())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(text.matches("• List files").count(), 2);
    assert!(text.contains("30"));
    assert!(text.contains("100"));
}

#[test]
fn compact_command_summary_groups_contiguous_commands() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    renderer.set_tool_display_mode(vtcode_core::config::ToolDisplayMode::Compact);
    let context = ToolSummaryRenderContext { workspace_root: None };

    for command in ["printf one", "printf two"] {
        render_tool_call_summary(
            &mut renderer,
            tool_names::UNIFIED_EXEC,
            &json!({"action": "run", "command": command}),
            None,
            &context,
            anstyle::Color::Ansi(anstyle::AnsiColor::Green),
        )
        .expect("summary should render");
    }

    let activities = std::iter::from_fn(|| receiver.try_recv().ok())
        .filter_map(|command| match command {
            InlineCommand::AppendCompactActivity(activity) | InlineCommand::ReplaceCompactActivity(activity) => {
                Some(activity)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(activities.len(), 2);
    assert_eq!(activities[0].command_count, 1);
    assert_eq!(activities[0].display_text(), "• Ran printf one");
    assert_eq!(activities[1].command_count, 2);
    assert_eq!(activities[1].display_text(), "• Ran 2 commands");
}

#[test]
fn compact_command_summary_groups_command_tool_aliases() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    renderer.set_tool_display_mode(vtcode_core::config::ToolDisplayMode::Compact);
    let context = ToolSummaryRenderContext { workspace_root: None };

    for (tool_name, command) in [("exec", "printf one"), ("container.exec", "printf two")] {
        render_tool_call_summary(
            &mut renderer,
            tool_name,
            &json!({"command": command}),
            None,
            &context,
            anstyle::Color::Ansi(anstyle::AnsiColor::Green),
        )
        .expect("summary should render");
    }

    let activities = std::iter::from_fn(|| receiver.try_recv().ok())
        .filter_map(|command| match command {
            InlineCommand::AppendCompactActivity(activity) | InlineCommand::ReplaceCompactActivity(activity) => {
                Some(activity)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(activities.len(), 2);
    assert_eq!(activities[1].command_count, 2);
    assert_eq!(activities[1].display_text(), "• Ran 2 commands");
}

#[test]
fn compact_command_summary_flushes_before_non_command_results() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    renderer.set_tool_display_mode(vtcode_core::config::ToolDisplayMode::Compact);
    let context = ToolSummaryRenderContext { workspace_root: None };

    for (tool_name, args) in [
        (tool_names::UNIFIED_EXEC, json!({"action": "run", "command": "printf one"})),
        (tool_names::LIST_FILES, json!({"path": "."})),
        (tool_names::UNIFIED_EXEC, json!({"action": "run", "command": "printf two"})),
    ] {
        render_tool_call_summary(
            &mut renderer,
            tool_name,
            &args,
            None,
            &context,
            anstyle::Color::Ansi(anstyle::AnsiColor::Green),
        )
        .expect("summary should render");
    }

    let activities = std::iter::from_fn(|| receiver.try_recv().ok())
        .filter_map(|command| match command {
            InlineCommand::AppendCompactActivity(activity) | InlineCommand::ReplaceCompactActivity(activity) => {
                Some(activity)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(activities.len(), 2);
    assert!(activities.iter().all(|activity| activity.command_count == 1));
}

#[test]
fn planning_indicators_carry_spinner_marker() {
    // The TUI transcript spinner matches indicator rows by their `❋`
    // text prefix (`transcript.rs::FILE_OPERATION_INDICATORS`). If an
    // indicator loses the marker, the planning row goes static while the
    // long synthesis runs — the exact stall this feature fixes.
    for indicator in [
        super::PLANNING_RESEARCHING_INDICATOR,
        super::PLANNING_VALIDATING_INDICATOR,
        super::PLANNING_PERSISTING_INDICATOR,
    ] {
        assert!(indicator.contains('❋'), "missing spinner marker in {indicator:?}");
        assert!(indicator.to_ascii_lowercase().contains("plan"), "indicator must name the plan phase: {indicator:?}");
    }
}

#[test]
fn render_planning_progress_indicator_emits_transcript_line() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());

    super::render_planning_progress_indicator(&mut renderer, super::PLANNING_RESEARCHING_INDICATOR)
        .expect("planning indicator should render");

    let text = std::iter::from_fn(|| receiver.try_recv().ok())
        .filter_map(|command| match command {
            InlineCommand::AppendLine { segments, .. } => {
                Some(segments.into_iter().map(|s| s.text).collect::<String>())
            }
            InlineCommand::Inline { segment, .. } => Some(segment.text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Drafting plan"), "got: {text:?}");
}

#[test]
fn prepare_summary_data_keeps_run_headline_in_full_without_truncation() {
    // Screenshot 2026-09-24: the expanded `• Ran` headline must carry the
    // complete pipeline so TUI wrapping (not `…`) owns the overflow.
    // `display_command_text` normalizes quoting (double to single), so
    // assert token completeness rather than byte equality. Exact
    // screenshot bytes: `||` in the pattern (3 pattern pipes + 3 shell
    // pipes = 6) and the backslash-escaped `\.backup` must survive.
    let command = "grep -rn \"@vinhnx/vtcode|npm install -g||npx @vinhnx\" docs | grep -v node_modules | grep -v package-lock | grep -v \"\\.backup\"";
    let data =
        prepare_summary_data(tool_names::UNIFIED_EXEC, &json!({"action": "run", "command": command}), None, None);
    assert!(!data.summary.contains('…'), "got: {:?}", data.summary);
    for fragment in [
        "grep",
        "-rn",
        "@vinhnx/vtcode|npm install -g||npx @vinhnx",
        "docs",
        "node_modules",
        "package-lock",
        "\\.backup",
    ] {
        assert!(data.summary.contains(fragment), "missing {fragment:?} in {:?}", data.summary);
    }
    assert_eq!(data.summary.matches('|').count(), 6, "pattern pipes + shell pipes must survive: {:?}", data.summary);
}
