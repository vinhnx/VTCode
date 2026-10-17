use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use vtcode_core::cli::args::Cli;
use vtcode_core::config::loader::{ConfigBuilder, VTCodeConfig, set_explicit_config_path};
use vtcode_core::utils::validation::validate_path_exists;

use super::first_run::maybe_run_first_run_setup;
use super::validation::{parse_cli_config_entries, resolve_config_path, resolve_workspace_path};
use vtcode_core::config::loader::ConfigManager;

pub(super) struct LoadedStartupConfig {
    pub(super) workspace: PathBuf,
    pub(super) config: VTCodeConfig,
    pub(super) first_run_occurred: bool,
    pub(super) full_auto_requested: bool,
    pub(super) automation_prompt: Option<String>,
    pub(super) primary_agent_explicitly_configured: bool,
}

pub(super) async fn load_startup_config(args: &Cli) -> Result<LoadedStartupConfig> {
    let workspace_override = args.workspace_path.clone().or_else(|| args.workspace.clone());

    let workspace = resolve_workspace_path(workspace_override).context("Failed to resolve workspace directory")?;
    if args.workspace_path.is_some() {
        validate_path_exists(&workspace, "Workspace")?;
    }

    let (cli_config_path_override, inline_config_overrides) = parse_cli_config_entries(&args.config);
    let env_config_path_override = std::env::var("VTCODE_CONFIG_PATH").ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
    });
    let config_path_override = cli_config_path_override.or(env_config_path_override);

    let mut builder = ConfigBuilder::new().workspace(workspace.clone());
    let resolved_explicit_path = match config_path_override {
        Some(path_override) => {
            let resolved_path = resolve_config_path(&workspace, &path_override);
            builder = builder.config_file(resolved_path.clone());
            Some(resolved_path)
        }
        None => None,
    };
    // Unconditionally synchronize the session override so a previous
    // `load_startup_config` in the same process (e.g. a test or a reload)
    // cannot leak a stale explicit path into this load.
    set_explicit_config_path(resolved_explicit_path.clone());

    if !inline_config_overrides.is_empty() {
        builder = builder.cli_overrides(&inline_config_overrides);
    }

    if let Some(ref model) = args.model {
        builder = builder.cli_override("agent.default_model".to_owned(), toml::Value::String(model.clone()));
    }

    let config_phase = std::time::Instant::now();
    let manager = match builder.build() {
        Ok(manager) => manager,
        Err(error) if super::is_config_reset_command(args) => {
            // The reset service can repair the selected malformed layer, so
            // do not make a broken effective stack prevent `config reset`
            // from reaching that service. Reset does not need provider auth,
            // model validation, or any agent runtime initialization.
            tracing::warn!("Configuration could not be loaded before reset: {error:#}");
            return Ok(LoadedStartupConfig {
                workspace,
                config: VTCodeConfig::default(),
                first_run_occurred: false,
                full_auto_requested: false,
                automation_prompt: None,
                primary_agent_explicitly_configured: false,
            });
        }
        Err(error) => return Err(error).context("Failed to load configuration"),
    };
    let config_duration = config_phase.elapsed();
    if let Some(timing) = manager.phase_timing() {
        tracing::debug!(target = "vtcode.startup", ?timing, "configuration phases recorded");
        vtcode_commons::startup_trace::record_duration(
            "config_path_resolution",
            std::time::Duration::from_micros(timing.path_resolution_us),
        );
        vtcode_commons::startup_trace::record_duration(
            "config_layer_loading",
            std::time::Duration::from_micros(timing.layer_loading_us),
        );
        vtcode_commons::startup_trace::record_duration(
            "config_merge_and_parse",
            std::time::Duration::from_micros(timing.merge_and_parse_us),
        );
        vtcode_commons::startup_trace::record_duration(
            "config_validation",
            std::time::Duration::from_micros(timing.validation_us),
        );
    }
    vtcode_commons::startup_trace::record_duration("config_loading", config_duration);
    // Borrow-only presence scan: avoids a second full TOML merge
    // (`effective_config()`) on every launch; `manager.config()` below is the
    // single owned clone for this startup.
    let primary_agent_explicitly_configured = manager.has_explicit_top_level_key("default_primary_agent");
    let mut config = manager.config().clone();
    apply_cli_provider_selection(&mut config, args);
    apply_aux_dotconfig_provider_defaults(&mut config, &manager, args.provider.is_some()).await;

    let (full_auto_requested, automation_prompt) = match args.full_auto.clone() {
        Some(value) if value.trim().is_empty() => (true, None),
        Some(value) => (true, Some(value)),
        None => (false, None),
    };

    let first_run_occurred = maybe_run_first_run_setup(args, &workspace, &mut config).await?;

    if automation_prompt.is_some() && args.command.is_some() {
        bail!("--auto/--full-auto with a prompt cannot be combined with other commands. Provide only the prompt.");
    }

    Ok(LoadedStartupConfig {
        workspace,
        config,
        first_run_occurred,
        full_auto_requested,
        automation_prompt,
        primary_agent_explicitly_configured,
    })
}

