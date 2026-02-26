use super::*;
use super::{parse_channel, parse_tagged};
use vtcode_core::config::constants::tools;

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
    assert_eq!(
        args,
        serde_json::json!({ "path": "notes.md", "content": "hi" })
    );
}

#[test]
fn test_detect_textual_tool_call_parses_function_style_block() {
    let message = "```rust\nrun_pty_cmd(\"ls -a\", workdir=WORKSPACE_DIR, max_tokens=1000)\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "run_pty_cmd");
    assert_eq!(args["command"], serde_json::json!(["ls", "-a"]));
    assert_eq!(args["workdir"], serde_json::json!("WORKSPACE_DIR"));
    assert_eq!(args["max_tokens"], serde_json::json!(1000));
}

#[test]
fn test_detect_textual_tool_call_skips_non_tool_function_blocks() {
    let message =
        "```rust\nprintf!(\"hi\");\n```\n```rust\nrun_pty_cmd {\n    command: \"pwd\"\n}\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "run_pty_cmd");
    assert_eq!(args["command"], serde_json::json!(["pwd"]));
}

#[test]
fn test_detect_textual_tool_call_handles_boolean_and_numbers() {
    let message =
        "default_api.search_workspace(query='todo', max_results=5, include_archived=false)";
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
    assert!(
        detect_textual_tool_call(&message).is_none(),
        "Overly deep bracketed payload should be rejected"
    );
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
    assert!(
        detect_textual_tool_call(&message).is_none(),
        "Overly deep function payload should be rejected"
    );
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

    let message =
        format!("default_api.read_file(path={malformed} ignored default_api.list_files(path='.')");
    let (name, args) = detect_textual_tool_call(&message).expect("should parse second call");
    assert_eq!(name, "list_files");
    assert_eq!(args["path"], serde_json::json!("."));
}

#[test]
fn test_detect_tagged_tool_call_parses_basic_command() {
    let message = "<tool_call>run_pty_cmd\n<arg_key>command\n<arg_value>ls -a\n</tool_call>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "run_pty_cmd");
    assert_eq!(
        args,
        serde_json::json!({
            "command": ["ls", "-a"]
        })
    );
}

#[test]
fn test_detect_tagged_tool_call_respects_indexed_arguments() {
    let message = "<tool_call>run_pty_cmd\n<arg_key>command.0\n<arg_value>python\n<arg_key>command.1\n<arg_value>-c\n<arg_key>command.2\n<arg_value>print('hi')\n</tool_call>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "run_pty_cmd");
    assert_eq!(
        args,
        serde_json::json!({
            "command": ["python", "-c", "print('hi')"]
        })
    );
}

#[test]
fn test_detect_tagged_tool_call_handles_one_based_indexes() {
    let message = "<tool_call>run_pty_cmd\n<arg_key>command.1\n<arg_value>ls\n<arg_key>command.2\n<arg_value>-a\n</tool_call>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "run_pty_cmd");
    assert_eq!(
        args,
        serde_json::json!({
            "command": ["ls", "-a"]
        })
    );
}

#[test]
fn test_detect_rust_struct_tool_call_parses_command_block() {
    let message = "Here you go:\n```rust\nrun_pty_cmd {\n    command: \"ls -a\",\n    workdir: \"/tmp\",\n    timeout: 5.0\n}\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "run_pty_cmd");
    assert_eq!(
        args,
        serde_json::json!({
            "command": ["ls", "-a"],
            "workdir": "/tmp",
            "timeout": 5.0
        })
    );
}

#[test]
fn test_detect_rust_struct_tool_call_handles_trailing_commas() {
    let message =
        "```rust\nrun_pty_cmd {\n    command: \"git status\",\n    workdir: \".\",\n}\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "run_pty_cmd");
    assert_eq!(
        args,
        serde_json::json!({
            "command": ["git", "status"],
            "workdir": "."
        })
    );
}

#[test]
fn test_detect_rust_struct_tool_call_handles_semicolons() {
    let message = "```rust\nrun_pty_cmd {\n    command = \"pwd\";\n    workdir = \"/tmp\";\n}\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "run_pty_cmd");
    assert_eq!(
        args,
        serde_json::json!({
            "command": ["pwd"],
            "workdir": "/tmp"
        })
    );
}

