use std::path::Path;

use anyhow::Result;
use chrono::Local;
use std::time::Duration;
use vtcode_core::core::threads::{SessionQueryScope, list_recent_sessions_in_scope};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::unified::palettes::format_duration_label;
use crate::cli::auth::{
    COPILOT_PROVIDER, OPENAI_PROVIDER, OPENROUTER_PROVIDER, supports_auth_provider,
};

use super::SlashCommandOutcome;
use super::{OAuthProviderAction, SessionModeCommand, SessionPaletteMode};

fn parse_session_palette_args(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<Option<(usize, bool)>> {
    let mut limit = 5usize;
    let mut show_all = false;

    for token in args.split_whitespace() {
        match token {
            "--all" | "all" => show_all = true,
            value => {
                if let Ok(parsed) = value.parse::<usize>() {
                    limit = parsed.clamp(1, 25);
                } else {
                    renderer.line(
                        MessageStyle::Error,
                        "Usage: /resume [limit] [--all] or /fork [limit] [--all]",
                    )?;
                    return Ok(None);
                }
            }
        }
    }

    Ok(Some((limit, show_all)))
}

async fn handle_session_palette_command(
    args: &str,
    renderer: &mut AnsiRenderer,
    workspace: &Path,
    mode: SessionPaletteMode,
) -> Result<SlashCommandOutcome> {
    let Some((limit, show_all)) = parse_session_palette_args(args, renderer)? else {
        return Ok(SlashCommandOutcome::Handled);
    };

    if renderer.supports_inline_ui() {
        return Ok(SlashCommandOutcome::StartSessionPalette {
            mode,
            limit,
            show_all,
        });
    }

    let scope = if show_all {
        SessionQueryScope::All
    } else {
        SessionQueryScope::CurrentWorkspace(workspace.to_path_buf())
    };

    match list_recent_sessions_in_scope(limit, &scope).await {
        Ok(listings) => {
            if listings.is_empty() {
                renderer.line(MessageStyle::Info, "No archived sessions found.")?;
            } else {
                let header = match mode {
                    SessionPaletteMode::Resume => "Recent sessions available to resume:",
                    SessionPaletteMode::Fork => "Recent sessions available to fork:",
                };
                renderer.line(MessageStyle::Info, header)?;
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

pub(super) async fn handle_resume_command(
    args: &str,
    renderer: &mut AnsiRenderer,
    workspace: &Path,
) -> Result<SlashCommandOutcome> {
    handle_session_palette_command(args, renderer, workspace, SessionPaletteMode::Resume).await
}

pub(super) async fn handle_fork_command(
    args: &str,
    renderer: &mut AnsiRenderer,
    workspace: &Path,
) -> Result<SlashCommandOutcome> {
    handle_session_palette_command(args, renderer, workspace, SessionPaletteMode::Fork).await
}

pub(super) fn handle_rewind_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    // Parse arguments for rewind command
    let tokens: Vec<&str> = args.split_whitespace().collect();

    if tokens.is_empty() {
        return if renderer.supports_inline_ui() {
            Ok(SlashCommandOutcome::OpenRewindPicker)
        } else {
            Ok(SlashCommandOutcome::RewindLatest {
                scope: vtcode_core::core::agent::snapshots::RevertScope::Both,
            })
        };
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

pub(super) fn handle_mode_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok(SlashCommandOutcome::StartModeSelection);
    }

    match trimmed.to_ascii_lowercase().as_str() {
        "edit" => Ok(SlashCommandOutcome::SetMode {
            mode: SessionModeCommand::Edit,
        }),
        "auto" | "trusted" | "trusted-auto" | "trusted_auto" => Ok(SlashCommandOutcome::SetMode {
            mode: SessionModeCommand::Auto,
        }),
        "plan" => Ok(SlashCommandOutcome::SetMode {
            mode: SessionModeCommand::Plan,
        }),
        "cycle" | "next" | "toggle" => Ok(SlashCommandOutcome::CycleMode),
        _ => {
            renderer.line(MessageStyle::Error, "Usage: /mode [edit|auto|plan|cycle]")?;
            renderer.line(
                MessageStyle::Info,
                "  /mode        - Open the interactive mode picker",
            )?;
            renderer.line(
                MessageStyle::Info,
                "  /mode edit   - Standard edit mode with normal confirmations",
            )?;
            renderer.line(
                MessageStyle::Info,
                "  /mode auto   - Auto mode with classifier-backed permission checks",
            )?;
            renderer.line(
                MessageStyle::Info,
                "  /mode plan   - Read-only planning mode",
            )?;
            renderer.line(
                MessageStyle::Info,
                "  /mode cycle  - Cycle Edit -> Auto -> Plan",
            )?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
}

pub(super) fn handle_login_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    let provider = args.trim().to_ascii_lowercase();
    if provider.is_empty() {
        if renderer.supports_inline_ui() {
            return Ok(SlashCommandOutcome::StartOAuthProviderPicker {
                action: OAuthProviderAction::Login,
            });
        }
        renderer.line(MessageStyle::Info, "Usage: /login <provider>")?;
        renderer.line(MessageStyle::Output, &format!("  {OPENAI_PROVIDER}"))?;
        renderer.line(MessageStyle::Output, &format!("  {OPENROUTER_PROVIDER}"))?;
        renderer.line(MessageStyle::Output, &format!("  {COPILOT_PROVIDER}"))?;
        return Ok(SlashCommandOutcome::Handled);
    }
    if !supports_auth_provider(&provider) {
        renderer.line(
            MessageStyle::Error,
            &format!(
                "Provider '{}' does not support VT Code authentication.",
                provider
            ),
        )?;
        renderer.line(
            MessageStyle::Info,
            "Supported authentication providers: openai, openrouter, copilot",
        )?;
        renderer.line(MessageStyle::Output, "")?;
        renderer.line(
            MessageStyle::Output,
            "For other providers, configure credentials via environment variables or workspace configuration.",
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
        if renderer.supports_inline_ui() {
            return Ok(SlashCommandOutcome::StartOAuthProviderPicker {
                action: OAuthProviderAction::Logout,
            });
        }
        renderer.line(MessageStyle::Info, "Usage: /logout <provider>")?;
        renderer.line(MessageStyle::Output, &format!("  {OPENAI_PROVIDER}"))?;
        renderer.line(MessageStyle::Output, &format!("  {OPENROUTER_PROVIDER}"))?;
        renderer.line(MessageStyle::Output, &format!("  {COPILOT_PROVIDER}"))?;
        return Ok(SlashCommandOutcome::Handled);
    }
    if !supports_auth_provider(&provider) {
        renderer.line(
            MessageStyle::Error,
            &format!(
                "Provider '{}' does not use VT Code-managed authentication.",
                provider
            ),
        )?;
        return Ok(SlashCommandOutcome::Handled);
    }
    Ok(SlashCommandOutcome::OAuthLogout { provider })
}

pub(super) fn handle_refresh_oauth_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    let provider = args.trim().to_ascii_lowercase();
    if provider.is_empty() {
        if renderer.supports_inline_ui() {
            return Ok(SlashCommandOutcome::StartOAuthProviderPicker {
                action: OAuthProviderAction::Refresh,
            });
        }
        renderer.line(MessageStyle::Info, "Usage: /refresh-oauth <provider>")?;
        renderer.line(MessageStyle::Output, &format!("  {OPENAI_PROVIDER}"))?;
        return Ok(SlashCommandOutcome::Handled);
    }
    if provider != OPENAI_PROVIDER && provider != OPENROUTER_PROVIDER {
        renderer.line(
            MessageStyle::Error,
            &format!("Provider '{}' does not support refresh-oauth.", provider),
        )?;
        return Ok(SlashCommandOutcome::Handled);
    }
    Ok(SlashCommandOutcome::RefreshOAuth { provider })
}

pub(super) fn handle_auth_command(args: &str) -> SlashCommandOutcome {
    let provider = if args.trim().is_empty() {
        None
    } else {
        Some(args.trim().to_ascii_lowercase())
    };
    SlashCommandOutcome::ShowAuthStatus { provider }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;
    use vtcode_core::utils::ansi::AnsiRenderer;
    use vtcode_tui::app::InlineHandle;

    #[test]
    fn rewind_without_args_opens_picker_in_inline_ui() {
        let (sender, _receiver) = unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());

        let outcome = handle_rewind_command("", &mut renderer).expect("rewind outcome");

        assert!(matches!(outcome, SlashCommandOutcome::OpenRewindPicker));
    }

    #[test]
    fn login_without_args_opens_oauth_picker_in_inline_ui() {
        let (sender, _receiver) = unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());

        let outcome = handle_login_command("", &mut renderer).expect("login outcome");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::StartOAuthProviderPicker {
                action: OAuthProviderAction::Login
            }
        ));
    }

    #[test]
    fn logout_without_args_opens_oauth_picker_in_inline_ui() {
        let (sender, _receiver) = unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());

        let outcome = handle_logout_command("", &mut renderer).expect("logout outcome");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::StartOAuthProviderPicker {
                action: OAuthProviderAction::Logout
            }
        ));
    }

    #[test]
    fn refresh_without_args_opens_oauth_picker_in_inline_ui() {
        let (sender, _receiver) = unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());

        let outcome =
            handle_refresh_oauth_command("", &mut renderer).expect("refresh-oauth outcome");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::StartOAuthProviderPicker {
                action: OAuthProviderAction::Refresh
            }
        ));
    }

    #[test]
    fn rewind_without_args_keeps_latest_rewind_in_plain_ui() {
        let mut renderer = AnsiRenderer::stdout();

        let outcome = handle_rewind_command("", &mut renderer).expect("rewind outcome");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::RewindLatest {
                scope: vtcode_core::core::agent::snapshots::RevertScope::Both,
            }
        ));
    }

    #[test]
    fn session_palette_args_clamp_limit_and_keep_all_flag() {
        let mut renderer = AnsiRenderer::stdout();

        let parsed =
            parse_session_palette_args("0 --all", &mut renderer).expect("args should parse");

        assert_eq!(parsed, Some((1, true)));
    }

    #[test]
    fn session_palette_args_reject_unknown_tokens() {
        let mut renderer = AnsiRenderer::stdout();

        let parsed =
            parse_session_palette_args("--bogus", &mut renderer).expect("parse should succeed");

        assert_eq!(parsed, None);
    }

    #[tokio::test]
    async fn resume_in_inline_ui_returns_palette_outcome_without_listing_eagerly() {
        let (sender, _receiver) = unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());

        let outcome = handle_resume_command("7 --all", &mut renderer, Path::new("."))
            .await
            .expect("resume outcome");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::StartSessionPalette {
                mode: SessionPaletteMode::Resume,
                limit: 7,
                show_all: true,
            }
        ));
    }
}