/// Apply the CLI `--provider` override on top of the configured provider.
///
/// `agent.api_key_env` is a single agent-wide variable rather than a
/// provider-scoped one, so it belongs to whichever provider `agent.provider`
/// selected. Switching providers on the CLI must therefore **not** inherit the
/// previous provider's variable: an explicit `--provider openai` with
/// `OPENAI_API_KEY` set would otherwise keep resolving a stale merge-gateway
/// variable and fail auth.
///
/// Provider-scoped overrides (`provider_overrides`, `custom_providers`) and an
/// explicit `--api-key-env` still take precedence later in
/// `resolve_runtime_model_selection`, so only the agent-wide fallback is reset.
fn apply_cli_provider_selection(config: &mut VTCodeConfig, args: &Cli) {
    let Some(ref provider) = args.provider else {
        return;
    };
    let requested = provider.trim();
    if requested.is_empty() {
        return;
    }
    if !config.agent.provider.eq_ignore_ascii_case(requested) {
        config.agent.api_key_env = vtcode_core::config::api_keys::api_key_env_var(requested);
    }
    config.agent.provider = requested.to_owned();
}

/// Honor the auxiliary global `config.toml` (`[preferences] default_provider` /
/// `default_model`, including legacy bare top-level keys) when the canonical
/// `vtcode.toml` layers do not explicitly configure `agent.provider` /
/// `agent.default_model`.
///
/// `vtcode review` / `exec` (and every other command) resolve the runtime
/// provider from `VTCodeConfig.agent.provider`. Users editing the global
/// `config.toml` expect those preferences to apply; previously they were
/// silently ignored and the runtime kept the compiled-in openrouter default,
/// failing with an openrouter auth error even after configuring e.g. ollama.
async fn apply_aux_dotconfig_provider_defaults(
    config: &mut VTCodeConfig,
    manager: &ConfigManager,
    cli_provider_override: bool,
) {
    let provider_explicit = explicit_agent_key(manager, "provider") || cli_provider_override;
    let model_explicit = explicit_agent_key(manager, "default_model");
    if provider_explicit && model_explicit {
        return;
    }

    let dot = match vtcode_core::utils::dot_config::load_user_config().await {
        Ok(dot) => dot,
        Err(_) => return,
    };

    apply_dot_preferences(config, &dot.preferences, provider_explicit, model_explicit);
}

