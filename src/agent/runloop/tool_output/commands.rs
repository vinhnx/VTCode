use anyhow::Result;
use serde_json::Value;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::commands_processing::{parse_command_tokens, preprocess_terminal_stdout};
use super::streams::{render_stream_section, resolve_stdout_tail_limit};
use super::styles::{GitStyles, LsStyles};

fn resolve_pty_session_id(payload: &Value) -> Option<&str> {
    payload
        .get("id")
        .or_else(|| payload.get("session_id"))
        .or_else(|| payload.get("process_id"))
        .and_then(Value::as_str)
}

fn infer_pty_completion(payload: &Value, session_id: Option<&str>, exit_code: Option<i64>) -> bool {
    payload
        .get("is_exited")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| exit_code.is_some() || session_id.is_none())
}

fn should_render_command_prompt(is_pty_session: bool, command: &str) -> bool {
    is_pty_session && !command.trim().is_empty() && command != "unknown"
}

pub(crate) async fn render_terminal_command_panel(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
    vt_config: Option<&VTCodeConfig>,
    allow_ansi: bool,
) -> Result<()> {
    // Check if stdout is JSON containing command output (from execute_code tool)
    let mut stdout_raw = payload.get("stdout").and_then(Value::as_str).unwrap_or("");
    let mut stderr_raw = payload.get("stderr").and_then(Value::as_str).unwrap_or("");
    let mut unwrapped_payload = payload.clone();

    // If stdout looks like JSON with stdout/stderr/returncode, unwrap it
    if let Ok(inner_json) = serde_json::from_str::<Value>(stdout_raw)
        && (inner_json.get("stdout").is_some()
            || inner_json.get("stderr").is_some()
            || inner_json.get("returncode").is_some())
    {
        unwrapped_payload = inner_json;
        stdout_raw = unwrapped_payload
            .get("stdout")
            .and_then(Value::as_str)
            .unwrap_or("");
        stderr_raw = unwrapped_payload
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("");
    }

    let output_raw = unwrapped_payload
        .get("output")
        .and_then(Value::as_str)
        .unwrap_or("");
    let command_tokens = parse_command_tokens(&unwrapped_payload);
    let disable_spool = unwrapped_payload
        .get("no_spool")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let spooled_to_file = unwrapped_payload
        .get("spooled_to_file")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let spool_path = unwrapped_payload.get("spool_path").and_then(Value::as_str);

    // Check for session completion status (is_exited indicates if process is still running)
    let exit_code = unwrapped_payload.get("exit_code").and_then(Value::as_i64);
    let session_id = resolve_pty_session_id(&unwrapped_payload);
    let is_completed = infer_pty_completion(&unwrapped_payload, session_id, exit_code);
    let command = if let Some(tokens) = &command_tokens {
        tokens.join(" ")
    } else {
        unwrapped_payload
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string()
    };
    let working_dir = unwrapped_payload
        .get("working_directory")
        .and_then(Value::as_str);
    let rows = unwrapped_payload
        .get("rows")
        .and_then(Value::as_u64)
        .unwrap_or(24);
    let cols = unwrapped_payload
        .get("cols")
        .and_then(Value::as_u64)
        .unwrap_or(80);

    // If there's an 'output' field, this is likely a PTY session result
    let is_pty_session = session_id.is_some()
        && (!output_raw.is_empty() || stdout_raw.is_empty() && stderr_raw.is_empty());

    let stdout = if is_pty_session {
        preprocess_terminal_stdout(command_tokens.as_deref(), output_raw)
    } else {
        preprocess_terminal_stdout(command_tokens.as_deref(), stdout_raw)
    };
    let stderr = preprocess_terminal_stdout(command_tokens.as_deref(), stderr_raw);

    let output_mode = vt_config
        .map(|cfg| cfg.ui.tool_output_mode)
        .unwrap_or(ToolOutputMode::Compact);
    let tail_limit = resolve_stdout_tail_limit(vt_config);

    // Display session status header if this is a PTY session
    let mut command_prompt_rendered = false;
    if is_pty_session {
        let status_symbol = if !is_completed { "▶" } else { "✓" };
        let status_badge = if !is_completed {
            format!("{} RUN", status_symbol)
        } else {
            format!("{} OK", status_symbol)
        };

        // Compact header: status · command · session info
        let header = if working_dir.is_some() {
            format!(
                "{} · {} · {}x{}",
                status_badge,
                if command.len() > 40 {
                    format!("{}…", &command[..37])
                } else {
                    command.clone()
                },
                cols,
                rows
            )
        } else {
            format!(
                "{} · {}",
                status_badge,
                if command.len() > 50 {
                    format!("{}…", &command[..47])
                } else {
                    command.clone()
                }
            )
        };

        renderer.line(MessageStyle::Tool, &header)?;

        if should_render_command_prompt(is_pty_session, &command) {
            renderer.line(MessageStyle::ToolDetail, &format!("$ {}", command))?;
            command_prompt_rendered = true;
        }
    }

    // Render stdin if available (user input to the terminal) - simulating command prompt
    if let Some(stdin) = unwrapped_payload.get("stdin").and_then(Value::as_str)
        && !stdin.trim().is_empty()
    {
        let stdin_trimmed = stdin.trim();
        if !command_prompt_rendered || stdin_trimmed != command.trim() {
            // Show the input as if it came from a command prompt
            let prompt = format!("$ {}", stdin_trimmed);
            renderer.line(MessageStyle::ToolDetail, &prompt)?;
        }
    }

    // Special handling for exit code 127 (command not found) - show critical message prominently
    if is_completed && exit_code == Some(127) {
        let critical_note = unwrapped_payload
            .get("critical_note")
            .and_then(Value::as_str);
        let output_msg = unwrapped_payload.get("output").and_then(Value::as_str);

        if let Some(note) = critical_note {
            renderer.line(
                MessageStyle::ToolError,
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
            )?;
            renderer.line(
                MessageStyle::ToolError,
                "⤫  COMMAND NOT FOUND (EXIT CODE 127)",
            )?;
            renderer.line(
                MessageStyle::ToolError,
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
            )?;
            renderer.line(MessageStyle::ToolError, note)?;
            renderer.line(MessageStyle::ToolError, "")?;
        }

        if let Some(msg) = output_msg {
            renderer.line(MessageStyle::ToolDetail, "Solution:")?;
            renderer.line(MessageStyle::ToolDetail, msg)?;
            renderer.line(MessageStyle::ToolDetail, "")?;
        }

        // For exit code 127, skip showing the raw PTY output that would confuse the agent
        // Instead, just show the solutions above
        return Ok(());
    }

    let inline_streaming = is_pty_session && renderer.prefers_untruncated_output();

    if stdout.trim().is_empty() && stderr.trim().is_empty() {
        if !inline_streaming && (!is_pty_session || is_completed) {
            renderer.line(MessageStyle::ToolDetail, "(no output)")?;
        } else if is_pty_session && !is_completed {
            // For running PTY sessions with no output yet, don't show "no output"
            // since the process may still be starting up or processing
        }
        return Ok(());
    }

    // Render stdout/PTY output (skipped for exit code 127 above)
    if !stdout.trim().is_empty() && !inline_streaming {
        let label = if is_pty_session { "" } else { "stdout" }; // Don't label PTY output as stdout
        render_stream_section(
            renderer,
            label,
            stdout.as_ref(),
            output_mode,
            tail_limit,
            Some(tools::RUN_PTY_CMD),
            git_styles,
            ls_styles,
            MessageStyle::ToolOutput, // Dimmed, non-italic style for tool output
            allow_ansi,
            disable_spool,
            vt_config,
        )
        .await?;
    }

    // Render stderr if present, even for PTY sessions
    if !inline_streaming && !stderr.trim().is_empty() {
        render_stream_section(
            renderer,
            "stderr",
            stderr.as_ref(),
            output_mode,
            tail_limit,
            Some(tools::RUN_PTY_CMD),
            git_styles,
            ls_styles,
            MessageStyle::ToolError, // Error output
            allow_ansi,
            disable_spool,
            vt_config,
        )
        .await?;
    }

    // Add session completion note if completed
    if is_pty_session && is_completed {
        let exit_badge = if let Some(code) = exit_code {
            if code == 0 {
                "exit 0".to_string()
            } else {
                format!("exit {}", code)
            }
        } else {
            "done".to_string()
        };
        renderer.line(MessageStyle::ToolDetail, &format!("✓ {}", exit_badge))?;
    }

    if spooled_to_file {
        renderer.line(MessageStyle::ToolDetail, "")?;
        if let Some(path) = spool_path {
            renderer.line(
                MessageStyle::ToolDetail,
                &format!(
                    "Large output was spooled to \"{}\". Use read_file/grep_file to inspect details.",
                    path
                ),
            )?;
        }
    }

    // Render follow-up prompt if present (with double-rendering protection)
    if let Some(follow_up_prompt) = unwrapped_payload
        .get("follow_up_prompt")
        .and_then(Value::as_str)
    {
        // Check if prompt already appears in output to avoid double-rendering
        let already_rendered = stdout.contains(follow_up_prompt);

        if !already_rendered {
            renderer.line(MessageStyle::ToolDetail, "")?; // Add spacing
            renderer.line(MessageStyle::ToolDetail, follow_up_prompt)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{infer_pty_completion, resolve_pty_session_id, should_render_command_prompt};
    use serde_json::json;

    #[test]
    fn resolves_pty_session_id_with_fallback_keys() {
        let from_id = json!({ "id": "run-1" });
        assert_eq!(resolve_pty_session_id(&from_id), Some("run-1"));

        let from_session = json!({ "session_id": "run-2" });
        assert_eq!(resolve_pty_session_id(&from_session), Some("run-2"));

        let from_process = json!({ "process_id": "run-3" });
        assert_eq!(resolve_pty_session_id(&from_process), Some("run-3"));
    }

    #[test]
    fn infers_running_state_without_is_exited() {
        let payload = json!({ "process_id": "run-1" });
        assert!(!infer_pty_completion(&payload, Some("run-1"), None));
    }

    #[test]
    fn infers_completed_state_from_exit_code() {
        let payload = json!({ "id": "run-1", "exit_code": 0 });
        assert!(infer_pty_completion(&payload, Some("run-1"), Some(0)));
    }

    #[test]
    fn command_prompt_only_for_known_pty_command() {
        assert!(should_render_command_prompt(true, "cargo check"));
        assert!(!should_render_command_prompt(false, "cargo check"));
        assert!(!should_render_command_prompt(true, "unknown"));
    }
}
