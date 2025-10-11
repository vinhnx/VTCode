//! VTCode - Research-preview Rust coding agent
//!
//! Thin binary entry point that delegates to modular CLI handlers.

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use colorchoice::ColorChoice as GlobalColorChoice;
use std::path::PathBuf;
use std::str::FromStr;
use tracing_subscriber;
use vtcode_core::cli::args::{Cli, Commands};
use vtcode_core::config::api_keys::{ApiKeySources, get_api_key, load_dotenv};
use vtcode_core::config::constants::defaults;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, ModelSelectionSource};
use vtcode_core::ui::theme::{self as ui_theme, DEFAULT_THEME_ID};
use vtcode_core::{initialize_dot_folder, load_user_config, update_theme_preference};

mod acp;
mod agent;
mod cli; // local CLI handlers in src/cli // agent runloops (single-agent only)
mod workspace_trust;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    if std::env::var("RUST_LOG").is_ok() {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .init();
    }

    // Load .env (non-fatal if missing)
    load_dotenv().ok();

    let args = Cli::parse();
    args.color.write_global();
    if args.no_color {
        GlobalColorChoice::Never.write_global();
    }

    // Resolve workspace (default: current dir, canonicalized when present)
    let workspace_override = args
        .workspace_path
        .clone()
        .or_else(|| args.workspace.clone());

    let workspace = resolve_workspace_path(workspace_override)
        .context("Failed to resolve workspace directory")?;

    if let Some(path) = &args.workspace_path {
        if !workspace.exists() {
            bail!(
                "Workspace path '{}' does not exist. Initialize it first or provide an existing directory.",
                path.display()
            );
        }
    }

    cli::set_workspace_env(&workspace);

    // Load configuration (vtcode.toml or defaults) from resolved workspace
    let config_manager = ConfigManager::load_from_workspace(&workspace).with_context(|| {
        format!(
            "Failed to load vtcode configuration for workspace {}",
            workspace.display()
        )
    })?;
    let cfg = config_manager.config();

    let (full_auto_requested, automation_prompt) = match args.full_auto.clone() {
        Some(value) => {
            if value.trim().is_empty() {
                (true, None)
            } else {
                (true, Some(value))
            }
        }
        None => (false, None),
    };

    if automation_prompt.is_some() && args.command.is_some() {
        bail!(
            "--auto/--full-auto with a prompt cannot be combined with other commands. Provide only the prompt."
        );
    }

    if full_auto_requested {
        let automation_cfg = &cfg.automation.full_auto;
        if !automation_cfg.enabled {
            bail!(
                "Full-auto mode is disabled in configuration. Enable it under [automation.full_auto]."
            );
        }

        if automation_cfg.require_profile_ack {
            let profile_path = automation_cfg.profile_path.clone().ok_or_else(|| {
                anyhow!(
                    "Full-auto mode requires 'profile_path' in [automation.full_auto] when require_profile_ack = true."
                )
            })?;
            let resolved_profile = if profile_path.is_absolute() {
                profile_path
            } else {
                workspace.join(profile_path)
            };

            if !resolved_profile.exists() {
                bail!(
                    "Full-auto profile '{}' not found. Create the acknowledgement file before using --full-auto.",
                    resolved_profile.display()
                );
            }
        }
    }

    let skip_confirmations = args.skip_confirmations || full_auto_requested;

    // Resolve provider/model/theme with CLI override
    let provider = args
        .provider
        .clone()
        .unwrap_or_else(|| cfg.agent.provider.clone());
    let (model, model_source) = match args.model.clone() {
        Some(value) => (value, ModelSelectionSource::CliOverride),
        None => (
            cfg.agent.default_model.clone(),
            ModelSelectionSource::WorkspaceConfig,
        ),
    };

    initialize_dot_folder().ok();
    let user_theme_pref = load_user_config().ok().and_then(|dot| {
        let trimmed = dot.preferences.theme.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });

    let mut theme_selection = args
        .theme
        .clone()
        .or(user_theme_pref)
        .or_else(|| Some(cfg.agent.theme.clone()))
        .unwrap_or_else(|| DEFAULT_THEME_ID.to_string());

    if let Err(err) = ui_theme::set_active_theme(&theme_selection) {
        if args.theme.is_some() {
            return Err(err.context(format!("Failed to activate theme '{}'", theme_selection)));
        }
        eprintln!(
            "Warning: {}. Falling back to default theme '{}'.",
            err, DEFAULT_THEME_ID
        );
        theme_selection = DEFAULT_THEME_ID.to_string();
        ui_theme::set_active_theme(&theme_selection)
            .with_context(|| format!("Failed to activate theme '{}'", theme_selection))?;
    }

    update_theme_preference(&theme_selection).ok();

    // Resolve API key for chosen provider
    let api_key = get_api_key(&provider, &ApiKeySources::default())
        .with_context(|| format!("API key not found for provider '{}'", provider))?;

    let provider_enum = Provider::from_str(&provider).unwrap_or(Provider::Gemini);
    let cli_api_key_env = args.api_key_env.trim();
    let api_key_env_override = if cli_api_key_env.is_empty()
        || cli_api_key_env.eq_ignore_ascii_case(defaults::DEFAULT_API_KEY_ENV)
    {
        None
    } else {
        Some(cli_api_key_env.to_string())
    };

    let configured_api_key_env = cfg.agent.api_key_env.trim();
    let resolved_api_key_env = if configured_api_key_env.is_empty()
        || configured_api_key_env.eq_ignore_ascii_case(defaults::DEFAULT_API_KEY_ENV)
    {
        provider_enum.default_api_key_env().to_string()
    } else {
        configured_api_key_env.to_string()
    };

    let api_key_env = api_key_env_override.unwrap_or(resolved_api_key_env);

    // Bridge to local CLI modules
    let core_cfg = CoreAgentConfig {
        model: model.clone(),
        api_key,
        provider: provider.clone(),
        api_key_env,
        workspace: workspace.clone(),
        verbose: args.verbose,
        theme: theme_selection.clone(),
        reasoning_effort: cfg.agent.reasoning_effort,
        ui_surface: cfg.agent.ui_surface,
        prompt_cache: cfg.prompt_cache.clone(),
        model_source,
        custom_api_keys: cfg.agent.custom_api_keys.clone(),
    };

    if let Some(prompt) = automation_prompt {
        cli::handle_auto_task_command(&core_cfg, cfg, &prompt).await?;
        return Ok(());
    }

    match &args.command {
        Some(Commands::AgentClientProtocol { target }) => {
            cli::handle_acp_command(&core_cfg, &cfg, *target).await?;
        }
        Some(Commands::ToolPolicy { command }) => {
            vtcode_core::cli::tool_policy_commands::handle_tool_policy_command(command.clone())
                .await?;
        }
        Some(Commands::Mcp { command }) => {
            cli::handle_mcp_command(command.clone()).await?;
        }
        Some(Commands::Models { command }) => {
            vtcode_core::cli::models_commands::handle_models_command(&args, command).await?;
        }
        Some(Commands::Chat) => {
            cli::handle_chat_command(&core_cfg, skip_confirmations, full_auto_requested).await?;
        }
        Some(Commands::Ask { prompt }) => {
            cli::handle_ask_single_command(&core_cfg, prompt).await?;
        }
        Some(Commands::ChatVerbose) => {
            // Reuse chat path; verbose behavior is handled in the module if applicable
            cli::handle_chat_command(&core_cfg, skip_confirmations, full_auto_requested).await?;
        }
        Some(Commands::Analyze) => {
            cli::handle_analyze_command(&core_cfg).await?;
        }
        Some(Commands::Performance) => {
            cli::handle_performance_command().await?;
        }
        Some(Commands::Trajectory { file, top }) => {
            cli::handle_trajectory_logs_command(&core_cfg, file.clone(), *top).await?;
        }
        Some(Commands::CreateProject { name, features }) => {
            cli::handle_create_project_command(&core_cfg, name, features).await?;
        }
        Some(Commands::CompressContext) => {
            cli::handle_compress_context_command(&core_cfg).await?;
        }
        Some(Commands::Revert { turn, partial }) => {
            cli::handle_revert_command(&core_cfg, *turn, partial.clone()).await?;
        }
        Some(Commands::Snapshots) => {
            cli::handle_snapshots_command(&core_cfg).await?;
        }
        Some(Commands::CleanupSnapshots { max }) => {
            cli::handle_cleanup_snapshots_command(&core_cfg, Some(*max)).await?;
        }
        Some(Commands::Init) => {
            cli::handle_init_command(&workspace, false, false).await?;
        }
        Some(Commands::Config { output, global }) => {
            cli::handle_config_command(output.as_deref(), *global).await?;
        }
        Some(Commands::InitProject {
            name,
            force,
            migrate,
        }) => {
            cli::handle_init_project_command(name.clone(), *force, *migrate).await?;
        }
        Some(Commands::Benchmark {
            task_file,
            task,
            output,
            max_tasks,
        }) => {
            let options = cli::BenchmarkCommandOptions {
                task_file: task_file.clone(),
                inline_task: task.clone(),
                output: output.clone(),
                max_tasks: *max_tasks,
            };
            cli::handle_benchmark_command(&core_cfg, cfg, options, full_auto_requested).await?;
        }
        Some(Commands::Man { command, output }) => {
            cli::handle_man_command(command.clone(), output.clone()).await?;
        }
        _ => {
            // Default to chat
            cli::handle_chat_command(&core_cfg, skip_confirmations, full_auto_requested).await?;
        }
    }

    Ok(())
}

fn resolve_workspace_path(workspace_arg: Option<PathBuf>) -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("Failed to determine current working directory")?;

    let mut resolved = match workspace_arg {
        Some(path) if path.is_absolute() => path,
        Some(path) => cwd.join(path),
        None => cwd,
    };

    if resolved.exists() {
        resolved = resolved.canonicalize().with_context(|| {
            format!(
                "Failed to canonicalize workspace path {}",
                resolved.display()
            )
        })?;
    }

    Ok(resolved)
}
