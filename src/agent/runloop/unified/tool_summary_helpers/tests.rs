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
