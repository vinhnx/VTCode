use super::{
    WriteStdinDispatch, command_session_missing_required_args, command_session_requires_command_safety, command_text,
    command_words, command_words_after_environment_prefix, contains_dynamic_shell_syntax,
    environment_prefix_has_injection_keys, extract_command_text_with_key, has_indexed_command_parts,
    has_unsafe_readonly_options, interactive_input_text, normalize_indexed_command_args, normalize_shell_args,
    normalized_command_value, parse_indexed_command_parts, raw_command_text, scan_environment_prefix, session_id_text,
    session_id_text_from_payload, working_dir_text, working_dir_text_from_payload, write_stdin_dispatch,
};
use serde_json::{Value, json};

#[test]
fn detects_indexed_command_keys() {
    assert!(has_indexed_command_parts(&json!({"command.0": "ls"})));
    assert!(has_indexed_command_parts(&json!({"command.1": "ls"})));
    assert!(!has_indexed_command_parts(&json!({"command.2": "ls"})));
}

#[test]
fn parses_zero_based_indexed_command_parts() {
    let parts = parse_indexed_command_parts(
        json!({
            "command.0": "ls",
            "command.1": "-a"
        })
        .as_object()
        .expect("object"),
    )
    .expect("valid indexed args");

    assert_eq!(parts, Some(vec!["ls".to_string(), "-a".to_string()]));
}

#[test]
fn parses_one_based_indexed_command_parts() {
    let parts = parse_indexed_command_parts(
        json!({
            "command.1": "ls",
            "command.2": "-a"
        })
        .as_object()
        .expect("object"),
    )
    .expect("valid indexed args");

    assert_eq!(parts, Some(vec!["ls".to_string(), "-a".to_string()]));
}

#[test]
fn rejects_non_string_indexed_command_parts() {
    let error = parse_indexed_command_parts(
        json!({
            "command.0": 42
        })
        .as_object()
        .expect("object"),
    )
    .expect_err("non-string segment should fail");

    assert_eq!(error, "command array must contain only strings");
}

#[test]
fn normalizes_indexed_command_args_into_command_string() {
    let normalized = normalize_indexed_command_args(&json!({
        "command.1": "ls",
        "command.2": "-a",
        "working_dir": "."
    }))
    .expect("valid indexed args")
    .expect("normalized payload");

    assert_eq!(normalized.get("command").and_then(Value::as_str), Some("ls -a"));
    assert_eq!(normalized.get("working_dir").and_then(Value::as_str), Some("."));
}

#[test]
fn normalized_command_value_prefers_cmd_aliases() {
    let normalized = normalized_command_value(&json!({"cmd": "ls -a"}))
        .expect("valid command alias")
        .expect("command value");

    assert_eq!(normalized.as_str(), Some("ls -a"));
}

#[test]
fn command_text_joins_command_arrays() {
    let command = command_text(&json!({"command": ["git", "status", "--short"]}))
        .expect("valid command")
        .expect("command text");

    assert_eq!(command, "git status --short");
}

#[test]
fn command_words_append_extra_args() {
    let words = command_words(&json!({
        "command": "cargo test",
        "args": ["-p", "vtcode-core"]
    }))
    .expect("valid command")
    .expect("command words");

    assert_eq!(words, vec!["cargo", "test", "-p", "vtcode-core"]);
}

#[test]
fn interactive_input_text_preserves_whitespace() {
    assert_eq!(interactive_input_text(&json!({"chars": "  echo hi\n"})), Some("  echo hi\n"));
}

#[test]
fn write_stdin_dispatch_distinguishes_write_from_poll() {
    assert_eq!(write_stdin_dispatch(&json!({"chars": ""})), Ok(WriteStdinDispatch::Poll));
    assert_eq!(write_stdin_dispatch(&json!({"chars": "  status\n"})), Ok(WriteStdinDispatch::Write));
}

#[test]
fn write_stdin_dispatch_accepts_explicit_wait_without_chars() {
    assert_eq!(
        write_stdin_dispatch(&json!({"action": "wait", "session_id": "run-1"})),
        Ok(WriteStdinDispatch::Wait)
    );
}

#[test]
fn write_stdin_dispatch_requires_public_chars() {
    assert_eq!(write_stdin_dispatch(&json!({"input": "status\n"})), Err("write_stdin requires string chars"));
    assert_eq!(write_stdin_dispatch(&json!({"chars": 1})), Err("write_stdin requires string chars"));
}

