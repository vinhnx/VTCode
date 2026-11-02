use super::*;

// ==================== Tests for malformed XML handling (GLM models) ====================

#[test]
fn test_parse_tagged_tool_call_handles_double_tag_malformed_xml() {
    // GLM models sometimes output: <tool_call>list_files<tool_call>list
    // Should extract tool name but with empty args
    let message = "<tool_call>list_files<tool_call>list";
    let result = parse_tagged::parse_tagged_tool_call(message);
    assert!(result.is_some(), "Should parse malformed double-tag XML");
    let (name, args) = result.unwrap();
    assert_eq!(name, tools::LIST_FILES);
    // Args should be empty object since no valid args were found
    assert!(args.as_object().is_none_or(|o| o.is_empty()));
}
#[test]
fn test_parse_tagged_tool_call_extracts_json_after_name() {
    // When JSON appears after the tool name
    let message = r#"<tool_call>read_file{"path": "/tmp/test.txt"}</tool_call>"#;
    let result = parse_tagged::parse_tagged_tool_call(message);
    assert!(result.is_some(), "Should parse JSON after tool name");
    let (name, args) = result.unwrap();
    assert_eq!(name, "read_file");
    assert_eq!(args.get("path").and_then(|v| v.as_str()), Some("/tmp/test.txt"));
}
#[test]
fn test_parse_tagged_tool_call_extracts_json_with_space() {
    // When JSON appears after tool name with space
    let message = r#"<tool_call>read_file {"path": "/tmp/test.txt"}</tool_call>"#;
    let result = parse_tagged::parse_tagged_tool_call(message);
    assert!(result.is_some(), "Should parse JSON with space after tool name");
    let (name, args) = result.unwrap();
    assert_eq!(name, "read_file");
    assert_eq!(args.get("path").and_then(|v| v.as_str()), Some("/tmp/test.txt"));
}
#[test]
fn test_parse_tagged_tool_call_handles_nested_json() {
    // Nested JSON should be parsed correctly
    let message = r#"<tool_call>run_pty_cmd{"command": "echo", "env": {"PATH": "/usr/bin"}}</tool_call>"#;
    // Use detect_textual_tool_call which applies canonicalization
    let result = detect_textual_tool_call(message);
    assert!(result.is_some(), "Should parse nested JSON");
    let (name, args) = result.unwrap();
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(args.get("action"), Some(&serde_json::json!("run")));
    assert_eq!(args.get("tty"), Some(&serde_json::json!(true)));
    assert!(args.get("command").and_then(|v| v.as_array()).is_some());
    assert_eq!(args.get("command").and_then(|v| v.get(0)).and_then(|v| v.as_str()), Some("echo"));
    assert!(args.get("env").and_then(|v| v.as_object()).is_some());
}
#[test]
fn test_parse_tagged_tool_call_stops_at_next_tool_call_tag() {
    // Content boundary should be the next <tool_call> tag
    let message = "<tool_call>list_files<tool_call>read_file";
    let result = parse_tagged::parse_tagged_tool_call(message);
    assert!(result.is_some());
    let (name, _) = result.unwrap();
    assert_eq!(name, tools::LIST_FILES);
}
#[test]
fn parse_function_equals_extracts_task_tracker_list_action() {
    let lt: char = 60.into();
    let gt: char = 62.into();
    let slash: char = 47.into();
    let text = format!(
        "{lt}function=task_tracker{gt}{lt}parameter=action{gt}list{lt}{slash}parameter{gt}{lt}{slash}function{gt}"
    );
    let result = parse_tagged::parse_tagged_tool_call(&text);
    assert!(result.is_some(), "should parse function= dialect");
    let (name, args) = result.unwrap();
    assert_eq!(name, "task_tracker");
    assert_eq!(args["action"], "list");
}
#[test]
fn parse_function_equals_extracts_multiple_parameters() {
    let lt: char = 60.into();
    let gt: char = 62.into();
    let slash: char = 47.into();
    let text = format!(
        "{lt}function=exec_command{gt}{lt}parameter=command{gt}ls -la{lt}{slash}parameter{gt}{lt}parameter=action{gt}run{lt}{slash}parameter{gt}{lt}{slash}function{gt}"
    );
    let result = parse_tagged::parse_tagged_tool_call(&text);
    assert!(result.is_some());
    let (name, args) = result.unwrap();
    assert_eq!(name, "exec_command");
    assert_eq!(args["action"], "run");
}
#[test]
fn parse_function_equals_handles_missing_close_tag() {
    let lt: char = 60.into();
    let gt: char = 62.into();
    let text = format!("{lt}function=task_tracker{gt}{lt}parameter=action{gt}create");
    let result = parse_tagged::parse_tagged_tool_call(&text);
    assert!(result.is_some());
    let (name, args) = result.unwrap();
    assert_eq!(name, "task_tracker");
    assert_eq!(args["action"], "create");
}
#[test]
fn parse_function_equals_returns_none_for_empty_name() {
    let lt: char = 60.into();
    let gt: char = 62.into();
    let text = format!("{lt}function={gt}{lt}parameter=action{gt}list");
    let result = parse_tagged::parse_tagged_tool_call(&text);
    assert!(result.is_none());
}
#[test]
fn parse_function_equals_returns_none_when_no_function_tag() {
    let result = parse_tagged::parse_tagged_tool_call("just some text without tags");
    assert!(result.is_none());
}
#[test]
fn parse_function_equals_strips_quotes_around_name() {
    let lt: char = 60.into();
    let gt: char = 62.into();
    let slash: char = 47.into();
    let text = format!(
        "{lt}function=\"task_tracker\"{gt}{lt}parameter=action{gt}list{lt}{slash}parameter{gt}{lt}{slash}function{gt}"
    );
    let result = parse_tagged::parse_tagged_tool_call(&text);
    assert!(result.is_some());
    let (name, _) = result.unwrap();
    assert_eq!(name, "task_tracker");
}
#[test]
fn parse_function_equals_handles_whitespace_around_values() {
    let lt: char = 60.into();
    let gt: char = 62.into();
    let slash: char = 47.into();
    let text = format!(
        "{lt}function=task_tracker{gt}\n  {lt}parameter=action{gt}\n  list  \n{lt}{slash}parameter{gt}\n{lt}{slash}function{gt}"
    );
    let result = parse_tagged::parse_tagged_tool_call(&text);
    assert!(result.is_some());
    let (name, args) = result.unwrap();
    assert_eq!(name, "task_tracker");
    assert_eq!(args["action"], "list");
}
#[test]
fn is_clean_tool_name_accepts_identifiers_only() {
    use super::canonical::is_clean_tool_name;
    assert!(is_clean_tool_name("exec_command"));
    assert!(is_clean_tool_name("run_pty_cmd"));
    assert!(is_clean_tool_name("Read_File"));
    assert!(!is_clean_tool_name(""));
    assert!(!is_clean_tool_name("` in content — could a skill"));
    assert!(!is_clean_tool_name("exec_command{\"command\": \"x\"}"));
    assert!(!is_clean_tool_name("has space"));
    assert!(!is_clean_tool_name("1leading_digit"));
    assert!(!is_clean_tool_name(&"x".repeat(65)));
}
#[test]
fn tagged_markup_inside_fence_is_not_a_tool_call() {
    // Documentation/test fixtures quoting tagged markup must not execute.
    let text = "The skill documentation quotes this example:\n\n```sh\n<tool_call>bash<arg_key>command</arg_key><arg_value>rm -rf /tmp/demo</arg_value></tool_call>\n```\n\nDo not run it.";
    assert!(detect_textual_tool_call(text).is_none(), "fenced tagged markup must not become a tool call");
}
#[test]
fn mid_prose_tool_call_mention_is_not_a_tool_call() {
    // Session 20260925T141131Z: prose that merely mentions the tag produced a
    // garbage tool name. The first tag occurrence yields a non-identifier name
    // and must be skipped; without a second clean call the text stays text.
    let text = "containing `<tool_call>` example markup trigger unintended tool execution? Possibly. Now check remaining commits.";
    assert!(detect_textual_tool_call(text).is_none(), "prose mention must not bind");
}
#[test]
fn tagged_scan_skips_prose_mention_and_parses_clean_call() {
    use super::canonical::is_clean_tool_name;
    // First occurrence is a backticked mention (invalid name); the later real
    // call must still parse.
    let text = "Docs mention `<tool_call>` as a tag. Then the model runs:\n\
                <tool_call>exec_command\n\
                <arg_key>command</arg_key><arg_value>echo hi</arg_value>\n\
                <arg_key>action</arg_key><arg_value>run</arg_value></tool_call>";
    assert!(!is_clean_tool_name("`"), "backtick name must be dirty");
    assert!(is_clean_tool_name("exec_command"));
    let direct = parse_tagged::parse_tagged_tool_call(text);
    assert!(direct.is_some(), "parse_tagged_tool_call should find the clean call");
    let (name, args) = detect_textual_tool_call(text).expect("clean call after prose mention should parse");
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(args["action"], "run");
}
#[test]
fn fenced_tagged_example_is_stripped_like_unfenced_intent() {
    // Strip removes tool-call-shaped regions wherever they appear (recovery
    // salvage and plan-mode cleanup want that). Execution is what the fence
    // gate blocks — see `tagged_markup_inside_fence_is_not_a_tool_call`.
    let text = "Docs:\n```text\n<tool_call>bash<arg_key>command</arg_key><arg_value>rm -rf /</arg_value></tool_call>\n```\nDone.";
    let stripped = strip_textual_tool_call_regions(text);
    assert!(stripped.contains("```text"), "fence markers must survive strip");
    assert!(!stripped.contains("rm -rf /"), "tool-call payload is stripped");
}
#[test]
fn shell_alias_maps_only_true_shell_names() {
    use crate::agent::runloop::text_tools::canonicalize_shell_tool_alias;
    assert_eq!(canonicalize_shell_tool_alias("bash").as_deref(), Some("exec_command"));
    assert_eq!(canonicalize_shell_tool_alias("shell").as_deref(), Some("exec_command"));
    assert_eq!(canonicalize_shell_tool_alias("run").as_deref(), Some("exec_command"));
    // PTY-family tools keep their identity
    assert_eq!(canonicalize_shell_tool_alias("run_pty_cmd"), None);
    assert_eq!(canonicalize_shell_tool_alias("create_pty_session"), None);
    assert_eq!(canonicalize_shell_tool_alias("read_file"), None);
}
#[test]
fn dispatchable_tool_name_allows_mcp_and_rejects_prose() {
    use crate::agent::runloop::text_tools::is_dispatchable_tool_name;
    assert!(is_dispatchable_tool_name("exec_command"));
    assert!(is_dispatchable_tool_name("mcp__github__list_issues"));
    assert!(is_dispatchable_tool_name("mcp::github::list_issues"));
    assert!(!is_dispatchable_tool_name("has space"));
    assert!(!is_dispatchable_tool_name("` in content"));
    assert!(!is_dispatchable_tool_name(&"x".repeat(65)));
}
