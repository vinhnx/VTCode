use crate::agent::runloop::ResumeSession;
use anyhow::Result;
use chrono::Local;
use hashbrown::HashMap;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

pub(super) fn render_resume_state_if_present(
    renderer: &mut AnsiRenderer,
    resume_state: Option<&ResumeSession>,
    supports_reasoning: bool,
) -> Result<()> {
    let Some(session) = resume_state else {
        return Ok(());
    };

    let ended_local = session
        .snapshot()
        .ended_at
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M");
    let action = if session.is_fork() {
        "Forking"
    } else {
        "Resuming"
    };
    renderer.line(
        MessageStyle::Info,
        &format!(
            "{} session {} · ended {} · {} messages",
            action,
            session.identifier(),
            ended_local,
            session.message_count()
        ),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!("Previous archive: {}", session.path().display()),
    )?;
    if session.is_fork() {
        renderer.line(MessageStyle::Info, "Starting independent forked session")?;
    }

    if !session.history().is_empty() {
        renderer.line(MessageStyle::Info, "Conversation history:")?;
        let lines = build_structured_resume_lines(session.history(), supports_reasoning);
        render_resume_lines(renderer, &lines)?;
    } else if !session.snapshot().transcript.is_empty() {
        renderer.line(
            MessageStyle::Info,
            "Conversation history (legacy transcript):",
        )?;
        let lines = build_legacy_resume_lines(&session.snapshot().transcript);
        render_resume_lines(renderer, &lines)?;
    }
    renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResumeRenderLine {
    pub(super) style: MessageStyle,
    pub(super) text: String,
}

impl ResumeRenderLine {
    fn new(style: MessageStyle, text: impl Into<String>) -> Self {
        Self {
            style,
            text: text.into(),
        }
    }
}

pub(crate) fn render_resume_lines(
    renderer: &mut AnsiRenderer,
    lines: &[ResumeRenderLine],
) -> Result<()> {
    for line in lines {
        renderer.line(line.style, &line.text)?;
    }
    Ok(())
}

pub(crate) fn build_structured_resume_lines(
    history: &[uni::Message],
    supports_reasoning: bool,
) -> Vec<ResumeRenderLine> {
    let mut lines = Vec::new();
    let mut tool_name_by_call_id: HashMap<String, String> = HashMap::new();

    for (index, message) in history.iter().enumerate() {
        if index > 0 {
            push_resume_spacing(&mut lines);
        }
        match message.role {
            uni::MessageRole::User => {
                push_content_lines(&mut lines, MessageStyle::User, &message.content);
            }
            uni::MessageRole::Assistant => {
                let mut rendered_any = false;

                if let Some(tool_calls) = &message.tool_calls {
                    for tool_call in tool_calls {
                        rendered_any = true;
                        let tool_name = tool_call
                            .function
                            .as_ref()
                            .map(|function| function.name.clone())
                            .unwrap_or_else(|| "unknown".to_string());
                        if !tool_call.id.trim().is_empty() {
                            tool_name_by_call_id.insert(tool_call.id.clone(), tool_name.clone());
                        }

                        lines.push(ResumeRenderLine::new(
                            MessageStyle::Tool,
                            format_resume_tool_header(&tool_name, Some(tool_call.id.as_str())),
                        ));

                        if let Some(function) = &tool_call.function {
                            let args_block = format_tool_arguments_for_resume(&function.arguments);
                            if !args_block.is_empty() {
                                lines.push(ResumeRenderLine::new(
                                    MessageStyle::ToolDetail,
                                    args_block,
                                ));
                            }
                        } else if let Some(text) = tool_call.text.as_deref()
                            && !text.trim().is_empty()
                        {
                            lines.push(ResumeRenderLine::new(
                                MessageStyle::ToolDetail,
                                text.trim().to_string(),
                            ));
                        }
                    }
                }

                let reasoning_text = if supports_reasoning {
                    message
                        .reasoning
                        .as_deref()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_string)
                        .or_else(|| {
                            message
                                .reasoning_details
                                .as_deref()
                                .and_then(
                                    vtcode_core::llm::providers::common::extract_reasoning_text_from_detail_values,
                                )
                        })
                } else {
                    None
                };

                if let Some(reasoning) = reasoning_text {
                    rendered_any = true;
                    let compact = vtcode_commons::formatting::compact_reasoning_text(&reasoning);
                    lines.push(ResumeRenderLine::new(
                        MessageStyle::Reasoning,
                        if compact.trim().is_empty() {
                            reasoning
                        } else {
                            compact
                        },
                    ));
                }

                if let Some(content) = project_content_text(&message.content) {
                    rendered_any = true;
                    lines.push(ResumeRenderLine::new(MessageStyle::Response, content));
                }

                if !rendered_any {
                    lines.push(ResumeRenderLine::new(
                        MessageStyle::Response,
                        "Assistant: [no content]",
                    ));
                }
            }
            uni::MessageRole::Tool => {
                let call_id = message.tool_call_id.as_deref();
                let tool_name = call_id
                    .and_then(|id| tool_name_by_call_id.get(id))
                    .cloned()
                    .or_else(|| message.origin_tool.clone())
                    .unwrap_or_else(|| "tool".to_string());
                lines.push(ResumeRenderLine::new(
                    MessageStyle::Tool,
                    format_resume_tool_header(&tool_name, call_id),
                ));
                if let Some(formatted) = format_tool_output_for_resume(&message.content) {
                    lines.push(ResumeRenderLine::new(MessageStyle::ToolOutput, formatted));
                } else {
                    push_content_lines(&mut lines, MessageStyle::ToolOutput, &message.content);
                }
            }
            uni::MessageRole::System => {
                lines.push(ResumeRenderLine::new(MessageStyle::Info, "System:"));
                push_content_lines(&mut lines, MessageStyle::Info, &message.content);
            }
        }
    }

    lines
}

