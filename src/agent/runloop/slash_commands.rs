use std::path::Path;

use anyhow::Result;
use vtcode_core::llm::provider::ResponsesCompactionOptions;
use vtcode_core::prompts::{expand_prompt_template, find_prompt_template};
use vtcode_core::scheduler::{LoopCommand, ScheduleCreateInput};
use vtcode_core::skills::{
    CommandSkillBackend, CommandSkillSpec, find_command_skill_by_slash_name,
};
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

#[path = "slash_commands/flow.rs"]
mod flow;
#[path = "slash_commands/management.rs"]
mod management;
#[path = "slash_commands/parsing.rs"]
mod parsing;
#[path = "slash_commands/rendering.rs"]
mod rendering;
use flow::{
    handle_auth_command, handle_fork_command, handle_login_command, handle_logout_command,
    handle_mode_command, handle_plan_command, handle_resume_command, handle_rewind_command,
};
use management::{handle_loop_command, handle_mcp_command, handle_schedule_command};
use parsing::{
    parse_analyze_scope, parse_compact_command, parse_prompt_template_args, parse_review_spec,
    parse_session_log_export_format, split_command_and_args,
};
use rendering::{render_help, render_theme_list};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThemePaletteMode {
    Select,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionPaletteMode {
    Resume,
    Fork,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatuslineTargetMode {
    User,
    Workspace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionModeCommand {
    Edit,
    Auto,
    Plan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OAuthProviderAction {
    Login,
    Logout,
    Refresh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionLogExportFormat {
    Json,
    Markdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentDefinitionScope {
    Project,
    User,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AgentManagerAction {
    List,
    Threads,
    Inspect {
        id: String,
    },
    Close {
        id: String,
    },
    Create {
        scope: Option<AgentDefinitionScope>,
        name: Option<String>,
    },
    Edit {
        name: Option<String>,
    },
    Delete {
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SubprocessManagerAction {
    List,
    ToggleDefault,
    Refresh,
    Inspect { id: String },
    Stop { id: String },
    Cancel { id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ScheduleCommandAction {
    Interactive,
    Browse,
    CreateInteractive,
    Create { input: ScheduleCreateInput },
    DeleteInteractive,
    Delete { id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CompactConversationCommand {
    Run { options: ResponsesCompactionOptions },
    EditDefaultPrompt,
    ResetDefaultPrompt,
}

pub(crate) enum SlashCommandOutcome {
    Handled,
    ThemeChanged(String),
    InitializeWorkspace {
        force: bool,
    },

    ShowSettings,
    ShowSettingsAtPath {
        path: String,
    },
    ShowMemoryConfig,
    ShowPermissions,
    ShowMemory,
    Exit,
    NewSession,
    OpenDocs,
    StartModelSelection,
    ToggleIdeContext,
    StartThemePalette {
        mode: ThemePaletteMode,
    },
    StartSessionPalette {
        mode: SessionPaletteMode,
        limit: usize,
        show_all: bool,
    },
    StartHistoryPicker,
    StartFileBrowser {
        initial_filter: Option<String>,
    },
    ToggleVimMode {
        enable: Option<bool>,
    },
    StartStatuslineSetup {
        instructions: Option<String>,
    },
    ClearScreen,
    ClearConversation,
    CompactConversation {
        command: CompactConversationCommand,
    },
    CopyLatestAssistantReply,
    TriggerPromptSuggestions,
    ToggleTasksPanel,
    ShowJobsPanel,
    ShowStatus,
    StopAgent,
    ManageMcp {
        action: McpCommandAction,
    },
    StartDoctorInteractive,
    RunDoctor {
        quick: bool,
    },
    Update {
        check_only: bool,
        install: bool,
        force: bool,
    },
    ManageLoop {
        command: LoopCommand,
    },
    ManageSchedule {
        action: ScheduleCommandAction,
    },
    LaunchEditor {
        file: Option<String>,
    },
    LaunchGit,
    ManageSkills {
        action: crate::agent::runloop::SkillCommandAction,
    },
    ManageAgents {
        action: AgentManagerAction,
    },
    ManageSubprocesses {
        action: SubprocessManagerAction,
    },
    ReplaceInput {
        content: String,
    },
    SubmitPrompt {
        prompt: String,
    },
    StartTerminalSetup,
    OpenRewindPicker,
    RewindToTurn {
        turn: usize,
        scope: vtcode_core::core::agent::snapshots::RevertScope,
    },
    RewindLatest {
        scope: vtcode_core::core::agent::snapshots::RevertScope,
    },
    TogglePlanMode {
        enable: Option<bool>,
        prompt: Option<String>,
    },
    /// /mode command - open interactive mode selection
    StartModeSelection,
    /// /mode edit|auto|plan
    SetMode {
        mode: SessionModeCommand,
    },
    /// /mode cycle - cycle through Edit → Auto → Plan → Edit
    CycleMode,
    /// /login command - OAuth login for a provider
    OAuthLogin {
        provider: String,
    },
    StartOAuthProviderPicker {
        action: OAuthProviderAction,
    },
    /// /logout command - Clear OAuth authentication for a provider
    OAuthLogout {
        provider: String,
    },
    RefreshOAuth {
        provider: String,
    },
    /// /auth command - Show authentication status
    ShowAuthStatus {
        provider: Option<String>,
    },
    /// /share-log command - Export current session log for debugging
    ShareLog {
        format: SessionLogExportFormat,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum McpCommandAction {
    Interactive,
    Overview,
    ListProviders,
    ListTools,
    RefreshTools,
    ShowConfig,
    EditConfig,
    Repair,
    Diagnose,
    Login(String),
    Logout(String),
}

pub(crate) async fn handle_slash_command(
    input: &str,
    renderer: &mut AnsiRenderer,
    workspace: &Path,
) -> Result<SlashCommandOutcome> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(SlashCommandOutcome::Handled);
    }

    let (command, rest) = split_command_and_args(trimmed);
    let command_key = command.to_ascii_lowercase();
    let command_key = normalize_command_key(&command_key);
    let args = rest.trim();

    if let Some(spec) = find_command_skill_by_slash_name(command_key) {
        return execute_command_skill_spec(spec, args, trimmed, renderer, workspace).await;
    }

    if let Some(template) = find_prompt_template(workspace, command_key).await {
        let template_args = match parse_prompt_template_args(args) {
            Ok(parsed) => parsed,
            Err(message) => {
                renderer.line(MessageStyle::Error, &message)?;
                return Ok(SlashCommandOutcome::Handled);
            }
        };
        let expanded = expand_prompt_template(&template.body, &template_args);
        return Ok(SlashCommandOutcome::ReplaceInput { content: expanded });
    }

    Ok(SlashCommandOutcome::SubmitPrompt {
        prompt: format!("/{}", input.trim()),
    })
}

pub(crate) async fn execute_command_skill_by_name(
    slash_name: &str,
    input: &str,
    renderer: &mut AnsiRenderer,
    workspace: &Path,
) -> Result<SlashCommandOutcome> {
    let command_key = normalize_command_key(slash_name.trim());
    let Some(spec) = find_command_skill_by_slash_name(command_key) else {
        anyhow::bail!("unknown command skill '{}'", slash_name);
    };

    execute_command_skill_spec(spec, input.trim(), input.trim(), renderer, workspace).await
}

async fn execute_command_skill_spec(
    spec: &'static CommandSkillSpec,
    args: &str,
    input: &str,
    renderer: &mut AnsiRenderer,
    workspace: &Path,
) -> Result<SlashCommandOutcome> {
    match spec.backend {
        CommandSkillBackend::TraditionalSkill { skill_name, .. } => {
            dispatch_traditional_command_skill(spec, skill_name, args, renderer)
        }
        CommandSkillBackend::BuiltInCommand { .. } => {
            execute_built_in_command_skill(spec, args, input, renderer, workspace).await
        }
    }
}

fn dispatch_traditional_command_skill(
    spec: &CommandSkillSpec,
    skill_name: &str,
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    let input = match spec.slash_name {
        "command" => {
            if args.trim().is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /command <program> [args...]")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            args.trim().to_string()
        }
        "review" => {
            if matches!(args.trim(), "--help" | "help") {
                renderer.line(
                    MessageStyle::Info,
                    "Usage: /review [--last-diff] [--target <expr>] [--style <style>] [--file <path> | files...]",
                )?;
                return Ok(SlashCommandOutcome::Handled);
            }
            if let Err(err) = parse_review_spec(args) {
                renderer.line(MessageStyle::Error, &err)?;
                renderer.line(
                    MessageStyle::Info,
                    "Usage: /review [--last-diff] [--target <expr>] [--style <style>] [--file <path> | files...]",
                )?;
                return Ok(SlashCommandOutcome::Handled);
            }
            args.trim().to_string()
        }
        "analyze" => {
            if matches!(args.trim(), "--help" | "help") {
                renderer.line(
                    MessageStyle::Info,
                    "Usage: /analyze [full|security|performance]",
                )?;
                return Ok(SlashCommandOutcome::Handled);
            }
            match parse_analyze_scope(args) {
                Ok(Some(scope)) => scope,
                Ok(None) => String::new(),
                Err(err) => {
                    renderer.line(MessageStyle::Error, &err)?;
                    renderer.line(
                        MessageStyle::Info,
                        "Usage: /analyze [full|security|performance]",
                    )?;
                    return Ok(SlashCommandOutcome::Handled);
                }
            }
        }
        _ => args.trim().to_string(),
    };

    Ok(SlashCommandOutcome::ManageSkills {
        action: crate::agent::runloop::SkillCommandAction::Use {
            name: skill_name.to_string(),
            input,
        },
    })
}

fn normalize_command_key(command_key: &str) -> &str {
    match command_key {
        "settings" | "setttings" => "config",
        "comman" => "command",
        "sharelog" | "export-log" => "share-log",
        "subprocesses" => "subprocess",
        "context" => "compact",
        other => other,
    }
}

async fn execute_built_in_command_skill(
    spec: &CommandSkillSpec,
    args: &str,
    input: &str,
    renderer: &mut AnsiRenderer,
    workspace: &Path,
) -> Result<SlashCommandOutcome> {
    match spec.slash_name {
        "donate" => {
            renderer.line(
                MessageStyle::Info,
                "Your support is invaluable, it enables me to dedicate more time to research, exploration, and creating work that pushes boundaries. Thank you for making this possible.",
            )?;
            renderer.line(
                MessageStyle::Info,
                "You can donate at: https://buymeacoffee.com/vinhnx",
            )?;
            Ok(SlashCommandOutcome::Handled)
        }

        "theme" => {
            let mut tokens = args.split_whitespace();
            if let Some(next_theme) = tokens.next() {
                let desired = next_theme.to_lowercase();
                match theme::set_active_theme(&desired) {
                    Ok(()) => {
                        let label = theme::active_theme_label();
                        renderer
                            .line(MessageStyle::Info, &format!("Theme switched to {}", label))?;
                        return Ok(SlashCommandOutcome::ThemeChanged(theme::active_theme_id()));
                    }
                    Err(err) => {
                        renderer.line(
                            MessageStyle::Error,
                            &format!("Theme '{}' not available: {}", next_theme, err),
                        )?;
                    }
                }
                return Ok(SlashCommandOutcome::Handled);
            }

            if renderer.supports_inline_ui() {
                return Ok(SlashCommandOutcome::StartThemePalette {
                    mode: ThemePaletteMode::Select,
                });
            } else {
                renderer.line(MessageStyle::Info, "Provide a theme name to switch themes")?;
                render_theme_list(renderer)?;
            }
            Ok(SlashCommandOutcome::Handled)
        }

        "init" => {
            let mut force = false;
            for flag in args.split_whitespace() {
                match flag {
                    "--force" | "-f" | "force" => force = true,
                    unknown => {
                        renderer.line(
                            MessageStyle::Error,
                            &format!("Unknown flag '{}' for /init", unknown),
                        )?;
                        return Ok(SlashCommandOutcome::Handled);
                    }
                }
            }
            Ok(SlashCommandOutcome::InitializeWorkspace { force })
        }
        "config" | "settings" | "setttings" => {
            if args.is_empty() {
                Ok(SlashCommandOutcome::ShowSettings)
            } else {
                match args.to_ascii_lowercase().as_str() {
                    "memory" | "agent.persistent_memory" => {
                        Ok(SlashCommandOutcome::ShowMemoryConfig)
                    }
                    "permissions" => Ok(SlashCommandOutcome::ShowPermissions),
                    "model" | "model.main" | "model.lightweight" => {
                        Ok(SlashCommandOutcome::ShowSettingsAtPath {
                            path: args.to_string(),
                        })
                    }
                    _ => Ok(SlashCommandOutcome::ShowSettingsAtPath {
                        path: args.to_string(),
                    }),
                }
            }
        }
        "permissions" => Ok(SlashCommandOutcome::ShowPermissions),
        "memory" => Ok(SlashCommandOutcome::ShowMemory),
        "vim" => {
            let enable = match args {
                "" | "toggle" => None,
                "on" | "enable" | "enabled" => Some(true),
                "off" | "disable" | "disabled" => Some(false),
                _ => {
                    renderer.line(MessageStyle::Error, "Usage: /vim [on|off|toggle]")?;
                    return Ok(SlashCommandOutcome::Handled);
                }
            };
            Ok(SlashCommandOutcome::ToggleVimMode { enable })
        }
        "statusline" => Ok(SlashCommandOutcome::StartStatuslineSetup {
            instructions: (!args.trim().is_empty()).then(|| args.trim().to_string()),
        }),
        "clear" => match args {
            "" => Ok(SlashCommandOutcome::ClearScreen),
            "new" | "--new" | "fresh" | "--fresh" => Ok(SlashCommandOutcome::ClearConversation),
            _ => {
                renderer.line(MessageStyle::Error, "Usage: /clear [new]")?;
                Ok(SlashCommandOutcome::Handled)
            }
        },
        "compact" | "context" => match parse_compact_command(args) {
            Ok(command) => Ok(SlashCommandOutcome::CompactConversation { command }),
            Err(err) => {
                renderer.line(MessageStyle::Error, &err)?;
                renderer.line(
                        MessageStyle::Info,
                        "Usage: /compact [--instructions <text>] [--max-output-tokens <n>] [--reasoning-effort <none|minimal|low|medium|high|xhigh>] [--verbosity <low|medium|high>] [--include <selector> ...] [--store|--no-store] [--service-tier <flex|priority>] [--prompt-cache-key <key>]",
                    )?;
                renderer.line(
                    MessageStyle::Info,
                    "       /compact edit-prompt | /compact reset-prompt",
                )?;
                Ok(SlashCommandOutcome::Handled)
            }
        },
        "copy" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /copy")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::CopyLatestAssistantReply)
        }
        "suggest" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /suggest")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::TriggerPromptSuggestions)
        }
        "tasks" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /tasks")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::ToggleTasksPanel)
        }
        "jobs" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /jobs")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::ShowJobsPanel)
        }
        "status" => Ok(SlashCommandOutcome::ShowStatus),
        "stop" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /stop")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::StopAgent)
        }
        "pause" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /pause")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            renderer.line(MessageStyle::Info, "No active run to pause.")?;
            Ok(SlashCommandOutcome::Handled)
        }
        "doctor" => match parse_doctor_args(args, renderer.supports_inline_ui()) {
            Ok(DoctorCommand::Interactive) => Ok(SlashCommandOutcome::StartDoctorInteractive),
            Ok(DoctorCommand::Run { quick }) => Ok(SlashCommandOutcome::RunDoctor { quick }),
            Err(message) => {
                renderer.line(MessageStyle::Error, &message)?;
                Ok(SlashCommandOutcome::Handled)
            }
        },
        "update" => match parse_update_args(args) {
            Ok((check_only, install, force)) => Ok(SlashCommandOutcome::Update {
                check_only,
                install,
                force,
            }),
            Err(message) => {
                renderer.line(MessageStyle::Error, &message)?;
                Ok(SlashCommandOutcome::Handled)
            }
        },
        "mcp" => handle_mcp_command(args, renderer),
        "model" => Ok(SlashCommandOutcome::StartModelSelection),
        "ide" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /ide")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::ToggleIdeContext)
        }
        "files" => {
            let initial_filter = if args.trim().is_empty() {
                None
            } else {
                Some(args.trim().to_string())
            };

            if renderer.supports_inline_ui() {
                return Ok(SlashCommandOutcome::StartFileBrowser { initial_filter });
            }

            renderer.line(
                MessageStyle::Error,
                "File browser requires inline UI mode. Use @ symbol instead.",
            )?;
            Ok(SlashCommandOutcome::Handled)
        }
        "loop" => handle_loop_command(args, renderer),
        "schedule" => handle_schedule_command(args, renderer),
        "share-log" | "sharelog" | "export-log" => match parse_session_log_export_format(args) {
            Ok(format) => Ok(SlashCommandOutcome::ShareLog { format }),
            Err(message) => {
                renderer.line(MessageStyle::Error, &message)?;
                Ok(SlashCommandOutcome::Handled)
            }
        },
        "resume" => handle_resume_command(args, renderer, workspace).await,
        "fork" => handle_fork_command(args, renderer, workspace).await,
        "history" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /history")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::StartHistoryPicker)
        }
        "new" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /new")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::NewSession)
        }
        "rewind" => handle_rewind_command(args, renderer),
        "docs" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /docs")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::OpenDocs)
        }
        "edit" => {
            let file = if args.trim().is_empty() {
                None
            } else {
                Some(args.trim().to_string())
            };
            Ok(SlashCommandOutcome::LaunchEditor { file })
        }
        "git" => Ok(SlashCommandOutcome::LaunchGit),
        "exit" => Ok(SlashCommandOutcome::Exit),
        "skills" => {
            // Reconstruct full command with / prefix for parser
            let full_command = format!("/{}", input);
            match crate::agent::runloop::parse_skill_command(&full_command) {
                Ok(Some(action)) => {
                    // Return skill command for processing in chat context
                    Ok(SlashCommandOutcome::ManageSkills { action })
                }
                Ok(None) => {
                    renderer.line(MessageStyle::Error, "Skills command parse error")?;
                    Ok(SlashCommandOutcome::Handled)
                }
                Err(e) => {
                    renderer.line(MessageStyle::Error, &format!("Skills command error: {}", e))?;
                    Ok(SlashCommandOutcome::Handled)
                }
            }
        }
        "agents" => match parse_agents_command(args) {
            Ok(action) => Ok(SlashCommandOutcome::ManageAgents { action }),
            Err(message) => {
                renderer.line(MessageStyle::Error, &message)?;
                Ok(SlashCommandOutcome::Handled)
            }
        },
        "agent" => match args.trim() {
            "" => Ok(SlashCommandOutcome::ManageAgents {
                action: AgentManagerAction::Threads,
            }),
            args => match parse_agents_command(args) {
                Ok(action) => Ok(SlashCommandOutcome::ManageAgents { action }),
                Err(message) => {
                    renderer.line(MessageStyle::Error, &message)?;
                    Ok(SlashCommandOutcome::Handled)
                }
            },
        },
        "subprocesses" | "subprocess" => match parse_subprocesses_command(args) {
            Ok(action) => Ok(SlashCommandOutcome::ManageSubprocesses { action }),
            Err(message) => {
                renderer.line(MessageStyle::Error, &message)?;
                Ok(SlashCommandOutcome::Handled)
            }
        },
        "plan" => handle_plan_command(args, renderer),
        "mode" => handle_mode_command(args, renderer),
        "login" => handle_login_command(args, renderer),
        "logout" => handle_logout_command(args, renderer),
        "refresh-oauth" => flow::handle_refresh_oauth_command(args, renderer),
        "auth" => Ok(handle_auth_command(args)),
        "help" => {
            let specific_cmd = if args.trim().is_empty() {
                None
            } else {
                Some(args.trim())
            };
            render_help(renderer, specific_cmd, workspace).await?;
            Ok(SlashCommandOutcome::Handled)
        }
        "terminal-setup" => {
            if !args.is_empty() {
                renderer.line(
                    MessageStyle::Error,
                    "Usage: /terminal-setup (no arguments supported yet)",
                )?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::StartTerminalSetup)
        }
        _ => unreachable!("unknown built-in command skill: {}", spec.slash_name),
    }
}

fn parse_agents_command(args: &str) -> std::result::Result<AgentManagerAction, String> {
    let trimmed = args.trim();
    if trimmed.is_empty() || matches!(trimmed, "list" | "manager") {
        return Ok(AgentManagerAction::List);
    }
    if matches!(trimmed, "threads" | "active") {
        return Ok(AgentManagerAction::Threads);
    }

    let parts = trimmed.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        ["inspect", id] => Ok(AgentManagerAction::Inspect {
            id: (*id).to_string(),
        }),
        ["close", id] => Ok(AgentManagerAction::Close {
            id: (*id).to_string(),
        }),
        ["edit"] => Ok(AgentManagerAction::Edit { name: None }),
        ["edit", name] => Ok(AgentManagerAction::Edit {
            name: Some((*name).to_string()),
        }),
        ["delete", name] => Ok(AgentManagerAction::Delete {
            name: (*name).to_string(),
        }),
        ["create"] => Ok(AgentManagerAction::Create {
            scope: None,
            name: None,
        }),
        ["create", "project"] => Ok(AgentManagerAction::Create {
            scope: Some(AgentDefinitionScope::Project),
            name: None,
        }),
        ["create", "user"] => Ok(AgentManagerAction::Create {
            scope: Some(AgentDefinitionScope::User),
            name: None,
        }),
        ["create", "project", name] => Ok(AgentManagerAction::Create {
            scope: Some(AgentDefinitionScope::Project),
            name: Some((*name).to_string()),
        }),
        ["create", "user", name] => Ok(AgentManagerAction::Create {
            scope: Some(AgentDefinitionScope::User),
            name: Some((*name).to_string()),
        }),
        _ => Err(
            "Usage: /agents [list|threads|inspect <id>|close <id>|create [project|user] [name]|edit [name]|delete <name>]".to_string(),
        ),
    }
}

fn parse_subprocesses_command(args: &str) -> std::result::Result<SubprocessManagerAction, String> {
    match args.split_whitespace().collect::<Vec<_>>().as_slice() {
        [] | ["list"] | ["panel"] => Ok(SubprocessManagerAction::List),
        ["toggle"] => Ok(SubprocessManagerAction::ToggleDefault),
        ["refresh"] => Ok(SubprocessManagerAction::Refresh),
        ["inspect", id] => Ok(SubprocessManagerAction::Inspect {
            id: (*id).to_string(),
        }),
        ["stop", id] => Ok(SubprocessManagerAction::Stop {
            id: (*id).to_string(),
        }),
        ["cancel", id] => Ok(SubprocessManagerAction::Cancel {
            id: (*id).to_string(),
        }),
        _ => Err(
            "Usage: /subprocesses [list|toggle|refresh|inspect <id>|stop <id>|cancel <id>]"
                .to_string(),
        ),
    }
}

fn parse_update_args(args: &str) -> std::result::Result<(bool, bool, bool), String> {
    let mut check_only = false;
    let mut install = false;
    let mut force = false;

    for token in args.split_whitespace() {
        match token.to_ascii_lowercase().as_str() {
            "check" | "--check" => check_only = true,
            "install" | "--install" => install = true,
            "force" | "--force" => force = true,
            "" => {}
            _ => {
                return Err(
                    "Usage: /update [check|install] [--force]\nExamples: /update, /update check, /update install --force".to_string(),
                );
            }
        }
    }

    if check_only && install {
        return Err("Use either 'check' or 'install', not both.".to_string());
    }

    Ok((check_only, install, force))
}

#[derive(Debug)]
enum DoctorCommand {
    Interactive,
    Run { quick: bool },
}

fn parse_doctor_args(
    args: &str,
    supports_inline_ui: bool,
) -> std::result::Result<DoctorCommand, String> {
    let mut quick = false;
    let mut full = false;

    for token in args.split_whitespace() {
        match token.to_ascii_lowercase().as_str() {
            "--quick" | "-q" | "quick" => quick = true,
            "--full" | "full" => full = true,
            "" => {}
            _ => {
                return Err(
                    "Usage: /doctor [--quick|--full]\nExamples: /doctor, /doctor --quick"
                        .to_string(),
                );
            }
        }
    }

    if quick && full {
        return Err("Use either --quick or --full, not both.".to_string());
    }

    if !quick && !full && supports_inline_ui {
        return Ok(DoctorCommand::Interactive);
    }

    Ok(DoctorCommand::Run { quick })
}

#[cfg(test)]
mod tests {
    use super::{
        AgentManagerAction, CompactConversationCommand, DoctorCommand, ScheduleCommandAction,
        SessionModeCommand, SlashCommandOutcome, SubprocessManagerAction, handle_slash_command,
        parse_doctor_args, parse_update_args,
    };
    use vtcode_core::llm::provider::ResponsesCompactionOptions;
    use vtcode_core::skills::command_skill_specs;
    use vtcode_core::utils::ansi::AnsiRenderer;
    use vtcode_tui::app::InlineHandle;

    fn renderer_for_tests() -> AnsiRenderer {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(tx), Default::default())
    }

    #[test]
    fn parse_doctor_defaults_to_full_mode() {
        let mode = parse_doctor_args("", false).expect("should parse");
        assert!(matches!(mode, DoctorCommand::Run { quick: false }));
    }

    #[test]
    fn parse_doctor_defaults_to_interactive_when_inline_ui_available() {
        let mode = parse_doctor_args("", true).expect("should parse");
        assert!(matches!(mode, DoctorCommand::Interactive));
    }

    #[test]
    fn parse_doctor_quick_aliases() {
        let mode = parse_doctor_args("--quick", true).expect("should parse");
        assert!(matches!(mode, DoctorCommand::Run { quick: true }));

        let mode = parse_doctor_args("-q", true).expect("should parse");
        assert!(matches!(mode, DoctorCommand::Run { quick: true }));

        let mode = parse_doctor_args("quick", true).expect("should parse");
        assert!(matches!(mode, DoctorCommand::Run { quick: true }));
    }

    #[test]
    fn parse_doctor_rejects_conflicting_flags() {
        let err = parse_doctor_args("--quick --full", true).expect_err("must reject");
        assert!(err.contains("either --quick or --full"));
    }

    #[test]
    fn parse_update_rejects_conflicting_modes() {
        let err = parse_update_args("check install").expect_err("must reject");
        assert!(err.contains("either 'check' or 'install'"));
    }

    #[tokio::test]
    async fn stop_command_returns_local_stop_outcome() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command("stop", &mut renderer, &workspace)
            .await
            .expect("stop command should parse");

        assert!(matches!(outcome, SlashCommandOutcome::StopAgent));
    }

    #[tokio::test]
    async fn pause_command_is_idle_noop() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command("pause", &mut renderer, &workspace)
            .await
            .expect("pause command should parse");

        assert!(matches!(outcome, SlashCommandOutcome::Handled));
    }

    #[tokio::test]
    async fn subprocess_alias_matches_plural_command() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command("subprocess inspect child-1", &mut renderer, &workspace)
            .await
            .expect("subprocess alias should parse");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::ManageSubprocesses {
                action: SubprocessManagerAction::Inspect { ref id }
            } if id == "child-1"
        ));
    }

    #[tokio::test]
    async fn ide_command_returns_toggle_outcome() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command("ide", &mut renderer, &workspace)
            .await
            .expect("ide command should parse");

        assert!(matches!(outcome, SlashCommandOutcome::ToggleIdeContext));
    }

    #[tokio::test]
    async fn memory_command_returns_memory_outcome() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command("memory", &mut renderer, &workspace)
            .await
            .expect("memory command should parse");

        assert!(matches!(outcome, SlashCommandOutcome::ShowMemory));
    }

    #[tokio::test]
    async fn config_memory_opens_memory_controls() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command("config memory", &mut renderer, &workspace)
            .await
            .expect("config memory command should parse");

        assert!(matches!(outcome, SlashCommandOutcome::ShowMemoryConfig));
    }

    #[tokio::test]
    async fn config_model_opens_model_settings_tree() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command("config model", &mut renderer, &workspace)
            .await
            .expect("config model command should parse");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::ShowSettingsAtPath { ref path } if path == "model"
        ));
    }

    #[tokio::test]
    async fn ide_command_rejects_arguments() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command("ide extra", &mut renderer, &workspace)
            .await
            .expect("ide command should parse");

        assert!(matches!(outcome, SlashCommandOutcome::Handled));
    }

    #[tokio::test]
    async fn vim_command_parses_enable_disable_and_toggle() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let toggle = handle_slash_command("vim", &mut renderer, &workspace)
            .await
            .expect("vim should parse");
        assert!(matches!(
            toggle,
            SlashCommandOutcome::ToggleVimMode { enable: None }
        ));

        let enable = handle_slash_command("vim on", &mut renderer, &workspace)
            .await
            .expect("vim on should parse");
        assert!(matches!(
            enable,
            SlashCommandOutcome::ToggleVimMode { enable: Some(true) }
        ));

        let disable = handle_slash_command("vim off", &mut renderer, &workspace)
            .await
            .expect("vim off should parse");
        assert!(matches!(
            disable,
            SlashCommandOutcome::ToggleVimMode {
                enable: Some(false)
            }
        ));
    }

    #[tokio::test]
    async fn agent_command_opens_active_agents_inspector() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command("agent", &mut renderer, &workspace)
            .await
            .expect("agent command should parse");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::ManageAgents {
                action: AgentManagerAction::Threads
            }
        ));
    }

    #[tokio::test]
    async fn agent_command_supports_direct_inspect_and_close() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let inspect = handle_slash_command("agent inspect thread-1", &mut renderer, &workspace)
            .await
            .expect("agent inspect should parse");
        assert!(matches!(
            inspect,
            SlashCommandOutcome::ManageAgents {
                action: AgentManagerAction::Inspect { ref id }
            } if id == "thread-1"
        ));

        let close = handle_slash_command("agent close thread-1", &mut renderer, &workspace)
            .await
            .expect("agent close should parse");
        assert!(matches!(
            close,
            SlashCommandOutcome::ManageAgents {
                action: AgentManagerAction::Close { ref id }
            } if id == "thread-1"
        ));
    }

    #[tokio::test]
    async fn agents_create_and_edit_commands_parse_guided_forms() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let create_default = handle_slash_command("agents create", &mut renderer, &workspace)
            .await
            .expect("agents create should parse");
        assert!(matches!(
            create_default,
            SlashCommandOutcome::ManageAgents {
                action: AgentManagerAction::Create {
                    scope: None,
                    name: None,
                }
            }
        ));

        let create_project =
            handle_slash_command("agents create project", &mut renderer, &workspace)
                .await
                .expect("agents create project should parse");
        assert!(matches!(
            create_project,
            SlashCommandOutcome::ManageAgents {
                action: AgentManagerAction::Create {
                    scope: Some(super::AgentDefinitionScope::Project),
                    name: None,
                }
            }
        ));

        let create_named =
            handle_slash_command("agents create project reviewer", &mut renderer, &workspace)
                .await
                .expect("agents create project <name> should parse");
        assert!(matches!(
            create_named,
            SlashCommandOutcome::ManageAgents {
                action: AgentManagerAction::Create {
                    scope: Some(super::AgentDefinitionScope::Project),
                    name: Some(ref name),
                }
            } if name == "reviewer"
        ));

        let edit_default = handle_slash_command("agents edit", &mut renderer, &workspace)
            .await
            .expect("agents edit should parse");
        assert!(matches!(
            edit_default,
            SlashCommandOutcome::ManageAgents {
                action: AgentManagerAction::Edit { name: None }
            }
        ));

        let edit_named = handle_slash_command("agents edit reviewer", &mut renderer, &workspace)
            .await
            .expect("agents edit <name> should parse");
        assert!(matches!(
            edit_named,
            SlashCommandOutcome::ManageAgents {
                action: AgentManagerAction::Edit { name: Some(ref name) }
            } if name == "reviewer"
        ));
    }

    #[tokio::test]
    async fn subprocesses_command_supports_toggle_and_refresh() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let toggle = handle_slash_command("subprocesses toggle", &mut renderer, &workspace)
            .await
            .expect("toggle command should parse");
        assert!(matches!(
            toggle,
            SlashCommandOutcome::ManageSubprocesses {
                action: SubprocessManagerAction::ToggleDefault
            }
        ));

        let refresh = handle_slash_command("subprocesses refresh", &mut renderer, &workspace)
            .await
            .expect("refresh command should parse");
        assert!(matches!(
            refresh,
            SlashCommandOutcome::ManageSubprocesses {
                action: SubprocessManagerAction::Refresh
            }
        ));
    }

    #[tokio::test]
    async fn subprocesses_command_supports_direct_actions() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let inspect = handle_slash_command("subprocesses inspect bg-1", &mut renderer, &workspace)
            .await
            .expect("inspect command should parse");
        assert!(matches!(
            inspect,
            SlashCommandOutcome::ManageSubprocesses {
                action: SubprocessManagerAction::Inspect { ref id }
            } if id == "bg-1"
        ));

        let stop = handle_slash_command("subprocesses stop bg-1", &mut renderer, &workspace)
            .await
            .expect("stop command should parse");
        assert!(matches!(
            stop,
            SlashCommandOutcome::ManageSubprocesses {
                action: SubprocessManagerAction::Stop { ref id }
            } if id == "bg-1"
        ));

        let cancel = handle_slash_command("subprocesses cancel bg-1", &mut renderer, &workspace)
            .await
            .expect("cancel command should parse");
        assert!(matches!(
            cancel,
            SlashCommandOutcome::ManageSubprocesses {
                action: SubprocessManagerAction::Cancel { ref id }
            } if id == "bg-1"
        ));
    }

    #[tokio::test]
    async fn statusline_command_parses_optional_instructions() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome =
            handle_slash_command("statusline show cwd and branch", &mut renderer, &workspace)
                .await
                .expect("statusline should parse");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::StartStatuslineSetup {
                instructions: Some(ref text)
            } if text == "show cwd and branch"
        ));
    }

    #[tokio::test]
    async fn interactive_mode_commands_parse_to_expected_outcomes() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let suggest = handle_slash_command("suggest", &mut renderer, &workspace)
            .await
            .expect("suggest should parse");
        assert!(matches!(
            suggest,
            SlashCommandOutcome::TriggerPromptSuggestions
        ));

        let tasks = handle_slash_command("tasks", &mut renderer, &workspace)
            .await
            .expect("tasks should parse");
        assert!(matches!(tasks, SlashCommandOutcome::ToggleTasksPanel));

        let jobs = handle_slash_command("jobs", &mut renderer, &workspace)
            .await
            .expect("jobs should parse");
        assert!(matches!(jobs, SlashCommandOutcome::ShowJobsPanel));

        let mode = handle_slash_command("mode", &mut renderer, &workspace)
            .await
            .expect("mode should parse");
        assert!(matches!(mode, SlashCommandOutcome::StartModeSelection));

        let auto_mode = handle_slash_command("mode auto", &mut renderer, &workspace)
            .await
            .expect("mode auto should parse");
        assert!(matches!(
            auto_mode,
            SlashCommandOutcome::SetMode {
                mode: SessionModeCommand::Auto
            }
        ));

        let cycle = handle_slash_command("mode cycle", &mut renderer, &workspace)
            .await
            .expect("mode cycle should parse");
        assert!(matches!(cycle, SlashCommandOutcome::CycleMode));
    }

    #[tokio::test]
    async fn schedule_commands_parse_to_interactive_and_direct_outcomes() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let interactive = handle_slash_command("schedule", &mut renderer, &workspace)
            .await
            .expect("schedule should parse");
        assert!(matches!(
            interactive,
            SlashCommandOutcome::ManageSchedule {
                action: ScheduleCommandAction::Interactive
            }
        ));

        let browse = handle_slash_command("schedule list", &mut renderer, &workspace)
            .await
            .expect("schedule list should parse");
        assert!(matches!(
            browse,
            SlashCommandOutcome::ManageSchedule {
                action: ScheduleCommandAction::Browse
            }
        ));

        let create_interactive = handle_slash_command("schedule create", &mut renderer, &workspace)
            .await
            .expect("schedule create should parse");
        assert!(matches!(
            create_interactive,
            SlashCommandOutcome::ManageSchedule {
                action: ScheduleCommandAction::CreateInteractive
            }
        ));

        let delete_interactive = handle_slash_command("schedule delete", &mut renderer, &workspace)
            .await
            .expect("schedule delete should parse");
        assert!(matches!(
            delete_interactive,
            SlashCommandOutcome::ManageSchedule {
                action: ScheduleCommandAction::DeleteInteractive
            }
        ));

        let delete_direct =
            handle_slash_command("schedule delete deadbeef", &mut renderer, &workspace)
                .await
                .expect("schedule delete id should parse");
        assert!(matches!(
            delete_direct,
            SlashCommandOutcome::ManageSchedule {
                action: ScheduleCommandAction::Delete { ref id }
            } if id == "deadbeef"
        ));

        let create_direct = handle_slash_command(
            "schedule create --prompt \"check the deployment\" --every 10m",
            &mut renderer,
            &workspace,
        )
        .await
        .expect("schedule create with flags should parse");
        assert!(matches!(
            create_direct,
            SlashCommandOutcome::ManageSchedule {
                action: ScheduleCommandAction::Create { ref input }
            } if input.prompt.as_deref() == Some("check the deployment")
                && input.every.as_deref() == Some("10m")
        ));
    }

    #[tokio::test]
    async fn compact_commands_parse_to_automatic_and_direct_outcomes() {
        let workspace = std::env::current_dir().expect("workspace");
        let mut renderer = renderer_for_tests();

        let automatic = handle_slash_command("compact", &mut renderer, &workspace)
            .await
            .expect("compact should parse");
        assert!(matches!(
            automatic,
            SlashCommandOutcome::CompactConversation {
                command: CompactConversationCommand::Run { ref options }
            } if *options == ResponsesCompactionOptions::default()
        ));

        let edit_prompt = handle_slash_command("compact edit-prompt", &mut renderer, &workspace)
            .await
            .expect("compact edit-prompt should parse");
        assert!(matches!(
            edit_prompt,
            SlashCommandOutcome::CompactConversation {
                command: CompactConversationCommand::EditDefaultPrompt
            }
        ));

        let reset_prompt = handle_slash_command("compact reset-prompt", &mut renderer, &workspace)
            .await
            .expect("compact reset-prompt should parse");
        assert!(matches!(
            reset_prompt,
            SlashCommandOutcome::CompactConversation {
                command: CompactConversationCommand::ResetDefaultPrompt
            }
        ));

        let direct = handle_slash_command(
            "compact --instructions \"keep decisions\" --include reasoning.encrypted_content --store",
            &mut renderer,
            &workspace,
        )
        .await
        .expect("compact flags should parse");
        assert!(matches!(
            direct,
            SlashCommandOutcome::CompactConversation {
                command: CompactConversationCommand::Run { ref options }
            } if options.instructions.as_deref() == Some("keep decisions")
                && options.responses_include.as_deref() == Some(&["reasoning.encrypted_content".to_string()][..])
                && options.response_store == Some(true)
        ));
    }

    #[tokio::test]
    async fn prompt_template_invocation_replaces_editor_input() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let template_dir = workspace.path().join(".vtcode/prompts/templates");
        std::fs::create_dir_all(&template_dir).expect("template dir");
        std::fs::write(
            template_dir.join("review-template.md"),
            "---\ndescription: Review template\n---\nReview $1 against $2.\nArgs: $@",
        )
        .expect("template");

        let mut renderer = renderer_for_tests();
        let outcome = handle_slash_command(
            "review-template src/lib.rs main",
            &mut renderer,
            workspace.path(),
        )
        .await
        .expect("review template should parse");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::ReplaceInput { ref content }
                if content == "Review src/lib.rs against main.\nArgs: src/lib.rs main"
        ));
    }

    #[tokio::test]
    async fn prompt_template_invocation_preserves_quoted_arguments() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let template_dir = workspace.path().join(".vtcode/prompts/templates");
        std::fs::create_dir_all(&template_dir).expect("template dir");
        std::fs::write(
            template_dir.join("rename-template.md"),
            "---\ndescription: Rename template\n---\nRename $1 to $2",
        )
        .expect("template");

        let mut renderer = renderer_for_tests();
        let outcome = handle_slash_command(
            r#"rename-template "src/old name.rs" "src/new name.rs""#,
            &mut renderer,
            workspace.path(),
        )
        .await
        .expect("rename template should parse");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::ReplaceInput { ref content }
                if content == "Rename src/old name.rs to src/new name.rs"
        ));
    }

    #[tokio::test]
    async fn built_in_slash_command_beats_same_named_template() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let template_dir = workspace.path().join(".vtcode/prompts/templates");
        std::fs::create_dir_all(&template_dir).expect("template dir");
        std::fs::write(
            template_dir.join("help.md"),
            "---\ndescription: shadow help\n---\nThis should not run.",
        )
        .expect("template");

        let mut renderer = renderer_for_tests();
        let outcome = handle_slash_command("help", &mut renderer, workspace.path())
            .await
            .expect("help should parse");

        assert!(matches!(outcome, SlashCommandOutcome::Handled));
    }

    #[tokio::test]
    async fn review_slash_routes_through_cmd_review_skill() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command("review --last-diff", &mut renderer, workspace.path())
            .await
            .expect("review should parse");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::ManageSkills {
                action: crate::agent::runloop::SkillCommandAction::Use { ref name, ref input }
            } if name == "cmd-review" && input == "--last-diff"
        ));
    }

    #[tokio::test]
    async fn command_alias_typo_routes_through_cmd_command_skill() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command("comman cargo check", &mut renderer, workspace.path())
            .await
            .expect("comman alias should parse");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::ManageSkills {
                action: crate::agent::runloop::SkillCommandAction::Use { ref name, ref input }
            } if name == "cmd-command" && input == "cargo check"
        ));
    }

    #[tokio::test]
    async fn analyze_slash_routes_normalized_scope_through_cmd_analyze_skill() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command("analyze SECURITY", &mut renderer, workspace.path())
            .await
            .expect("analyze should parse");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::ManageSkills {
                action: crate::agent::runloop::SkillCommandAction::Use { ref name, ref input }
            } if name == "cmd-analyze" && input == "security"
        ));
    }

    #[tokio::test]
    async fn invalid_analyze_scope_is_handled_locally() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command("analyze nope", &mut renderer, workspace.path())
            .await
            .expect("analyze should parse");

        assert!(matches!(outcome, SlashCommandOutcome::Handled));
    }

    #[tokio::test]
    async fn unknown_slash_command_falls_back_to_normal_prompt_submission() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command(
            "totally-unknown keep this raw",
            &mut renderer,
            workspace.path(),
        )
        .await
        .expect("unknown slash should pass through");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::SubmitPrompt { ref prompt }
                if prompt == "/totally-unknown keep this raw"
        ));
    }

    #[tokio::test]
    async fn permissions_slash_command_opens_permissions_view() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command("permissions", &mut renderer, workspace.path())
            .await
            .expect("permissions should parse");

        assert!(matches!(outcome, SlashCommandOutcome::ShowPermissions));
    }

    #[tokio::test]
    async fn init_slash_command_parses_force_flag() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let mut renderer = renderer_for_tests();

        let outcome = handle_slash_command("init --force", &mut renderer, workspace.path())
            .await
            .expect("init should parse");

        assert!(matches!(
            outcome,
            SlashCommandOutcome::InitializeWorkspace { force: true }
        ));
    }

    #[tokio::test]
    async fn every_registered_slash_command_resolves_without_prompt_fallback() {
        let workspace = tempfile::TempDir::new().expect("workspace");

        for spec in command_skill_specs() {
            let mut renderer = renderer_for_tests();
            let outcome = handle_slash_command(spec.slash_name, &mut renderer, workspace.path())
                .await
                .unwrap_or_else(|error| panic!("{} should parse: {error}", spec.slash_name));

            assert!(
                !matches!(outcome, SlashCommandOutcome::SubmitPrompt { .. }),
                "/{} fell through to plain prompt submission",
                spec.slash_name
            );
        }
    }
}