pub(super) fn apply_dot_preferences(
    config: &mut VTCodeConfig,
    preferences: &vtcode_core::utils::dot_config::UserPreferences,
    provider_explicit: bool,
    model_explicit: bool,
) {
    let compiled_default_provider = vtcode_core::config::constants::defaults::DEFAULT_PROVIDER;
    let compiled_default_model = vtcode_core::config::constants::defaults::DEFAULT_MODEL;

    // Resolve the provider first so a provider switch can drive the model
    // fallback below.
    let mut provider_changed = false;
    if !provider_explicit {
        let preferred = preferences.default_provider.trim();
        // A pristine auxiliary file carries the compiled-in default
        // (`openrouter`), which is a no-op. Only an explicit, non-default
        // auxiliary provider overrides the canonical default.
        if !preferred.is_empty()
            && !preferred.eq_ignore_ascii_case(compiled_default_provider)
            && !preferred.eq_ignore_ascii_case(&config.agent.provider)
        {
            config.agent.provider = preferred.to_owned();
            provider_changed = true;
            // The placeholder default (`OPENROUTER_API_KEY`) resolves per-provider
            // via `resolve_api_key_env`; reset it so diagnostics and downstream
            // resolution use the new provider's default env key.
            let current_env = config.agent.api_key_env.trim().to_owned();
            if current_env.is_empty()
                || current_env.eq_ignore_ascii_case(vtcode_core::config::constants::defaults::DEFAULT_API_KEY_ENV)
            {
                config.agent.api_key_env = vtcode_core::config::api_keys::api_key_env_var(&config.agent.provider);
            }
        }
    }

    if model_explicit {
        return;
    }

    let preferred_model = preferences.default_model.trim().to_owned();
    let dot_model_customized = !preferred_model.is_empty() && preferred_model != compiled_default_model;
    if dot_model_customized && preferred_model != config.agent.default_model {
        config.agent.default_model = preferred_model;
    } else if provider_changed && config.agent.default_model == compiled_default_model {
        // Provider switched via the auxiliary file but the auxiliary model was
        // left at the compiled-in default: adopt the new provider's default
        // model instead of keeping the old provider's default (which would
        // fail model validation, e.g. an openrouter route with ollama).
        if let Some(provider_default) =
            vtcode_core::config::constants::model_helpers::default_for(&config.agent.provider)
            && provider_default != config.agent.default_model
        {
            config.agent.default_model = provider_default.to_owned();
        }
    }
}

