use anyhow::Result;
use serde_json::Value;
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
#[path = "slash_commands/team_agent.rs"]
mod team_agent;
use flow::{
    handle_agent_command, handle_auth_command, handle_login_command, handle_logout_command,
    handle_mode_command, handle_plan_command, handle_rewind_command, handle_sessions_command,
};
use management::{handle_add_dir_command, handle_mcp_command};
use parsing::{parse_session_log_export_format, split_command_and_args};
use rendering::{render_generate_agent_file_usage, render_help, render_theme_list};
use team_agent::{handle_agents_command, handle_team_command};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemePaletteMode {
    Select,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionLogExportFormat {
    Json,
    Markdown,
}

pub enum SlashCommandOutcome {
    Handled,
    ThemeChanged(String),
    #[allow(dead_code)]
    ExecuteTool {
        name: String,
        args: Value,
    },
    #[allow(dead_code)]
    GenerateAgentFile {
        overwrite: bool,
    },
    InitializeWorkspace {
        force: bool,
    },

    ShowConfig,
    Exit,
    NewSession,
    OpenDocs,
    StartModelSelection,
    StartThemePalette {
        mode: ThemePaletteMode,
    },
    StartSessionsPalette {
        limit: usize,
    },
    StartFileBrowser {
        initial_filter: Option<String>,
    },
    ClearScreen,
    ClearConversation,
    CopyLatestAssistantReply,
    ShowStatus,
    ManageMcp {
        action: McpCommandAction,
    },
    RunDoctor,

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
        action: AgentCommandAction,
    },
    ManageTeams {
        action: TeamCommandAction,
    },
    ManageSubagentConfig {
        action: SubagentConfigCommandAction,
    },
    SubmitPrompt {
        prompt: String,
    },
    StartTerminalSetup,
    RewindToTurn {
        turn: usize,
        scope: vtcode_core::core::agent::snapshots::RevertScope,
    },
    TogglePlanMode {
        enable: Option<bool>,
        prompt: Option<String>,
    },
    /// /agent command - toggle autonomous mode (auto-approve safe tools)
    ToggleAutonomous {
        enable: Option<bool>,
    },
    /// /mode command - cycle through Edit → Plan → Edit
    CycleMode,
    /// /login command - OAuth login for a provider
    OAuthLogin {
        provider: String,
    },
    /// /logout command - Clear OAuth authentication for a provider
    OAuthLogout {
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
pub enum AgentCommandAction {
    List,
    Create,
    Edit(String),
    Delete(String),
    #[allow(dead_code)]
    Help,
}

#[derive(Clone, Debug)]
pub enum TeamCommandAction {
    Start {
        name: Option<String>,
        count: Option<usize>,
        subagent_type: Option<String>,
        model: Option<String>,
    },
    Add {
        name: String,
        subagent_type: Option<String>,
        model: Option<String>,
    },
    Remove {
        name: String,
    },
    TaskAdd {
        description: String,
        depends_on: Vec<u64>,
    },
    TaskClaim {
        task_id: u64,
    },
    TaskComplete {
        task_id: u64,
        success: bool,
        summary: Option<String>,
    },
    Assign {
        task_id: u64,
        teammate: String,
    },
    Message {
        recipient: String,
        message: String,
    },
    Broadcast {
        message: String,
    },
    Tasks,
    Teammates,
    Model,
    Stop,
    Help,
}

#[derive(Clone, Debug)]
pub enum SubagentConfigCommandAction {
    Model,
}

#[derive(Clone, Debug)]
pub enum McpCommandAction {
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
pub enum WorkspaceDirectoryCommand {
    Add(Vec<String>),
    List,
    Remove(Vec<String>),
}

pub async fn handle_slash_command(
    input: &str,
    renderer: &mut AnsiRenderer,
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
        "config" | "settings" => Ok(SlashCommandOutcome::ShowConfig),
        "clear" => match args {
            "" => Ok(SlashCommandOutcome::ClearScreen),
            "new" | "--new" | "fresh" | "--fresh" => Ok(SlashCommandOutcome::ClearConversation),
            _ => {
                renderer.line(MessageStyle::Error, "Usage: /clear [new]")?;
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
        "status" => Ok(SlashCommandOutcome::ShowStatus),
        "doctor" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /doctor")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::RunDoctor)
        }
        "mcp" => handle_mcp_command(args, renderer),
        "model" => Ok(SlashCommandOutcome::StartModelSelection),
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
        "sessions" => handle_sessions_command(args, renderer).await,
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
        "agents" => handle_agents_command(args, renderer),
        "team" => handle_team_command(args, renderer),
        "subagent" => {
            if args.trim().is_empty() || args.trim().eq_ignore_ascii_case("model") {
                return Ok(SlashCommandOutcome::ManageSubagentConfig {
                    action: SubagentConfigCommandAction::Model,
                });
            }
            renderer.line(MessageStyle::Error, "Usage: /subagent model")?;
            Ok(SlashCommandOutcome::Handled)
        }
        "plan" => handle_plan_command(args, renderer),
        "agent" => handle_agent_command(args, renderer),
        "mode" => handle_mode_command(args, renderer),
        "login" => handle_login_command(args, renderer),
        "logout" => handle_logout_command(args, renderer),
        "auth" => Ok(handle_auth_command(args)),
        "help" => {
            let specific_cmd = if args.trim().is_empty() {
                None
            } else {
                Some(args.trim())
            };
            render_help(renderer, specific_cmd)?;
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
            renderer.line(
                MessageStyle::Error,
                &format!("Unknown command '/{}'. Try /help.", command_key),
            )?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
}