fn format_resume_tool_header(tool_name: &str, tool_call_id: Option<&str>) -> String {
    match tool_call_id {
        Some(id) if !id.trim().is_empty() && tool_name.trim().eq_ignore_ascii_case("tool") => {
            format!("Tool [tool_call_id: {id}]:")
        }
        Some(id) if !id.trim().is_empty() => {
            format!("Tool {tool_name} [tool_call_id: {id}]:")
        }
        _ if tool_name.trim().eq_ignore_ascii_case("tool") => "Tool:".to_string(),
        _ => format!("Tool {tool_name}:"),
    }
}

fn format_tool_arguments_for_resume(arguments: &str) -> String {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return format!("```text\n{trimmed}\n```");
    };

    let Some(obj) = value.as_object() else {
        return serde_json::to_string_pretty(&value)
            .map(|pretty| format!("```json\n{pretty}\n```"))
            .unwrap_or_else(|_| format!("```json\n{trimmed}\n```"));
    };

    // Build a concise summary from key fields instead of showing raw JSON
    let mut summary_parts = Vec::new();

    // For command_session: show command
    if let Some(cmd) = obj
        .get("command")
        .or_else(|| obj.get("cmd"))
        .and_then(|v| v.as_str())
    {
        summary_parts.push(format!("command: {cmd}"));
    }
    // For file_operation / task_tracker: show action and related fields
    if let Some(action) = obj.get("action").and_then(|v| v.as_str()) {
        summary_parts.push(format!("action: {action}"));
        if let Some(path) = obj.get("path").and_then(|v| v.as_str()) {
            summary_parts.push(format!("path: {path}"));
        }
        if let Some(subject) = obj.get("subject").and_then(|v| v.as_str()) {
            summary_parts.push(format!("subject: {subject}"));
        }
        if let Some(status) = obj.get("status").and_then(|v| v.as_str()) {
            summary_parts.push(format!("status: {status}"));
        }
    }
    // For code_search: show query.
    if let Some(query) = obj.get("query").and_then(|v| v.as_str()) {
        summary_parts.push(format!("query: {query}"));
    }
    // For command_session: show session_id if present
    if let Some(sid) = obj.get("session_id").and_then(|v| v.as_str()) {
        summary_parts.push(format!("session: {sid}"));
    }

    if !summary_parts.is_empty() {
        return format!("[{}]", summary_parts.join(", "));
    }

    // Fallback: show pretty-printed JSON in a code block
    serde_json::to_string_pretty(&value)
        .map(|pretty| format!("```json\n{pretty}\n```"))
        .unwrap_or_else(|_| format!("```json\n{trimmed}\n```"))
}