#[test]
fn write_stdin_dispatch_validates_control_actions() {
    for (action, expected) in [
        ("inspect", WriteStdinDispatch::Inspect),
        ("terminate", WriteStdinDispatch::Terminate),
        ("close", WriteStdinDispatch::Close),
        ("poll", WriteStdinDispatch::Poll),
    ] {
        assert_eq!(write_stdin_dispatch(&json!({"action": action})), Ok(expected));
    }
    assert!(write_stdin_dispatch(&json!({"action": "typo", "chars": "echo unsafe\n"})).is_err());
    assert!(write_stdin_dispatch(&json!({"action": "poll", "chars": "echo unsafe\n"})).is_err());
    assert!(write_stdin_dispatch(&json!({"action": 1, "chars": ""})).is_err());
}

#[test]
fn write_stdin_dispatch_matches_actions_case_insensitively() {
    // Asymmetric: mixed-case spellings resolve like lowercase; non-ASCII
    // lookalikes stay rejected (ASCII-only folding, as before).
    for (action, expected) in [
        ("WAIT", WriteStdinDispatch::Wait),
        ("WaIt", WriteStdinDispatch::Wait),
        ("INSPECT", WriteStdinDispatch::Inspect),
        ("Terminate", WriteStdinDispatch::Terminate),
        ("CLOSE", WriteStdinDispatch::Close),
        ("Poll", WriteStdinDispatch::Poll),
    ] {
        assert_eq!(
            write_stdin_dispatch(&json!({"action": action})),
            Ok(expected),
            "{action:?} must resolve like its lowercase form"
        );
    }
    assert_eq!(
        write_stdin_dispatch(&json!({"action": "WRITE", "chars": "status\n"})),
        Ok(WriteStdinDispatch::Write)
    );
    assert!(write_stdin_dispatch(&json!({"action": "wäit"})).is_err());
    assert!(write_stdin_dispatch(&json!({"action": "WAIT "})).is_err());
}

#[test]
fn session_id_text_trims_whitespace() {
    assert_eq!(session_id_text(&json!({"session_id": " run-1 "})), Some("run-1"));
}

#[test]
fn session_id_text_accepts_compact_alias() {
    assert_eq!(session_id_text(&json!({"s": " run-1 "})), Some("run-1"));
}

#[test]
fn session_id_text_from_payload_accepts_aliases() {
    let value = json!({"s": " run-1 "});
    let payload = value.as_object().expect("object");
    assert_eq!(session_id_text_from_payload(payload), Some("run-1"));
}

#[test]
fn working_dir_text_accepts_aliases() {
    assert_eq!(working_dir_text(&json!({"workdir": " src "})), Some("src"));
    assert_eq!(working_dir_text(&json!({"cwd": "."})), Some("."));
}

#[test]
fn working_dir_text_from_payload_accepts_aliases() {
    let value = json!({"workdir": " src "});
    let payload = value.as_object().expect("object");
    assert_eq!(working_dir_text_from_payload(payload), Some("src"));
}

#[test]
fn normalize_shell_args_maps_codex_fields() {
    let normalized = normalize_shell_args(&json!({
        "cmd": "echo hi",
        "chars": "status\n"
    }))
    .expect("valid shell args");

    assert_eq!(normalized.get("command").and_then(Value::as_str), Some("echo hi"));
    assert_eq!(normalized.get("input").and_then(Value::as_str), Some("status\n"));
}

#[test]
fn normalize_shell_args_maps_compact_session_id() {
    let normalized = normalize_shell_args(&json!({
        "s": "run-1"
    }))
    .expect("valid shell args");

    assert_eq!(normalized.get("session_id").and_then(Value::as_str), Some("run-1"));
}

#[test]
fn normalize_shell_args_copies_max_output_tokens_to_max_tokens() {
    let normalized = normalize_shell_args(&json!({
        "command": "echo hi",
        "max_output_tokens": 42
    }))
    .expect("valid shell args");

    assert_eq!(normalized.get("max_output_tokens").and_then(Value::as_u64), Some(42));
    assert_eq!(normalized.get("max_tokens").and_then(Value::as_u64), Some(42));
}

#[test]
fn normalize_shell_args_copies_max_tokens_to_max_output_tokens() {
    let normalized = normalize_shell_args(&json!({
        "command": "echo hi",
        "max_tokens": 42
    }))
    .expect("valid shell args");

    assert_eq!(normalized.get("max_tokens").and_then(Value::as_u64), Some(42));
    assert_eq!(normalized.get("max_output_tokens").and_then(Value::as_u64), Some(42));
}

