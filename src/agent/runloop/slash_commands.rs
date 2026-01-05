use anyhow::{Context, Result};
use chrono::Local;
use serde_json::Value;
use shell_words::split as shell_split;
use std::collections::BTreeMap;
use std::time::Duration;
use vtcode_core::prompts::{
    CustomPrompt, CustomPromptRegistry, CustomSlashCommandRegistry, PromptInvocation,
};
use vtcode_core::ui::slash::SLASH_COMMANDS;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemePaletteMode {
    Select,
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
    ClearConversation,
    ShowStatus,
    ManageMcp {
        action: McpCommandAction,
    },
    RunDoctor,
    DebugAgent,
    AnalyzeAgent,
    ManageLsp {
        action: LspCommandAction,
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
        action: AgentCommandAction,
    },
    SubmitPrompt {
        prompt: String,
    },
    StartTerminalSetup,
    RewindToTurn {
        turn: usize,
        scope: vtcode_core::core::agent::snapshots::RevertScope,
    },
}

#[derive(Clone, Debug)]
pub enum LspCommandAction {
    Status,
    Detect,
    Help,
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
    custom_prompts: &CustomPromptRegistry,
    custom_slash_commands: Option<&CustomSlashCommandRegistry>,
) -> Result<SlashCommandOutcome> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(SlashCommandOutcome::Handled);
    }

    let (command, rest) = split_command_and_args(trimmed);
    let command_key = command.to_ascii_lowercase();
    let args = rest.trim();

    if let Some(prompt_name) = command_key.strip_prefix("prompt:") {
        return handle_custom_prompt(prompt_name, args, renderer, custom_prompts);
    }
    if let Some(prompt_name) = command_key.strip_prefix("prompts:") {
        return handle_custom_prompt(prompt_name, args, renderer, custom_prompts);
    }

    // Check for custom slash commands
    if let Some(custom_slash_commands) = custom_slash_commands
        && custom_slash_commands.enabled()
        && custom_slash_commands.get(&command_key).is_some()
    {
        return handle_custom_slash_command(&command_key, args, renderer, custom_slash_commands);
    }

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
        "prompt" | "prompts" => {
            render_custom_prompt_list(renderer, custom_prompts)?;
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
        "clear" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /clear")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::ClearConversation)
        }
        "status" => Ok(SlashCommandOutcome::ShowStatus),
        "doctor" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /doctor")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::RunDoctor)
        }
        "mcp" => {
            if args.is_empty() {
                return Ok(SlashCommandOutcome::ManageMcp {
                    action: McpCommandAction::Overview,
                });
            }

            let tokens = match shell_split(args) {
                Ok(tokens) => tokens,
                Err(err) => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to parse arguments: {}", err),
                    )?;
                    return Ok(SlashCommandOutcome::Handled);
                }
            };

            if tokens.is_empty() {
                return Ok(SlashCommandOutcome::ManageMcp {
                    action: McpCommandAction::Overview,
                });
            }

            let subcommand = tokens[0].to_ascii_lowercase();
            match subcommand.as_str() {
                "status" | "overview" => Ok(SlashCommandOutcome::ManageMcp {
                    action: McpCommandAction::Overview,
                }),
                "list" | "providers" => Ok(SlashCommandOutcome::ManageMcp {
                    action: McpCommandAction::ListProviders,
                }),
                "tools" => Ok(SlashCommandOutcome::ManageMcp {
                    action: McpCommandAction::ListTools,
                }),
                "refresh" | "reload" => Ok(SlashCommandOutcome::ManageMcp {
                    action: McpCommandAction::RefreshTools,
                }),
                "config" => {
                    if tokens.len() > 1 {
                        let mode = tokens[1].to_ascii_lowercase();
                        match mode.as_str() {
                            "edit" | "--edit" => Ok(SlashCommandOutcome::ManageMcp {
                                action: McpCommandAction::EditConfig,
                            }),
                            "show" | "list" | "status" => Ok(SlashCommandOutcome::ManageMcp {
                                action: McpCommandAction::ShowConfig,
                            }),
                            other if other.starts_with("--") => {
                                if other == "--edit" {
                                    Ok(SlashCommandOutcome::ManageMcp {
                                        action: McpCommandAction::EditConfig,
                                    })
                                } else {
                                    render_mcp_usage(renderer)?;
                                    Ok(SlashCommandOutcome::Handled)
                                }
                            }
                            _ => {
                                render_mcp_usage(renderer)?;
                                Ok(SlashCommandOutcome::Handled)
                            }
                        }
                    } else {
                        Ok(SlashCommandOutcome::ManageMcp {
                            action: McpCommandAction::ShowConfig,
                        })
                    }
                }
                "edit" => Ok(SlashCommandOutcome::ManageMcp {
                    action: McpCommandAction::EditConfig,
                }),
                "repair" | "fix" => Ok(SlashCommandOutcome::ManageMcp {
                    action: McpCommandAction::Repair,
                }),
                "diagnose" | "diagnostics" | "health" => Ok(SlashCommandOutcome::ManageMcp {
                    action: McpCommandAction::Diagnose,
                }),
                "login" => {
                    if tokens.len() < 2 {
                        render_mcp_usage(renderer)?;
                        return Ok(SlashCommandOutcome::Handled);
                    }
                    Ok(SlashCommandOutcome::ManageMcp {
                        action: McpCommandAction::Login(tokens[1].clone()),
                    })
                }
                "logout" => {
                    if tokens.len() < 2 {
                        render_mcp_usage(renderer)?;
                        return Ok(SlashCommandOutcome::Handled);
                    }
                    Ok(SlashCommandOutcome::ManageMcp {
                        action: McpCommandAction::Logout(tokens[1].clone()),
                    })
                }
                "help" | "--help" => {
                    render_mcp_usage(renderer)?;
                    Ok(SlashCommandOutcome::Handled)
                }
                other if other.starts_with("--") => {
                    if other == "--list" {
                        return Ok(SlashCommandOutcome::ManageMcp {
                            action: McpCommandAction::ListProviders,
                        });
                    }
                    render_mcp_usage(renderer)?;
                    Ok(SlashCommandOutcome::Handled)
                }
                _ => {
                    render_mcp_usage(renderer)?;
                    Ok(SlashCommandOutcome::Handled)
                }
            }
        }
        "lsp" => {
            if args.is_empty() {
                return Ok(SlashCommandOutcome::ManageLsp {
                    action: LspCommandAction::Status,
                });
            }

            let tokens = match shell_split(args) {
                Ok(tokens) => tokens,
                Err(err) => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to parse arguments: {}", err),
                    )?;
                    return Ok(SlashCommandOutcome::Handled);
                }
            };

            if tokens.is_empty() {
                return Ok(SlashCommandOutcome::ManageLsp {
                    action: LspCommandAction::Status,
                });
            }

            let subcommand = tokens[0].to_ascii_lowercase();
            match subcommand.as_str() {
                "status" => Ok(SlashCommandOutcome::ManageLsp {
                    action: LspCommandAction::Status,
                }),
                "detect" => Ok(SlashCommandOutcome::ManageLsp {
                    action: LspCommandAction::Detect,
                }),
                _ => Ok(SlashCommandOutcome::ManageLsp {
                    action: LspCommandAction::Help,
                }),
            }
        }
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
        "add-dir" => {
            if args.is_empty() {
                render_add_dir_usage(renderer)?;
                return Ok(SlashCommandOutcome::Handled);
            }

            let tokens = match shell_split(args) {
                Ok(tokens) => tokens,
                Err(err) => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to parse arguments: {}", err),
                    )?;
                    return Ok(SlashCommandOutcome::Handled);
                }
            };

            if tokens.is_empty() {
                render_add_dir_usage(renderer)?;
                return Ok(SlashCommandOutcome::Handled);
            }

            let first = tokens[0].to_ascii_lowercase();
            if matches!(first.as_str(), "--list" | "list") {
                return Ok(SlashCommandOutcome::ManageWorkspaceDirectories {
                    command: WorkspaceDirectoryCommand::List,
                });
            }

            if matches!(first.as_str(), "--remove" | "remove") {
                if tokens.len() < 2 {
                    renderer.line(
                        MessageStyle::Error,
                        "Usage: /add-dir --remove <alias|path> [more...]",
                    )?;
                    return Ok(SlashCommandOutcome::Handled);
                }
                return Ok(SlashCommandOutcome::ManageWorkspaceDirectories {
                    command: WorkspaceDirectoryCommand::Remove(tokens[1..].to_vec()),
                });
            }

            if matches!(first.as_str(), "--help" | "help") {
                render_add_dir_usage(renderer)?;
                return Ok(SlashCommandOutcome::Handled);
            }

            Ok(SlashCommandOutcome::ManageWorkspaceDirectories {
                command: WorkspaceDirectoryCommand::Add(tokens),
            })
        }
        "sessions" => {
            let limit = args
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<usize>().ok())
                .map(|value| value.clamp(1, 25))
                .unwrap_or(5);

            if renderer.supports_inline_ui() {
                return Ok(SlashCommandOutcome::StartSessionsPalette { limit });
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
                            let duration_std =
                                duration.to_std().unwrap_or_else(|_| Duration::from_secs(0));
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
                                renderer
                                    .line(MessageStyle::Info, &format!("    Prompt: {prompt}"))?;
                            }

                            if let Some(reply) = listing.first_reply_preview() {
                                renderer
                                    .line(MessageStyle::Info, &format!("    Reply: {reply}"))?;
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
        "new" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /new")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::NewSession)
        }
        "rewind" => {
            // Parse arguments for rewind command
            let tokens: Vec<&str> = args.split_whitespace().collect();

            if tokens.is_empty() {
                // Show available snapshots when no arguments provided
                renderer.line(MessageStyle::Info, "Available rewind options:")?;
                renderer.line(
                    MessageStyle::Info,
                    "  /rewind <turn_number> - Rewind to specific turn",
                )?;
                renderer.line(
                    MessageStyle::Info,
                    "  /rewind conversation - Rewind conversation only",
                )?;
                renderer.line(
                    MessageStyle::Info,
                    "  /rewind code - Rewind code changes only",
                )?;
                renderer.line(
                    MessageStyle::Info,
                    "  /rewind both - Rewind both conversation and code",
                )?;
                return Ok(SlashCommandOutcome::Handled);
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
                // If no turn number, show available snapshots
                renderer.line(
                    MessageStyle::Info,
                    "Please specify a turn number to rewind to.",
                )?;
                renderer.line(
                    MessageStyle::Info,
                    "Use /snapshots to see available checkpoints.",
                )?;
                Ok(SlashCommandOutcome::Handled)
            }
        }
        "docs" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /docs")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::OpenDocs)
        }
        "debug" => {
            // Accept optional arguments for debugging specific targets
            if args.split_whitespace().count() > 1 {
                renderer.line(
                    MessageStyle::Error,
                    "Usage: /debug [file|directory|problem] - accepts at most one argument",
                )?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::DebugAgent)
        }
        "analyze" => {
            // Parse and validate analysis type argument
            let analysis_type = if args.trim().is_empty() {
                "full"
            } else {
                let tokens: Vec<&str> = args.split_whitespace().collect();
                if tokens.len() > 1 {
                    renderer.line(
                        MessageStyle::Error,
                        "Usage: /analyze [full|security|performance|dependencies|complexity|structure]",
                    )?;
                    return Ok(SlashCommandOutcome::Handled);
                }
                tokens[0]
            };

            // Validate analysis type
            match analysis_type {
                "full" | "security" | "performance" | "dependencies" | "complexity"
                | "structure" => {
                    // Use the AnalyzeAgent outcome to trigger the proper handler
                    Ok(SlashCommandOutcome::AnalyzeAgent)
                }
                _ => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!(
                            "Unknown analysis type '{}'. Valid types: full, security, performance, dependencies, complexity, structure",
                            analysis_type
                        ),
                    )?;
                    Ok(SlashCommandOutcome::Handled)
                }
            }
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
        "agents" => {
            if args.is_empty() {
                return Ok(SlashCommandOutcome::ManageAgents {
                    action: AgentCommandAction::List,
                });
            }

            let tokens = match shell_split(args) {
                Ok(tokens) => tokens,
                Err(err) => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to parse arguments: {}", err),
                    )?;
                    return Ok(SlashCommandOutcome::Handled);
                }
            };

            if tokens.is_empty() {
                return Ok(SlashCommandOutcome::ManageAgents {
                    action: AgentCommandAction::List,
                });
            }

            let subcommand = tokens[0].to_ascii_lowercase();
            match subcommand.as_str() {
                "list" | "ls" => Ok(SlashCommandOutcome::ManageAgents {
                    action: AgentCommandAction::List,
                }),
                "create" | "new" => Ok(SlashCommandOutcome::ManageAgents {
                    action: AgentCommandAction::Create,
                }),
                "edit" => {
                    if tokens.len() < 2 {
                        renderer.line(MessageStyle::Error, "Usage: /agents edit <agent-name>")?;
                        return Ok(SlashCommandOutcome::Handled);
                    }
                    Ok(SlashCommandOutcome::ManageAgents {
                        action: AgentCommandAction::Edit(tokens[1].clone()),
                    })
                }
                "delete" | "remove" | "rm" => {
                    if tokens.len() < 2 {
                        renderer.line(MessageStyle::Error, "Usage: /agents delete <agent-name>")?;
                        return Ok(SlashCommandOutcome::Handled);
                    }
                    Ok(SlashCommandOutcome::ManageAgents {
                        action: AgentCommandAction::Delete(tokens[1].clone()),
                    })
                }
                "help" | "--help" => {
                    renderer.line(MessageStyle::Info, "Subagent Management")?;
                    renderer.line(
                        MessageStyle::Info,
                        "Usage: /agents [list|create|edit|delete] [options]",
                    )?;
                    renderer.line(MessageStyle::Info, "")?;
                    renderer.line(
                        MessageStyle::Info,
                        "  /agents              List all available subagents",
                    )?;
                    renderer.line(
                        MessageStyle::Info,
                        "  /agents create       Create a new subagent interactively",
                    )?;
                    renderer.line(
                        MessageStyle::Info,
                        "  /agents edit NAME    Edit an existing subagent",
                    )?;
                    renderer.line(
                        MessageStyle::Info,
                        "  /agents delete NAME  Delete a subagent",
                    )?;
                    Ok(SlashCommandOutcome::Handled)
                }
                _ => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!(
                            "Unknown subcommand '/agents {}'. Try /agents help.",
                            subcommand
                        ),
                    )?;
                    Ok(SlashCommandOutcome::Handled)
                }
            }
        }
        "help" => {
            let specific_cmd = if args.trim().is_empty() {
                None
            } else {
                Some(args.trim())
            };
            render_help(renderer, specific_cmd, custom_slash_commands)?;
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

fn handle_custom_prompt(
    name: &str,
    args: &str,
    renderer: &mut AnsiRenderer,
    registry: &CustomPromptRegistry,
) -> Result<SlashCommandOutcome> {
    if !registry.enabled() {
        renderer.line(
            MessageStyle::Error,
            "Custom prompts are disabled. Set `agent.custom_prompts.enabled = true` in vtcode.toml.",
        )?;
        return Ok(SlashCommandOutcome::Handled);
    }

    if registry.is_empty() {
        renderer.line(
            MessageStyle::Error,
            "No custom prompts found. Create markdown files in your custom prompts directory or run /prompt (or /prompts) for setup guidance.",
        )?;
        return Ok(SlashCommandOutcome::Handled);
    }

    let prompt = match registry.get(name) {
        Some(prompt) => prompt,
        None => {
            renderer.line(
                MessageStyle::Error,
                &format!(
                    "Unknown custom prompt `{}`. Run /prompt (or /prompts) to list available prompts.",
                    name
                ),
            )?;
            return Ok(SlashCommandOutcome::Handled);
        }
    };

    let invocation = match PromptInvocation::parse(args) {
        Ok(invocation) => invocation,
        Err(err) => {
            renderer.line(
                MessageStyle::Error,
                &format!("Failed to parse arguments: {}", err),
            )?;
            return Ok(SlashCommandOutcome::Handled);
        }
    };

    match prompt.expand(&invocation) {
        Ok(expanded) => {
            renderer.line(
                MessageStyle::Info,
                &format!("Expanding custom prompt /prompt:{}", prompt.name),
            )?;
            Ok(SlashCommandOutcome::SubmitPrompt { prompt: expanded })
        }
        Err(err) => {
            renderer.line(MessageStyle::Error, &err.to_string())?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
}

fn handle_custom_slash_command(
    name: &str,
    args: &str,
    renderer: &mut AnsiRenderer,
    registry: &CustomSlashCommandRegistry,
) -> Result<SlashCommandOutcome> {
    if !registry.enabled() {
        renderer.line(
            MessageStyle::Error,
            "Custom slash commands are disabled. Enable them in configuration.",
        )?;
        return Ok(SlashCommandOutcome::Handled);
    }

    let command = match registry.get(name) {
        Some(command) => command,
        None => {
            renderer.line(
                MessageStyle::Error,
                &format!("Unknown custom slash command `{}`.", name),
            )?;
            return Ok(SlashCommandOutcome::Handled);
        }
    };

    // Parse arguments similar to how custom prompts work
    let invocation = match parse_command_arguments(args) {
        Ok(invocation) => invocation,
        Err(err) => {
            renderer.line(
                MessageStyle::Error,
                &format!("Failed to parse arguments: {}", err),
            )?;
            return Ok(SlashCommandOutcome::Handled);
        }
    };

    // Check if the command has bash execution (contains !`command`)
    if command.has_bash_execution {
        renderer.line(
            MessageStyle::Error,
            &format!("Command `{}` contains bash execution which is not yet supported in this implementation.", name),
        )?;
        // For now, we'll just expand the content without executing bash commands
        let expanded = expand_command_content_with_args(&command.content, &invocation);
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Expanding custom slash command /{} (bash execution skipped)",
                command.name
            ),
        )?;
        return Ok(SlashCommandOutcome::SubmitPrompt { prompt: expanded });
    }

    let expanded = expand_command_content_with_args(&command.content, &invocation);
    renderer.line(
        MessageStyle::Info,
        &format!("Expanding custom slash command /{}", command.name),
    )?;
    Ok(SlashCommandOutcome::SubmitPrompt { prompt: expanded })
}

// Parse arguments for custom slash commands (similar to custom prompts)
fn parse_command_arguments(raw: &str) -> Result<CommandInvocation> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(CommandInvocation::default());
    }

    let tokens = shell_split(trimmed)
        .with_context(|| "failed to parse custom slash command arguments".to_owned())?;

    let mut positional = Vec::new();
    let mut named = BTreeMap::new();
    for token in tokens {
        if let Some((key, value)) = token.split_once('=') {
            let key_trimmed = key.trim();
            if key_trimmed.is_empty() {
                positional.push(token);
            } else {
                named.insert(key_trimmed.to_owned(), value.to_owned());
            }
        } else {
            positional.push(token);
        }
    }

    let all_arguments = if positional.is_empty() {
        None
    } else {
        Some(positional.join(" "))
    };

    Ok(CommandInvocation {
        positional,
        named,
        all_arguments,
    })
}

