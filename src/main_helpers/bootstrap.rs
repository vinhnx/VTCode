use anyhow::{Context, Result};
use clap::builder::Styles;
use clap::builder::styling::{AnsiColor, Effects};
use clap::{ColorChoice as CliColorChoice, CommandFactory};
use vtcode_commons::color_policy::{self, ColorOutputPolicy, ColorOutputPolicySource};
use vtcode_core::cli::args::Cli;
use vtcode_core::config::loader::ConfigManager;

use crate::startup::StartupContext;

fn env_flag_enabled(var_name: &str) -> bool {
    std::env::var(var_name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on" | "debug"
            )
        })
        .unwrap_or(false)
}

pub(crate) fn debug_runtime_flag_enabled(debug_arg_enabled: bool, env_var: &str) -> bool {
    cfg!(debug_assertions) && (debug_arg_enabled || env_flag_enabled(env_var))
}

pub(crate) fn resolve_runtime_color_policy(args: &Cli) -> ColorOutputPolicy {
    if args.no_color {
        return ColorOutputPolicy {
            enabled: false,
            source: ColorOutputPolicySource::CliNoColor,
        };
    }

    match args.color.color {
        CliColorChoice::Always => ColorOutputPolicy {
            enabled: true,
            source: ColorOutputPolicySource::CliColorAlways,
        },
        CliColorChoice::Never => ColorOutputPolicy {
            enabled: false,
            source: ColorOutputPolicySource::CliColorNever,
        },
        CliColorChoice::Auto => {
            if color_policy::no_color_env_active() {
                ColorOutputPolicy {
                    enabled: false,
                    source: ColorOutputPolicySource::NoColorEnv,
                }
            } else {
                ColorOutputPolicy {
                    enabled: true,
                    source: ColorOutputPolicySource::DefaultAuto,
                }
            }
        }
    }
}

pub(crate) fn build_augmented_cli_command() -> clap::Command {
    let mut cmd = Cli::command();
    if let Some(choice) = requested_help_color_choice() {
        cmd = cmd.color(choice);
    }
    cmd = cmd.styles(clap_help_styles());
    cmd = cmd.before_help(build_quick_start_help());

    let version_info = vtcode_core::cli::args::long_version();
    let version_leak: &'static str = Box::leak(version_info.into_boxed_str());
    cmd = cmd.long_version(version_leak);

    let after_help = "\nSlash commands (type / in chat):\n  /init     - Guided AGENTS.md + workspace setup\n  /config   - Browse settings sections\n  /status   - Show current configuration\n  /doctor   - Diagnose setup issues (inline picker, or use --quick/--full)\n  /update   - Check for VT Code updates (use --list, --pin, --channel)\n  /plan     - Toggle read-only planning mode\n  /loop     - Schedule a recurring prompt in this session\n  /schedule - Open the durable scheduled-task manager\n  /theme    - Switch UI theme\n  /title    - Configure terminal title items\n  /history  - Open command history picker\n  /help     - Show all slash commands";
    cmd.after_help(after_help)
}

fn clap_help_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::BrightBlue.on_default().effects(Effects::BOLD))
        .usage(AnsiColor::BrightBlue.on_default().effects(Effects::BOLD))
        .literal(AnsiColor::BrightGreen.on_default().effects(Effects::BOLD))
        .placeholder(AnsiColor::BrightCyan.on_default())
}

fn requested_help_color_choice() -> Option<CliColorChoice> {
    let mut requested = None;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        if arg == "--no-color" {
            requested = Some(CliColorChoice::Never);
            continue;
        }

        if let Some(value) = arg.strip_prefix("--color=") {
            if let Some(choice) = parse_help_color_choice(value) {
                requested = Some(choice);
            }
            continue;
        }

        if arg == "--color"
            && let Some(value) = args.next()
            && let Some(choice) = parse_help_color_choice(&value)
        {
            requested = Some(choice);
        }
    }

    requested
}

fn build_quick_start_help() -> String {
    if has_provider_or_model_configuration() {
        "Quick start:\n  1. Start interactive chat: vtcode chat\n  2. Run one prompt directly: vtcode --print \"summarize this repository\"\n\nUse `vtcode <command> --help` for command-specific details.".to_string()
    } else {
        "Quick start:\n  1. Export your provider API key (examples: OPENAI_API_KEY, ANTHROPIC_API_KEY, GEMINI_API_KEY)\n  2. Start chat with a provider/model: vtcode chat --provider openai --model gpt-5\n  3. Run one prompt directly: vtcode --provider anthropic --model claude-sonnet-4-6 --print \"summarize this repository\"\n\nUse `vtcode <command> --help` for command-specific details.".to_string()
    }
}

fn has_provider_or_model_configuration() -> bool {
    cli_args_include_provider_or_model() || config_includes_provider_or_model()
}

fn cli_args_include_provider_or_model() -> bool {
    std::env::args().skip(1).any(|arg| {
        arg == "--provider"
            || arg == "--model"
            || arg.starts_with("--provider=")
            || arg.starts_with("--model=")
    })
}

fn config_includes_provider_or_model() -> bool {
    let Ok(manager) = ConfigManager::load() else {
        return false;
    };
    has_provider_or_model_keys(&manager.effective_config())
}

fn has_provider_or_model_keys(config: &toml::Value) -> bool {
    let Some(root) = config.as_table() else {
        return false;
    };

    root.contains_key("provider")
        || root.contains_key("model")
        || root
            .get("agent")
            .and_then(toml::Value::as_table)
            .is_some_and(|agent| {
                agent.contains_key("provider")
                    || agent.contains_key("model")
                    || agent.contains_key("default_model")
            })
}

fn parse_help_color_choice(value: &str) -> Option<CliColorChoice> {
    match value.trim().to_ascii_lowercase().as_str() {
        "always" => Some(CliColorChoice::Always),
        "auto" => Some(CliColorChoice::Auto),
        "never" => Some(CliColorChoice::Never),
        _ => None,
    }
}

pub(crate) async fn resolve_startup_context(args: &Cli) -> Result<StartupContext> {
    let startup = StartupContext::from_cli_args(args)
        .await
        .context("failed to initialize VT Code startup context")?;
    Ok(startup)
}

#[cfg(test)]
mod tests {
    use super::build_augmented_cli_command;
    use clap::Parser;
    use vtcode_core::cli::args::Cli;

    #[test]
    fn invalid_positional_workspace_fails_during_cli_parse() {
        let mut command = build_augmented_cli_command();
        let err = command
            .try_get_matches_from_mut(["vtcode", "hellp"])
            .expect_err("invalid positional workspace should fail at clap parsing");
        let err_text = err.to_string();
        assert!(
            err_text.contains("Workspace path does not exist"),
            "unexpected clap error: {err_text}"
        );
    }

    #[test]
    fn invalid_positional_workspace_fails_with_derive_parser_too() {
        let err = Cli::try_parse_from(["vtcode", "hellp"])
            .expect_err("invalid positional workspace should fail at derive parser");
        let err_text = err.to_string();
        assert!(
            err_text.contains("Workspace path does not exist"),
            "unexpected clap error: {err_text}"
        );
    }
}
