use anyhow::Result;
use shell_words::split as shell_split;
use vtcode_core::llm::providers::local_server::LocalProvider;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::rendering::{render_local_usage, render_mcp_usage};
use super::{LocalServerAction, McpCommandAction, SlashCommandOutcome};

pub(super) fn handle_mcp_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    if args.is_empty() {
        return Ok(SlashCommandOutcome::ManageMcp { action: McpCommandAction::Interactive });
    }

    let tokens = match shell_split(args) {
        Ok(tokens) => tokens,
        Err(err) => {
            renderer.line(MessageStyle::Error, &format!("Failed to parse arguments: {err}"))?;
            return Ok(SlashCommandOutcome::Handled);
        }
    };

    if tokens.is_empty() {
        return Ok(SlashCommandOutcome::ManageMcp { action: McpCommandAction::Interactive });
    }

    let subcommand = tokens[0].to_ascii_lowercase();
    match subcommand.as_str() {
        "status" | "overview" => {
            Ok(SlashCommandOutcome::ManageMcp { action: McpCommandAction::Overview })
        }
        "list" | "providers" => {
            Ok(SlashCommandOutcome::ManageMcp { action: McpCommandAction::ListProviders })
        }
        "tools" => Ok(SlashCommandOutcome::ManageMcp { action: McpCommandAction::ListTools }),
        "refresh" | "reload" => {
            Ok(SlashCommandOutcome::ManageMcp { action: McpCommandAction::RefreshTools })
        }
        "config" => {
            if tokens.len() > 1 {
                let mode = tokens[1].to_ascii_lowercase();
                match mode.as_str() {
                    "edit" | "--edit" => {
                        Ok(SlashCommandOutcome::ManageMcp { action: McpCommandAction::EditConfig })
                    }
                    "show" | "list" | "status" => {
                        Ok(SlashCommandOutcome::ManageMcp { action: McpCommandAction::ShowConfig })
                    }
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
                Ok(SlashCommandOutcome::ManageMcp { action: McpCommandAction::ShowConfig })
            }
        }
        "edit" => Ok(SlashCommandOutcome::ManageMcp { action: McpCommandAction::EditConfig }),
        "repair" | "fix" => Ok(SlashCommandOutcome::ManageMcp { action: McpCommandAction::Repair }),
        "diagnose" | "diagnostics" | "health" => {
            Ok(SlashCommandOutcome::ManageMcp { action: McpCommandAction::Diagnose })
        }
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

pub(super) fn handle_local_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok(SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Interactive,
        });
    }

    let tokens = match shell_split(trimmed) {
        Ok(tokens) => tokens,
        Err(err) => {
            renderer.line(MessageStyle::Error, &format!("Failed to parse arguments: {err}"))?;
            render_local_usage(renderer)?;
            return Ok(SlashCommandOutcome::Handled);
        }
    };

    if tokens.is_empty() {
        return Ok(SlashCommandOutcome::ManageLocalServer {
            action: LocalServerAction::Interactive,
        });
    }

    // Normalize a provider argument to its canonical key (e.g. "lm-studio" -> "lmstudio")
    let normalize = |s: &str| -> Option<String> {
        LocalProvider::from_key(&s.to_ascii_lowercase()).map(|p| p.key().to_string())
    };

    let first = tokens[0].to_ascii_lowercase();
    match first.as_str() {
        "status" => {
            let provider = tokens.get(1).and_then(|s| normalize(s));
            Ok(SlashCommandOutcome::ManageLocalServer {
                action: LocalServerAction::Status { provider },
            })
        }
        "start" => {
            let provider = tokens.get(1).and_then(|s| normalize(s));
            Ok(SlashCommandOutcome::ManageLocalServer {
                action: LocalServerAction::Start { provider },
            })
        }
        "stop" => {
            let provider = tokens.get(1).and_then(|s| normalize(s));
            Ok(SlashCommandOutcome::ManageLocalServer {
                action: LocalServerAction::Stop { provider },
            })
        }
        "configure" | "config" => {
            let provider = tokens.get(1).and_then(|s| normalize(s));
            Ok(SlashCommandOutcome::ManageLocalServer {
                action: LocalServerAction::Configure { provider },
            })
        }
        "troubleshoot" | "diagnose" | "fix" => {
            let provider = tokens.get(1).and_then(|s| normalize(s));
            Ok(SlashCommandOutcome::ManageLocalServer {
                action: LocalServerAction::Troubleshoot { provider },
            })
        }
        "help" | "--help" => {
            render_local_usage(renderer)?;
            Ok(SlashCommandOutcome::Handled)
        }
        _ => {
            // Check if it's a valid provider name (from_key handles aliases)
            if let Some(resolved) = LocalProvider::from_key(&first) {
                let canonical = resolved.key().to_string();
                if tokens.len() > 1 {
                    let action_str = tokens[1].to_ascii_lowercase();
                    let action = match action_str.as_str() {
                        "status" => LocalServerAction::Status { provider: Some(canonical.clone()) },
                        "start" => LocalServerAction::Start { provider: Some(canonical.clone()) },
                        "stop" => LocalServerAction::Stop { provider: Some(canonical.clone()) },
                        "configure" | "config" => {
                            LocalServerAction::Configure { provider: Some(canonical.clone()) }
                        }
                        "troubleshoot" | "diagnose" | "fix" => {
                            LocalServerAction::Troubleshoot { provider: Some(canonical.clone()) }
                        }
                        _ => {
                            render_local_usage(renderer)?;
                            return Ok(SlashCommandOutcome::Handled);
                        }
                    };
                    Ok(SlashCommandOutcome::ManageLocalServer { action })
                } else {
                    Ok(SlashCommandOutcome::ManageLocalServer {
                        action: LocalServerAction::Provider { name: canonical },
                    })
                }
            } else {
                render_local_usage(renderer)?;
                Ok(SlashCommandOutcome::Handled)
            }
        }
    }
}