#[derive(Debug, Clone, Default)]
struct CommandInvocation {
    positional: Vec<String>,
    named: BTreeMap<String, String>,
    all_arguments: Option<String>,
}

impl CommandInvocation {
    fn all_arguments(&self) -> Option<&str> {
        self.all_arguments.as_deref()
    }

    fn positional(&self) -> &[String] {
        &self.positional
    }

    fn named(&self) -> &BTreeMap<String, String> {
        &self.named
    }
}

fn expand_command_content_with_args(content: &str, invocation: &CommandInvocation) -> String {
    let mut result = content.to_string();

    // Replace $ARGUMENTS with all arguments
    if let Some(all_args) = invocation.all_arguments() {
        result = result.replace("$ARGUMENTS", all_args);
    }

    // Replace $1, $2, etc. with positional arguments
    for (i, arg) in invocation.positional().iter().enumerate() {
        let placeholder = format!("${}", i + 1);
        result = result.replace(&placeholder, arg);
    }

    // Replace named placeholders like $FILE, $TASK, etc.
    for (key, value) in invocation.named() {
        let placeholder = format!("${}", key);
        result = result.replace(&placeholder, value);
    }

    // Replace $$ with literal $
    result = result.replace("$$", "$");

    result
}

fn render_custom_prompt_list(
    renderer: &mut AnsiRenderer,
    registry: &CustomPromptRegistry,
) -> Result<()> {
    if !registry.enabled() {
        renderer.line(
            MessageStyle::Info,
            "Custom prompts are disabled. Enable them with `agent.custom_prompts.enabled = true` in vtcode.toml.",
        )?;
        return Ok(());
    }

    if registry.is_empty() {
        renderer.line(
            MessageStyle::Info,
            "No custom prompts are registered yet. Add .md files to your prompts directory and restart the session.",
        )?;
    } else {
        renderer.line(
            MessageStyle::Info,
            "Custom prompts available (invoke with /prompt:<name>):",
        )?;
        for prompt in registry.iter() {
            render_prompt_summary(renderer, prompt)?;
        }
    }

    if !registry.directories().is_empty() {
        let (existing_dirs, missing_dirs): (Vec<_>, Vec<_>) = registry
            .directories()
            .iter()
            .partition(|path| path.exists());

        if !existing_dirs.is_empty() {
            renderer.line(MessageStyle::Info, "Prompt directories:")?;
            for path in existing_dirs {
                renderer.line(MessageStyle::Info, &format!("  - {}", path.display()))?;
            }
        }

        if !missing_dirs.is_empty() {
            renderer.line(
                MessageStyle::Info,
                "Configured prompt directories (create these to enable discovery):",
            )?;
            for path in missing_dirs {
                renderer.line(MessageStyle::Info, &format!("  - {}", path.display()))?;
            }
        }
    }

    Ok(())
}