#[test]
fn command_session_missing_required_args_is_action_aware() {
    assert_eq!(command_session_missing_required_args(&json!({"action": "run"})), vec!["command"]);
    assert_eq!(
        command_session_missing_required_args(&json!({"action": "write", "session_id": "run-1"})),
        vec!["input or chars or text"]
    );
    assert_eq!(
        command_session_missing_required_args(&json!({"action": "inspect"})),
        vec!["session_id or spool_path"]
    );
    assert!(command_session_missing_required_args(&json!({"action": "list"})).is_empty());
}

#[test]
fn command_session_requires_command_safety_only_for_run() {
    assert!(command_session_requires_command_safety(&json!({
        "action": "run",
        "command": "cargo check"
    })));
    assert!(!command_session_requires_command_safety(&json!({
        "action": "poll",
        "session_id": "run-1"
    })));
}

#[test]
fn raw_command_text_extracts_command_string() {
    assert_eq!(raw_command_text(&json!({"command": "rg foo"})), Some("rg foo".to_string()));
    assert_eq!(raw_command_text(&json!({"cmd": "ls -la"})), Some("ls -la".to_string()));
    assert_eq!(
        raw_command_text(&json!({"command.0": "cat", "command.1": "file.txt"})),
        Some("cat file.txt".to_string())
    );
    assert_eq!(raw_command_text(&json!({"command": ["wc", "-l"]})), Some("wc -l".to_string()));
    assert!(raw_command_text(&json!({})).is_none());
}

#[test]
fn display_extraction_reports_key_and_preserves_precedence() {
    assert_eq!(
        extract_command_text_with_key(&json!({"command": ["bash", "-lc", "ls -R"]})),
        Some(("bash -lc ls -R".to_string(), "command"))
    );
    assert_eq!(
        extract_command_text_with_key(&json!({"bash_command": "pwd"})),
        Some(("pwd".to_string(), "bash_command"))
    );
    assert_eq!(
        extract_command_text_with_key(&json!({"raw_command": "cargo test"})),
        Some(("cargo test".to_string(), "raw_command"))
    );
    // Missing `command` must not block later keys (the copilot
    // early-return bug this canonical helper fixes).
    assert_eq!(extract_command_text_with_key(&json!({"cmd": "ls -la"})), Some(("ls -la".to_string(), "cmd")));
    // `command` trims; other keys keep raw emptiness semantics.
    assert_eq!(extract_command_text_with_key(&json!({"command": "  "})), None);
    // First non-empty key in canonical order wins.
    assert_eq!(
        extract_command_text_with_key(&json!({"raw_command": "cargo check -p vtcode", "bash_command": "cargo test"})),
        Some(("cargo check -p vtcode".to_string(), "raw_command"))
    );
    assert!(extract_command_text_with_key(&json!({})).is_none());
}

#[test]
fn env_prefix_injection_keys_are_detected() {
    let words = |command: &str| -> Vec<String> { shell_words::split(command).expect("split command") };

    // The A4 vector: config injection via env values reopens the blocked
    // `git -c` path without any `-c` token.
    assert!(environment_prefix_has_injection_keys(&words(
        "env GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.fsmonitor GIT_CONFIG_VALUE_0=touch git status"
    )));
    assert!(environment_prefix_has_injection_keys(&words(
        "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.fsmonitor GIT_CONFIG_VALUE_0=touch git status"
    )));
    assert!(environment_prefix_has_injection_keys(&words("env GIT_EXTERNAL_DIFF=evil git diff")));
    assert!(environment_prefix_has_injection_keys(&words("env GIT_TEXTCONV=evil git log")));
    assert!(environment_prefix_has_injection_keys(&words("env LD_PRELOAD=./evil.so ls")));
    assert!(environment_prefix_has_injection_keys(&words("env BASH_ENV=./evil.sh bash -lc 'echo hi'")));
    assert!(environment_prefix_has_injection_keys(&words("env NODE_OPTIONS=--require ./evil.js node --version")));
    // `env -S` re-parses at runtime; cannot audit.
    assert!(environment_prefix_has_injection_keys(&words("env -S 'GIT_CONFIG_COUNT=1 git status'")));

    // Benign env stays allowed.
    assert!(!environment_prefix_has_injection_keys(&words("env LANG=C.UTF-8 rg foo")));
    assert!(!environment_prefix_has_injection_keys(&words("FOO=bar git status")));
    assert!(!environment_prefix_has_injection_keys(&words("env FOO=bar git status")));
    // Unset is not an assignment.
    assert!(!environment_prefix_has_injection_keys(&words("env -u GIT_CONFIG_COUNT git status")));
    assert!(!environment_prefix_has_injection_keys(&words("git status")));
}

