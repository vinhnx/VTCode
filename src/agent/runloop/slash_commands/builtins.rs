use std::path::Path;

use anyhow::Result;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::skills::CommandSkillSpec;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::flow::{
    handle_auth_command, handle_fork_command, handle_login_command, handle_logout_command,
    handle_mode_command, handle_plan_command, handle_resume_command, handle_rewind_command,
};
use super::management::{handle_loop_command, handle_mcp_command, handle_schedule_command};
use super::models::{
    AgentDefinitionScope, AgentManagerAction, SlashCommandOutcome, SubprocessManagerAction,
    ThemePaletteMode,
};
use super::parsing::{self, parse_compact_command, parse_session_log_export_format};
use super::rendering::{render_help, render_theme_list};

pub(in crate::agent::runloop::slash_commands) async fn execute_built_in_command_skill(
    spec: &'static CommandSkillSpec,
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
            }

            renderer.line(MessageStyle::Info, "Provide a theme name to switch themes")?;
            render_theme_list(renderer)?;
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
        "statusline" => Ok(SlashCommandOutcome::StartStatuslineSetup {
            instructions: (!args.trim().is_empty()).then(|| args.trim().to_string()),
        }),
        "title" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /title")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::StartTerminalTitleSetup)
        }
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
        "notify" => Ok(SlashCommandOutcome::Notify {
            message: if args.is_empty() {
                "Manual notification from /notify".to_string()
            } else {
                args.to_string()
            },
        }),
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
        "effort" => match parse_effort_args(args) {
            Ok((level, persist)) => Ok(SlashCommandOutcome::SetEffort { level, persist }),
            Err(message) => {
                renderer.line(MessageStyle::Error, &message)?;
                renderer.line(
                    MessageStyle::Info,
                    "Usage: /effort [--persist] [none|minimal|low|medium|high|xhigh|max]",
                )?;
                Ok(SlashCommandOutcome::Handled)
            }
        },
        "hooks" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /hooks")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::ShowHooks)
        }
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
        "share" => match parse_session_log_export_format(args) {
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
            let full_command = format!("/{}", input);
            match crate::agent::runloop::parse_skill_command(&full_command) {
                Ok(Some(action)) => Ok(SlashCommandOutcome::ManageSkills { action }),
                Ok(None) => {
                    renderer.line(MessageStyle::Error, "Skills command parse error")?;
                    Ok(SlashCommandOutcome::Handled)
                }
                Err(error) => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!("Skills command error: {}", error),
                    )?;
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
        "refresh-oauth" => super::flow::handle_refresh_oauth_command(args, renderer),
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

pub(in crate::agent::runloop::slash_commands) fn parse_subprocesses_command(
    args: &str,
) -> std::result::Result<SubprocessManagerAction, String> {
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
                    "Usage: /update [check|install] [--force]\nExamples: /update, /update check, /update install --force".to_string(),
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

pub(in crate::agent::runloop::slash_commands) fn parse_effort_args(
    args: &str,
) -> std::result::Result<(Option<ReasoningEffortLevel>, bool), String> {
    let mut persist = false;
    let mut level = None;

    parsing::for_each_token(args, |token| {
        match token {
            "--persist" | "persist" => persist = true,
            _ => {
                let Some(parsed) = ReasoningEffortLevel::parse(token) else {
                    return Err(format!("Unknown effort value '{}'", token));
                };
                if level.replace(parsed).is_some() {
                    return Err("Specify at most one effort level.".to_string());
                }
            }
        }
        Ok(())
    })?;

    Ok((level, persist))
}

#[derive(Debug)]
pub(in crate::agent::runloop::slash_commands) enum DoctorCommand {
    Interactive,
    Run { quick: bool },
}

pub(in crate::agent::runloop::slash_commands) fn parse_doctor_args(
    args: &str,
    supports_inline_ui: bool,
) -> std::result::Result<DoctorCommand, String> {
    let mut quick = false;
    let mut full = false;

    parsing::for_each_token(args, |token| {
        match token {
            "--quick" | "-q" | "quick" => quick = true,
            "--full" | "full" => full = true,
            _ => {
                return Err(
                    "Usage: /doctor [--quick|--full]\nExamples: /doctor, /doctor --quick"
                        .to_string(),
                );
            }
        }
        Ok(())
    })?;

    if quick && full {
        return Err("Use either --quick or --full, not both.".to_string());
    }

    if !quick && !full && supports_inline_ui {
        return Ok(DoctorCommand::Interactive);
    }

    Ok(DoctorCommand::Run { quick })
}
