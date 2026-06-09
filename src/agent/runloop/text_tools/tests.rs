use super::*;
use super::{parse_channel, parse_tagged};
use vtcode_core::config::constants::tools;

// ── contains_pseudo_tool_call_markers ─────────────────────────────────────────

#[test]
fn pseudo_markers_detect_bare_tool_call_tag() {
    assert!(contains_pseudo_tool_call_markers(
        "Let me apply:\n<tool_call>\n<function=apply_patch>...</function>\n</tool_call>"
    ));
}

#[test]
fn pseudo_markers_detect_function_eq_without_tool_call_wrapper() {
    assert!(contains_pseudo_tool_call_markers(
        "<function=write_file>content</function>"
    ));
}

#[test]
fn pseudo_markers_detect_parameter_eq() {
    assert!(contains_pseudo_tool_call_markers(
        "<parameter=patch>some diff</parameter>"
    ));
}

#[test]
fn pseudo_markers_detect_invoke_name() {
    assert!(contains_pseudo_tool_call_markers(
        r#"<invoke name="apply_patch"><parameter name="p">x</parameter></invoke>"#
    ));
}

#[test]
fn pseudo_markers_detect_minimax_tool_call() {
    assert!(contains_pseudo_tool_call_markers(
        "<minimax:tool_call>...</minimax:tool_call>"
    ));
}

#[test]
fn pseudo_markers_detect_closing_tool_call_tag_alone() {
    assert!(contains_pseudo_tool_call_markers("</tool_call>"));
}

#[test]
fn pseudo_markers_is_case_insensitive() {
    assert!(contains_pseudo_tool_call_markers(
        "<TOOL_CALL>...</TOOL_CALL>"
    ));
    assert!(contains_pseudo_tool_call_markers("<Function=foo>"));
}

#[test]
fn pseudo_markers_returns_false_for_clean_prose() {
    assert!(!contains_pseudo_tool_call_markers(
        "I searched the codebase and found 3 matches. Here are the results."
    ));
}

#[test]
fn pseudo_markers_returns_false_for_ordinary_xml_attributes() {
    // A plain XML attribute like `name="foo"` should not trigger the check.
    assert!(!contains_pseudo_tool_call_markers(
        r#"<config name="foo" value="bar"/>"#
    ));
}

// ── strip_textual_tool_call_regions — function= / parameter= coverage ─────────

#[test]
fn strip_regions_removes_bare_function_eq_block() {
    let text = "Here is what I would do:\n\
                <function=apply_patch>--- a/f\n+++ b/f\n@@ -1 +1 @@\n-old\n+new</function>\n\
                Ask me if you want me to actually run this.";
    let stripped = strip_textual_tool_call_regions(text);
    assert!(
        !stripped.contains("<function="),
        "function= block should be stripped"
    );
    assert!(
        stripped.contains("Here is what I would do"),
        "surrounding prose should be preserved"
    );
}

#[test]
fn strip_regions_removes_bare_parameter_eq_block() {
    let text = "Patch content:\n<parameter=patch>diff goes here</parameter>";
    let stripped = strip_textual_tool_call_regions(text);
    assert!(!stripped.contains("<parameter="));
}

/// Regression: when a model uses parameterised close tags (`</function=name>`,
/// `</parameter=name>`) the stripper must not fall back to `end = text.len()`
/// and must not eat prose that follows the block.
#[test]
fn strip_regions_parameterised_close_tags_do_not_eat_trailing_prose() {
    // Bare block with parameterised close tag, followed by trailing prose.
    let bare = "Prose before.\n\
                <function=apply_patch>diff content</function=apply_patch>\n\
                Prose after.";
    let stripped = strip_textual_tool_call_regions(bare);
    assert!(
        !stripped.contains("<function="),
        "function= block should be stripped"
    );
    assert!(
        stripped.contains("Prose before."),
        "leading prose must survive"
    );
    assert!(
        stripped.contains("Prose after."),
        "trailing prose must survive bare parameterised close tag"
    );

    // When the function= block is nested inside <tool_call> with prose after
    // </tool_call>, the fallback `end=text.len()` must NOT swallow trailing prose.
    // Note: <tool_call> itself may remain (collect_enclosed_regions requires a
    // parseable payload), but the key invariant is that trailing prose is preserved.
    let wrapped = "Prose before.\n\
                   <tool_call>\n\
                   <function=apply_patch>diff</function=apply_patch>\n\
                   </tool_call>\n\
                   Prose after.";
    let stripped2 = strip_textual_tool_call_regions(wrapped);
    assert!(
        !stripped2.contains("<function=apply_patch>"),
        "function= content must be stripped"
    );
    assert!(
        stripped2.contains("Prose before."),
        "leading prose must survive"
    );
    assert!(
        stripped2.contains("Prose after."),
        "trailing prose must not be eaten by merged regions"
    );
}