/// Format tool output content for resume display.
///
/// Tool responses are stored as JSON objects with fields like `output`, `error`,
/// `exit_code`, `failure_kind`, etc. This function extracts the meaningful parts
/// and renders them as human-readable text instead of showing raw JSON.
fn format_tool_output_for_resume(content: &uni::MessageContent) -> Option<String> {
    let text = project_content_text(content)?;
    let trimmed = text.trim();

    let value = serde_json::from_str::<serde_json::Value>(trimmed).ok()?;

    let obj = value.as_object()?;

    // Error responses: {"error":"...","failure_kind":"...","error_class":"...","is_recoverable":...}
    if let Some(error) = obj.get("error").and_then(|v| v.as_str()) {
        let mut parts = vec![format!("Error: {}", error)];
        if let Some(kind) = obj.get("failure_kind").and_then(|v| v.as_str()) {
            parts.push(format!("Kind: {kind}"));
        }
        if let Some(class) = obj.get("error_class").and_then(|v| v.as_str()) {
            parts.push(format!("Class: {class}"));
        }
        if let Some(recoverable) = obj.get("is_recoverable").and_then(|v| v.as_bool()) {
            parts.push(format!("Recoverable: {recoverable}"));
        }
        return Some(parts.join("\n"));
    }

    // Success responses with output: {"output":"...","exit_code":0,"backend":"pipe"}
    if obj.contains_key("output") || obj.contains_key("exit_code") {
        let mut parts = Vec::new();

        if let Some(output) = obj.get("output").and_then(|v| v.as_str()) {
            let trimmed_output = output.trim();
            if !trimmed_output.is_empty() {
                // Truncate very long output to keep resume display concise
                let display = if trimmed_output.len() > 500 {
                    let mut end = 500;
                    while !trimmed_output.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}...", &trimmed_output[..end])
                } else {
                    trimmed_output.to_string()
                };
                parts.push(display);
            }
        }

        if let Some(code) = obj.get("exit_code").and_then(|v| v.as_i64())
            && code != 0
        {
            parts.push(format!("Exit code: {code}"));
        }

        if parts.is_empty() {
            return None;
        }
        return Some(parts.join("\n"));
    }

    // Unknown JSON structure - fall back to raw display
    None
}

fn push_resume_spacing(lines: &mut Vec<ResumeRenderLine>) {
    if lines.last().is_none_or(|line| !line.text.is_empty()) {
        lines.push(ResumeRenderLine::new(MessageStyle::Info, ""));
    }
}

fn push_content_lines(
    lines: &mut Vec<ResumeRenderLine>,
    style: MessageStyle,
    content: &uni::MessageContent,
) {
    if let Some(projected) = project_content_text(content) {
        lines.push(ResumeRenderLine::new(style, projected));
    } else {
        lines.push(ResumeRenderLine::new(style, "[no textual content]"));
    }
}

fn project_content_text(content: &uni::MessageContent) -> Option<String> {
    match content {
        uni::MessageContent::Text(text) => (!text.trim().is_empty()).then(|| text.clone()),
        uni::MessageContent::Parts(parts) => {
            let mut fragments = Vec::new();
            for part in parts {
                match part {
                    uni::ContentPart::Text { text } => {
                        if !text.trim().is_empty() {
                            fragments.push(text.clone());
                        }
                    }
                    uni::ContentPart::Image { mime_type, .. } => {
                        fragments.push(format!("[image content: {mime_type}]"));
                    }
                    uni::ContentPart::File {
                        filename,
                        file_id,
                        file_url,
                        ..
                    } => {
                        if let Some(name) = filename {
                            fragments.push(format!("[file attachment: {name}]"));
                        } else if let Some(id) = file_id {
                            fragments.push(format!("[file attachment id: {id}]"));
                        } else if let Some(url) = file_url {
                            fragments.push(format!("[file attachment url: {url}]"));
                        } else {
                            fragments.push("[file attachment]".to_string());
                        }
                    }
                }
            }

            (!fragments.is_empty()).then(|| fragments.join("\n"))
        }
    }
}

fn build_legacy_resume_lines(transcript: &[String]) -> Vec<ResumeRenderLine> {
    transcript
        .iter()
        .map(|line| ResumeRenderLine::new(infer_legacy_line_style(line), line.clone()))
        .collect()
}

pub(super) fn infer_legacy_line_style(line: &str) -> MessageStyle {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return MessageStyle::Info;
    }

    if trimmed.contains("You:") {
        return MessageStyle::User;
    }
    if trimmed.contains("Assistant:") {
        return MessageStyle::Response;
    }
    if trimmed.contains("System:") {
        return MessageStyle::Info;
    }
    if trimmed.contains("Tool ")
        || trimmed.contains("[tool_call_id:")
        || trimmed.contains("\"tool_call_id\"")
    {
        return MessageStyle::ToolOutput;
    }
    MessageStyle::Info
}