#[test]
fn env_prefix_single_scan_agrees_with_both_public_helpers() {
    let words = |command: &str| -> Vec<String> { shell_words::split(command).expect("split command") };
    for command in [
        "git status",
        "env LANG=C.UTF-8 rg foo",
        "FOO=bar LANG=C git status",
        "env FOO=bar git status",
        "env -- FOO=bar git status",
        "env -u GIT_CONFIG_COUNT git status",
        "env -C /tmp git status",
        "env GIT_EXTERNAL_DIFF=evil git diff",
        "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=k GIT_CONFIG_VALUE_0=v git status",
        "env -S 'GIT_CONFIG_COUNT=1 git status'",
        "env --split-string='x=1 ls'",
    ] {
        let parsed = words(command);
        let scan = scan_environment_prefix(&parsed);
        assert_eq!(command_words_after_environment_prefix(&parsed), &parsed[scan.stripped_start..], "{command}");
        assert_eq!(environment_prefix_has_injection_keys(&parsed), scan.has_injection, "{command}");
    }
}

#[test]
fn env_prefix_empty_key_stays_fail_closed() {
    let words = |command: &str| -> Vec<String> { shell_words::split(command).expect("split command") };
    // `=foo` is not a valid assignment: the executable stays `=foo`
    // (unknown, therefore mutating downstream), never silently stripped.
    for command in ["=foo rg foo", "env =foo rg foo"] {
        let parsed = words(command);
        let stripped = command_words_after_environment_prefix(&parsed);
        assert_eq!(stripped.first().map(String::as_str), Some("=foo"), "{command}");
        assert!(!environment_prefix_has_injection_keys(&parsed), "{command}");
    }
}

#[test]
fn env_prefix_split_string_reports_empty_remainder_and_unsafe() {
    let words = |command: &str| -> Vec<String> { shell_words::split(command).expect("split command") };
    let parsed = words("env -S 'GIT_CONFIG_COUNT=1 git status'");
    assert!(command_words_after_environment_prefix(&parsed).is_empty());
    assert!(environment_prefix_has_injection_keys(&parsed));
    assert!(has_unsafe_readonly_options(&parsed));
}

#[test]
fn has_unsafe_readonly_options_flags_injection_without_second_walk() {
    let words = |command: &str| -> Vec<String> { shell_words::split(command).expect("split command") };
    assert!(has_unsafe_readonly_options(&words("env LD_PRELOAD=./evil.so ls")));
    assert!(has_unsafe_readonly_options(&words("LD_PRELOAD=./evil.so ls")));
    assert!(!has_unsafe_readonly_options(&words("env LANG=C.UTF-8 rg foo")));
    assert!(!has_unsafe_readonly_options(&words("git status")));
}

/// `bat --pager <command>` names the program bat spawns to page its output.
/// bat pages only on a terminal, so under the PTY backend (which sets a
/// controlling terminal) that program runs — turning a read-only inspection
/// into a command launcher.
#[test]
fn has_unsafe_readonly_options_rejects_bat_pager_selection() {
    let words = |command: &str| -> Vec<String> { shell_words::split(command).expect("split command") };
    assert!(has_unsafe_readonly_options(&words("bat --pager 'rm -rf /' README.md")));
    assert!(has_unsafe_readonly_options(&words("bat --pager=rm README.md")));
    assert!(has_unsafe_readonly_options(&words("bat -p rm README.md")));
    assert!(has_unsafe_readonly_options(&words("bat --paging=always README.md")));
    // A plain inspection stays read-only.
    assert!(!has_unsafe_readonly_options(&words("bat README.md")));
    assert!(!has_unsafe_readonly_options(&words("bat --style=numbers README.md")));
}

#[test]
fn dynamic_shell_syntax_allows_quoted_globs_only() {
    assert!(!contains_dynamic_shell_syntax("find src -name '*.rs'"));
    assert!(!contains_dynamic_shell_syntax("find src -name \"*.rs\""));
    assert!(contains_dynamic_shell_syntax("find src -exe$''c touch {} +"));
    assert!(contains_dynamic_shell_syntax("find src -ex{e,}c touch {} +"));
}