#[test]
fn test_detect_rust_struct_tool_call_maps_run_alias() {
    let message = "```rust\nrun {\n    command: \"ls\",\n    args: [\"-a\"]\n}\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "run_pty_cmd");
    assert_eq!(
        args,
        serde_json::json!({
            "command": ["ls"],
            "args": ["-a"]
        })
    );
}

#[test]
fn test_detect_textual_function_maps_run_alias() {
    let message = "run(command: \"npm\", args: [\"test\"])";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "run_pty_cmd");
    assert_eq!(
        args,
        serde_json::json!({
            "command": ["npm"],
            "args": ["test"]
        })
    );
}

#[test]
fn test_detect_textual_tool_call_canonicalizes_name_variants() {
    let message = "```rust\nRun Pty Cmd {\n    command = \"pwd\";\n}\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, "run_pty_cmd");
    assert_eq!(args, serde_json::json!({ "command": ["pwd"] }));
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
    assert_eq!(
        args["path"],
        serde_json::json!("/Users/example/workspace/hellovinhnx.md")
    );
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
    assert_eq!(
        blocks[0].lines,
        vec!["TZ=Asia/Tokyo date +\"%Y-%m-%d %H:%M:%S %Z\""]
    );
    assert_eq!(blocks[1].language.as_deref(), Some("rust"));
    assert_eq!(
        blocks[1].lines,
        vec!["run_pty_cmd {", "    command: \"ls -a\"", "}"]
    );
}

#[test]
fn test_parse_harmony_channel_tool_call_with_constrain() {
    let message = "<|start|>assistant<|channel|>commentary to=repo_browser.list_files <|constrain|>json<|message|>{\"path\":\"\", \"recursive\":\"true\"}<|call|>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse harmony format");
    assert_eq!(name, "list_files");
    assert_eq!(args["path"], serde_json::json!(""));
    assert_eq!(args["recursive"], serde_json::json!("true"));
}

#[test]
fn test_parse_harmony_channel_tool_call_without_constrain() {
    let message = "<|start|>assistant<|channel|>commentary to=container.exec<|message|>{\"cmd\":[\"ls\", \"-la\"]}<|call|>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse harmony format");
    assert_eq!(name, "run_pty_cmd");
    assert_eq!(args["command"], serde_json::json!(["ls", "-la"]));
}

#[test]
fn test_parse_harmony_channel_tool_call_with_string_cmd() {
    let message =
        "<|start|>assistant<|channel|>commentary to=bash<|message|>{\"cmd\":\"pwd\"}<|call|>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse harmony format");
    assert_eq!(name, "run_pty_cmd");
    assert_eq!(args["command"], serde_json::json!(["pwd"]));
}

#[test]
fn test_convert_harmony_args_rejects_empty_command_array() {
    let parsed = serde_json::json!({ "cmd": [] });
    let result = parse_channel::convert_harmony_args_to_tool_format("run_pty_cmd", parsed);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "command executable cannot be empty");
}

#[test]
fn test_convert_harmony_args_rejects_empty_command_string() {
    let parsed = serde_json::json!({ "cmd": "" });
    let result = parse_channel::convert_harmony_args_to_tool_format("run_pty_cmd", parsed);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "command executable cannot be empty");
}

#[test]
fn test_convert_harmony_args_rejects_whitespace_only_command() {
    let parsed = serde_json::json!({ "cmd": "   " });
    let result = parse_channel::convert_harmony_args_to_tool_format("run_pty_cmd", parsed);
    assert!(result.is_err());
}

#[test]
fn test_convert_harmony_args_rejects_empty_executable_in_array() {
    let parsed = serde_json::json!({ "cmd": ["", "arg1"] });
    let result = parse_channel::convert_harmony_args_to_tool_format("run_pty_cmd", parsed);
    assert!(result.is_err());
}

#[test]
fn test_convert_harmony_args_accepts_valid_command_array() {
    let parsed = serde_json::json!({ "cmd": ["ls", "-la"] });
    let result = parse_channel::convert_harmony_args_to_tool_format("run_pty_cmd", parsed);
    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["command"], serde_json::json!(["ls", "-la"]));
}

