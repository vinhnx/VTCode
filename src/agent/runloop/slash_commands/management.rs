use anyhow::Result;
use shell_words::split as shell_split;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::rendering::{render_add_dir_usage, render_mcp_usage};
use super::{McpCommandAction, SlashCommandOutcome, WorkspaceDirectoryCommand};

pub(super) fn handle_mcp_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
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

pub(super) fn handle_add_dir_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
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