#[test]
fn strip_regions_removes_function_eq_block_with_parameterised_close() {
    // `</function=name>` close tag (without a tool_call wrapper) must be consumed.
    let text = "<function=write_file>hello world</function=write_file>";
    let stripped = strip_textual_tool_call_regions(text);
    assert!(
        !stripped.contains("<function="),
        "function= block should be fully stripped"
    );
    assert!(
        !stripped.contains("</function="),
        "parameterised close tag must be consumed"
    );
}

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
    assert_eq!(name, tools::UNIFIED_EXEC);
    assert_eq!(args["action"], serde_json::json!("run"));
    assert_eq!(args["tty"], serde_json::json!(true));
    assert_eq!(args["command"], serde_json::json!(["ls", "-a"]));
    assert_eq!(args["workdir"], serde_json::json!("WORKSPACE_DIR"));
    assert_eq!(args["max_tokens"], serde_json::json!(1000));
}

#[test]
fn test_detect_textual_tool_call_skips_non_tool_function_blocks() {
    let message =
        "```rust\nprintf!(\"hi\");\n```\n```rust\nrun_pty_cmd {\n    command: \"pwd\"\n}\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::UNIFIED_EXEC);
    assert_eq!(args["action"], serde_json::json!("run"));
    assert_eq!(args["tty"], serde_json::json!(true));
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
    assert_eq!(name, tools::LIST_FILES);
    assert_eq!(args["path"], serde_json::json!("."));
}

#[test]
fn test_detect_tagged_tool_call_parses_basic_command() {
    let message = "<tool_call>run_pty_cmd\n<arg_key>command\n<arg_value>ls -a\n</tool_call>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::UNIFIED_EXEC);
    assert_eq!(
        args,
        serde_json::json!({
            "action": "run",
            "tty": true,
            "command": ["ls", "-a"]
        })
    );
}

#[test]
fn test_detect_tagged_tool_call_respects_indexed_arguments() {
    let message = "<tool_call>run_pty_cmd\n<arg_key>command.0\n<arg_value>python\n<arg_key>command.1\n<arg_value>-c\n<arg_key>command.2\n<arg_value>print('hi')\n</tool_call>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::UNIFIED_EXEC);
    assert_eq!(
        args,
        serde_json::json!({
            "action": "run",
            "tty": true,
            "command": ["python", "-c", "print('hi')"]
        })
    );
}

#[test]
fn test_detect_tagged_tool_call_handles_one_based_indexes() {
    let message = "<tool_call>run_pty_cmd\n<arg_key>command.1\n<arg_value>ls\n<arg_key>command.2\n<arg_value>-a\n</tool_call>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::UNIFIED_EXEC);
    assert_eq!(
        args,
        serde_json::json!({
            "action": "run",
            "tty": true,
            "command": ["ls", "-a"]
        })
    );
}

#[test]
fn test_detect_tagged_tool_call_parses_minimax_xml_invocation() {
    let message = r#"
<minimax:tool_call>
<invoke name="unified_file">
<parameter name="action">read</parameter>
<parameter name="path">vtcode-core/src/core/agent/runtime/mod.rs</parameter>
<parameter name="offset">1</parameter>
<parameter name="limit">400</parameter>
</invoke>
</minimax:tool_call>
"#;
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::UNIFIED_FILE);
    assert_eq!(
        args,
        serde_json::json!({
            "action": "read",
            "path": "vtcode-core/src/core/agent/runtime/mod.rs",
            "offset": 1,
            "limit": 400
        })
    );
}

