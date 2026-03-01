use anyhow::Result;
use chrono::Local;
use std::time::Duration;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive;

use crate::agent::runloop::unified::palettes::format_duration_label;

use super::SlashCommandOutcome;

pub(super) async fn handle_resume_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    let limit = args
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .map(|value| value.clamp(1, 25))
        .unwrap_or(5);

    if renderer.supports_inline_ui() {
        return Ok(SlashCommandOutcome::StartResumePalette { limit });
    }

    match session_archive::list_recent_sessions(limit).await {
        Ok(listings) => {
            if listings.is_empty() {
                renderer.line(MessageStyle::Info, "No archived sessions found.")?;
            } else {
                renderer.line(MessageStyle::Info, "Recent sessions:")?;
                for (index, listing) in listings.iter().enumerate() {
                    if index > 0 {
                        renderer.line(MessageStyle::Info, "")?;
                    }

                    let ended_local = listing
                        .snapshot
                        .ended_at
                        .with_timezone(&Local)
                        .format("%Y-%m-%d %H:%M");
                    let duration = listing
                        .snapshot
                        .ended_at
                        .signed_duration_since(listing.snapshot.started_at);
                    let duration_std = duration.to_std().unwrap_or_else(|_| Duration::from_secs(0));
                    let duration_label = format_duration_label(duration_std);
                    let tool_count = listing.snapshot.distinct_tools.len();
                    let header = format!(
                        "- (ID: {}) {} · Model: {} · Workspace: {}",
                        listing.identifier(),
                        ended_local,
                        listing.snapshot.metadata.model,
                        listing.snapshot.metadata.workspace_label,
                    );
                    renderer.line(MessageStyle::Info, &header)?;

                    let detail = format!(
                        "    Duration: {} · {} msgs · {} tools",
                        duration_label, listing.snapshot.total_messages, tool_count,
                    );
                    renderer.line(MessageStyle::Info, &detail)?;

                    if let Some(prompt) = listing.first_prompt_preview() {
                        renderer.line(MessageStyle::Info, &format!("    Prompt: {prompt}"))?;
                    }

                    if let Some(reply) = listing.first_reply_preview() {
                        renderer.line(MessageStyle::Info, &format!("    Reply: {reply}"))?;
                    }

                    renderer.line(
                        MessageStyle::Info,
                        &format!("    File: {}", listing.path.display()),
                    )?;
                }
            }
        }
        Err(err) => {
            renderer.line(
                MessageStyle::Error,
                &format!("Failed to load session archives: {}", err),
            )?;
        }
    }
    Ok(SlashCommandOutcome::Handled)
}

pub(super) fn handle_rewind_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    // Parse arguments for rewind command
    let tokens: Vec<&str> = args.split_whitespace().collect();

    if tokens.is_empty() {
        return Ok(SlashCommandOutcome::RewindLatest {
            scope: vtcode_core::core::agent::snapshots::RevertScope::Both,
        });
    }

    // Parse the arguments
    let mut turn_number: Option<usize> = None;
    let mut scope_str: Option<&str> = None;

    for token in &tokens {
        if let Ok(turn) = token.parse::<usize>() {
            turn_number = Some(turn);
        } else {
            scope_str = Some(token);
        }
    }

    // Determine the revert scope
    let scope = if let Some(scope_str) = scope_str {
        match scope_str.to_ascii_lowercase().as_str() {
            "conversation" | "chat" => {
                vtcode_core::core::agent::snapshots::RevertScope::Conversation
            }
            "code" | "files" => vtcode_core::core::agent::snapshots::RevertScope::Code,
            "both" | "full" => vtcode_core::core::agent::snapshots::RevertScope::Both,
            _ => {
                renderer.line(
                    MessageStyle::Error,
                    &format!(
                        "Unknown revert scope '{}'. Use conversation, code, or both.",
                        scope_str
                    ),
                )?;
                return Ok(SlashCommandOutcome::Handled);
            }
        }
    } else {
        // Default to both if no scope specified
        vtcode_core::core::agent::snapshots::RevertScope::Both
    };

    // Use turn number if provided, otherwise use a default behavior
    if let Some(turn) = turn_number {
        // Return a command to handle the revert with specific turn and scope
        Ok(SlashCommandOutcome::RewindToTurn { turn, scope })
    } else {
        // With no turn number, rewind to the latest checkpoint for the requested scope.
        Ok(SlashCommandOutcome::RewindLatest { scope })
    }
}

