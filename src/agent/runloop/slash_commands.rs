use anyhow::Result;
use serde_json::{Map, Value};
use vtcode_core::ui::slash::SLASH_COMMANDS;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

pub enum SlashCommandOutcome {
    Handled,
    ThemeChanged(String),
    ExecuteTool { name: String, args: Value },
    Exit,
}

pub fn handle_slash_command(
    input: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    let mut parts = input.split_whitespace();
    let command = parts.next().unwrap_or("").to_lowercase();
    if command.is_empty() {
        return Ok(SlashCommandOutcome::Handled);
    }

    match command.as_str() {
        "theme" => {
            let Some(next_theme) = parts.next() else {
                renderer.line(MessageStyle::Error, "Usage: /theme <theme-id>")?;
                return Ok(SlashCommandOutcome::Handled);
            };
            let desired = next_theme.to_lowercase();
            match theme::set_active_theme(&desired) {
                Ok(()) => {
                    let label = theme::active_theme_label();
                    renderer.line(MessageStyle::Info, &format!("Theme switched to {}", label))?;
                    return Ok(SlashCommandOutcome::ThemeChanged(theme::active_theme_id()));
                }
                Err(err) => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!("Theme '{}' not available: {}", next_theme, err),
                    )?;
                }
            }
            Ok(SlashCommandOutcome::Handled)
        }
        "help" => {
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
            let program = parts.next();
            if program.is_none() {
                renderer.line(MessageStyle::Error, "Usage: /command <program> [args...]")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            let mut command_vec = Vec::new();
            command_vec.push(Value::String(program.unwrap().to_string()));
            command_vec.extend(parts.map(|segment| Value::String(segment.to_string())));

            let mut args_map = Map::new();
            args_map.insert("command".to_string(), Value::Array(command_vec));
            Ok(SlashCommandOutcome::ExecuteTool {
                name: "run_terminal_cmd".to_string(),
                args: Value::Object(args_map),
            })
        }
        "exit" => Ok(SlashCommandOutcome::Exit),
        _ => {
            renderer.line(
                MessageStyle::Error,
                &format!("Unknown command '/{}'. Try /help.", command),
            )?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
}