fn render_prompt_summary(renderer: &mut AnsiRenderer, prompt: &CustomPrompt) -> Result<()> {
    let mut line = format!("  /prompt:{}", prompt.name);
    if let Some(description) = &prompt.description
        && !description.trim().is_empty()
    {
        line.push_str(" — ");
        line.push_str(description.trim());
    }
    renderer.line(MessageStyle::Info, &line)?;

    if let Some(hint) = &prompt.argument_hint
        && !hint.trim().is_empty()
    {
        renderer.line(MessageStyle::Info, &format!("      hint: {}", hint.trim()))?;
    }

    Ok(())
}

fn split_command_and_args(input: &str) -> (&str, &str) {
    if let Some((idx, _)) = input.char_indices().find(|(_, ch)| ch.is_whitespace()) {
        let (command, rest) = input.split_at(idx);
        (command, rest)
    } else {
        (input, "")
    }
}

fn render_mcp_usage(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        "Usage: /mcp [status|list|tools|refresh|config|config edit|repair|diagnose|login <name>|logout <name>]",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  status  – Show overall MCP connection health",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  list    – List configured providers from vtcode.toml",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  tools   – Show tools exposed by active providers",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  refresh – Reindex MCP tools without restarting",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  config  – Summarize MCP settings from vtcode.toml",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  config edit – Show the config file path and editing guidance",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  repair  – Restart MCP connections and refresh tool indices",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  diagnose – Validate config and run MCP health checks",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  login/logout <name> – Manage OAuth sessions (if supported)",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Examples: /mcp list, /mcp tools, /mcp login github",
    )?;
    Ok(())
}

