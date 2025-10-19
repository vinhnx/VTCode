use anyhow::Result;
use chrono::Local;
use serde_json::{Map, Value};
use shell_words::split as shell_split;
use std::time::Duration;
use vtcode_core::prompts::{CustomPrompt, CustomPromptRegistry, PromptInvocation};
use vtcode_core::ui::slash::SLASH_COMMANDS;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemePaletteMode {
    Select,
    Inspect,
}

pub enum SlashCommandOutcome {
    Handled,
    ThemeChanged(String),
    #[allow(dead_code)]
    ExecuteTool {
        name: String,
        args: Value,
    },
    InitializeWorkspace {
        force: bool,
    },
    ShowConfig,
    Exit,
    StartModelSelection,
    StartThemePalette {
        mode: ThemePaletteMode,
    },
    StartSessionsPalette {
        limit: usize,
    },
    StartHelpPalette,
    ClearConversation,
    ShowStatus,
    ShowCost,
    ManageMcp {
        action: McpCommandAction,
    },
    RunDoctor,
    ManageWorkspaceDirectories {
        command: WorkspaceDirectoryCommand,
    },
    SubmitPrompt {
        prompt: String,
    },
}

#[derive(Clone, Debug)]
pub enum McpCommandAction {
    Overview,
    ListProviders,
    ListTools,
    RefreshTools,
    Login(String),
    Logout(String),
}

#[derive(Clone, Debug)]
pub enum WorkspaceDirectoryCommand {
    Add(Vec<String>),
    List,
    Remove(Vec<String>),
}

pub fn handle_slash_command(
    input: &str,
    renderer: &mut AnsiRenderer,
    custom_prompts: &CustomPromptRegistry,
) -> Result<SlashCommandOutcome> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(SlashCommandOutcome::Handled);
    }

    let (command, rest) = split_command_and_args(trimmed);
    let command_key = command.to_ascii_lowercase();
    let args = rest.trim();

    if let Some(prompt_name) = command_key.strip_prefix("prompts:") {
        return handle_custom_prompt(prompt_name, args, renderer, custom_prompts);
    }

    match command_key.as_str() {
        "prompts" => {
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
            }

            renderer.line(MessageStyle::Error, "Usage: /theme <theme-id>")?;
            Ok(SlashCommandOutcome::Handled)
        }
        "help" => {
            if renderer.supports_inline_ui() {
                return Ok(SlashCommandOutcome::StartHelpPalette);
            }
            renderer.line(MessageStyle::Info, "Available commands:")?;
            for info in SLASH_COMMANDS.iter() {
                renderer.line(
                    MessageStyle::Info,
                    &format!("  /{} - {}", info.name, info.description),
                )?;
            }
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "  Themes available: {}",
                    theme::available_themes().join(", ")
                ),
            )?;
            Ok(SlashCommandOutcome::Handled)
        }
        "list-themes" => {
            if renderer.supports_inline_ui() {
                return Ok(SlashCommandOutcome::StartThemePalette {
                    mode: ThemePaletteMode::Inspect,
                });
            }
            renderer.line(MessageStyle::Info, "Available themes:")?;
            for id in theme::available_themes() {
                let marker = if theme::active_theme_id() == id {
                    "*"
                } else {
                    " "
                };
                let label = theme::theme_label(id).unwrap_or(id);
                renderer.line(
                    MessageStyle::Info,
                    &format!("{} {} ({})", marker, id, label),
                )?;
            }
            Ok(SlashCommandOutcome::Handled)
        }
        "command" => {
            if args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /command <program> [args...]")?;
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
                renderer.line(MessageStyle::Error, "Usage: /command <program> [args...]")?;
                return Ok(SlashCommandOutcome::Handled);
            }

            let mut command_vec = Vec::new();
            command_vec.push(Value::String(tokens[0].clone()));
            command_vec.extend(
                tokens
                    .iter()
                    .skip(1)
                    .map(|segment| Value::String(segment.clone())),
            );

            let mut args_map = Map::new();
            args_map.insert("command".to_string(), Value::Array(command_vec));
            Ok(SlashCommandOutcome::ExecuteTool {
                name: "run_terminal_cmd".to_string(),
                args: Value::Object(args_map),
            })
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
        "config" => Ok(SlashCommandOutcome::ShowConfig),
        "clear" => {
            if !args.is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /clear")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            Ok(SlashCommandOutcome::ClearConversation)
        }
        "status" => Ok(SlashCommandOutcome::ShowStatus),
        "cost" => Ok(SlashCommandOutcome::ShowCost),
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
        "model" => Ok(SlashCommandOutcome::StartModelSelection),
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

            match session_archive::list_recent_sessions(limit) {
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
        "exit" => Ok(SlashCommandOutcome::Exit),
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
            "No custom prompts found. Create markdown files in your custom prompts directory or run /prompts for setup guidance.",
        )?;
        return Ok(SlashCommandOutcome::Handled);
    }

    let prompt = match registry.get(name) {
        Some(prompt) => prompt,
        None => {
            renderer.line(
                MessageStyle::Error,
                &format!(
                    "Unknown custom prompt `{}`. Run /prompts to list available prompts.",
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
                &format!("Expanding custom prompt /prompts:{}", prompt.name),
            )?;
            Ok(SlashCommandOutcome::SubmitPrompt { prompt: expanded })
        }
        Err(err) => {
            renderer.line(MessageStyle::Error, &format!("{}", err))?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
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
            "Custom prompts available (invoke with /prompts:<name>):",
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
    let mut line = format!("  /prompts:{}", prompt.name);
    if let Some(description) = &prompt.description {
        if !description.trim().is_empty() {
            line.push_str(" — ");
            line.push_str(description.trim());
        }
    }
    renderer.line(MessageStyle::Info, &line)?;

    if let Some(hint) = &prompt.argument_hint {
        if !hint.trim().is_empty() {
            renderer.line(MessageStyle::Info, &format!("      hint: {}", hint.trim()))?;
        }
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
        "Usage: /mcp [status|list|tools|refresh|login <name>|logout <name>]",
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
