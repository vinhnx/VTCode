use anyhow::{Context, Result};
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::file_utils::write_file_with_context_sync;

use crate::agent::runloop::slash_commands::SessionLogExportFormat;

use super::{SlashCommandContext, SlashCommandControl};

fn build_session_log_messages(messages: &[uni::Message]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|msg| {
            let mut entry = serde_json::json!({
                "role": format!("{:?}", msg.role),
                "content": msg.content.as_text(),
            });
            if let Some(tool_calls) = &msg.tool_calls {
                let calls: Vec<serde_json::Value> = tool_calls
                    .iter()
                    .map(|tc| {
                        serde_json::json!({
                            "id": tc.id,
                            "function": tc.function.as_ref().map(|f| serde_json::json!({
                                "name": f.name,
                                "arguments": f.arguments,
                            })),
                        })
                    })
                    .collect();
                entry["tool_calls"] = serde_json::Value::Array(calls);
            }
            if let Some(tool_call_id) = &msg.tool_call_id {
                entry["tool_call_id"] = serde_json::Value::String(tool_call_id.clone());
            }
            entry
        })
        .collect()
}

fn render_session_log_markdown(
    exported_at: &str,
    model: &str,
    workspace: &std::path::Path,
    messages: &[serde_json::Value],
) -> String {
    let mut markdown = String::new();
    markdown.push_str("# VT Code Session Log\n\n");
    markdown.push_str(&format!("- Exported at: {}\n", exported_at));
    markdown.push_str(&format!("- Model: `{}`\n", model));
    markdown.push_str(&format!("- Workspace: `{}`\n", workspace.display()));
    markdown.push_str(&format!("- Total messages: {}\n\n", messages.len()));
    markdown.push_str("## Messages\n\n");

    for (index, message) in messages.iter().enumerate() {
        let role = message
            .get("role")
            .and_then(|value| value.as_str())
            .unwrap_or("Unknown");
        let content = message
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or("");

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

        if let Some(tool_calls) = message.get("tool_calls").and_then(|value| value.as_array())
            && !tool_calls.is_empty()
        {
            markdown.push_str("Tool calls:\n");
            for call in tool_calls {
                let id = call
                    .get("id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                let function = call.get("function");
                let function_name = function
                    .and_then(|value| value.get("name"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
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

        if let Some(tool_call_id) = message.get("tool_call_id").and_then(|value| value.as_str()) {
            markdown.push_str(&format!("Tool call id: `{}`\n\n", tool_call_id));
        }
    }

    markdown
}

pub async fn handle_share_log(
    ctx: SlashCommandContext<'_>,
    format: SessionLogExportFormat,
) -> Result<SlashCommandControl> {
    use chrono::Local;

    let exported_at = Local::now().to_rfc3339();
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    let extension = match format {
        SessionLogExportFormat::Json => "json",
        SessionLogExportFormat::Markdown => "md",
    };
    let filename = format!("vtcode-session-log-{}.{}", timestamp, extension);
    let output_path = ctx.config.workspace.join(&filename);

    let log_messages = build_session_log_messages(ctx.conversation_history);

    match format {
        SessionLogExportFormat::Json => {
            let export = serde_json::json!({
                "exported_at": exported_at,
                "model": &ctx.config.model,
                "workspace": ctx.config.workspace.display().to_string(),
                "total_messages": log_messages.len(),
                "messages": log_messages,
            });

            let json =
                serde_json::to_string_pretty(&export).context("Failed to serialize session log")?;
            write_file_with_context_sync(&output_path, &json, "session log")?;
        }
        SessionLogExportFormat::Markdown => {
            let markdown = render_session_log_markdown(
                &exported_at,
                &ctx.config.model,
                &ctx.config.workspace,
                &log_messages,
            );
            write_file_with_context_sync(&output_path, &markdown, "session log")?;
        }
    }

    let format_label = match format {
        SessionLogExportFormat::Json => "JSON",
        SessionLogExportFormat::Markdown => "Markdown",
    };
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Session log exported to: {} ({})",
            output_path.display(),
            format_label
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        "You can share this file for debugging purposes.",
    )?;

    Ok(SlashCommandControl::Continue)
}