fn render_add_dir_usage(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(MessageStyle::Info, "Usage: /add-dir <path> [more paths...]")?;
    renderer.line(MessageStyle::Info, "       /add-dir --list")?;
    renderer.line(
        MessageStyle::Info,
        "       /add-dir --remove <alias|path> [more]",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Linked directories are mounted under .vtcode/external/.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Use quotes if your path contains spaces.",
    )?;
    Ok(())
}

fn render_generate_agent_file_usage(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(MessageStyle::Info, "Usage: /generate-agent-file [--force]")?;
    renderer.line(
        MessageStyle::Info,
        "  --force  Overwrite an existing AGENTS.md with regenerated content.",
    )?;
    Ok(())
}

fn format_duration_label(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 || hours > 0 {
        parts.push(format!("{}m", minutes));
    }
    parts.push(format!("{}s", seconds));
    parts.join(" ")
}

fn render_theme_list(renderer: &mut AnsiRenderer) -> Result<()> {
    let available_themes = theme::available_themes();
    renderer.line(MessageStyle::Info, "Available themes:")?;

    for theme_id in available_themes {
        if let Some(label) = theme::theme_label(theme_id) {
            renderer.line(
                MessageStyle::Info,
                &format!("  /theme {} – {}", theme_id, label),
            )?;
        } else {
            renderer.line(
                MessageStyle::Info,
                &format!("  /theme {} – {}", theme_id, theme_id),
            )?;
        }
    }

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(
        MessageStyle::Info,
        &format!("Current theme: {}", theme::active_theme_label()),
    )?;
    Ok(())
}