#[test]
fn test_convert_harmony_args_accepts_valid_command_string() {
    let parsed = serde_json::json!({ "cmd": "echo test" });
    let result = parse_channel::convert_harmony_args_to_tool_format("run_pty_cmd", parsed);
    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["command"], serde_json::json!(["echo test"]));
}

#[test]
fn test_parse_harmony_channel_rejects_empty_command() {
    // Harmony format parser (OpenAI/GPT-OSS): should reject tool call if command is empty
    // because convert_harmony_args_to_tool_format() returns Err which parse_channel_tool_call() rejects
    let message =
        "<|start|>assistant<|channel|>commentary to=bash<|message|>{\"cmd\":\"\"}<|call|>";
    let result = parse_channel::parse_channel_tool_call(message);
    assert!(
        result.is_none(),
        "Should reject Harmony format with empty command"
    );
}

#[test]
fn test_parse_harmony_channel_rejects_empty_array() {
    // Harmony format parser: should reject tool call if command array is empty
    let message =
        "<|start|>assistant<|channel|>commentary to=container.exec<|message|>{\"cmd\":[]}<|call|>";
    let result = parse_channel::parse_channel_tool_call(message);
    assert!(
        result.is_none(),
        "Should reject Harmony format with empty array"
    );
}

#[test]
fn test_parse_harmony_channel_rejects_whitespace_command() {
    // Harmony format parser: should reject tool call if command is whitespace-only
    let message =
        "<|start|>assistant<|channel|>commentary to=bash<|message|>{\"cmd\":\"   \"}<|call|>";
    let result = parse_channel::parse_channel_tool_call(message);
    assert!(
        result.is_none(),
        "Should reject Harmony format with whitespace-only command"
    );
}

// ==================== Tests for malformed XML handling (GLM models) ====================

#[test]
fn test_parse_tagged_tool_call_handles_double_tag_malformed_xml() {
    // GLM models sometimes output: <tool_call>list_files<tool_call>list
    // Should extract tool name but with empty args
    let message = "<tool_call>list_files<tool_call>list";
    let result = parse_tagged::parse_tagged_tool_call(message);
    assert!(result.is_some(), "Should parse malformed double-tag XML");
    let (name, args) = result.unwrap();
    assert_eq!(name, "list_files");
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
    assert_eq!(
        args.get("path").and_then(|v| v.as_str()),
        Some("/tmp/test.txt")
    );
}

#[test]
fn test_parse_tagged_tool_call_extracts_json_with_space() {
    // When JSON appears after tool name with space
    let message = r#"<tool_call>read_file {"path": "/tmp/test.txt"}</tool_call>"#;
    let result = parse_tagged::parse_tagged_tool_call(message);
    assert!(
        result.is_some(),
        "Should parse JSON with space after tool name"
    );
    let (name, args) = result.unwrap();
    assert_eq!(name, "read_file");
    assert_eq!(
        args.get("path").and_then(|v| v.as_str()),
        Some("/tmp/test.txt")
    );
}

#[test]
fn test_parse_tagged_tool_call_handles_nested_json() {
    // Nested JSON should be parsed correctly
    let message =
        r#"<tool_call>run_pty_cmd{"command": "echo", "env": {"PATH": "/usr/bin"}}</tool_call>"#;
    let result = parse_tagged::parse_tagged_tool_call(message);
    assert!(result.is_some(), "Should parse nested JSON");
    let (name, args) = result.unwrap();
    assert_eq!(name, "run_pty_cmd");
    assert!(args.get("command").and_then(|v| v.as_array()).is_some());
    assert_eq!(
        args.get("command")
            .and_then(|v| v.get(0))
            .and_then(|v| v.as_str()),
        Some("echo")
    );
    assert!(args.get("env").and_then(|v| v.as_object()).is_some());
}

#[test]
fn test_parse_tagged_tool_call_stops_at_next_tool_call_tag() {
    // Content boundary should be the next <tool_call> tag
    let message = "<tool_call>list_files<tool_call>read_file";
    let result = parse_tagged::parse_tagged_tool_call(message);
    assert!(result.is_some());
    let (name, _) = result.unwrap();
    assert_eq!(name, "list_files");
}
