use super::*;

#[test]
fn test_detect_textual_tool_call_parses_python_style_arguments() {
    let message = "call\nprint(default_api.read_file(path='AGENTS.md'))";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "read_file");
    assert_eq!(args, serde_json::json!({ "path": "AGENTS.md" }));
}
#[test]
fn test_detect_textual_tool_call_supports_json_payload() {
    let message = "print(default_api.write_file({\"path\": \"notes.md\", \"content\": \"hi\"}))";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "write_file");
    assert_eq!(args, serde_json::json!({ "path": "notes.md", "content": "hi" }));
}
#[test]
fn test_detect_textual_tool_call_parses_function_style_block() {
    let message = "```rust\nrun_pty_cmd(\"ls -a\", workdir=WORKSPACE_DIR, max_tokens=1000)\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(args["action"], serde_json::json!("run"));
    assert_eq!(args["tty"], serde_json::json!(true));
    assert_eq!(args["command"], serde_json::json!(["ls", "-a"]));
    assert_eq!(args["workdir"], serde_json::json!("WORKSPACE_DIR"));
    assert_eq!(args["max_tokens"], serde_json::json!(1000));
}
#[test]
fn test_detect_textual_tool_call_skips_non_tool_function_blocks() {
    let message = "```rust\nprintf!(\"hi\");\n```\n```rust\nrun_pty_cmd {\n    command: \"pwd\"\n}\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(args["action"], serde_json::json!("run"));
    assert_eq!(args["tty"], serde_json::json!(true));
    assert_eq!(args["command"], serde_json::json!(["pwd"]));
}
#[test]
fn test_detect_textual_tool_call_handles_boolean_and_numbers() {
    let message = "default_api.search_workspace(query='todo', max_results=5, include_archived=false)";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "search_workspace");
    assert_eq!(
        args,
        serde_json::json!({
            "query": "todo",
            "max_results": 5,
            "include_archived": false
        })
    );
}
#[test]
fn test_detect_textual_tool_call_rejects_excessive_bracketed_nesting() {
    let mut nested = String::new();
    for _ in 0..260 {
        nested.push_str("{\"a\":");
    }
    nested.push_str("\"x\"");
    for _ in 0..260 {
        nested.push('}');
    }

    let message = format!("[tool: read_file] {nested}");
    assert!(detect_textual_tool_call(&message).is_none(), "Overly deep bracketed payload should be rejected");
}
#[test]
fn unclosed_fence_tail_still_parses_tool_call() {
    // Truncated streaming output can leave a fence opener without a closer.
    // Fail-open: the tail after the opener is treated as unfenced so a real
    // tool call there still parses instead of being silently dropped.
    // Tag literals in this test are assembled from parts so this test adds
    // no new contiguous tool-call tag to the source (other tests in this
    // file already hold literal tags; a literal tag here would itself be
    // scanned as a call by tooling that greps test sources).
    let open = format!("<{}", "tool_call>");
    let key = format!("<{}", "arg_key>");
    let key_close = format!("</{}", "arg_key>");
    let value = format!("<{}", "arg_value>");
    let value_close = format!("</{}", "arg_value>");
    let text = format!(
        "Partial output:\n```sh\necho demo\n{open}exec_command\n{key}command{key_close}{value}echo hi{value_close}\n{key}action{key_close}{value}run{value_close}\n"
    );
    let (name, args) = detect_textual_tool_call(&text).expect("call after unclosed fence opener must parse");
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(args["action"], "run");
}
#[test]
fn test_detect_textual_tool_call_rejects_excessive_function_nesting() {
    let mut nested = String::new();
    for _ in 0..260 {
        nested.push('(');
    }
    nested.push_str("'x'");
    for _ in 0..260 {
        nested.push(')');
    }

    let message = format!("default_api.read_file(path={nested})");
    assert!(detect_textual_tool_call(&message).is_none(), "Overly deep function payload should be rejected");
}
#[test]
fn test_detect_textual_tool_call_rejects_excessive_mixed_function_nesting() {
    let mut nested = String::new();
    for _ in 0..90 {
        nested.push_str("({[");
    }
    nested.push_str("'x'");
    for _ in 0..90 {
        nested.push_str("]})");
    }

    let message = format!("default_api.read_file(path={nested})");
    assert!(
        detect_textual_tool_call(&message).is_none(),
        "Overly deep mixed-delimiter payload should be rejected"
    );
}
#[test]
fn test_detect_textual_tool_call_handles_closing_delimiters_inside_strings() {
    let message = "default_api.read_file(path='docs/notes})].md')";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "read_file");
    assert_eq!(args["path"], serde_json::json!("docs/notes})].md"));
}
#[test]
fn test_detect_textual_tool_call_skips_malformed_deep_candidate() {
    let mut malformed = String::new();
    for _ in 0..260 {
        malformed.push('(');
    }

    let message = format!("default_api.read_file(path={malformed} ignored default_api.list_files(path='.')");
    let (name, args) = detect_textual_tool_call(&message).expect("should parse second call");
    assert_eq!(name, tools::LIST_FILES);
    assert_eq!(args["path"], serde_json::json!("."));
}