fn render_help(
    renderer: &mut AnsiRenderer,
    specific_command: Option<&str>,
    custom_slash_commands: Option<&CustomSlashCommandRegistry>,
) -> Result<()> {
    if let Some(cmd_name) = specific_command {
        // Look for a specific command
        if let Some(cmd) = SLASH_COMMANDS.iter().find(|cmd| cmd.name == cmd_name) {
            renderer.line(MessageStyle::Info, &format!("Help for /{}:", cmd.name))?;
            renderer.line(
                MessageStyle::Info,
                &format!("  Description: {}", cmd.description),
            )?;
            // Additional usage examples could be added here in the future
        } else if let Some(custom_slash_commands) = custom_slash_commands {
            // Check if it's a custom slash command
            if let Some(cmd) = custom_slash_commands.get(cmd_name) {
                renderer.line(MessageStyle::Info, &format!("Help for /{}:", cmd.name))?;
                if let Some(description) = &cmd.description {
                    renderer.line(
                        MessageStyle::Info,
                        &format!("  Description: {}", description),
                    )?;
                } else {
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "  Description: Custom slash command from {}",
                            cmd.path.display()
                        ),
                    )?;
                }
            } else {
                renderer.line(
                    MessageStyle::Error,
                    &format!(
                        "Unknown command '{}'. Use /help without arguments to see all commands.",
                        cmd_name
                    ),
                )?;
            }
        } else {
            renderer.line(
                MessageStyle::Error,
                &format!(
                    "Unknown command '{}'. Use /help without arguments to see all commands.",
                    cmd_name
                ),
            )?;
        }
    } else {
        // Show all commands
        renderer.line(MessageStyle::Info, "Available slash commands:")?;
        for cmd in SLASH_COMMANDS.iter() {
            renderer.line(
                MessageStyle::Info,
                &format!("  /{} – {}", cmd.name, cmd.description),
            )?;
        }

        // Add custom slash commands if available
        if let Some(custom_slash_commands) = custom_slash_commands
            && !custom_slash_commands.is_empty()
        {
            renderer.line(MessageStyle::Info, "")?;
            renderer.line(MessageStyle::Info, "Custom slash commands:")?;
            for cmd in custom_slash_commands.iter() {
                let description = cmd.description.as_deref().unwrap_or("Custom slash command");
                renderer.line(
                    MessageStyle::Info,
                    &format!("  /{} – {}", cmd.name, description),
                )?;
            }
        }

        // Show information about where custom slash commands can be defined if no custom commands are loaded or if there are none
        if custom_slash_commands.is_none_or(|cmds| cmds.is_empty()) {
            renderer.line(MessageStyle::Info, "")?;
            renderer.line(
                MessageStyle::Info,
                "Custom slash commands (project-specific or personal):",
            )?;
            renderer.line(MessageStyle::Info, "  Custom slash commands can be defined in .vtcode/commands/ (project) or ~/.vtcode/commands/ (personal)")?;
            renderer.line(
                MessageStyle::Info,
                "  Example: Create .vtcode/commands/review.md to use /review command",
            )?;
        }

        // Add information about interactive features
        renderer.line(MessageStyle::Info, "")?;
        renderer.line(MessageStyle::Info, "Interactive mode features:")?;
        renderer.line(
            MessageStyle::Info,
            "  Ctrl+C – Cancel current input or generation",
        )?;
        renderer.line(MessageStyle::Info, "  Ctrl+D – Exit VTCode session")?;
        renderer.line(MessageStyle::Info, "  Ctrl+L – Clear terminal screen")?;
        renderer.line(
            MessageStyle::Info,
            "  Ctrl+R – Reverse search command history",
        )?;
        renderer.line(MessageStyle::Info, "  Ctrl+V – Paste image from clipboard")?;
        renderer.line(
            MessageStyle::Info,
            "  Up/Down arrows – Navigate command history",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  Esc+Esc – Rewind the code/conversation",
        )?;
        renderer.line(MessageStyle::Info, "  Shift+Tab – Toggle permission modes")?;
        renderer.line(MessageStyle::Info, "")?;
        renderer.line(MessageStyle::Info, "Multiline input:")?;
        renderer.line(
            MessageStyle::Info,
            "  \\ + Enter – Quick escape (insert newline without submitting)",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  Shift+Enter – Multiline input (if configured)",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  Ctrl+J – Line feed character for multiline",
        )?;
        renderer.line(MessageStyle::Info, "")?;
        renderer.line(MessageStyle::Info, "Bash mode:")?;
        renderer.line(
            MessageStyle::Info,
            "  !command – Run bash commands directly (e.g., !ls -la)",
        )?;
        renderer.line(MessageStyle::Info, "")?;
        renderer.line(MessageStyle::Info, "Vim mode (enable with /vim):")?;
        renderer.line(
            MessageStyle::Info,
            "  i – Insert before cursor (INSERT mode)",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  a – Insert after cursor (INSERT mode)",
        )?;
        renderer.line(MessageStyle::Info, "  o – Open line below (INSERT mode)")?;
        renderer.line(MessageStyle::Info, "  Esc – Enter NORMAL mode")?;
        renderer.line(MessageStyle::Info, "  h/j/k/l – Move left/down/up/right")?;
        renderer.line(MessageStyle::Info, "  w/e/b – Move by words")?;
        renderer.line(MessageStyle::Info, "  0/$ – Move to beginning/end of line")?;
        renderer.line(MessageStyle::Info, "  dd/dw – Delete line/word")?;
        renderer.line(MessageStyle::Info, "  cc/cw – Change line/word")?;
    }
    Ok(())
}
