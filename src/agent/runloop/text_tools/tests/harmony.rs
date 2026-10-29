use super::*;

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
    let message =
        "<|start|>assistant<|channel|>commentary to=container.exec<|message|>{\"cmd\":[\"ls\", \"-la\"]}<|call|>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse harmony format");
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(args["command"], serde_json::json!(["ls", "-la"]));
}
#[test]
fn test_parse_harmony_channel_tool_call_with_string_cmd() {
    let message = "<|start|>assistant<|channel|>commentary to=bash<|message|>{\"cmd\":\"pwd\"}<|call|>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse harmony format");
    assert_eq!(name, tools::EXEC_COMMAND);
    assert_eq!(args["command"], serde_json::json!("pwd"));
}
#[test]
fn test_parse_harmony_channel_tool_call_with_recipient_before_channel() {
    let message = "<|start|>assistant to=functions.code_search<|channel|>commentary <|constrain|>json<|message|>{\"query\":\"Widget\",\"path\":\"src\",\"result_types\":[\"definition\"]}<|call|>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse harmony format");
    assert_eq!(name, tools::CODE_SEARCH);
    assert_eq!(args["query"], serde_json::json!("Widget"));
    assert_eq!(args["path"], serde_json::json!("src"));
    assert_eq!(args["result_types"], serde_json::json!(["definition"]));
}
#[test]
fn test_parse_harmony_channel_tool_call_tolerates_single_quoted_payload() {
    let message = "<|start|>assistant to=functions.code_search<|channel|>commentary <|constrain|>json<|message|>{'query':'Widget','path':'src','result_types':['definition']}<|call|>";
    let (name, args) = detect_textual_tool_call(message).expect("should parse harmony format");
    assert_eq!(name, tools::CODE_SEARCH);
    assert_eq!(args["query"], serde_json::json!("Widget"));
    assert_eq!(args["path"], serde_json::json!("src"));
    assert_eq!(args["result_types"], serde_json::json!(["definition"]));
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
    assert_eq!(result.unwrap_err(), "command array must contain only strings");
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
    let message = "<|start|>assistant<|channel|>commentary to=bash<|message|>{\"cmd\":\"\"}<|call|>";
    let result = parse_channel::parse_channel_tool_call(message);
    assert!(result.is_none(), "Should reject Harmony format with empty command");
}
#[test]
fn test_parse_harmony_channel_rejects_empty_array() {
    // Harmony format parser: should reject tool call if command array is empty
    let message = "<|start|>assistant<|channel|>commentary to=container.exec<|message|>{\"cmd\":[]}<|call|>";
    let result = parse_channel::parse_channel_tool_call(message);
    assert!(result.is_none(), "Should reject Harmony format with empty array");
}
#[test]
fn test_parse_harmony_channel_rejects_whitespace_command() {
    // Harmony format parser: should reject tool call if command is whitespace-only
    let message = "<|start|>assistant<|channel|>commentary to=bash<|message|>{\"cmd\":\"   \"}<|call|>";
    let result = parse_channel::parse_channel_tool_call(message);
    assert!(result.is_none(), "Should reject Harmony format with whitespace-only command");
}