#[test]
fn test_detect_tagged_tool_call_parses_minimax_xml_invocation_without_parameters() {
    let message = r#"
<minimax:tool_call>
<invoke name="list_pty_sessions">
</invoke>
</minimax:tool_call>
"#;
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::UNIFIED_EXEC);
    assert_eq!(args, serde_json::json!({ "action": "list" }));
}

#[test]
fn test_strip_textual_tool_call_regions_removes_minimax_markup() {
    let message = r#"Done with the analysis.
<minimax:tool_call>
<invoke name="unified_file">
<parameter name="action">read</parameter>
<parameter name="path">README.md</parameter>
</invoke>
</minimax:tool_call>
Please review the summary."#;

    let stripped = strip_textual_tool_call_regions(message);

    assert!(stripped.contains("Done with the analysis."));
    assert!(stripped.contains("Please review the summary."));
    assert!(!stripped.contains("<invoke"));
    assert!(detect_textual_tool_call(&stripped).is_none());
}

#[test]
fn test_strip_textual_tool_call_regions_removes_channel_and_function_markup() {
    let message = concat!(
        "Summary before.\n",
        "<|start|>assistant<|channel|>commentary to=bash<|message|>{\"cmd\":\"pwd\"}<|call|>\n",
        "Middle.\n",
        "print(default_api.read_file(path='AGENTS.md'))\n",
        "Summary after."
    );

    let stripped = strip_textual_tool_call_regions(message);

    assert!(stripped.contains("Summary before."));
    assert!(stripped.contains("Middle."));
    assert!(stripped.contains("Summary after."));
    assert!(!stripped.contains("<|start|>"));
    assert!(!stripped.contains("print("));
    assert!(!stripped.contains("default_api.read_file"));
    assert!(detect_textual_tool_call(&stripped).is_none());
}

#[test]
fn test_detect_rust_struct_tool_call_parses_command_block() {
    let message = "Here you go:\n```rust\nrun_pty_cmd {\n    command: \"ls -a\",\n    workdir: \"/tmp\",\n    timeout: 5.0\n}\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::UNIFIED_EXEC);
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
    let message =
        "```rust\nrun_pty_cmd {\n    command: \"git status\",\n    workdir: \".\",\n}\n```";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::UNIFIED_EXEC);
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
    assert_eq!(name, tools::UNIFIED_EXEC);
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
    assert_eq!(name, "unified_exec");
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
    assert_eq!(name, "unified_exec");
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
    assert_eq!(name, tools::UNIFIED_EXEC);
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
    assert_eq!(name, tools::LIST_FILES);
    assert_eq!(args["path"], serde_json::json!(""));
    assert_eq!(args["recursive"], serde_json::json!("true"));
}

#[test]
fn test_parse_harmony_channel_tool_call_without_constrain() {
    let message = "<|start|>assistant<|channel|>commentary to=container.exec<|message|>{\"cmd\":[\"ls\", \"-la\"]}<|call|>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse harmony format");
    assert_eq!(name, "unified_exec");
    assert_eq!(args["command"], serde_json::json!(["ls", "-la"]));
}

#[test]
fn test_parse_harmony_channel_tool_call_with_string_cmd() {
    let message =
        "<|start|>assistant<|channel|>commentary to=bash<|message|>{\"cmd\":\"pwd\"}<|call|>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse harmony format");
    assert_eq!(name, "unified_exec");
    assert_eq!(args["command"], serde_json::json!("pwd"));
}

#[test]
fn test_parse_harmony_channel_tool_call_with_recipient_before_channel() {
    let message = "<|start|>assistant to=functions.unified_search<|channel|>commentary <|constrain|>json<|message|>{\"action\":\"list\",\"path\":\"src\"}<|call|>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse harmony format");
    assert_eq!(name, "unified_search");
    assert_eq!(args["action"], serde_json::json!("list"));
    assert_eq!(args["path"], serde_json::json!("src"));
}

#[test]
fn test_parse_harmony_channel_tool_call_tolerates_single_quoted_payload() {
    let message = "<|start|>assistant to=functions.unified_search<|channel|>commentary <|constrain|>json<|message|>{'action':'list','path':'src'}<|call|>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse harmony format");
    assert_eq!(name, "unified_search");
    assert_eq!(args["action"], serde_json::json!("list"));
    assert_eq!(args["path"], serde_json::json!("src"));
}

