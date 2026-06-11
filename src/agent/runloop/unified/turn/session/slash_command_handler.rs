use anyhow::Result;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::Utc;

use crate::agent::runloop::slash_commands::handle_slash_command as process_slash_command;
use crate::agent::runloop::unified::turn::session::interaction_loop::{
    InteractionLoopContext, InteractionOutcome, InteractionState,
};
use crate::agent::runloop::unified::turn::session::slash_commands::{
    self, SlashCommandContext, SlashCommandControl,
};
use vtcode_commons::fs::{is_windows_absolute_path, trim_trailing_image_path, unescape_whitespace};
use vtcode_core::hooks::SessionEndReason;
use vtcode_core::scheduler::{ScheduleSpec, SessionLanguageCommand, scheduled_tasks_enabled};
use vtcode_core::tools::file_ops::is_image_path;
use vtcode_core::utils::ansi::MessageStyle;

pub(crate) enum CommandProcessingResult {
    Outcome(InteractionOutcome),
    ContinueLoop,
    NotHandled,
    UpdateInput(String),
}

pub(crate) async fn handle_input_commands(
    input: &str,
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
) -> Result<CommandProcessingResult> {
    match input {
        "" => return Ok(CommandProcessingResult::ContinueLoop),
        "exit" | "quit" => {
            ctx.renderer.line(MessageStyle::Info, "✓")?;
            return Ok(CommandProcessingResult::Outcome(InteractionOutcome::Exit {
                reason: SessionEndReason::Exit,
            }));
        }
        "help" => {
            ctx.renderer
                .line(MessageStyle::Info, "Commands: exit, help")?;
            return Ok(CommandProcessingResult::ContinueLoop);
        }
        input if input.starts_with('/') && !is_absolute_image_path_input(input) => {
            if let Some(command_input) = input.strip_prefix('/') {
                let outcome =
                    match process_slash_command(command_input, ctx.renderer, &ctx.config.workspace)
                        .await
                    {
                        Ok(outcome) => outcome,
                        Err(err) => {
                            tracing::error!("slash command parse/dispatch failed: {err:#}");
                            ctx.renderer.line(
                                MessageStyle::Error,
                                &format!("Slash command failed: {}", err),
                            )?;
                            return Ok(CommandProcessingResult::ContinueLoop);
                        }
                    };

                let command_result = match slash_commands::handle_outcome(
                    outcome,
                    SlashCommandContext {
                        thread_id: ctx.thread_id,
                        active_thread_label: ctx.active_thread_label,
                        thread_handle: ctx.thread_handle,
                        renderer: ctx.renderer,
                        handle: ctx.handle,
                        session: ctx.session,
                        header_context: ctx.header_context,
                        ide_context_bridge: ctx.ide_context_bridge,
                        config: ctx.config,
                        vt_cfg: ctx.vt_cfg,
                        provider_client: ctx.provider_client,
                        session_bootstrap: ctx.session_bootstrap,
                        model_picker_state: state.model_picker_state,
                        palette_state: state.palette_state,
                        tool_registry: ctx.tool_registry,
                        conversation_history: ctx.conversation_history,
                        decision_ledger: ctx.decision_ledger,
                        context_manager: ctx.context_manager,
                        active_primary_agent: ctx.active_primary_agent,
                        session_stats: ctx.session_stats,
                        plan_session: ctx.plan_session,
                        input_status_state: state.input_status_state,
                        tools: ctx.tools,
                        tool_catalog: ctx.tool_catalog,
                        async_mcp_manager: ctx.async_mcp_manager.as_ref(),
                        mcp_panel_state: ctx.mcp_panel_state,
                        linked_directories: ctx.linked_directories,
                        ctrl_c_state: ctx.ctrl_c_state,
                        ctrl_c_notify: ctx.ctrl_c_notify,
                        full_auto: ctx.full_auto,
                        loaded_skills: ctx.loaded_skills,
                        checkpoint_manager: ctx.checkpoint_manager,
                        lifecycle_hooks: ctx.lifecycle_hooks.as_ref(),
                        harness_emitter: ctx.harness_emitter,
                    },
                )
                .await
                {
                    Ok(result) => result,
                    Err(err) => {
                        tracing::error!("slash command execution failed: {err:#}");
                        ctx.renderer.line(
                            MessageStyle::Error,
                            &format!("Slash command failed: {}", err),
                        )?;
                        return Ok(CommandProcessingResult::ContinueLoop);
                    }
                };

                match command_result {
                    SlashCommandControl::SubmitPrompt(prompt) => {
                        return Ok(CommandProcessingResult::UpdateInput(prompt));
                    }
                    SlashCommandControl::ReplaceInput(content) => {
                        ctx.handle.set_input(content);
                        return Ok(CommandProcessingResult::ContinueLoop);
                    }
                    SlashCommandControl::Continue => {
                        return Ok(CommandProcessingResult::ContinueLoop);
                    }
                    SlashCommandControl::BreakWithReason(reason) => {
                        return Ok(CommandProcessingResult::Outcome(InteractionOutcome::Exit {
                            reason,
                        }));
                    }
                }
            }
        }
        _ => {}
    }

    if scheduler_enabled(ctx)
        && let Some(result) = handle_session_language_command(input, ctx).await?
    {
        return Ok(result);
    }

    Ok(CommandProcessingResult::NotHandled)
}