fn explicit_agent_key(manager: &ConfigManager, key: &str) -> bool {
    let effective = manager.effective_config();
    effective.get("agent").and_then(|agent| agent.get(key)).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use serial_test::serial;
    use tempfile::TempDir;
    use vtcode_config::loader::explicit_config_path;
    use vtcode_core::cli::args::Cli;

    /// RAII guard for the process-global session override so a panicking test
    /// cannot leak a stale explicit path into sibling tests.
    struct SessionOverrideGuard;

    impl SessionOverrideGuard {
        fn reset() -> Self {
            set_explicit_config_path(None);
            Self
        }
    }

    impl Drop for SessionOverrideGuard {
        fn drop(&mut self) {
            set_explicit_config_path(None);
        }
    }

    #[tokio::test]
    #[serial]
    async fn cli_config_path_override_loads_requested_file() {
        let _guard = SessionOverrideGuard::reset();
        let temp_dir = TempDir::new().expect("temp dir");
        let workspace = temp_dir.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace dir");
        std::fs::create_dir(workspace.join(".vtcode")).expect("workspace dot dir");

        let config_path = temp_dir.path().join("custom-config.toml");
        std::fs::write(
            &config_path,
            r#"
[debug]
enable_tracing = true
"#,
        )
        .expect("custom config");

        let args = Cli::parse_from([
            "vtcode",
            "--workspace",
            workspace.to_str().expect("workspace path"),
            "--config",
            config_path.to_str().expect("config path"),
        ]);

        let loaded = load_startup_config(&args).await.expect("startup config should load");

        assert!(loaded.config.debug.enable_tracing);
    }

    #[tokio::test]
    #[serial]
    async fn env_config_path_override_loads_requested_file_and_captures_session_override() {
        use vtcode_commons::env_lock;

        let _guard = SessionOverrideGuard::reset();
        let env_guard = env_lock::lock();
        let previous = std::env::var_os("VTCODE_CONFIG_PATH");

        let temp_dir = TempDir::new().expect("temp dir");
        let workspace = temp_dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace dir");
        std::fs::create_dir_all(workspace.join(".vtcode")).expect("workspace dot dir");

        let config_path = temp_dir.path().join("custom-env-config.toml");
        std::fs::write(&config_path, "[debug]\nenable_tracing = true\n").expect("custom env config");

        env_guard.set_var("VTCODE_CONFIG_PATH", config_path.to_str().expect("config path"));

        let args = Cli::parse_from(["vtcode", "--workspace", workspace.to_str().expect("workspace path")]);

        let loaded = load_startup_config(&args).await;
        let session_override = explicit_config_path();

        env_guard.restore_var("VTCODE_CONFIG_PATH", previous);

        let loaded = loaded.expect("startup config should load from VTCODE_CONFIG_PATH");
        assert!(loaded.config.debug.enable_tracing);
        assert_eq!(
            session_override.and_then(|path| vtcode_commons::canonicalize(path).ok()),
            vtcode_commons::canonicalize(&config_path).ok(),
            "startup must capture the resolved env path as the session override"
        );
    }

    #[test]
    fn aux_dotconfig_ollama_provider_applies_when_canonical_is_default() {
        let mut config = VTCodeConfig::default();
        assert_eq!(config.agent.provider, "openrouter");
        let preferences = vtcode_core::utils::dot_config::UserPreferences {
            default_provider: "ollama".to_string(),
            ..Default::default()
        };
        // Auxiliary model left at the compiled-in default: the ollama default
        // model must be adopted instead of keeping the openrouter route.
        apply_dot_preferences(&mut config, &preferences, false, false);
        assert_eq!(config.agent.provider, "ollama");
        assert_eq!(config.agent.default_model, vtcode_core::config::constants::models::ollama::DEFAULT_MODEL);
    }

    #[test]
    fn aux_dotconfig_does_not_clobber_explicit_canonical_provider() {
        let mut config = VTCodeConfig::default();
        config.agent.provider = "openai".to_string();
        let preferences = vtcode_core::utils::dot_config::UserPreferences {
            default_provider: "ollama".to_string(),
            ..Default::default()
        };
        apply_dot_preferences(&mut config, &preferences, true, false);
        assert_eq!(config.agent.provider, "openai");
    }

    #[test]
    fn cli_provider_override_resets_stale_agent_api_key_env() {
        let mut config = VTCodeConfig::default();
        config.agent.provider = "merge-gateway".to_string();
        config.agent.api_key_env = "MERGE_GATEWAY_API_KEY".to_string();

        let args = Cli::parse_from(["vtcode", "--provider", "openai"]);
        apply_cli_provider_selection(&mut config, &args);

        assert_eq!(config.agent.provider, "openai");
        assert_eq!(
            config.agent.api_key_env,
            vtcode_core::config::api_keys::api_key_env_var("openai"),
            "switching providers must not inherit the previous provider's agent-wide key"
        );
    }

    #[test]
    fn cli_provider_override_keeps_agent_api_key_env_for_same_provider() {
        let mut config = VTCodeConfig::default();
        config.agent.provider = "openai".to_string();
        config.agent.api_key_env = "CORP_OPENAI_SECRET".to_string();

        let args = Cli::parse_from(["vtcode", "--provider", "openai"]);
        apply_cli_provider_selection(&mut config, &args);

        assert_eq!(config.agent.provider, "openai");
        assert_eq!(
            config.agent.api_key_env, "CORP_OPENAI_SECRET",
            "re-selecting the configured provider keeps its persisted key"
        );
    }

    #[test]
    fn no_cli_provider_override_leaves_agent_api_key_env_untouched() {
        let mut config = VTCodeConfig::default();
        config.agent.provider = "merge-gateway".to_string();
        config.agent.api_key_env = "MERGE_GATEWAY_API_KEY".to_string();

        let args = Cli::parse_from(["vtcode"]);
        apply_cli_provider_selection(&mut config, &args);

        assert_eq!(config.agent.provider, "merge-gateway");
        assert_eq!(config.agent.api_key_env, "MERGE_GATEWAY_API_KEY");
    }

    #[test]
    fn aux_dotconfig_custom_model_applies_when_canonical_model_is_default() {
        let mut config = VTCodeConfig::default();
        let preferences = vtcode_core::utils::dot_config::UserPreferences {
            default_provider: "openrouter".to_string(),
            default_model: "custom-route/model".to_string(),
            ..Default::default()
        };
        apply_dot_preferences(&mut config, &preferences, true, false);
        assert_eq!(config.agent.default_model, "custom-route/model");
    }
}