#[test]
fn test_parse_harmony_channel_requires_explicit_recipient() {
    let message = "<|start|>assistant<|channel|>commentary<|message|>{\"cmd\":\"pwd\"}<|call|>";
    assert!(
        parse_channel::parse_channel_tool_call(message).is_none(),
        "harmony tool calls should require an explicit recipient"
    );
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
    result.unwrap_err();
}

#[test]
fn test_convert_harmony_args_rejects_empty_executable_in_array() {
    let parsed = serde_json::json!({ "cmd": ["", "arg1"] });
    let result = parse_channel::convert_harmony_args_to_tool_format("run_pty_cmd", parsed);
    result.unwrap_err();
}

#[test]
fn test_convert_harmony_args_accepts_valid_command_array() {
    let parsed = serde_json::json!({ "cmd": ["ls", "-la"] });
    let result = parse_channel::convert_harmony_args_to_tool_format("run_pty_cmd", parsed);
    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["tty"], serde_json::json!(true));
    assert_eq!(value["command"], serde_json::json!(["ls", "-la"]));
}

#[test]
fn test_convert_harmony_args_accepts_valid_command_string() {
    let parsed = serde_json::json!({ "cmd": "echo test" });
    let result = parse_channel::convert_harmony_args_to_tool_format("run_pty_cmd", parsed);
    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["tty"], serde_json::json!(true));
    assert_eq!(value["command"], serde_json::json!("echo test"));
}

#[test]
fn test_convert_harmony_args_maps_create_pty_session_to_run_with_tty() {
    let parsed = serde_json::json!({ "command": "pwd" });
    let result = parse_channel::convert_harmony_args_to_tool_format("create_pty_session", parsed);
    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value["action"], serde_json::json!("run"));
    assert_eq!(value["tty"], serde_json::json!(true));
    assert_eq!(value["command"], serde_json::json!("pwd"));
}

#[test]
fn test_convert_harmony_args_rejects_non_string_command_array_entries() {
    let parsed = serde_json::json!({ "cmd": ["ls", 1] });
    let result = parse_channel::convert_harmony_args_to_tool_format("run_pty_cmd", parsed);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "command array must contain only strings"
    );
}

#[test]
fn test_convert_harmony_args_maps_read_pty_session_to_poll() {
    let parsed = serde_json::json!({
        "session_id": "run-1",
        "yield_time_ms": 10
    });
    let result = parse_channel::convert_harmony_args_to_tool_format("read_pty_session", parsed);
    assert!(result.is_ok());
    assert_eq!(
        result.unwrap(),
        serde_json::json!({
            "action": "poll",
            "session_id": "run-1",
            "yield_time_ms": 10
        })
    );
}

#[test]
fn test_convert_harmony_args_maps_send_pty_input_to_write() {
    let parsed = serde_json::json!({
        "session_id": "run-1",
        "chars": "status\n"
    });
    let result = parse_channel::convert_harmony_args_to_tool_format("send_pty_input", parsed);
    assert!(result.is_ok());
    assert_eq!(
        result.unwrap(),
        serde_json::json!({
            "action": "write",
            "session_id": "run-1",
            "chars": "status\n"
        })
    );
}

#[test]
fn test_convert_harmony_args_maps_list_pty_sessions_to_list() {
    let parsed = serde_json::json!({
        "yield_time_ms": 10
    });
    let result = parse_channel::convert_harmony_args_to_tool_format("list_pty_sessions", parsed);
    assert!(result.is_ok());
    assert_eq!(
        result.unwrap(),
        serde_json::json!({
            "action": "list",
            "yield_time_ms": 10
        })
    );
}

#[test]
fn test_convert_harmony_args_maps_close_pty_session_to_close() {
    let parsed = serde_json::json!({
        "session_id": "run-1"
    });
    let result = parse_channel::convert_harmony_args_to_tool_format("close_pty_session", parsed);
    assert!(result.is_ok());
    assert_eq!(
        result.unwrap(),
        serde_json::json!({
            "action": "close",
            "session_id": "run-1"
        })
    );
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
    assert_eq!(name, tools::UNIFIED_EXEC);
    assert_eq!(args.get("action"), Some(&serde_json::json!("run")));
    assert_eq!(args.get("tty"), Some(&serde_json::json!(true)));
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
    assert_eq!(name, tools::LIST_FILES);
}
