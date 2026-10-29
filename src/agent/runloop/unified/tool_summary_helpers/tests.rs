use super::*;
use serde_json::json;

#[test]
fn test_describe_shell_command_new_format() {
    let args = json!({
        "command": ["bash", "-lc", "ls -R"]
    });

    let result = describe_shell_command(&args);
    assert!(result.is_some());

    let (description, _used) = result.unwrap();
    assert_eq!(description, "bash -lc ls -R");
}

#[test]
fn test_describe_shell_command_bash_command_format() {
    let args = json!({
        "bash_command": "pwd"
    });

    let result = describe_shell_command(&args);
    assert!(result.is_some());

    let (description, _used) = result.unwrap();
    assert_eq!(description, "pwd");
}

#[test]
fn test_describe_shell_command_truncation() {
    let long_command = "a".repeat(100);
    let args = json!({
        "command": [long_command]
    });

    let result = describe_shell_command(&args);
    assert!(result.is_some());

    let (description, _used) = result.unwrap();
    assert!(description.contains("…"));
}

#[test]
fn test_describe_shell_command_string_format() {
    let args = json!({
        "command": "cargo check -p vtcode"
    });

    let result = describe_shell_command(&args);
    assert!(result.is_some());

    let (description, _used) = result.unwrap();
    assert_eq!(description, "cargo check -p vtcode");
}

