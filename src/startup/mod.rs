use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use toml::Value as TomlValue;
use toml::value::Table as TomlTable;

mod first_run;

use first_run::maybe_run_first_run_setup;
use vtcode_core::cli::args::Cli;
use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::constants::defaults;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, ModelSelectionSource};
use vtcode_core::config::validator::ConfigValidator;
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
    pub session_resume: Option<SessionResumeMode>,
}

#[derive(Debug, Clone)]
pub enum SessionResumeMode {
    Interactive,
    Latest,
    Specific(String),
}

impl StartupContext {
    pub async fn from_cli_args(args: &Cli) -> Result<Self> {
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

        let (config_path_override, inline_config_overrides) =
            parse_cli_config_entries(&args.config);

        let mut config = if let Some(path_override) = config_path_override {
            let resolved_path = resolve_config_path(&workspace, &path_override);
            ConfigManager::load_from_file(&resolved_path)
                .with_context(|| {
                    format!(
                        "Failed to load vtcode configuration from {}",
                        resolved_path.display()
                    )
                })?
                .config()
                .clone()
        } else {
            ConfigManager::load_from_workspace(&workspace)
                .with_context(|| {
                    format!(
                        "Failed to load vtcode configuration for workspace {}",
                        workspace.display()
                    )
                })?
                .config()
                .clone()
        };

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

        let _first_run = maybe_run_first_run_setup(args, &workspace, &mut config).await?;

        if !inline_config_overrides.is_empty() {
            apply_inline_config_overrides(&mut config, &inline_config_overrides)
                .context("Failed to apply inline --config overrides")?;
        }

        if automation_prompt.is_some() && args.command.is_some() {
            bail!(
                "--auto/--full-auto with a prompt cannot be combined with other commands. Provide only the prompt."
            );
        }

        if full_auto_requested {
            validate_full_auto_configuration(&config, &workspace)?;
        }

        // Validate configuration against models database
        validate_startup_configuration(&config, &workspace)?;

        let session_resume = if let Some(value) = args.resume_session.as_ref() {
            if value == "__interactive__" {
                Some(SessionResumeMode::Interactive)
            } else {
                Some(SessionResumeMode::Specific(value.clone()))
            }
        } else if args.continue_latest {
            Some(SessionResumeMode::Latest)
        } else {
            None
        };

        if session_resume.is_some() && args.command.is_some() {
            bail!(
                "--resume/--continue cannot be combined with other commands. Run the resume operation without a subcommand."
            );
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

        initialize_dot_folder().await.ok();
        let theme_selection = determine_theme(args, &config).await?;

        update_theme_preference(&theme_selection).await.ok();

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

        let checkpointing_storage_dir =
            config.agent.checkpointing.storage_dir.as_ref().map(|dir| {
                let candidate = PathBuf::from(dir);
                if candidate.is_absolute() {
                    candidate
                } else {
                    workspace.join(candidate)
                }
            });

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
            checkpointing_enabled: config.agent.checkpointing.enabled,
            checkpointing_storage_dir,
            checkpointing_max_snapshots: config.agent.checkpointing.max_snapshots,
            checkpointing_max_age_days: config.agent.checkpointing.max_age_days,
        };

        let skip_confirmations = args.skip_confirmations || full_auto_requested;

        Ok(StartupContext {
            workspace,
            config,
            agent_config,
            skip_confirmations,
            full_auto_requested,
            automation_prompt,
            session_resume,
        })
    }
}

fn parse_cli_config_entries(entries: &[String]) -> (Option<PathBuf>, Vec<(String, String)>) {
    let mut config_path: Option<PathBuf> = None;
    let mut overrides = Vec::new();

    for entry in entries {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            overrides.push((key.to_string(), value.trim().to_string()));
        } else if config_path.is_none() {
            config_path = Some(PathBuf::from(trimmed));
        }
    }

    (config_path, overrides)
}

fn resolve_config_path(workspace: &Path, candidate: &Path) -> PathBuf {
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }

    let workspace_candidate = workspace.join(candidate);
    if workspace_candidate.exists() {
        return workspace_candidate;
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| workspace.to_path_buf());
    let cwd_candidate = cwd.join(candidate);
    if cwd_candidate.exists() {
        cwd_candidate
    } else {
        workspace_candidate
    }
}

fn apply_inline_config_overrides(
    config: &mut VTCodeConfig,
    overrides: &[(String, String)],
) -> Result<()> {
    let serialized =
        toml::to_string(config).context("Failed to serialize configuration for CLI overrides")?;
    let mut doc: TomlValue =
        toml::from_str(&serialized).context("Failed to convert configuration into TOML value")?;

    for (key, raw_value) in overrides {
        let parsed_value = parse_override_value(raw_value)
            .with_context(|| format!("Failed to parse override value for '{}'.", key))?;
        apply_override_value(&mut doc, key, parsed_value)
            .with_context(|| format!("Failed to apply override for key '{}'.", key))?;
    }

    let updated_serialized =
        toml::to_string(&doc).context("Failed to serialize overridden configuration")?;
    let updated: VTCodeConfig = toml::from_str(&updated_serialized)
        .context("Failed to deserialize configuration after CLI overrides")?;

    updated
        .validate()
        .context("Configuration overrides failed validation")?;

    *config = updated;
    Ok(())
}

