use super::*;

#[test]
fn test_detect_tagged_tool_call_parses_basic_command() {
    let message = "<tool_call>run_pty_cmd\n<arg_key>command\n<arg_value>ls -a\n</tool_call>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::EXEC_COMMAND);
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
fn test_detect_tagged_tool_call_parses_cmd_arg_with_prose_preamble() {
    // Exact shape from the approved-plan recovery failure: a prose preamble
    // followed by a complete `<tool_call>` block using the `cmd` alias. The
    // recovery guard relies on this parsing successfully to distinguish an
    // action attempt (contract violation) from stray markup.
    let message = "•   I have sufficient evidence from prior reads. Applying the section rewrite now.\
                   <tool_call>exec_command<arg_key>cmd\n\
                   </arg_key><arg_value>grep -n \"## Why VT Code\" README.md</arg_value></tool_call>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse complete tagged call");
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(args["cmd"], serde_json::json!("grep -n \"## Why VT Code\" README.md"));
    assert!(contains_pseudo_tool_call_markers(message));
    let stripped = strip_textual_tool_call_regions(message);
    assert!(detect_textual_tool_call(&stripped).is_none());
    assert!(!stripped.contains("<tool_call>"));
    assert!(stripped.contains("Applying the section rewrite"));
}
#[test]
fn test_detect_tagged_tool_call_respects_indexed_arguments() {
    let message = "<tool_call>run_pty_cmd\n<arg_key>command.0\n<arg_value>python\n<arg_key>command.1\n<arg_value>-c\n<arg_key>command.2\n<arg_value>print('hi')\n</tool_call>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::EXEC_COMMAND);
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
    let message =
        "<tool_call>run_pty_cmd\n<arg_key>command.1\n<arg_value>ls\n<arg_key>command.2\n<arg_value>-a\n</tool_call>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::EXEC_COMMAND);
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
<invoke name="read_file">
<parameter name="action">read</parameter>
<parameter name="path">crates/codegen/vtcode-core/src/core/agent/runtime/mod.rs</parameter>
<parameter name="offset">1</parameter>
<parameter name="limit">400</parameter>
</invoke>
</minimax:tool_call>
"#;
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::READ_FILE);
    assert_eq!(
        args,
        serde_json::json!({
            "action": "read",
            "path": "crates/codegen/vtcode-core/src/core/agent/runtime/mod.rs",
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
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(args, serde_json::json!({ "action": "list" }));
}
#[test]
fn test_detect_tagged_tool_call_parses_tools_call_dialect_inside_tool_call_wrapper() {
    // Regression: minimax-m3:cloud (ollama-cloud, no tool schemas on the wire)
    // emitted `<tools:call>` blocks inside a `<tool_call>` envelope in plan
    // mode; the payload previously leaked as a raw-XML final answer (turn_887).
    let message = r#"I'll research the codebase to understand current launch flow, then draft a focused plan.
<tool_call>
<tools:call name="exec_command">
<parameter name="cmd">ls -la /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode</parameter>
</tools:call>
<tools:call name="code_search">
<parameter name="pattern">fn main</parameter>
<parameter name="path">/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode</parameter>
<parameter name="max_results">30</parameter>
<parameter name="lang">rust</parameter>
</tools:call>
</tool_call>"#;
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(
        args,
        serde_json::json!({
            "cmd": "ls -la /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode",
            "action": "run"
        })
    );
}
#[test]
fn test_detect_tagged_tool_call_parses_bare_tools_call_dialect() {
    // Second <tools:call> block from the turn_887 payload, without the wrapper.
    let message = r#"<tools:call name="code_search">
<parameter name="pattern">async fn main|fn main()</parameter>
<parameter name="path">/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode</parameter>
<parameter name="max_results">30</parameter>
</tools:call>"#;
    let (name, args) = detect_textual_tool_call(message).expect("should parse");
    assert_eq!(name, tools::CODE_SEARCH);
    assert_eq!(
        args,
        serde_json::json!({
            "pattern": "async fn main|fn main()",
            "path": "/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode",
            "max_results": 30
        })
    );
}
