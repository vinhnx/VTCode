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
use flow::{
    handle_auth_command, handle_login_command, handle_logout_command, handle_mode_command,
    handle_plan_command, handle_resume_command, handle_review_command, handle_rewind_command,
};
use management::{handle_add_dir_command, handle_mcp_command};
use parsing::{parse_session_log_export_format, split_command_and_args};
use rendering::{render_generate_agent_file_usage, render_help, render_theme_list};

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

    ShowSettings,
    Exit,
    NewSession,
    OpenDocs,
    StartModelSelection,
    StartThemePalette {
        mode: ThemePaletteMode,
    },
    StartResumePalette {
        limit: usize,
    },
    StartHistoryPicker,
    StartFileBrowser {
        initial_filter: Option<String>,
    },
    ClearScreen,
    ClearConversation,
    CompactConversation,
    CopyLatestAssistantReply,
    ShowStatus,
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
    SubmitPrompt {
        prompt: String,
    },
    StartTerminalSetup,
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
pub enum McpCommandAction {
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
        "status" => Ok(SlashCommandOutcome::ShowStatus),
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
        "resume" => handle_resume_command(args, renderer).await,
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
        "plan" => handle_plan_command(args, renderer),
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
    use super::{DoctorCommand, parse_doctor_args, parse_update_args};

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
}
