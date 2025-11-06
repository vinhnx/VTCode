mod commands;
mod files;
mod git_diff;
mod mcp;
mod panels;
mod plan;
mod streams;
mod styles;

pub(crate) use streams::render_code_fence_blocks;

use anyhow::Result;
use serde_json::Value;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::mcp::McpRendererProfile;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use commands::{render_curl_result, render_terminal_command_panel};
use files::{render_list_dir_output, render_read_file_output, render_write_file_preview};
use git_diff::render_git_diff;
use mcp::{
    render_context7_output, render_generic_output, render_sequential_output,
    resolve_renderer_profile,
};
use plan::render_plan_update;
use streams::{render_stream_section, resolve_stdout_tail_limit};
use styles::{GitStyles, LsStyles};

pub(crate) fn render_tool_output(
    renderer: &mut AnsiRenderer,
    tool_name: Option<&str>,
    val: &Value,
    vt_config: Option<&VTCodeConfig>,
) -> Result<()> {
    let allow_tool_ansi = vt_config.map(|cfg| cfg.ui.allow_tool_ansi).unwrap_or(false);

    match tool_name {
        Some(tools::UPDATE_PLAN) => return render_plan_update(renderer, val),
        Some(tools::WRITE_FILE) | Some(tools::CREATE_FILE) => {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            return render_write_file_preview(renderer, val, &git_styles, &ls_styles);
        }
        Some(tools::GIT_DIFF) => {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            let output_mode = vt_config
                .map(|cfg| cfg.ui.tool_output_mode)
                .unwrap_or(ToolOutputMode::Compact);
            let tail_limit = resolve_stdout_tail_limit(vt_config);
            return render_git_diff(
                renderer,
                val,
                output_mode,
                tail_limit,
                &git_styles,
                &ls_styles,
                allow_tool_ansi,
                vt_config,
            );
        }
        Some(tools::RUN_COMMAND) => {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            return render_terminal_command_panel(
                renderer,
                val,
                &git_styles,
                &ls_styles,
                vt_config,
                allow_tool_ansi,
            );
        }
        Some(tools::CURL) => {
            let output_mode = vt_config
                .map(|cfg| cfg.ui.tool_output_mode)
                .unwrap_or(ToolOutputMode::Compact);
            let tail_limit = resolve_stdout_tail_limit(vt_config);
            return render_curl_result(
                renderer,
                val,
                output_mode,
                tail_limit,
                allow_tool_ansi,
                vt_config,
            );
        }
        Some(tools::LIST_FILES) => {
            let ls_styles = LsStyles::from_env();
            return render_list_dir_output(renderer, val, &ls_styles);
        }
        Some(tools::READ_FILE) => {
            return render_read_file_output(renderer, val);
        }
        _ => {}
    }

    render_simple_tool_status(renderer, tool_name, val)?;

    if let Some(notice) = val.get("security_notice").and_then(Value::as_str) {
        renderer.line(MessageStyle::Info, notice)?;
    }

    if let Some(tool) = tool_name
        && tool.starts_with("mcp_")
    {
        if let Some(profile) = resolve_renderer_profile(tool, vt_config) {
            match profile {
                McpRendererProfile::Context7 => render_context7_output(renderer, val)?,
                McpRendererProfile::SequentialThinking => render_sequential_output(renderer, val)?,
            }
        } else {
            render_generic_output(renderer, val)?;
        }
    }

    let output_mode = vt_config
        .map(|cfg| cfg.ui.tool_output_mode)
        .unwrap_or(ToolOutputMode::Compact);
    let tail_limit = resolve_stdout_tail_limit(vt_config);
    let git_styles = GitStyles::new();
    let ls_styles = LsStyles::from_env();

    // PTY tools use "output" field instead of "stdout"
    if let Some(output) = val.get("output").and_then(Value::as_str) {
        render_stream_section(
            renderer,
            "",
            output,
            output_mode,
            tail_limit,
            tool_name,
            &git_styles,
            &ls_styles,
            MessageStyle::Response,
            allow_tool_ansi,
            vt_config,
        )?;
    } else if let Some(stdout) = val.get("stdout").and_then(Value::as_str) {
        render_stream_section(
            renderer,
            "stdout",
            stdout,
            output_mode,
            tail_limit,
            tool_name,
            &git_styles,
            &ls_styles,
            MessageStyle::Response,
            allow_tool_ansi,
            vt_config,
        )?;
    }
    if let Some(stderr) = val.get("stderr").and_then(Value::as_str) {
        render_stream_section(
            renderer,
            "stderr",
            stderr,
            output_mode,
            tail_limit,
            tool_name,
            &git_styles,
            &ls_styles,
            MessageStyle::Error,
            allow_tool_ansi,
            vt_config,
        )?;
    }
    Ok(())
}

fn render_simple_tool_status(
    renderer: &mut AnsiRenderer,
    _tool_name: Option<&str>,
    val: &Value,
) -> Result<()> {
    let has_error = val.get("error").is_some() || val.get("error_type").is_some();

    if has_error {
        render_error_details(renderer, val)?;
    }

    Ok(())
}

fn render_error_details(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    if let Some(error_msg) = val.get("message").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::Error, &format!("  Error: {}", error_msg))?;
    }

    if let Some(error_type) = val.get("error_type").and_then(|v| v.as_str()) {
        let type_description = match error_type {
            "InvalidParameters" => "Invalid parameters provided",
            "ToolNotFound" => "Tool not found",
            "ResourceNotFound" => "Resource not found",
            "PermissionDenied" => "Permission denied",
            "ExecutionError" => "Execution error",
            "PolicyViolation" => "Policy violation",
            "Timeout" => "Operation timed out",
            "NetworkError" => "Network error",
            "EncodingError" => "Encoding error",
            "FileSystemError" => "File system error",
            _ => error_type,
        };
        renderer.line(MessageStyle::Info, &format!("  Type: {}", type_description))?;
    }

    if let Some(original) = val.get("original_error").and_then(|v| v.as_str())
        && !original.trim().is_empty()
    {
        let display_error = if original.len() > 200 {
            format!("{}...", &original[..197])
        } else {
            original.to_string()
        };
        renderer.line(MessageStyle::Info, &format!("  Details: {}", display_error))?;
    }

    if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::Info, &format!("  Path: {}", path))?;
    }

    if let Some(line) = val.get("line").and_then(|v| v.as_u64()) {
        if let Some(col) = val.get("column").and_then(|v| v.as_u64()) {
            renderer.line(
                MessageStyle::Info,
                &format!("  Location: line {}, column {}", line, col),
            )?;
        } else {
            renderer.line(MessageStyle::Info, &format!("  Location: line {}", line))?;
        }
    }

    if let Some(suggestions) = val.get("recovery_suggestions").and_then(|v| v.as_array())
        && !suggestions.is_empty()
    {
        renderer.line(MessageStyle::Info, "")?;
        renderer.line(MessageStyle::Info, "  Suggestions:")?;
        for (idx, suggestion) in suggestions.iter().take(5).enumerate() {
            if let Some(text) = suggestion.as_str() {
                renderer.line(MessageStyle::Info, &format!("    {}. {}", idx + 1, text))?;
            }
        }
        if suggestions.len() > 5 {
            renderer.line(
                MessageStyle::Info,
                &format!("    ... and {} more", suggestions.len() - 5),
            )?;
        }
    }

    Ok(())
}
