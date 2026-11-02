use super::*;

pub(super) fn parse_observed_tool_call(update: &Value) -> Option<CopilotObservedToolCall> {
    let tool_call_id = update.get("toolCallId")?.as_str()?.to_string();
    let tool_name = derived_copilot_tool_name(
        update.get("title").and_then(Value::as_str),
        update.get("kind").and_then(Value::as_str),
    );
    let status = parse_observed_tool_status(
        update.get("status").and_then(Value::as_str),
        update.get("sessionUpdate").and_then(Value::as_str),
    );
    let arguments = update.get("rawInput").cloned();
    let output = extract_observed_tool_output(update);
    let terminal_id = extract_tool_call_terminal_id(update.get("content"));

    Some(CopilotObservedToolCall {
        tool_call_id,
        tool_name,
        status,
        arguments,
        output,
        terminal_id,
    })
}

fn parse_observed_tool_status(status: Option<&str>, session_update: Option<&str>) -> CopilotObservedToolCallStatus {
    match status.unwrap_or_else(|| {
        if session_update == Some("tool_call") {
            "pending"
        } else {
            "in_progress"
        }
    }) {
        "pending" => CopilotObservedToolCallStatus::Pending,
        "in_progress" => CopilotObservedToolCallStatus::InProgress,
        "completed" => CopilotObservedToolCallStatus::Completed,
        "failed" => CopilotObservedToolCallStatus::Failed,
        _ => CopilotObservedToolCallStatus::InProgress,
    }
}

fn extract_observed_tool_output(update: &Value) -> Option<String> {
    update
        .get("rawOutput")
        .and_then(extract_observed_tool_raw_output)
        .or_else(|| extract_tool_call_content_text(update.get("content")))
}

fn extract_observed_tool_raw_output(raw_output: &Value) -> Option<String> {
    match raw_output {
        Value::String(text) => Some(text.clone()).filter(|text| !text.trim().is_empty()),
        Value::Object(object) => object
            .get("content")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                object
                    .get("detailedContent")
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                    .map(ToString::to_string)
            })
            .or_else(|| Some(json_to_string_pretty(raw_output))),
        _ => Some(json_to_string_pretty(raw_output)),
    }
}

fn extract_tool_call_content_text(content: Option<&Value>) -> Option<String> {
    content.and_then(Value::as_array).into_iter().flatten().find_map(|item| {
        item.get("content").and_then(|content| {
            content
                .get("type")
                .and_then(Value::as_str)
                .filter(|value| *value == "text")
                .and_then(|_| content.get("text"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
    })
}

fn extract_tool_call_terminal_id(content: Option<&Value>) -> Option<String> {
    content.and_then(Value::as_array).into_iter().flatten().find_map(|item| {
        item.get("content").and_then(|content| {
            content
                .get("type")
                .and_then(Value::as_str)
                .filter(|value| *value == "terminal")
                .and_then(|_| content.get("terminalId"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
    })
}