async fn handle_session_language_command(
    input: &str,
    ctx: &mut InteractionLoopContext<'_>,
) -> Result<Option<CommandProcessingResult>> {
    let Some(command) =
        vtcode_core::scheduler::parse_session_language_command(input, chrono::Local::now())
    else {
        return Ok(None);
    };
    match command? {
        SessionLanguageCommand::CreateOneShotPrompt { prompt, run_at } => {
            let summary = ctx
                .tool_registry
                .create_session_prompt_task(
                    None,
                    prompt,
                    ScheduleSpec::one_shot(run_at),
                    Utc::now(),
                )
                .await?;
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Scheduled session task {} ({}) for {}.",
                    summary.id,
                    summary.name,
                    run_at
                        .with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M:%S")
                ),
            )?;
            Ok(Some(CommandProcessingResult::ContinueLoop))
        }
        SessionLanguageCommand::ListTasks => {
            let tasks = ctx.tool_registry.list_session_tasks().await;
            if tasks.is_empty() {
                ctx.renderer
                    .line(MessageStyle::Info, "No session scheduled tasks.")?;
                return Ok(Some(CommandProcessingResult::ContinueLoop));
            }
            for task in tasks {
                let next_run = task
                    .next_run_at
                    .map(|value| {
                        value
                            .with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_else(|| "none".to_string());
                let status = task.last_status.unwrap_or_else(|| "never_run".to_string());
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "{}  {}  {}  next={}  status={}",
                        task.id, task.name, task.schedule, next_run, status
                    ),
                )?;
            }
            Ok(Some(CommandProcessingResult::ContinueLoop))
        }
        SessionLanguageCommand::CancelTask { query } => {
            let Some(task) = ctx.tool_registry.delete_session_task(&query).await else {
                return Ok(None);
            };
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Cancelled session scheduled task {} ({}).",
                    task.id, task.name
                ),
            )?;
            Ok(Some(CommandProcessingResult::ContinueLoop))
        }
    }
}

fn scheduler_enabled(ctx: &InteractionLoopContext<'_>) -> bool {
    let enabled = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.automation.scheduled_tasks.enabled)
        .unwrap_or(false);
    scheduled_tasks_enabled(enabled)
}

fn is_absolute_image_path_input(input: &str) -> bool {
    let trimmed = input.trim_start();
    if let Some(token) = leading_path_token(trimmed) {
        let mut candidate = token.as_str();
        if let Some(rest) = candidate.strip_prefix("file://") {
            candidate = rest;
        }

        let candidate = unescape_whitespace(candidate);
        let candidate = candidate.as_str();

        let path = if let Some(rest) = candidate.strip_prefix("~/") {
            if let Some(home) = dirs::home_dir() {
                home.join(rest)
            } else {
                return false;
            }
        } else if is_windows_absolute_path(candidate) || Path::new(candidate).is_absolute() {
            PathBuf::from(candidate)
        } else {
            return false;
        };

        if is_image_path(&path) {
            return true;
        }
    }

    if matches_absolute_image_path(trimmed) {
        return true;
    }

    trimmed.starts_with('/') && contains_image_extension(trimmed)
}

