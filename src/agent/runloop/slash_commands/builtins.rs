use std::path::Path;

use anyhow::Result;
use vtcode_core::skills::CommandSkillSpec;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::flow::{
    handle_auth_command, handle_fork_command, handle_login_command, handle_logout_command, handle_plan_command,
    handle_resume_command, handle_rewind_command,
};
use super::management::{handle_local_command, handle_mcp_command, handle_secret_command};
use super::models::{
    AgentDefinitionScope, AgentManagerAction, LogFormat, LogScope, SlashCommandOutcome, SubprocessManagerAction,
    ThemePaletteMode,
};
use super::parsing::{self, parse_compact_command, parse_session_log_export_format};
use super::rendering::{render_help, render_theme_list};

// ---- Built-in command handlers ----
fn handle_fast_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    let enable = match args.trim() {
        "" => None,
        arg if arg.eq_ignore_ascii_case("on") => Some(true),
        arg if arg.eq_ignore_ascii_case("off") => Some(false),
        _ => {
            renderer.line(MessageStyle::Error, "Usage: /fast [on|off]")?;
            return Ok(SlashCommandOutcome::Handled);
        }
    };
    Ok(SlashCommandOutcome::ToggleFastMode { enable })
}

// Each command is an independently testable function. The dispatch match in
// `execute_built_in_command_skill` is the strict interface guard: adding a
// new command requires a handler and a match-arm registration.
//
// Handlers that are only referenced through the dynamic dispatch match are
// marked `#[allow(dead_code)]` because the compiler cannot prove they are used.

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_donate_command(renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    renderer.line(
        MessageStyle::Info,
        "I build VT Code in my spare time. It supports open-weight models and will stay open source, no matter what. If it has saved you some time, you can buy me a coffee:",
    )?;
    Ok(SlashCommandOutcome::OpenDonateLinks)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_theme_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    let mut tokens = args.split_whitespace();
    if let Some(next_theme) = tokens.next() {
        let desired = next_theme.to_lowercase();
        match theme::set_active_theme(&desired) {
            Ok(()) => {
                let label = theme::active_theme_label();
                renderer.line(MessageStyle::Info, &format!("Theme switched to {label}"))?;
                return Ok(SlashCommandOutcome::ThemeChanged(theme::active_theme_id()));
            }
            Err(err) => {
                renderer.line(MessageStyle::Error, &format!("Theme '{next_theme}' not available: {err}"))?;
            }
        }
        return Ok(SlashCommandOutcome::Handled);
    }

    if renderer.supports_inline_ui() {
        return Ok(SlashCommandOutcome::StartThemePalette { mode: ThemePaletteMode::Select });
    }

    renderer.line(MessageStyle::Info, "Provide a theme name to switch themes")?;
    render_theme_list(renderer)?;
    Ok(SlashCommandOutcome::Handled)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_init_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    let mut force = false;
    for flag in args.split_whitespace() {
        match flag {
            "--force" | "-f" | "force" => force = true,
            unknown => {
                renderer.line(MessageStyle::Error, &format!("Unknown flag '{unknown}' for /init"))?;
                return Ok(SlashCommandOutcome::Handled);
            }
        }
    }
    Ok(SlashCommandOutcome::InitializeWorkspace { force })
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_config_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    const USAGE: &str =
        "Usage: /config [memory|permissions|model|tasks|jobs|log|subprocess|notify|checkup|<path>|reset]";
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok(SlashCommandOutcome::ShowSettings);
    }
    let (command, rest) = match trimmed.split_once(char::is_whitespace) {
        Some((command, rest)) => (command, rest.trim()),
        None => (trimmed, ""),
    };
    let normalized = command.to_ascii_lowercase();
    match normalized.as_str() {
        "reset" | "clear" => {
            if !rest.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /config reset")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::ShowSettingsReset)
        }
        "memory" | "agent.persistent_memory" => {
            if !rest.is_empty() {
                renderer.line(MessageStyle::Error, USAGE)?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::ShowMemoryConfig)
        }
        "permissions" => {
            if !rest.is_empty() {
                renderer.line(MessageStyle::Error, USAGE)?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::ShowPermissions)
        }
        "model" | "model.main" => {
            if !rest.is_empty() {
                renderer.line(MessageStyle::Error, USAGE)?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::ShowSettingsAtPath { path: command.to_string() })
        }
        "tasks" => handle_tasks_command(rest, renderer),
        "jobs" => handle_jobs_command(rest, renderer),
        "log" => handle_log_command(rest, renderer),
        "subprocess" | "subprocesses" => handle_subprocesses_command(rest, renderer),
        "notify" => handle_notify_command(rest),
        "checkup" | "doctor" => handle_checkup_command(rest, renderer, renderer.supports_inline_ui()),
        _ => {
            if !rest.is_empty() {
                renderer.line(MessageStyle::Error, USAGE)?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::ShowSettingsAtPath { path: command.to_string() })
        }
    }
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_statusline_command(args: &str) -> Result<SlashCommandOutcome> {
    Ok(SlashCommandOutcome::StartStatuslineSetup {
        instructions: (!args.trim().is_empty()).then(|| args.trim().to_string()),
    })
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_title_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    if !args.is_empty() {
        renderer.line(MessageStyle::Error, "Usage: /title")?;
        return Ok(SlashCommandOutcome::Handled);
    }
    Ok(SlashCommandOutcome::StartTerminalTitleSetup)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_clear_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    match args {
        "" => Ok(SlashCommandOutcome::ClearScreen),
        "new" | "--new" | "fresh" | "--fresh" => Ok(SlashCommandOutcome::ClearConversation),
        _ => {
            renderer.line(MessageStyle::Error, "Usage: /clear [new]")?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
}

fn handle_transcript_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    let args = args.trim();
    let (action, rest) = match args.split_once(char::is_whitespace) {
        Some((action, rest)) => (action, rest.trim()),
        None => (args, ""),
    };

    match action {
        "stats" | "" => Ok(SlashCommandOutcome::ShowTranscriptStats),
        "clear" => Ok(SlashCommandOutcome::ClearScreen),
        "export" => Ok(SlashCommandOutcome::ExportTranscript { path: (!rest.is_empty()).then(|| rest.to_string()) }),
        _ => {
            renderer.line(MessageStyle::Error, "Usage: /transcript [stats|clear|export [path]]")?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_compact_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    match parse_compact_command(args) {
        Ok(command) => Ok(SlashCommandOutcome::CompactConversation { command }),
        Err(err) => {
            renderer.line(MessageStyle::Error, &err)?;
            renderer.line(
                MessageStyle::Info,
                "Usage: /compact [--instructions <text>] [--max-output-tokens <n>] [--reasoning-effort <none|minimal|low|medium|high|xhigh>] [--verbosity <low|medium|high>] [--native-only]",
            )?;
            renderer.line(MessageStyle::Info, "       /compact edit-prompt | /compact reset-prompt")?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_log_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    let mut format = LogFormat::Text;
    let mut scope = LogScope::Thread;
    let mut save = false;
    for token in args.split_whitespace() {
        match token {
            "--json" => format = LogFormat::Json,
            "--text" => format = LogFormat::Text,
            "--thread" => scope = LogScope::Thread,
            "--all" => scope = LogScope::All,
            "--save" => save = true,
            _ => {
                renderer.line(MessageStyle::Error, "Usage: /log [--json|--text] [--thread|--all] [--save]")?;
                return Ok(SlashCommandOutcome::Handled);
            }
        }
    }
    Ok(SlashCommandOutcome::ShowLogViewer { format, scope, save })
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_notify_command(args: &str) -> Result<SlashCommandOutcome> {
    Ok(SlashCommandOutcome::Notify {
        message: if args.is_empty() {
            "Manual notification from /config notify".to_string()
        } else {
            args.to_string()
        },
    })
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_checkup_command(
    args: &str,
    renderer: &mut AnsiRenderer,
    supports_inline_ui: bool,
) -> Result<SlashCommandOutcome> {
    match parse_checkup_args(args, supports_inline_ui) {
        Ok(CheckupCommand::Interactive) => Ok(SlashCommandOutcome::StartCheckupInteractive),
        Ok(CheckupCommand::Run { quick }) => Ok(SlashCommandOutcome::RunCheckup { quick }),
        Err(message) => {
            renderer.line(MessageStyle::Error, &message)?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_update_command(args: &str) -> Result<SlashCommandOutcome> {
    let (check_only, install, force) = parse_update_args(args).map_err(anyhow::Error::msg)?;
    Ok(SlashCommandOutcome::Update { check_only, install, force })
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_mode_command(args: &str) -> Result<SlashCommandOutcome> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        Ok(SlashCommandOutcome::StartModePalette)
    } else {
        Ok(SlashCommandOutcome::SelectPrimaryAgent { name: trimmed.to_string() })
    }
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_files_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    let initial_filter = if args.trim().is_empty() {
        None
    } else {
        Some(args.trim().to_string())
    };

    if renderer.supports_inline_ui() {
        return Ok(SlashCommandOutcome::StartFileBrowser { initial_filter });
    }

    renderer.line(MessageStyle::Error, "File browser requires inline UI mode. Use @ symbol instead.")?;
    Ok(SlashCommandOutcome::Handled)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_share_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    match parse_session_log_export_format(args) {
        Ok(format) => Ok(SlashCommandOutcome::ShareLog { format }),
        Err(message) => {
            renderer.line(MessageStyle::Error, &message)?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_history_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    if !args.is_empty() {
        renderer.line(MessageStyle::Error, "Usage: /history")?;
        return Ok(SlashCommandOutcome::Handled);
    }
    Ok(SlashCommandOutcome::StartHistoryPicker)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_skills_command(input: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    let full_command = format!("/{input}");
    match crate::agent::runloop::parse_skill_command(&full_command) {
        Ok(Some(action)) => Ok(SlashCommandOutcome::ManageSkills { action }),
        Ok(None) => {
            renderer.line(MessageStyle::Error, "Skills command parse error")?;
            Ok(SlashCommandOutcome::Handled)
        }
        Err(error) => {
            renderer.line(MessageStyle::Error, &format!("Skills command error: {error}"))?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_plugin_command(input: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    let full_command = format!("/{input}");
    match crate::agent::runloop::parse_plugin_command(&full_command) {
        Ok(Some(action)) => Ok(SlashCommandOutcome::ManagePlugins { action }),
        Ok(None) => {
            renderer.line(MessageStyle::Error, "Plugin command parse error")?;
            Ok(SlashCommandOutcome::Handled)
        }
        Err(error) => {
            renderer.line(MessageStyle::Error, &format!("Plugin command error: {error}"))?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_agent_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    match args.trim() {
        "" => Ok(SlashCommandOutcome::ManageAgents { action: AgentManagerAction::List }),
        args => match parse_agents_command(args) {
            Ok(action) => Ok(SlashCommandOutcome::ManageAgents { action }),
            Err(message) => {
                renderer.line(MessageStyle::Error, &message)?;
                Ok(SlashCommandOutcome::Handled)
            }
        },
    }
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_subprocesses_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    match parse_subprocesses_command(args) {
        Ok(action) => Ok(SlashCommandOutcome::ManageSubprocesses { action }),
        Err(message) => {
            renderer.line(MessageStyle::Error, &message)?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
}

async fn handle_help_command(args: &str, renderer: &mut AnsiRenderer, workspace: &Path) -> Result<SlashCommandOutcome> {
    let specific_cmd = if args.trim().is_empty() {
        None
    } else {
        Some(args.trim())
    };
    render_help(renderer, specific_cmd, workspace).await?;
    Ok(SlashCommandOutcome::Handled)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_status_command() -> Result<SlashCommandOutcome> {
    Ok(SlashCommandOutcome::ShowStatus)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_permissions_command() -> Result<SlashCommandOutcome> {
    Ok(SlashCommandOutcome::ShowPermissions)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_stop_command() -> Result<SlashCommandOutcome> {
    Ok(SlashCommandOutcome::StopAgent)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_pause_command(renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    renderer.line(MessageStyle::Info, "No active run to pause.")?;
    Ok(SlashCommandOutcome::Handled)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_model_command() -> Result<SlashCommandOutcome> {
    Ok(SlashCommandOutcome::StartModelSelection)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_new_command() -> Result<SlashCommandOutcome> {
    Ok(SlashCommandOutcome::NewSession)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_docs_command() -> Result<SlashCommandOutcome> {
    Ok(SlashCommandOutcome::OpenDocs)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_feedback_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    if !args.is_empty() {
        renderer.line(MessageStyle::Error, "Usage: /feedback")?;
        return Ok(SlashCommandOutcome::Handled);
    }
    Ok(SlashCommandOutcome::OpenFeedback)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_exit_command() -> Result<SlashCommandOutcome> {
    Ok(SlashCommandOutcome::Exit)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_copy_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    if !args.is_empty() {
        renderer.line(MessageStyle::Error, "Usage: /copy")?;
        return Ok(SlashCommandOutcome::Handled);
    }
    Ok(SlashCommandOutcome::CopyLatestAssistantReply)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_tasks_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    if !args.is_empty() {
        renderer.line(MessageStyle::Error, "Usage: /tasks")?;
        return Ok(SlashCommandOutcome::Handled);
    }
    Ok(SlashCommandOutcome::ToggleTasksPanel)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_jobs_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    if !args.is_empty() {
        renderer.line(MessageStyle::Error, "Usage: /jobs")?;
        return Ok(SlashCommandOutcome::Handled);
    }
    Ok(SlashCommandOutcome::ShowJobsPanel)
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_terminal_setup_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    match args.trim().to_ascii_lowercase().as_str() {
        "" => Ok(SlashCommandOutcome::StartTerminalSetup),
        "install-iterm2-icon" => {
            vtcode_core::terminal_setup::terminals::iterm2::run_profile_icon_install(renderer)?;
            Ok(SlashCommandOutcome::Handled)
        }
        "reset-iterm2-icon" => {
            vtcode_core::terminal_setup::terminals::iterm2::run_profile_icon_reset(renderer)?;
            Ok(SlashCommandOutcome::Handled)
        }
        _ => {
            renderer.line(MessageStyle::Error, "Usage: /terminal-setup [install-iterm2-icon|reset-iterm2-icon]")?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_vim_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    match args.trim().to_ascii_lowercase().as_str() {
        "" => Ok(SlashCommandOutcome::ToggleVimMode { enable: None }),
        "on" | "enable" | "true" => Ok(SlashCommandOutcome::ToggleVimMode { enable: Some(true) }),
        "off" | "disable" | "false" => Ok(SlashCommandOutcome::ToggleVimMode { enable: Some(false) }),
        _ => {
            renderer.line(MessageStyle::Error, "Usage: /vim [on|off]")?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn handle_webmcp_command(args: &str, renderer: &mut AnsiRenderer) -> Result<SlashCommandOutcome> {
    match args.trim() {
        "" | "status" => return Ok(SlashCommandOutcome::ShowWebmcpStatus),
        "help" => return Ok(SlashCommandOutcome::ShowWebmcpHelp),
        "tools" => {
            renderer.line(MessageStyle::Info, "WebMCP tools: workspace.list_files, workspace.read_file, patch.propose, patch.apply, checks.run, patch.revert, turn.request, cancel")?;
        }
        "roots" => {
            renderer.line(
                MessageStyle::Info,
                "Headless WebMCP roots are configured with [webmcp].allowed_roots or --allowed-root.",
            )?;
        }
        "pair" => renderer.line(
            MessageStyle::Info,
            "Usage: /webmcp pair [--replace] <exact-browser-origin> (for example, /webmcp pair http://localhost:5173)",
        )?,
        "unpair" => return Ok(SlashCommandOutcome::StopWebmcp),
        pair_args if pair_args.starts_with("pair ") => {
            let pair_args = pair_args.strip_prefix("pair ").unwrap_or_default();
            if let Some((origin, replace)) = parse_webmcp_pair_args(pair_args) {
                return Ok(SlashCommandOutcome::StartWebmcp { origin, replace });
            }
            renderer.line(
                MessageStyle::Error,
                "Usage: /webmcp pair [--replace] <exact-browser-origin> (for example, /webmcp pair http://localhost:5173)",
            )?;
        }
        _ => {
            renderer.line(
                MessageStyle::Error,
                "Usage: /webmcp [help|status|tools|roots|pair [--replace] <origin>|unpair]",
            )?;
        }
    }
    Ok(SlashCommandOutcome::Handled)
}

fn parse_webmcp_pair_args(args: &str) -> Option<(String, bool)> {
    let tokens = args.split_whitespace().collect::<Vec<_>>();
    if tokens.is_empty() {
        return None;
    }

    let mut origin = None;
    let mut replace = false;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index] {
            "--replace" if !replace => replace = true,
            "--origin" if origin.is_none() => {
                index += 1;
                let value = tokens.get(index).copied()?;
                if value.starts_with('-') {
                    return None;
                }
                origin = Some(value);
            }
            token if origin.is_none() && !token.starts_with('-') => origin = Some(token),
            _ => return None,
        }
        index += 1;
    }

    origin.map(|origin| (origin.to_string(), replace))
}

pub(in crate::agent::runloop::slash_commands) async fn execute_built_in_command_skill(
    spec: &'static CommandSkillSpec,
    args: &str,
    input: &str,
    renderer: &mut AnsiRenderer,
    workspace: &Path,
) -> Result<SlashCommandOutcome> {
    match spec.slash_name {
        "donate" => handle_donate_command(renderer),
        "theme" => handle_theme_command(args, renderer),
        "init" => handle_init_command(args, renderer),
        "config" | "settings" | "setttings" => handle_config_command(args, renderer),
        "statusline" => handle_statusline_command(args),
        "title" => handle_title_command(args, renderer),
        "clear" => handle_clear_command(args, renderer),
        "transcript" => handle_transcript_command(args, renderer),
        "explain" => Ok(SlashCommandOutcome::Explain { args: args.to_owned() }),
        "compact" | "context" => handle_compact_command(args, renderer),
        "copy" => handle_copy_command(args, renderer),
        "status" => Ok(SlashCommandOutcome::ShowStatus),
        "stop" => Ok(SlashCommandOutcome::StopAgent),
        "pause" => handle_pause_command(renderer),
        "update" => handle_update_command(args),
        "mcp" => handle_mcp_command(args, renderer),
        "webmcp" => handle_webmcp_command(args, renderer),
        "local" => handle_local_command(args, renderer),
        "model" => Ok(SlashCommandOutcome::StartModelSelection),
        "fast" => handle_fast_command(args, renderer),
        "mode" => handle_mode_command(args),
        "files" => handle_files_command(args, renderer),
        "share" => handle_share_command(args, renderer),
        "resume" => handle_resume_command(args, renderer, workspace).await,
        "fork" => handle_fork_command(args, renderer, workspace).await,
        "history" => handle_history_command(args, renderer),
        "new" => Ok(SlashCommandOutcome::NewSession),
        "rewind" => handle_rewind_command(args, renderer),
        "redo" => Ok(SlashCommandOutcome::Redo),
        "docs" => Ok(SlashCommandOutcome::OpenDocs),
        "feedback" => handle_feedback_command(args, renderer),
        "exit" => Ok(SlashCommandOutcome::Exit),
        "skills" => handle_skills_command(input, renderer),
        "plugin" => handle_plugin_command(input, renderer),
        "agent" => handle_agent_command(args, renderer),
        "subprocesses" | "subprocess" => handle_subprocesses_command(args, renderer),
        "plan" => handle_plan_command(args, renderer),
        "login" => handle_login_command(args, renderer),
        "logout" => handle_logout_command(args, renderer),
        "refresh-oauth" => super::flow::handle_refresh_oauth_command(args, renderer),
        "auth" => Ok(handle_auth_command(args)),
        "secret" => handle_secret_command(args, renderer),
        "help" => handle_help_command(args, renderer, workspace).await,
        "terminal-setup" => handle_terminal_setup_command(args, renderer),
        "vim" => handle_vim_command(args, renderer),
        _ => anyhow::bail!("unknown built-in command skill: {}", spec.slash_name),
    }
}

pub(in crate::agent::runloop::slash_commands) fn parse_agents_command(
    args: &str,
) -> std::result::Result<AgentManagerAction, String> {
    let trimmed = args.trim();
    if trimmed.is_empty() || matches!(trimmed, "list" | "manager") {
        return Ok(AgentManagerAction::List);
    }
    if matches!(trimmed, "threads" | "active") {
        return Ok(AgentManagerAction::Threads);
    }

    let mut parts = trimmed.split_whitespace();
    let Some(first) = parts.next() else {
        return Ok(AgentManagerAction::List);
    };

    match first {
        "inspect" => {
            let id = parts.next().ok_or("Usage: /agent inspect <id>")?;
            Ok(AgentManagerAction::Inspect {
                id: id.to_string(),
            })
        }
        "close" => {
            let id = parts.next().ok_or("Usage: /agent close <id>")?;
            Ok(AgentManagerAction::Close {
                id: id.to_string(),
            })
        }
        "edit" => Ok(AgentManagerAction::Edit {
            name: parts.next().map(|n| n.to_string()),
        }),
        "delete" => {
            let name = parts.next().ok_or("Usage: /agent delete <name>")?;
            Ok(AgentManagerAction::Delete {
                name: name.to_string(),
            })
        }
        "create" => {
            let scope = match parts.next() {
                Some("project") => Some(AgentDefinitionScope::Project),
                Some("user") => Some(AgentDefinitionScope::User),
                Some(other) => {
                    // Treat as name with no scope
                    return Ok(AgentManagerAction::Create {
                        scope: None,
                        name: Some(other.to_string()),
                    });
                }
                None => {
                    return Ok(AgentManagerAction::Create {
                        scope: None,
                        name: None,
                    });
                }
            };
            let name = parts.next().map(|n| n.to_string());
            Ok(AgentManagerAction::Create { scope, name })
        }
        _ => Err(
            "Usage: /agent [list|threads|inspect <id>|close <id>|create [project|user] [name]|edit [name]|delete <name>]".to_string(),
        ),
    }
}

pub(in crate::agent::runloop::slash_commands) fn parse_subprocesses_command(
    args: &str,
) -> std::result::Result<SubprocessManagerAction, String> {
    let mut parts = args.split_whitespace();
    let Some(first) = parts.next() else {
        return Ok(SubprocessManagerAction::List);
    };

    match first {
        "list" | "panel" => Ok(SubprocessManagerAction::List),
        "toggle" => Ok(SubprocessManagerAction::ToggleDefault),
        "refresh" => Ok(SubprocessManagerAction::Refresh),
        "inspect" => {
            let id = parts.next().ok_or("Usage: /subprocesses inspect <id>")?;
            Ok(SubprocessManagerAction::Inspect { id: id.to_string() })
        }
        "stop" => {
            let id = parts.next().ok_or("Usage: /subprocesses stop <id>")?;
            Ok(SubprocessManagerAction::Stop { id: id.to_string() })
        }
        "cancel" => {
            let id = parts.next().ok_or("Usage: /subprocesses cancel <id>")?;
            Ok(SubprocessManagerAction::Cancel { id: id.to_string() })
        }
        _ => Err("Usage: /subprocesses [list|toggle|refresh|inspect <id>|stop <id>|cancel <id>]".to_string()),
    }
}

pub(in crate::agent::runloop::slash_commands) fn parse_update_args(
    args: &str,
) -> std::result::Result<(bool, bool, bool), String> {
    let mut check_only = false;
    let mut install = false;
    let mut force = false;

    parsing::for_each_token(args, |token| {
        match token {
            "check" | "--check" => check_only = true,
            "install" | "--install" => install = true,
            "force" | "--force" => force = true,
            _ => {
                return Err(
                    "Usage: /update [check|install] [--force]\nExamples: /update, /update check, /update install --force\n\nTip: You can also run `vtcode update` from the CLI.".to_string(),
                );
            }
        }
        Ok(())
    })?;

    if check_only && install {
        return Err("Use either 'check' or 'install', not both.".to_string());
    }

    Ok((check_only, install, force))
}

#[derive(Debug)]
pub(in crate::agent::runloop::slash_commands) enum CheckupCommand {
    Interactive,
    Run { quick: bool },
}

pub(in crate::agent::runloop::slash_commands) fn parse_checkup_args(
    args: &str,
    supports_inline_ui: bool,
) -> std::result::Result<CheckupCommand, String> {
    let mut quick = false;
    let mut full = false;

    parsing::for_each_token(args, |token| {
        match token {
            "--quick" | "-q" | "quick" => quick = true,
            "--full" | "full" => full = true,
            _ => {
                return Err("Usage: /checkup [--quick|--full]\nExamples: /checkup, /checkup --quick".to_string());
            }
        }
        Ok(())
    })?;

    if quick && full {
        return Err("Use either --quick or --full, not both.".to_string());
    }

    if !quick && !full && supports_inline_ui {
        return Ok(CheckupCommand::Interactive);
    }

    Ok(CheckupCommand::Run { quick })
}