fn parse_override_value(raw: &str) -> Result<TomlValue> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(TomlValue::String(String::new()));
    }

    let candidate = format!("value = {}", trimmed);
    match toml::from_str::<TomlTable>(&candidate) {
        Ok(mut table) => Ok(table
            .remove("value")
            .unwrap_or_else(|| TomlValue::String(trimmed.to_string()))),
        Err(_) => Ok(TomlValue::String(trimmed.to_string())),
    }
}

fn apply_override_value(target: &mut TomlValue, key: &str, value: TomlValue) -> Result<()> {
    let segments: Vec<&str> = key
        .split('.')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect();

    if segments.is_empty() {
        bail!("Configuration override key must not be empty");
    }

    let mut current = target;
    for segment in &segments[..segments.len() - 1] {
        let table = current.as_table_mut().ok_or_else(|| {
            anyhow!(
                "Cannot set configuration override '{}': '{}' is not a table",
                key,
                segment
            )
        })?;

        current = table
            .entry(segment.to_string())
            .or_insert_with(|| TomlValue::Table(TomlTable::new()));
    }

    let table = current.as_table_mut().ok_or_else(|| {
        anyhow!(
            "Cannot set configuration override '{}': parent is not a table",
            key
        )
    })?;

    let last_segment = segments.last().unwrap().to_string();
    table.insert(last_segment, value);
    Ok(())
}

async fn determine_theme(args: &Cli, config: &VTCodeConfig) -> Result<String> {
    let user_theme_pref = load_user_config().await.ok().and_then(|dot| {
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

fn validate_startup_configuration(config: &VTCodeConfig, workspace: &Path) -> Result<()> {
    // Find models.json in workspace or standard locations
    let mut models_json_paths = vec![workspace.join("docs/models.json")];

    if let Ok(cwd) = std::env::current_dir() {
        models_json_paths.push(cwd.join("docs/models.json"));
    }

    let models_json_path = models_json_paths
        .iter()
        .find(|p| p.exists())
        .map(|p| p.to_path_buf());

    if let Some(models_path) = models_json_path {
        match ConfigValidator::new(&models_path) {
            Ok(validator) => {
                match validator.validate(config) {
                    Ok(result) => {
                        // Display warnings (errors would have been caught earlier)
                        if !result.warnings.is_empty() {
                            eprintln!("{}", result.format_for_display());
                        }
                    }
                    Err(e) => {
                        // Non-critical validation error - log but don't fail startup
                        eprintln!("Warning: Configuration validation failed: {}", e);
                    }
                }
            }
            Err(e) => {
                // Non-critical validator creation error
                eprintln!(
                    "Warning: Could not load models database for validation: {}",
                    e
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::TempDir;
    use std::env;
    use std::sync::{Mutex, OnceLock};

    fn workspace_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("workspace lock")
    }

    #[test]
    fn resolves_current_dir_when_none() -> Result<()> {
        let _guard = workspace_guard();
        let original_cwd = env::current_dir()?;
        let temp_dir = TempDir::new()?;
        env::set_current_dir(temp_dir.path())?;

        let resolved = resolve_workspace_path(None)?;
        assert_eq!(resolved, temp_dir.path().canonicalize()?);

        env::set_current_dir(original_cwd)?;
        Ok(())
    }

    #[test]
    fn resolves_relative_workspace_path() -> Result<()> {
        let _guard = workspace_guard();
        let original_cwd = env::current_dir()?;
        let temp_dir = TempDir::new()?;
        let workspace_dir = temp_dir.path().join("project");
        std::fs::create_dir(&workspace_dir)?;
        env::set_current_dir(temp_dir.path())?;

        let resolved = resolve_workspace_path(Some(PathBuf::from("project")))?;
        assert_eq!(resolved, workspace_dir.canonicalize()?);

        env::set_current_dir(original_cwd)?;
        Ok(())
    }

    #[test]
    fn parses_cli_config_entries_with_overrides() {
        let entries = vec![
            "agent.provider=openai".to_string(),
            "custom-config/vtcode.toml".to_string(),
        ];

        let (path, overrides) = parse_cli_config_entries(&entries);

        assert_eq!(path, Some(PathBuf::from("custom-config/vtcode.toml")));
        assert_eq!(
            overrides,
            vec![("agent.provider".to_string(), "openai".to_string())]
        );
    }

    #[test]
    fn applies_inline_overrides_to_config() -> Result<()> {
        let mut config = VTCodeConfig::default();
        let overrides = vec![("agent.provider".to_string(), "\"openai\"".to_string())];

        apply_inline_config_overrides(&mut config, &overrides)?;

        assert_eq!(config.agent.provider, "openai");
        Ok(())
    }
}
