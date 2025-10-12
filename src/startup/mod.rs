use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};

use vtcode_core::cli::args::Cli;
use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::constants::defaults;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, ModelSelectionSource};
use vtcode_core::ui::theme::{self as ui_theme, DEFAULT_THEME_ID};
use vtcode_core::{initialize_dot_folder, load_user_config, update_theme_preference};

/// Aggregated data required for CLI command execution after startup.
#[derive(Debug, Clone)]
pub struct StartupContext {
    pub workspace: PathBuf,
    pub config: VTCodeConfig,
    pub agent_config: CoreAgentConfig,
    pub skip_confirmations: bool,
    pub full_auto_requested: bool,
    pub automation_prompt: Option<String>,
}

impl StartupContext {
    pub fn from_cli_args(args: &Cli) -> Result<Self> {
        let workspace_override = args
            .workspace_path
            .clone()
            .or_else(|| args.workspace.clone());

        let workspace = resolve_workspace_path(workspace_override)
            .context("Failed to resolve workspace directory")?;

        if let Some(path) = &args.workspace_path
            && !workspace.exists()
        {
            bail!(
                "Workspace path '{}' does not exist. Initialize it first or provide an existing directory.",
                path.display()
            );
        }

        let config_manager = ConfigManager::load_from_workspace(&workspace).with_context(|| {
            format!(
                "Failed to load vtcode configuration for workspace {}",
                workspace.display()
            )
        })?;

        let config = config_manager.config().clone();

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
            validate_full_auto_configuration(&config, &workspace)?;
        }

        let provider = args
            .provider
            .clone()
            .unwrap_or_else(|| config.agent.provider.clone());

        let (model, model_source) = match args.model.clone() {
            Some(value) => (value, ModelSelectionSource::CliOverride),
            None => (
                config.agent.default_model.clone(),
                ModelSelectionSource::WorkspaceConfig,
            ),
        };

        initialize_dot_folder().ok();
        let theme_selection = determine_theme(args, &config)?;

        update_theme_preference(&theme_selection).ok();

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

        let configured_api_key_env = config.agent.api_key_env.trim();
        let resolved_api_key_env = if configured_api_key_env.is_empty()
            || configured_api_key_env.eq_ignore_ascii_case(defaults::DEFAULT_API_KEY_ENV)
        {
            provider_enum.default_api_key_env().to_string()
        } else {
            configured_api_key_env.to_string()
        };

        let api_key_env = api_key_env_override.unwrap_or(resolved_api_key_env);

        let agent_config = CoreAgentConfig {
            model,
            api_key,
            provider,
            api_key_env,
            workspace: workspace.clone(),
            verbose: args.verbose,
            theme: theme_selection,
            reasoning_effort: config.agent.reasoning_effort,
            ui_surface: config.agent.ui_surface,
            prompt_cache: config.prompt_cache.clone(),
            model_source,
            custom_api_keys: config.agent.custom_api_keys.clone(),
        };

        let skip_confirmations = args.skip_confirmations || full_auto_requested;

        Ok(StartupContext {
            workspace,
            config,
            agent_config,
            skip_confirmations,
            full_auto_requested,
            automation_prompt,
        })
    }
}

fn determine_theme(args: &Cli, config: &VTCodeConfig) -> Result<String> {
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
        .or_else(|| Some(config.agent.theme.clone()))
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

    Ok(theme_selection)
}

fn validate_full_auto_configuration(config: &VTCodeConfig, workspace: &Path) -> Result<()> {
    let automation_cfg = &config.automation.full_auto;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::tempdir;

    #[test]
    fn resolves_current_dir_when_none() -> Result<()> {
        let original_cwd = env::current_dir()?;
        let temp_dir = tempdir()?;
        env::set_current_dir(temp_dir.path())?;

        let resolved = resolve_workspace_path(None)?;
        assert_eq!(resolved, temp_dir.path().canonicalize()?);

        env::set_current_dir(original_cwd)?;
        Ok(())
    }

    #[test]
    fn resolves_relative_workspace_path() -> Result<()> {
        let original_cwd = env::current_dir()?;
        let temp_dir = tempdir()?;
        let workspace_dir = temp_dir.path().join("project");
        std::fs::create_dir(&workspace_dir)?;
        env::set_current_dir(temp_dir.path())?;

        let resolved = resolve_workspace_path(Some(PathBuf::from("project")))?;
        assert_eq!(resolved, workspace_dir.canonicalize()?);

        env::set_current_dir(original_cwd)?;
        Ok(())
    }
}
