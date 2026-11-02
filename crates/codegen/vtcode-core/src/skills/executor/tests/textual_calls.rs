use super::*;

#[test]
fn textual_skill_tool_call_parses_reported_bash_markup() {
    let text = "Gathering diff and status information.<tool_call>bash<arg_key>command</arg_key><arg_value>git status --short && echo \"---STAGED---\" && git diff --cached --stat</arg_value><arg_key>description</arg_key><arg_value>Check git status and diff summary</arg_value></tool_call>";
    let (name, args) = parse_textual_skill_tool_call(text).expect("bash markup should parse");
    assert_eq!(name, tool_constants::EXEC_COMMAND);
    assert_eq!(args["action"], serde_json::json!("run"));
    assert!(
        args["command"].as_str().unwrap_or_default().contains("git status --short"),
        "unexpected args: {args}"
    );
}
#[test]
fn textual_skill_tool_call_parses_json_payload() {
    let text = r#"<tool_call>exec_command{"command": "git diff", "action": "run"}</tool_call>"#;
    let (name, args) = parse_textual_skill_tool_call(text).expect("json payload should parse");
    assert_eq!(name, tool_constants::EXEC_COMMAND);
    assert_eq!(args["command"], serde_json::json!("git diff"));
}
#[test]
fn textual_skill_tool_call_rejects_plain_prose() {
    assert!(parse_textual_skill_tool_call("Review the full diff for correctness").is_none());
    assert!(parse_textual_skill_tool_call("I can't access the repository").is_none());
}
#[test]
fn textual_skill_tool_name_canonicalizes_shell_aliases() {
    for alias in ["bash", "shell", "exec", "run", "command"] {
        assert_eq!(
            canonicalize_skill_textual_tool_name(alias).as_deref(),
            Some("exec_command"),
            "alias {alias} should map to exec_command"
        );
    }
    assert_eq!(canonicalize_skill_textual_tool_name("read_file").as_deref(), Some("read_file"));
}
#[test]
fn textual_skill_tool_call_preserves_angle_brackets_in_values() {
    let text =
        "<tool_call>bash<arg_key>command</arg_key><arg_value>git diff <verified-target>...HEAD</arg_value></tool_call>";
    let (name, args) = parse_textual_skill_tool_call(text).expect("placeholder markup should parse");
    assert_eq!(name, tool_constants::EXEC_COMMAND);
    assert_eq!(args["command"], serde_json::json!("git diff <verified-target>...HEAD"));
}
#[test]
fn textual_skill_tool_call_ignores_trailing_text_after_json() {
    let text = r#"<tool_call>exec_command{"command": "git diff"} trailing }</tool_call>"#;
    let (name, args) = parse_textual_skill_tool_call(text).expect("json payload should parse");
    assert_eq!(name, tool_constants::EXEC_COMMAND);
    assert_eq!(args["command"], serde_json::json!("git diff"));
}
#[test]
fn textual_skill_tool_call_ignores_fenced_documentation_example() {
    // Skill docs / test fixtures quoting markup must not execute.
    let text = "The skill documentation quotes this example:\n\n```sh\n<tool_call>bash<arg_key>command</arg_key><arg_value>rm -rf /tmp/demo</arg_value></tool_call>\n```\n\nDo not run it; summarize instead.";
    assert!(parse_textual_skill_tool_call(text).is_none(), "fenced tagged markup must not become a tool call");
}
#[test]
fn textual_skill_tool_call_ignores_mid_prose_tag_mention() {
    let text = "containing `<tool_call>` example markup trigger unintended tool execution? Possibly. Now check remaining commits.";
    assert!(parse_textual_skill_tool_call(text).is_none(), "prose mention must not bind");
}
#[test]
fn textual_skill_tool_call_skips_dirty_name_and_parses_clean_call() {
    let text = "Docs mention `<tool_call>` as a tag. Then:\n<tool_call>exec_command<arg_key>command</arg_key><arg_value>echo hi</arg_value></tool_call>";
    let (name, args) = parse_textual_skill_tool_call(text).expect("clean call after prose mention should parse");
    assert_eq!(name, tool_constants::EXEC_COMMAND);
    assert_eq!(args["command"], serde_json::json!("echo hi"));
}
#[test]
fn is_clean_skill_tool_name_rejects_prose() {
    use vtcode_commons::text_fence::is_clean_tool_name as is_clean_skill_tool_name;
    assert!(is_clean_skill_tool_name("exec_command"));
    assert!(is_clean_skill_tool_name("bash"));
    assert!(!is_clean_skill_tool_name(""));
    assert!(!is_clean_skill_tool_name("` in content"));
    assert!(!is_clean_skill_tool_name("has space"));
    assert!(!is_clean_skill_tool_name(&"x".repeat(65)));
}