fn leading_path_token(input: &str) -> Option<String> {
    if input.is_empty() {
        return None;
    }

    let mut chars = input.char_indices().peekable();
    let first = chars.peek().map(|(_, ch)| *ch)?;
    let (start, quote) = if first == '"' || first == '\'' {
        chars.next();
        (first.len_utf8(), Some(first))
    } else {
        (0, None)
    };

    let mut end = input.len();
    if let Some(quote) = quote {
        for (idx, ch) in chars {
            if ch == quote {
                end = idx;
                break;
            }
        }
    } else {
        let mut idx = start;
        while idx < input.len() {
            let ch = input[idx..].chars().next().unwrap();
            if ch.is_ascii_whitespace() {
                end = idx;
                break;
            }
            if ch == '\\'
                && let Some(next) = input[idx + ch.len_utf8()..].chars().next()
                && next.is_ascii_whitespace()
            {
                idx += ch.len_utf8() + next.len_utf8();
                continue;
            }
            idx += ch.len_utf8();
        }
        if end == input.len() {
            end = idx;
        }
    }

    let token = input[start..end]
        .trim_matches(|ch: char| matches!(ch, ',' | '.' | ';' | ':' | ')' | ']' | '}' | '!' | '?'));
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

static ABSOLUTE_IMAGE_PATH_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?ix)
        ^\s*['\"]?
        (
            (?:file://)?
            (?:
                ~/(?:[^\n/]+/)+
              | /(?:[^\n/]+/)+
              | [A-Za-z]:[\\/](?:[^\n\\\/]+[\\/])+
            )
            [^\n]+?
            \.(?:png|jpe?g|gif|bmp|webp|tiff?|svg)
        )"#,
    )
    .expect("Failed to compile absolute image path regex")
});

fn matches_absolute_image_path(input: &str) -> bool {
    let Some(caps) = ABSOLUTE_IMAGE_PATH_REGEX.captures(input) else {
        return false;
    };
    let Some(path_match) = caps.get(1) else {
        return false;
    };
    let raw = path_match.as_str();
    // The regex may consume trailing text after the image extension.
    // Try progressively shorter suffixes to find a valid image path.
    let trimmed = trim_trailing_image_path(raw, is_absolute_image_path_str);
    is_absolute_image_path_str(trimmed)
}

/// Check if a string is an absolute image path, handling `file://` and `~/`.
fn is_absolute_image_path_str(token: &str) -> bool {
    let unescaped = unescape_whitespace(token);
    let mut candidate = unescaped.as_str();
    if let Some(rest) = candidate.strip_prefix("file://") {
        candidate = rest;
    }
    if let Some(rest) = candidate.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return is_image_path(&home.join(rest));
        }
        return false;
    }
    is_image_path(Path::new(candidate))
}

fn contains_image_extension(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp", ".tiff", ".tif", ".svg",
    ]
    .iter()
    .any(|ext| lower.contains(ext))
}

#[cfg(test)]
mod tests {
    use super::is_absolute_image_path_input;

    #[test]
    fn absolute_image_path_is_not_treated_as_slash_command() {
        assert!(is_absolute_image_path_input(
            "/Users/vinhnguyenxuan/Desktop/Screenshot 2026-02-06 at 3.39.48 PM.png"
        ));
    }

    #[test]
    fn absolute_image_path_with_text_is_not_treated_as_slash_command() {
        assert!(is_absolute_image_path_input(
            "/Users/vinhnguyenxuan/Desktop/Screenshot 2026-02-06 at 3.39.48 PM.png can you see"
        ));
    }

    #[test]
    fn absolute_non_image_path_is_still_slash_command_candidate() {
        assert!(!is_absolute_image_path_input(
            "/Users/vinhnguyenxuan/Desktop/notes.txt"
        ));
    }

    #[test]
    fn absolute_image_path_with_unescaped_spaces_is_not_treated_as_slash_command() {
        assert!(is_absolute_image_path_input(
            "/Users/vinhnguyenxuan/Desktop/Screenshot 2026-02-06 at 4.01.01 PM.png can you see"
        ));
    }

    #[test]
    fn absolute_image_path_with_narrow_no_break_space_is_not_treated_as_slash_command() {
        let input = "/Users/vinhnguyenxuan/Desktop/Screenshot 2026-02-06 at 4.00.44\u{202F}PM.png can you see";
        assert!(is_absolute_image_path_input(input));
    }

    #[test]
    fn regex_does_not_include_trailing_text_in_match() {
        let input = "/Users/foo/Desktop/Screenshot 2026-02-06 at 3.39.48 PM.png can you see";
        let captures: Vec<_> = super::ABSOLUTE_IMAGE_PATH_REGEX
            .captures_iter(input)
            .collect();
        assert_eq!(captures.len(), 1, "Should match exactly one image path");
        let matched = captures[0].get(1).unwrap().as_str();
        assert!(
            !matched.contains("can you"),
            "Match should not include trailing text, got: {matched}"
        );
        assert!(matched.ends_with(".png"));
    }
}
