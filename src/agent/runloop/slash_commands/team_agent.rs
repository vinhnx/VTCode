use anyhow::Result;
use shell_words::split as shell_split;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::parsing::{extract_flag_value, parse_depends_on};
use super::{AgentCommandAction, SlashCommandOutcome, TeamCommandAction};

pub(super) fn handle_agents_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
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

pub(super) fn handle_team_command(
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    if args.is_empty() {
        return Ok(SlashCommandOutcome::ManageTeams {
            action: TeamCommandAction::Help,
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
        return Ok(SlashCommandOutcome::ManageTeams {
            action: TeamCommandAction::Help,
        });
    }

    let subcommand = tokens[0].to_ascii_lowercase();
    let mut args = tokens.iter().skip(1).cloned().collect::<Vec<_>>();

    match subcommand.as_str() {
        "start" => {
            let model = extract_flag_value(&mut args, "--model");
            Ok(SlashCommandOutcome::ManageTeams {
                action: TeamCommandAction::Start {
                    name: args.first().cloned(),
                    count: args.get(1).and_then(|v| v.parse::<usize>().ok()),
                    subagent_type: args.get(2).cloned(),
                    model,
                },
            })
        }
        "add" => {
            let model = extract_flag_value(&mut args, "--model");
            if args.is_empty() {
                return Ok(SlashCommandOutcome::ManageTeams {
                    action: TeamCommandAction::Help,
                });
            }
            Ok(SlashCommandOutcome::ManageTeams {
                action: TeamCommandAction::Add {
                    name: args[0].clone(),
                    subagent_type: args.get(1).cloned(),
                    model,
                },
            })
        }
        "remove" | "rm" => {
            if args.is_empty() {
                return Ok(SlashCommandOutcome::ManageTeams {
                    action: TeamCommandAction::Help,
                });
            }
            Ok(SlashCommandOutcome::ManageTeams {
                action: TeamCommandAction::Remove {
                    name: args[0].clone(),
                },
            })
        }
        "task" => {
            if args.is_empty() {
                return Ok(SlashCommandOutcome::ManageTeams {
                    action: TeamCommandAction::Help,
                });
            }
            handle_team_task_command(&args)
        }
        "tasks" => Ok(SlashCommandOutcome::ManageTeams {
            action: TeamCommandAction::Tasks,
        }),
        "assign" => {
            if args.len() < 2 {
                return Ok(SlashCommandOutcome::ManageTeams {
                    action: TeamCommandAction::Help,
                });
            }
            let task_id = args[0].parse::<u64>().ok();
            if task_id.is_none() {
                return Ok(SlashCommandOutcome::ManageTeams {
                    action: TeamCommandAction::Help,
                });
            }
            Ok(SlashCommandOutcome::ManageTeams {
                action: TeamCommandAction::Assign {
                    task_id: task_id.unwrap(),
                    teammate: args[1].clone(),
                },
            })
        }
        "message" => {
            if args.len() < 2 {
                return Ok(SlashCommandOutcome::ManageTeams {
                    action: TeamCommandAction::Help,
                });
            }
            Ok(SlashCommandOutcome::ManageTeams {
                action: TeamCommandAction::Message {
                    recipient: args[0].clone(),
                    message: args.iter().skip(1).cloned().collect::<Vec<_>>().join(" "),
                },
            })
        }
        "broadcast" => {
            if args.is_empty() {
                return Ok(SlashCommandOutcome::ManageTeams {
                    action: TeamCommandAction::Help,
                });
            }
            Ok(SlashCommandOutcome::ManageTeams {
                action: TeamCommandAction::Broadcast {
                    message: args.join(" "),
                },
            })
        }
        "teammates" => Ok(SlashCommandOutcome::ManageTeams {
            action: TeamCommandAction::Teammates,
        }),
        "model" => Ok(SlashCommandOutcome::ManageTeams {
            action: TeamCommandAction::Model,
        }),
        "stop" | "end" => Ok(SlashCommandOutcome::ManageTeams {
            action: TeamCommandAction::Stop,
        }),
        "help" | "--help" => Ok(SlashCommandOutcome::ManageTeams {
            action: TeamCommandAction::Help,
        }),
        _ => Ok(SlashCommandOutcome::ManageTeams {
            action: TeamCommandAction::Help,
        }),
    }
}

fn handle_team_task_command(args: &[String]) -> Result<SlashCommandOutcome> {
    let action = args[0].to_ascii_lowercase();
    match action.as_str() {
        "add" => {
            let mut task_args = args.iter().skip(1).cloned().collect::<Vec<_>>();
            let depends_value = extract_flag_value(&mut task_args, "--depends-on");
            let depends_on = depends_value
                .as_deref()
                .map(parse_depends_on)
                .unwrap_or_default();
            let description = task_args.join(" ");
            if description.is_empty() {
                return Ok(SlashCommandOutcome::ManageTeams {
                    action: TeamCommandAction::Help,
                });
            }
            Ok(SlashCommandOutcome::ManageTeams {
                action: TeamCommandAction::TaskAdd {
                    description,
                    depends_on,
                },
            })
        }
        "claim" => {
            let task_id = args.get(1).and_then(|v| v.parse::<u64>().ok());
            if task_id.is_none() {
                return Ok(SlashCommandOutcome::ManageTeams {
                    action: TeamCommandAction::Help,
                });
            }
            Ok(SlashCommandOutcome::ManageTeams {
                action: TeamCommandAction::TaskClaim {
                    task_id: task_id.unwrap(),
                },
            })
        }
        "complete" => {
            let task_id = args.get(1).and_then(|v| v.parse::<u64>().ok());
            if task_id.is_none() {
                return Ok(SlashCommandOutcome::ManageTeams {
                    action: TeamCommandAction::Help,
                });
            }
            let summary = if args.len() > 2 {
                Some(args.iter().skip(2).cloned().collect::<Vec<_>>().join(" "))
            } else {
                None
            };
            Ok(SlashCommandOutcome::ManageTeams {
                action: TeamCommandAction::TaskComplete {
                    task_id: task_id.unwrap(),
                    success: true,
                    summary,
                },
            })
        }
        "fail" => {
            let task_id = args.get(1).and_then(|v| v.parse::<u64>().ok());
            if task_id.is_none() {
                return Ok(SlashCommandOutcome::ManageTeams {
                    action: TeamCommandAction::Help,
                });
            }
            let summary = if args.len() > 2 {
                Some(args.iter().skip(2).cloned().collect::<Vec<_>>().join(" "))
            } else {
                None
            };
            Ok(SlashCommandOutcome::ManageTeams {
                action: TeamCommandAction::TaskComplete {
                    task_id: task_id.unwrap(),
                    success: false,
                    summary,
                },
            })
        }
        _ => Ok(SlashCommandOutcome::ManageTeams {
            action: TeamCommandAction::Help,
        }),
    }
}
