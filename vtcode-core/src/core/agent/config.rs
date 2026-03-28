use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::cli::args::Cli;
pub use crate::config::api_keys::api_key_env_var;
use crate::config::api_keys::resolve_api_key_env;
use crate::config::constants::defaults;
use crate::config::loader::VTCodeConfig;
use crate::config::models::Provider;
use crate::config::types::{AgentConfig, ModelSelectionSource};
use crate::llm::factory::infer_provider;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeModelSelection {
    pub model: String,
    pub provider: String,
    pub model_source: ModelSelectionSource,
}

pub fn resolve_runtime_model_selection(args: &Cli, config: &VTCodeConfig) -> RuntimeModelSelection {
    let (model, model_source) = if let Some(agent) = args.agent.clone() {
        (agent, ModelSelectionSource::CliOverride)
    } else if let Some(model) = args.model.clone() {
        (model, ModelSelectionSource::CliOverride)
    } else {
        (
            config.agent.default_model.clone(),
            ModelSelectionSource::WorkspaceConfig,
        )
    };

    let provider = resolve_provider(
        args.provider.clone().or_else(provider_env_override),
        config.agent.provider.as_str(),
        &model,
        model_source,
    );

    RuntimeModelSelection {
        model,
        provider,
        model_source,
    }
}

pub fn build_runtime_agent_config(
    args: &Cli,
    config: &VTCodeConfig,
    workspace: PathBuf,
    selection: RuntimeModelSelection,
    api_key: String,
    theme_selection: String,
) -> AgentConfig {
    let cli_api_key_env = args.api_key_env.trim();
    let api_key_env_override = if cli_api_key_env.is_empty()
        || cli_api_key_env.eq_ignore_ascii_case(defaults::DEFAULT_API_KEY_ENV)
    {
        None
    } else {
        Some(cli_api_key_env.to_owned())
    };

    let checkpointing_storage_dir = resolve_checkpointing_storage_dir(
        &workspace,
        config.agent.checkpointing.storage_dir.as_deref(),
    );
    let RuntimeModelSelection {
        model,
        provider,
        model_source,
    } = selection;
    let api_key_env = api_key_env_override
        .unwrap_or_else(|| resolve_api_key_env(&provider, &config.agent.api_key_env));

    AgentConfig {
        model,
        api_key,
        provider,
        api_key_env,
        workspace,
        verbose: args.verbose,
        quiet: args.quiet,
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
        max_conversation_turns: config.agent.max_conversation_turns,
        model_behavior: Some(config.model.clone()),
        openai_chatgpt_auth: None,
    }
}

pub fn resolve_checkpointing_storage_dir(
    workspace: &Path,
    storage_dir: Option<&str>,
) -> Option<PathBuf> {
    storage_dir.map(PathBuf::from).map(|candidate| {
        if candidate.is_absolute() {
            candidate
        } else {
            workspace.join(candidate)
        }
    })
}

pub fn provider_label(provider: &str, vt_cfg: Option<&VTCodeConfig>) -> String {
    if let Some(vt_cfg) = vt_cfg {
        return vt_cfg.provider_display_name(provider);
    }

    Provider::from_str(provider)
        .map(|resolved| resolved.label().to_string())
        .unwrap_or_else(|_| provider.to_string())
}

fn resolve_provider(
    cli_provider: Option<String>,
    configured_provider: &str,
    model: &str,
    model_source: ModelSelectionSource,
) -> String {
    if let Some(provider) = cli_provider {
        return provider;
    }

    if matches!(model_source, ModelSelectionSource::CliOverride)
        && let Some(provider) = infer_provider(None, model)
    {
        return provider.to_string();
    }

    let configured_provider = configured_provider.trim();
    if !configured_provider.is_empty() {
        return configured_provider.to_owned();
    }

    infer_provider(None, model)
        .map(|provider| provider.to_string())
        .unwrap_or_else(|| defaults::DEFAULT_PROVIDER.to_owned())
}

