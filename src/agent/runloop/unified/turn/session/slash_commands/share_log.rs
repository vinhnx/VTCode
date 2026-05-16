use std::sync::LazyLock;

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{Value, json};
use vtcode_commons::sanitizer::redact_secrets;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::file_utils::write_file_with_context_sync;

use crate::agent::runloop::slash_commands::SessionLogExportFormat;

use super::{SlashCommandContext, SlashCommandControl};

#[path = "share_log/timeline.rs"]
mod timeline;

use timeline::{build_timeline_export, redact_timeline_export, render_session_timeline_html};

static EMAIL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[A-Z0-9._%+\-]+@[A-Z0-9.\-]+\.[A-Z]{2,}\b").expect("valid email regex")
});
static USER_PATH_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<prefix>/(?:Users|home)/)[^/\s]+").expect("valid user path regex")
});

fn build_session_log_messages(messages: &[uni::Message]) -> Vec<Value> {
    messages
        .iter()
        .map(|msg| {
            let mut entry = json!({
                "role": format!("{:?}", msg.role),
                "content": msg.content.as_text(),
            });
            if let Some(tool_calls) = &msg.tool_calls {
                let calls: Vec<Value> = tool_calls
                    .iter()
                    .map(|tc| {
                        json!({
                            "id": tc.id,
                            "function": tc.function.as_ref().map(|f| json!({
                                "name": f.name,
                                "arguments": f.arguments,
                            })),
                        })
                    })
                    .collect();
                entry["tool_calls"] = Value::Array(calls);
            }
            if let Some(tool_call_id) = &msg.tool_call_id {
                entry["tool_call_id"] = Value::String(tool_call_id.clone());
            }
            entry
        })
        .collect()
}

fn render_session_log_markdown(
    exported_at: &str,
    model: &str,
    workspace: &std::path::Path,
    messages: &[Value],
) -> String {
    let mut markdown = String::new();
    markdown.push_str("# VT Code Session Log\n\n");
    markdown.push_str(&format!("- Exported at: {}\n", exported_at));
    markdown.push_str(&format!("- Model: `{}`\n", model));
    markdown.push_str(&format!(
        "- Workspace: `{}`\n",
        redact_sensitive_text(&workspace.display().to_string())
    ));
    markdown.push_str(&format!("- Total messages: {}\n\n", messages.len()));
    markdown.push_str("## Messages\n\n");

    for (index, message) in messages.iter().enumerate() {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("Unknown");
        let content = message.get("content").and_then(Value::as_str).unwrap_or("");

        markdown.push_str(&format!("### {}. {}\n\n", index + 1, role));
        if content.trim().is_empty() {
            markdown.push_str("_No textual content._\n\n");
        } else {
            markdown.push_str("```text\n");
            markdown.push_str(content);
            if !content.ends_with('\n') {
                markdown.push('\n');
            }
            markdown.push_str("```\n\n");
        }

        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array)
            && !tool_calls.is_empty()
        {
            markdown.push_str("Tool calls:\n");
            for call in tool_calls {
                let id = call.get("id").and_then(Value::as_str).unwrap_or("unknown");
                let function = call.get("function");
                let function_name = function
                    .and_then(|value| value.get("name"))
                    .and_then(Value::as_str)
                    .map(canonical_tool_name)
                    .unwrap_or_else(|| "unknown".to_string());
                markdown.push_str(&format!("- `{}`: `{}`\n", id, function_name));

                if let Some(arguments) = function.and_then(|value| value.get("arguments")) {
                    let arguments_text = serde_json::to_string_pretty(arguments)
                        .unwrap_or_else(|_| arguments.to_string());
                    markdown.push_str("```json\n");
                    markdown.push_str(&arguments_text);
                    markdown.push_str("\n```\n");
                }
            }
            markdown.push('\n');
        }

        if let Some(tool_call_id) = message.get("tool_call_id").and_then(Value::as_str) {
            markdown.push_str(&format!("Tool call id: `{}`\n\n", tool_call_id));
        }
    }

    markdown
}

