use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use vtcode_core::cli::args::{Cli, Commands};
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::{WorkspaceTrustLevel, WorkspaceTrustRecord, get_dot_manager};
use vtcode_core::utils::file_utils::ensure_dir_exists_sync;
use vtcode_core::{initialize_dot_folder, update_model_preference};

use super::first_run_prompts::{
    default_model_for_provider, prompt_model, prompt_provider, prompt_reasoning_effort,
    prompt_trust, resolve_initial_provider,
};

/// Drive the first-run interactive setup wizard when a workspace lacks VT Code artifacts.
pub async fn maybe_run_first_run_setup(
    args: &Cli,
    workspace: &Path,
    config: &mut VTCodeConfig,
) -> Result<bool> {
    if !is_fresh_workspace(workspace) {
        return Ok(false);
    }

    if args.provider.is_some() || args.model.is_some() {
        return Ok(false);
    }

    if let Some(command) = &args.command {
        match command {
            Commands::Chat | Commands::ChatVerbose => {}
            _ => return Ok(false),
        }
    }

    let full_auto_requested = args.full_auto.is_some();
    let non_interactive = args.skip_confirmations || full_auto_requested;
    let mode = if non_interactive {
        SetupMode::NonInteractive {
            full_auto: full_auto_requested,
        }
    } else {
        SetupMode::Interactive
    };

    run_first_run_setup(workspace, config, mode).await?;
    Ok(true)
}

enum SetupMode {
    Interactive,
    NonInteractive { full_auto: bool },
}

fn is_fresh_workspace(workspace: &Path) -> bool {
    let config_path = workspace.join("vtcode.toml");
    let dot_dir = workspace.join(".vtcode");
    !config_path.exists() && !dot_dir.exists()
}

async fn run_first_run_setup(
    workspace: &Path,
    config: &mut VTCodeConfig,
    mode: SetupMode,
) -> Result<()> {
    initialize_dot_folder().await.ok();

    if !workspace.exists() {
        return Err(anyhow!(
            "Workspace '{}' does not exist for setup",
            workspace.display()
        ));
    }

    let workspace_dot_dir = workspace.join(".vtcode");
    ensure_dir_exists_sync(&workspace_dot_dir).with_context(|| {
        format!(
            "Failed to create workspace .vtcode directory at {}",
            workspace_dot_dir.display()
        )
    })?;

    let mut renderer = AnsiRenderer::stdout();
    renderer.line(
        MessageStyle::Info,
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
    )?;
    renderer.line(MessageStyle::Info, "  VT Code - First-time setup wizard")?;
    renderer.line(
        MessageStyle::Info,
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
    )?;
    let (provider, model, trust) = match mode {
        SetupMode::Interactive => {
            renderer.line(
                MessageStyle::Status,
                "Let's configure your default provider, model, and workspace trust.",
            )?;
            renderer.line(
                MessageStyle::Status,
                "Press Enter to accept the suggested value in brackets.",
            )?;
            renderer.line(MessageStyle::Info, "")?;

            let provider = resolve_initial_provider(config);
            let provider = prompt_provider(&mut renderer, provider)?;
            renderer.line(MessageStyle::Info, "")?;

            let default_model = default_model_for_provider(provider);
            let model = prompt_model(&mut renderer, provider, default_model)?;
            renderer.line(MessageStyle::Info, "")?;

            let reasoning = prompt_reasoning_effort(&mut renderer, ReasoningEffortLevel::Medium)?;
            renderer.line(MessageStyle::Info, "")?;

            let trust = prompt_trust(&mut renderer, WorkspaceTrustLevel::ToolsPolicy)?;
            renderer.line(MessageStyle::Info, "")?;

            (provider, model, reasoning, trust)
        }
        SetupMode::NonInteractive { full_auto } => {
            renderer.line(
                MessageStyle::Status,
                "Non-interactive setup flags detected. Applying defaults without prompts.",
            )?;
            renderer.line(MessageStyle::Info, "")?;

            let provider = resolve_initial_provider(config);
            let default_model = default_model_for_provider(provider);
            let model = default_model.to_owned();
            let trust = if full_auto {
                WorkspaceTrustLevel::FullAuto
            } else {
                WorkspaceTrustLevel::ToolsPolicy
            };

            renderer.line(
                MessageStyle::Info,
                &format!("Provider: {}", provider.label()),
            )?;
            renderer.line(MessageStyle::Info, &format!("Model: {}", model))?;
            renderer.line(
                MessageStyle::Info,
                &format!("Workspace trust: {}", trust_label(trust)),
            )?;
            renderer.line(MessageStyle::Info, "")?;

            (provider, model, trust)
        }
    };

    renderer.line(
        MessageStyle::Status,
        "Saving your configuration to vtcode.toml ...",
    )?;

    // Compute provider key once to avoid repeated allocations from `to_string()`.
    let provider_key = provider.to_string();
    // Set API key environment name from provider defaults; do this once to avoid recomputing.
    config.agent.api_key_env = provider.default_api_key_env().to_owned();
    apply_selection(config, &provider_key, &model);

    let config_path = workspace.join("vtcode.toml");
    ConfigManager::save_config_to_path(&config_path, config).with_context(|| {
        format!(
            "Failed to write initial configuration to {}",
            config_path.display()
        )
    })?;

    update_model_preference(&provider_key, &model).await.ok();

    persist_workspace_trust(workspace, trust)
        .await
        .with_context(|| {
            format!(
                "Failed to persist workspace trust level for {}",
                workspace.display()
            )
        })?;

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(
        MessageStyle::Status,
        &format!(
            "Setup complete. Provider: {} • Model: {} • Trust: {}",
            provider.label(),
            model,
            trust_label(trust)
        ),
    )?;
    renderer.line(MessageStyle::Info, "")?;
    renderer.line(
        MessageStyle::Status,
        "Tip: run `/init` anytime to rerun this setup and refresh other workspace defaults.",
    )?;
    renderer.line(MessageStyle::Info, "")?;

    Ok(())
}

fn apply_selection(config: &mut VTCodeConfig, provider_key: &str, model: &str) {
    config.agent.provider = provider_key.to_owned();
    config.agent.default_model = model.to_owned();
}

fn trust_label(level: WorkspaceTrustLevel) -> &'static str {
    match level {
        WorkspaceTrustLevel::ToolsPolicy => "Tools policy",
        WorkspaceTrustLevel::FullAuto => "Full auto",
    }
}

async fn persist_workspace_trust(workspace: &Path, level: WorkspaceTrustLevel) -> Result<()> {
    let canonical = workspace
        .canonicalize()
        .with_context(|| {
            format!(
                "Failed to canonicalize workspace path {} for trust setup",
                workspace.display()
            )
        })?
        .to_string_lossy()
        .into_owned();

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock is before UNIX_EPOCH while persisting workspace trust")?
        .as_secs();

    let manager = get_dot_manager()
        .context("Failed to initialize dot manager while persisting workspace trust")?
        .lock()
        .map_err(|err| anyhow!("Dot manager lock poisoned while persisting trust: {err}"))?
        .clone();

    manager
        .update_config(|cfg| {
            cfg.workspace_trust.entries.insert(
                canonical.clone(),
                WorkspaceTrustRecord {
                    level,
                    trusted_at: timestamp,
                },
            );
        })
        .await
        .context("Failed to update workspace trust in dot config")
}