fn provider_env_override() -> Option<String> {
    std::env::var("VTCODE_PROVIDER")
        .ok()
        .or_else(|| std::env::var("provider").ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn provider_resolution_prefers_configured_provider_for_config_model() {
        let mut config = VTCodeConfig::default();
        config.agent.provider = "zai".to_owned();
        config.agent.default_model =
            crate::config::constants::models::ollama::MINIMAX_M25_CLOUD.to_owned();

        let args = Cli::parse_from(["vtcode"]);
        let selection = resolve_runtime_model_selection(&args, &config);

        assert_eq!(selection.provider, "zai");
        assert_eq!(
            selection.model_source,
            ModelSelectionSource::WorkspaceConfig
        );
    }

    #[test]
    fn provider_resolution_infers_from_cli_model_without_cli_provider() {
        let mut config = VTCodeConfig::default();
        config.agent.provider = "zai".to_owned();

        let args = Cli::parse_from([
            "vtcode",
            "--model",
            crate::config::constants::models::ollama::MINIMAX_M25_CLOUD,
        ]);
        let selection = resolve_runtime_model_selection(&args, &config);

        assert_eq!(selection.provider, "ollama");
        assert_eq!(selection.model_source, ModelSelectionSource::CliOverride);
    }

    #[test]
    fn provider_resolution_uses_cli_provider_when_present() {
        let mut config = VTCodeConfig::default();
        config.agent.provider = "zai".to_owned();

        let args = Cli::parse_from([
            "vtcode",
            "--model",
            crate::config::constants::models::ollama::MINIMAX_M25_CLOUD,
            "--provider",
            "minimax",
        ]);
        let selection = resolve_runtime_model_selection(&args, &config);

        assert_eq!(selection.provider, "minimax");
    }

    #[test]
    fn build_runtime_agent_config_uses_provider_default_api_key_env() {
        let mut config = VTCodeConfig::default();
        config.agent.api_key_env = defaults::DEFAULT_API_KEY_ENV.to_owned();

        let args = Cli::parse_from(["vtcode", "--provider", "openai"]);
        let selection = RuntimeModelSelection {
            model: crate::config::constants::models::openai::GPT_5.to_owned(),
            provider: "openai".to_owned(),
            model_source: ModelSelectionSource::CliOverride,
        };

        let agent_config = build_runtime_agent_config(
            &args,
            &config,
            PathBuf::from("/workspace"),
            selection,
            "test-key".to_owned(),
            "dark".to_owned(),
        );

        assert_eq!(agent_config.api_key_env, "OPENAI_API_KEY");
    }

    #[test]
    fn build_runtime_agent_config_respects_cli_api_key_env_override() {
        let config = VTCodeConfig::default();
        let args = Cli::parse_from([
            "vtcode",
            "--provider",
            "openai",
            "--api-key-env",
            "CUSTOM_OPENAI_KEY",
        ]);
        let selection = RuntimeModelSelection {
            model: crate::config::constants::models::openai::GPT_5.to_owned(),
            provider: "openai".to_owned(),
            model_source: ModelSelectionSource::CliOverride,
        };

        let agent_config = build_runtime_agent_config(
            &args,
            &config,
            PathBuf::from("/workspace"),
            selection,
            "test-key".to_owned(),
            "dark".to_owned(),
        );

        assert_eq!(agent_config.api_key_env, "CUSTOM_OPENAI_KEY");
    }

    #[test]
    fn provider_label_uses_custom_provider_display_name() {
        let mut config = VTCodeConfig::default();
        config
            .custom_providers
            .push(vtcode_config::core::CustomProviderConfig {
                name: "mycorp".to_string(),
                display_name: "MyCorporateName".to_string(),
                base_url: "https://llm.example/v1".to_string(),
                api_key_env: "MYCORP_API_KEY".to_string(),
                model: "gpt-5-mini".to_string(),
            });

        assert_eq!(provider_label("mycorp", Some(&config)), "MyCorporateName");
    }

    #[test]
    fn resolve_checkpointing_storage_dir_preserves_absolute_path() {
        let resolved = resolve_checkpointing_storage_dir(
            Path::new("/workspace"),
            Some("/tmp/vtcode-checkpoints"),
        );

        assert_eq!(resolved, Some(PathBuf::from("/tmp/vtcode-checkpoints")));
    }
}