#[test]
fn test_describe_shell_command_raw_command_fallback() {
    let args = json!({
        "raw_command": "cargo test -- --nocapture"
    });

    let result = describe_shell_command(&args);
    assert!(result.is_some());

    let (description, _used) = result.unwrap();
    assert_eq!(description, "cargo test -- --nocapture");
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
    let mut keys = HashSet::new();
    keys.insert("pattern".to_string());
    let details = collect_param_details(&args, &keys, None);
    // Only detail_level, max_results, and scope should remain;
    // pattern is in keys (highlighted), noise params (including action) are skipped.
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
    let mut keys = HashSet::new();
    keys.insert("path".to_string());
    let details = collect_param_details(&args, &keys, None);
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
fn command_line_for_args_avoids_mid_string_ellipsis() {
    // Long chained commands must not cut a path in half: `preview_command`
    // head-truncates at a word boundary with a trailing ellipsis, while the
    // old `truncate_middle` produced `crates/…onfig/…`-style artifacts in
    // `• Ran` summaries (screenshot 2026-09-24 11.40.47).
    let long = "cargo nextest run -p vtcode --bin vtcode && cargo fmt --all -- --check && \
                git status --short && git diff --stat && git diff --cached --stat && \
                ./scripts/check-dev.sh --workspace && echo done";
    assert!(long.chars().count() > 120, "fixture must actually overflow the cap");
    let args = json!({ "cmd": long });
    let command = command_line_for_args(&args).expect("command line");

    assert!(command.ends_with('\u{2026}'), "tail ellipsis expected, got: {command:?}");
    // The head survives intact: no path is cut in half by a middle cut.
    assert!(command.starts_with("cargo nextest run -p vtcode"), "got: {command:?}");
    // A middle cut would splice the middle of a later token into the preview.
    let body = command.trim_end_matches('\u{2026}');
    assert!(!body.contains('\u{2026}'), "mid-string ellipsis leaked: {command:?}");
    // Exactly one ellipsis, terminating the command.
    assert_eq!(command.matches('\u{2026}').count(), 1, "got: {command:?}");
}

#[test]
fn command_line_for_args_keeps_short_commands_unchanged() {
    let args = json!({ "cmd": "git status --short" });
    assert_eq!(command_line_for_args(&args).as_deref(), Some("git status --short"));
}

#[test]
fn truncate_path_middle_breaks_at_separator() {
    let path = "/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/hello/src/main.rs";
    let truncated = truncate_path_middle(path, 40);
    // Should break at a '/' not in the middle of a word
    assert!(truncated.contains("…"));
    // The character after '…' should be a '/' or start of a path component
    if let Some(char_idx) = truncated.char_indices().find(|(_, c)| *c == '…') {
        let after: String = truncated[char_idx.0 + '…'.len_utf8()..].chars().collect();
        assert!(
            after.starts_with('/') || after.starts_with('h') || after.starts_with('s'),
            "Expected path break after ellipsis, got: {after}"
        );
    }
}

#[test]
fn truncate_path_middle_short_path_not_truncated() {
    let path = "src/main.rs";
    let truncated = truncate_path_middle(path, 40);
    assert_eq!(truncated, "src/main.rs");
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
fn display_command_text_leaves_operators_unquoted() {
    let args = json!({
        "command": ["cat", "docs/guides/agent-loop-contract.md", "2>/dev/null", "|", "head", "-120", ";", "echo", "---"]
    });
    let display = display_command_text(&args).expect("command display text");
    assert_eq!(display, "cat docs/guides/agent-loop-contract.md 2>/dev/null | head -120 ; echo ---");
}

#[test]
fn display_command_text_quotes_only_whitespace_words() {
    let args = json!({ "command": ["echo", "hello world", "|", "tr", "a-z", "A-Z"] });
    let display = display_command_text(&args).expect("command display text");
    assert_eq!(display, "echo 'hello world' | tr a-z A-Z");
}

#[test]
fn describe_shell_command_no_mid_string_ellipsis() {
    let args = json!({
        "command": "python3 -c \"\nimport json\nwith open('.vtcode/checkpoints/turn_1032.json') as f: d = json.load(f)\""
    });
    let (summary, used) = describe_shell_command(&args).expect("shell command summary");
    assert_eq!(used.iter().collect::<Vec<_>>(), ["command"]);
    assert!(!summary.contains("tur…ool"), "mid-string ellipsis leaked: {summary}");
    assert!(summary.starts_with("python3"));
    assert!(summary.contains("import json"), "script content folded in: {summary}");
    assert_ne!(summary, "python3 -c \"");
}

#[test]
fn display_command_text_collapses_script_newlines_without_nesting() {
    // `python3 -c "a\nb"` splits into a script word with newlines and `'`.
    // The header must stay single-line and prefer `"` outer quotes so
    // `open('…')` does not render as nested `'...open('...')...'`.
    let args = json!({
        "command": "python3 -c \"import json\nwith open('.vtcode/x.json') as f: pass\""
    });
    let display = display_command_text(&args).expect("command display text");
    assert!(!display.contains('\n'), "newlines leaked: {display:?}");
    assert!(display.starts_with("python3 -c "), "got: {display}");
    assert!(display.contains("import json"), "got: {display}");
    assert!(!display.contains("'import json"), "nested single quotes: {display}");
}

#[test]
fn display_command_text_falls_back_to_bash_command() {
    let args = json!({ "bash_command": "cargo check -p vtcode" });
    assert_eq!(display_command_text(&args).as_deref(), Some("cargo check -p vtcode"));
    // Both present: fallback follows `extract_command` order
    // (`raw_command` before `bash_command`) so display and describe agree.
    let args = json!({ "raw_command": "cargo check -p vtcode", "bash_command": "cargo test" });
    assert_eq!(display_command_text(&args).as_deref(), Some("cargo check -p vtcode"));
}

#[test]
fn display_command_text_skips_blank_words_and_keeps_args() {
    // Whitespace-only array elements collapse away instead of leaving a
    // trailing space (`echo '  '` -> `echo`).
    let args = json!({ "command": ["echo", "  "] });
    assert_eq!(display_command_text(&args).as_deref(), Some("echo"));
    // Unbalanced `command` still yields a header via the raw fallback and
    // preserves `args[]` extras like the primary path does.
    let args = json!({ "command": "echo 'unclosed", "args": ["extra"] });
    assert_eq!(display_command_text(&args).as_deref(), Some("echo 'unclosed extra"));
}

#[test]
fn describe_code_search_marks_query_used() {
    let args = json!({ "query": "agent loop implementation", "max_results": 15 });
    let (summary, used) = describe_code_search(&args).expect("code search summary");
    assert_eq!(summary, "Search code for agent loop implementation");
    assert!(used.contains("query"));
    assert!(!used.contains("max_results"));
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
        assert!(used.contains(key), "used should record the matched key, got {used:?}");
    }
}

#[test]
fn mcp_tool_display_name_handles_all_spellings() {
    assert_eq!(mcp_tool_display_name("mcp__server__tool"), Some("tool"));
    assert_eq!(mcp_tool_display_name("mcp__tool"), Some("tool"));
    assert_eq!(mcp_tool_display_name("mcp::provider::tool"), Some("tool"));
    assert_eq!(mcp_tool_display_name("mcp_fetch"), Some("fetch"));
    assert_eq!(mcp_tool_display_name("fetch"), None);
}