pub(super) fn handle_plan_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    let trimmed = args.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("toggle") {
        return Ok(SlashCommandOutcome::TogglePlanMode {
            enable: None,
            prompt: None,
        });
    }

    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();

    match first.to_ascii_lowercase().as_str() {
        "on" | "enable" => Ok(SlashCommandOutcome::TogglePlanMode {
            enable: Some(true),
            prompt: if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            },
        }),
        "off" | "disable" => {
            if !rest.is_empty() {
                renderer.line(
                    MessageStyle::Error,
                    "Usage: /plan [on|off] [task] - Enable Plan Mode and optionally submit a planning prompt",
                )?;
                renderer.line(
                    MessageStyle::Info,
                    "  /plan <task> - Enable Plan Mode and start planning",
                )?;
                renderer.line(
                    MessageStyle::Info,
                    "  /plan on <task> - Enable Plan Mode and start planning",
                )?;
                renderer.line(MessageStyle::Info, "  /plan off - Disable Plan Mode")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::TogglePlanMode {
                enable: Some(false),
                prompt: None,
            })
        }
        _ => Ok(SlashCommandOutcome::TogglePlanMode {
            enable: Some(true),
            prompt: Some(trimmed.to_string()),
        }),
    }
}

pub(super) fn handle_agent_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    let arg = args.trim().to_ascii_lowercase();
    let enable = match arg.as_str() {
        "" | "toggle" => None,
        "on" | "enable" => Some(true),
        "off" | "disable" => Some(false),
        _ => {
            renderer.line(
                MessageStyle::Error,
                "Usage: /agent [on|off] - Toggle Autonomous Mode (auto-approve safe tools)",
            )?;
            return Ok(SlashCommandOutcome::Handled);
        }
    };

    Ok(SlashCommandOutcome::ToggleAutonomous { enable })
}

pub(super) fn handle_mode_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    if !args.trim().is_empty() {
        renderer.line(
            MessageStyle::Error,
            "Usage: /mode - Cycle through Edit -> Plan modes",
        )?;
        return Ok(SlashCommandOutcome::Handled);
    }
    Ok(SlashCommandOutcome::CycleMode)
}

pub(super) fn handle_login_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    let provider = args.trim().to_ascii_lowercase();
    if provider.is_empty() {
        // Show available OAuth providers
        renderer.line(MessageStyle::Info, "OAuth Authentication")?;
        renderer.line(MessageStyle::Output, "")?;
        renderer.line(MessageStyle::Output, "Available providers:")?;
        renderer.line(
            MessageStyle::Output,
            "  openrouter  - OpenRouter API (OAuth PKCE)",
        )?;
        renderer.line(MessageStyle::Output, "")?;
        renderer.line(MessageStyle::Info, "Usage: /login <provider>")?;
        renderer.line(MessageStyle::Output, "  Example: /login openrouter")?;
        return Ok(SlashCommandOutcome::Handled);
    }
    if provider != "openrouter" {
        renderer.line(
            MessageStyle::Error,
            &format!("Provider '{}' does not support OAuth.", provider),
        )?;
        renderer.line(MessageStyle::Info, "Supported OAuth providers: openrouter")?;
        renderer.line(MessageStyle::Output, "")?;
        renderer.line(
            MessageStyle::Output,
            "For other providers, set the API key via environment variable or .env file.",
        )?;
        return Ok(SlashCommandOutcome::Handled);
    }
    Ok(SlashCommandOutcome::OAuthLogin { provider })
}

pub(super) fn handle_logout_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    let provider = args.trim().to_ascii_lowercase();
    if provider.is_empty() {
        renderer.line(MessageStyle::Info, "Clear OAuth Authentication")?;
        renderer.line(MessageStyle::Output, "")?;
        renderer.line(MessageStyle::Output, "Usage: /logout <provider>")?;
        renderer.line(MessageStyle::Output, "  Example: /logout openrouter")?;
        renderer.line(MessageStyle::Output, "")?;
        renderer.line(
            MessageStyle::Info,
            "Use /auth to check current authentication status.",
        )?;
        return Ok(SlashCommandOutcome::Handled);
    }
    if provider != "openrouter" {
        renderer.line(
            MessageStyle::Error,
            &format!("Provider '{}' does not use OAuth authentication.", provider),
        )?;
        return Ok(SlashCommandOutcome::Handled);
    }
    Ok(SlashCommandOutcome::OAuthLogout { provider })
}

pub(super) fn handle_auth_command(args: &str) -> SlashCommandOutcome {
    let provider = if args.trim().is_empty() {
        None
    } else {
        Some(args.trim().to_ascii_lowercase())
    };
    SlashCommandOutcome::ShowAuthStatus { provider }
}