fn redact_sensitive_text(input: &str) -> String {
    let mut redacted = redact_secrets(input.to_string());

    if let Some(home_dir) = std::env::var_os("HOME")
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.is_empty())
    {
        redacted = redacted.replace(&home_dir, "~");
    }

    redacted = USER_PATH_REGEX
        .replace_all(&redacted, "${prefix}[REDACTED]")
        .into_owned();
    EMAIL_REGEX
        .replace_all(&redacted, "[REDACTED_EMAIL]")
        .into_owned()
}

fn redact_json_value(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(redact_sensitive_text(text)),
        Value::Array(items) => Value::Array(items.iter().map(redact_json_value).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), redact_json_value(value)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn canonical_tool_name(name: &str) -> String {
    vtcode_core::tools::tool_intent::canonical_unified_exec_tool_name(name)
        .unwrap_or(name)
        .to_string()
}

pub(crate) async fn handle_share_log(
    ctx: SlashCommandContext<'_>,
    format: SessionLogExportFormat,
) -> Result<SlashCommandControl> {
    use chrono::Local;

    let exported_at = Local::now().to_rfc3339();
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    let log_messages = build_session_log_messages(ctx.conversation_history);
    let redacted_log_messages: Vec<Value> = log_messages.iter().map(redact_json_value).collect();
    let thread_events = ctx.thread_handle.replay_recent();
    let redacted_session_log_export = json!({
        "exported_at": exported_at,
        "provider": ctx.provider_client.name(),
        "model": &ctx.config.model,
        "workspace": redact_sensitive_text(&ctx.config.workspace.display().to_string()),
        "redaction_enabled": true,
        "total_messages": redacted_log_messages.len(),
        "messages": redacted_log_messages,
    });
    let json_output_path = ctx
        .config
        .workspace
        .join(format!("vtcode-session-log-{}.json", timestamp));
    let markdown_output_path = ctx
        .config
        .workspace
        .join(format!("vtcode-session-log-{}.md", timestamp));
    let html_output_path = ctx
        .config
        .workspace
        .join(format!("vtcode-session-timeline-{}.html", timestamp));

    if matches!(
        format,
        SessionLogExportFormat::Both | SessionLogExportFormat::Json
    ) {
        let json = serde_json::to_string_pretty(&redacted_session_log_export)
            .context("Failed to serialize session log")?;
        write_file_with_context_sync(&json_output_path, &json, "session log")?;
    }

    if matches!(format, SessionLogExportFormat::Markdown) {
        let markdown = render_session_log_markdown(
            &exported_at,
            &ctx.config.model,
            &ctx.config.workspace,
            redacted_session_log_export
                .get("messages")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
        );
        write_file_with_context_sync(&markdown_output_path, &markdown, "session log")?;
    }

    if matches!(
        format,
        SessionLogExportFormat::Both | SessionLogExportFormat::Html
    ) {
        let timeline_export = redact_timeline_export(&build_timeline_export(
            &exported_at,
            ctx.provider_client.name(),
            &ctx.config.model,
            &ctx.config.workspace,
            ctx.thread_id,
            &thread_events,
            ctx.conversation_history,
            Some(&ctx.session_stats.prompt_cache_diagnostics()),
        ));
        let html = render_session_timeline_html(&timeline_export, &redacted_session_log_export)?;
        write_file_with_context_sync(&html_output_path, &html, "session timeline")?;
    }

    match format {
        SessionLogExportFormat::Both => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Share exports ready:\nJSON: {}\nHTML: {}\nHTML is self-contained for offline sharing; JSON is useful for debugging.",
                    json_output_path.display(),
                    html_output_path.display()
                ),
            )?;
        }
        SessionLogExportFormat::Html => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Share HTML ready:\n{}\nThis HTML file is self-contained and can be shared offline.",
                    html_output_path.display()
                ),
            )?;
        }
        SessionLogExportFormat::Json => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Share JSON ready:\n{}\nYou can share this file for debugging purposes.",
                    json_output_path.display()
                ),
            )?;
        }
        SessionLogExportFormat::Markdown => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Session log exported to: {} ({})",
                    markdown_output_path.display(),
                    "Markdown"
                ),
            )?;
            ctx.renderer.line(
                MessageStyle::Info,
                "You can share this file for debugging purposes.",
            )?;
        }
    }

    Ok(SlashCommandControl::Continue)
}
