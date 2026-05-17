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
pub(super) struct ResumeRenderLine {
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

fn render_resume_lines(renderer: &mut AnsiRenderer, lines: &[ResumeRenderLine]) -> Result<()> {
    for line in lines {
        renderer.line(line.style, &line.text)?;
    }
    Ok(())
}

pub(super) fn build_structured_resume_lines(
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
                    lines.push(ResumeRenderLine::new(MessageStyle::Reasoning, reasoning));
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
                push_content_lines(&mut lines, MessageStyle::ToolOutput, &message.content);
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
    let tool_name = vtcode_core::tools::tool_intent::canonical_unified_exec_tool_name(tool_name)
        .unwrap_or(tool_name);
    match tool_call_id {
        Some(id) if !id.trim().is_empty() && tool_name.trim().eq_ignore_ascii_case("tool") => {
            format!("Tool [tool_call_id: {}]:", id)
        }
        Some(id) if !id.trim().is_empty() => {
            format!("Tool {} [tool_call_id: {}]:", tool_name, id)
        }
        _ if tool_name.trim().eq_ignore_ascii_case("tool") => "Tool:".to_string(),
        _ => format!("Tool {}:", tool_name),
    }
}

fn format_tool_arguments_for_resume(arguments: &str) -> String {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => serde_json::to_string_pretty(&value)
            .map(|pretty| format!("```json\n{}\n```", pretty))
            .unwrap_or_else(|_| format!("```json\n{}\n```", trimmed)),
        Err(_) => format!("```text\n{}\n```", trimmed),
    }
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
                        fragments.push(format!("[image content: {}]", mime_type));
                    }
                    uni::ContentPart::File {
                        filename,
                        file_id,
                        file_url,
                        ..
                    } => {
                        if let Some(name) = filename {
                            fragments.push(format!("[file attachment: {}]", name));
                        } else if let Some(id) = file_id {
                            fragments.push(format!("[file attachment id: {}]", id));
                        } else if let Some(url) = file_url {
                            fragments.push(format!("[file attachment url: {}]", url));
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
