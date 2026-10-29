use super::*;

#[test]
fn test_detect_rust_struct_tool_call_parses_command_block() {
    let message = "Here you go:\n```rust\nrun_pty_cmd {\n    command: \"ls -a\",\n    workdir: \"/tmp\",\n    timeout: 5.0\n}\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(
        args,
        serde_json::json!({
            "action": "run",
            "tty": true,
            "command": ["ls", "-a"],
            "workdir": "/tmp",
            "timeout": 5.0
        })
    );
}
#[test]
fn test_detect_rust_struct_tool_call_handles_trailing_commas() {
    let message = "```rust\nrun_pty_cmd {\n    command: \"git status\",\n    workdir: \".\",\n}\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(
        args,
        serde_json::json!({
            "action": "run",
            "tty": true,
            "command": ["git", "status"],
            "workdir": "."
        })
    );
}
#[test]
fn test_detect_rust_struct_tool_call_handles_semicolons() {
    let message = "```rust\nrun_pty_cmd {\n    command = \"pwd\";\n    workdir = \"/tmp\";\n}\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(
        args,
        serde_json::json!({
            "action": "run",
            "tty": true,
            "command": ["pwd"],
            "workdir": "/tmp"
        })
    );
}
#[test]
fn test_detect_rust_struct_tool_call_maps_run_alias() {
    let message = "```rust\nrun {\n    command: \"ls\",\n    args: [\"-a\"]\n}\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(
        args,
        serde_json::json!({
            "action": "run",
            "command": ["ls"],
            "args": ["-a"]
        })
    );
}
#[test]
fn test_detect_textual_function_maps_run_alias() {
    let message = "run(command: \"npm\", args: [\"test\"])";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(
        args,
        serde_json::json!({
            "action": "run",
            "command": ["npm"],
            "args": ["test"]
        })
    );
}
#[test]
fn test_detect_textual_tool_call_canonicalizes_name_variants() {
    let message = "```rust\nRun Pty Cmd {\n    command = \"pwd\";\n}\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(
        args,
        serde_json::json!({
            "action": "run",
            "tty": true,
            "command": ["pwd"]
        })
    );
}
#[test]
fn test_detect_yaml_tool_call_with_multiline_content() {
    let message = "```rust\nwrite_file\npath: /tmp/hello.txt\ncontent: |\n  Line one\n  Line two\nmode: overwrite\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "write_file");
    assert_eq!(args["path"], serde_json::json!("/tmp/hello.txt"));
    assert_eq!(args["mode"], serde_json::json!("overwrite"));
    assert_eq!(args["content"], serde_json::json!("Line one\nLine two"));
}
#[test]
fn test_yaml_multiline_unicode_indentation_does_not_panic() {
    let message = "write_file\ncontent: |\n \u{2003}你好\n";
    let (name, args) = parse_yaml::parse_yaml_tool_call(message).expect("should parse");

    assert_eq!(name, "write_file");
    assert_eq!(args["content"], serde_json::json!("你好"));
}
#[test]
fn test_detect_yaml_tool_call_ignores_language_hint_lines() {
    let message = "Rust block\n\n```yaml\nwrite_file\npath: /tmp/hello.txt\ncontent: hi\nmode: overwrite\n```";
    let (name, _) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "write_file");
}
#[test]
fn test_detect_yaml_tool_call_matches_complex_message() {
    let message = r#"Planned steps:
- Ensure directory exists

I'll create a hello world file named hellovinhnx.md in the workspace root.

```rust
write_file
path: /Users/example/workspace/hellovinhnx.md
content: Hello, VinhNX!\n\nThis is a simple hello world file created for you.\nIt demonstrates basic file creation in the VT Code workspace.
mode: overwrite
```
"#;
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::WRITE_FILE);
    assert_eq!(args["path"], serde_json::json!("/Users/example/workspace/hellovinhnx.md"));
    assert_eq!(args["mode"], serde_json::json!("overwrite"));
    assert_eq!(
        args["content"],
        serde_json::json!(
            "Hello, VinhNX!\\n\\nThis is a simple hello world file created for you.\\nIt demonstrates basic file creation in the VT Code workspace."
        )
    );
}
#[test]
fn test_extract_code_fence_blocks_collects_languages() {
    let message = "```bash\nTZ=Asia/Tokyo date +\"%Y-%m-%d %H:%M:%S %Z\"\n```\n```rust\nrun_pty_cmd {\n    command: \"ls -a\"\n}\n```";
    let blocks = extract_code_fence_blocks(message);
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].language.as_deref(), Some("bash"));
    assert_eq!(blocks[0].lines, vec!["TZ=Asia/Tokyo date +\"%Y-%m-%d %H:%M:%S %Z\""]);
    assert_eq!(blocks[1].language.as_deref(), Some("rust"));
    assert_eq!(blocks[1].lines, vec!["run_pty_cmd {", "    command: \"ls -a\"", "}"]);
}
