use anyhow::{Context, Result};
use clap::{ColorChoice as CliColorChoice, CommandFactory};
use std::path::PathBuf;
use vtcode::startup::StartupContext;
use vtcode_commons::color_policy::{self, ColorOutputPolicy, ColorOutputPolicySource};
use vtcode_core::cli::args::Cli;

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
    cmd = cmd.before_help(
        "Quick start:\n  1. Set your API key: export ANTHROPIC_API_KEY=\"your_key\"\n  2. Run: vtcode chat\n  3. First-time setup will run automatically\n\nFor help: vtcode --help",
    );

    let version_info = vtcode_core::cli::args::long_version();
    let version_leak: &'static str = Box::leak(version_info.into_boxed_str());
    cmd = cmd.long_version(version_leak);

    let help_extra = vtcode_core::cli::help::openai_responses_models_help();
    let help_leak: &'static str = Box::leak(help_extra.into_boxed_str());
    cmd = cmd.after_help(help_leak);

    cmd.after_help(
        "\n\nSlash commands (type / in chat):\n  /init     - Reconfigure provider, model, and settings\n  /config   - Browse settings sections\n  /status   - Show current configuration\n  /doctor   - Diagnose setup issues (inline picker, or use --quick/--full)\n  /update   - Check for VT Code updates (use --list, --pin, --channel)\n  /plan     - Toggle read-only planning mode\n  /theme    - Switch UI theme\n  /history  - Open command history picker\n  /help     - Show all slash commands",
    )
}

pub(crate) async fn resolve_startup_context(
    args: &Cli,
) -> Result<(StartupContext, Option<String>)> {
    if let Some(workspace_path) = &args.workspace_path
        && (!workspace_path.exists() || !workspace_path.is_dir())
    {
        let prompt_text = workspace_path.to_string_lossy().to_string();
        let mut modified_args = args.clone();
        modified_args.workspace_path =
            Some(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let startup = StartupContext::from_cli_args(&modified_args)
            .await
            .context("failed to initialize VT Code startup context")?;
        return Ok((startup, Some(prompt_text)));
    }

    let startup = StartupContext::from_cli_args(args)
        .await
        .context("failed to initialize VT Code startup context")?;
    Ok((startup, None))
}
