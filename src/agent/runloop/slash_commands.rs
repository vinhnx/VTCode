use std::path::Path;

use anyhow::Result;
use vtcode_core::prompts::{expand_prompt_template, find_prompt_template};
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
    handle_mode_command, handle_plan_command, handle_resume_command, handle_review_command,
    handle_rewind_command,
};
use management::{handle_add_dir_command, handle_mcp_command};
use parsing::{
    parse_prompt_template_args, parse_session_log_export_format, split_command_and_args,
};
use rendering::{render_generate_agent_file_usage, render_help, render_theme_list};

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
        scope: AgentDefinitionScope,
        name: String,
    },
    Edit {
        name: String,
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

pub(crate) enum SlashCommandOutcome {
    Handled,
    ThemeChanged(String),
    InitializeWorkspace {
        force: bool,
    },

    ShowSettings,
    ShowPermissions,
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
    CompactConversation,
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

    ManageWorkspaceDirectories {
        command: WorkspaceDirectoryCommand,
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

#[derive(Clone, Debug)]
pub(crate) enum WorkspaceDirectoryCommand {
    Add(Vec<String>),
    List,
    Remove(Vec<String>),
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
    let args = rest.trim();

    match command_key.as_str() {
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
        "review" => handle_review_command(args, renderer),
        "generate-agent-file" => {
            let mut overwrite = false;
            for flag in args.split_whitespace() {
                match flag {
                    "--force" | "-f" | "--overwrite" => overwrite = true,
                    "--help" | "help" => {
                        render_generate_agent_file_usage(renderer)?;
                        return Ok(SlashCommandOutcome::Handled);
                    }
                    unknown => {
                        renderer.line(
                            MessageStyle::Error,
                            &format!("Unknown flag '{}' for /generate-agent-file", unknown),
                        )?;
                        render_generate_agent_file_usage(renderer)?;
                        return Ok(SlashCommandOutcome::Handled);
                    }
                }
            }

            // Create a custom prompt for generating the agent file
            let prompt_text = if overwrite {
                "Generate a comprehensive AGENTS.md file for this workspace that documents the project structure, available tools, and recommended usage patterns. The file should overwrite any existing AGENTS.md file. Include detailed information about the project's architecture, main components, and how to work with the codebase effectively."
            } else {
                "Generate a comprehensive AGENTS.md file for this workspace that documents the project structure, available tools, and recommended usage patterns. If an AGENTS.md file already exists, consider updating it rather than overwriting, unless specifically needed. Include detailed information about the project's architecture, main components, and how to work with the codebase effectively."
            };

            Ok(SlashCommandOutcome::SubmitPrompt {
                prompt: prompt_text.to_string(),
            })
        }
        "config" | "settings" | "setttings" => Ok(SlashCommandOutcome::ShowSettings),
        "permissions" => Ok(SlashCommandOutcome::ShowPermissions),
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
        "compact" | "context" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /compact")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::CompactConversation)
        }
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
        "analyze" => {
            let scope = if args.trim().is_empty() {
                "full"
            } else {
                args.trim()
            };
            let prompt = format!(
                "Perform a comprehensive {} codebase analysis for this workspace. Include key findings, risks, and prioritized next actions.",
                scope
            );
            Ok(SlashCommandOutcome::SubmitPrompt { prompt })
        }
        "mcp" => handle_mcp_command(args, renderer),
        "model" => Ok(SlashCommandOutcome::StartModelSelection),
        "ide" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /ide")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::ToggleIdeContext)
        }
        "command" | "comman" => {
            if args.trim().is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /command <program> [args...]")?;
                return Ok(SlashCommandOutcome::Handled);
            }

            let command = args.trim();
            let prompt = format!(
                "Run this terminal command in the current workspace and show the result: {}",
                command
            );
            Ok(SlashCommandOutcome::SubmitPrompt { prompt })
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
        "add-dir" => handle_add_dir_command(args, renderer),
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
        _ => {
            if let Some(template) = find_prompt_template(workspace, &command_key).await {
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
        ["edit", name] => Ok(AgentManagerAction::Edit {
            name: (*name).to_string(),
        }),
        ["delete", name] => Ok(AgentManagerAction::Delete {
            name: (*name).to_string(),
        }),
        ["create", "project", name] => Ok(AgentManagerAction::Create {
            scope: AgentDefinitionScope::Project,
            name: (*name).to_string(),
        }),
        ["create", "user", name] => Ok(AgentManagerAction::Create {
            scope: AgentDefinitionScope::User,
            name: (*name).to_string(),
        }),
        _ => Err(
            "Usage: /agents [list|threads|inspect <id>|close <id>|create project <name>|create user <name>|edit <name>|delete <name>]".to_string(),
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
        AgentManagerAction, DoctorCommand, SessionModeCommand, SlashCommandOutcome,
        SubprocessManagerAction, handle_slash_command, parse_doctor_args, parse_update_args,
    };
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
}
