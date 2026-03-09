use std::path::PathBuf;
use std::str::FromStr;

use vtcode_core::cli::args::Cli;
use vtcode_core::config::constants::defaults;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, ModelSelectionSource};
use vtcode_core::llm::factory::infer_provider;

pub(super) fn provider_label(provider: &str) -> String {
    Provider::from_str(provider)
        .map(|resolved| resolved.label().to_string())
        .unwrap_or_else(|_| provider.to_string())
}

pub(super) fn api_key_env_var(provider: &str) -> String {
    Provider::from_str(provider)
        .map(|resolved| resolved.default_api_key_env().to_owned())
        .unwrap_or_else(|_| format!("{}_API_KEY", provider.to_uppercase()))
}

pub(super) fn resolve_provider(
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

pub(super) fn provider_env_override() -> Option<String> {
    std::env::var("VTCODE_PROVIDER")
        .ok()
        .or_else(|| std::env::var("provider").ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub(super) fn build_agent_config(
    args: &Cli,
    config: &VTCodeConfig,
    workspace: PathBuf,
    provider: String,
    model: String,
    model_source: ModelSelectionSource,
    api_key: String,
    theme_selection: String,
) -> CoreAgentConfig {
    let provider_enum = Provider::from_str(&provider).unwrap_or(Provider::Gemini);
    let cli_api_key_env = args.api_key_env.trim();
    let api_key_env_override = if cli_api_key_env.is_empty()
        || cli_api_key_env.eq_ignore_ascii_case(defaults::DEFAULT_API_KEY_ENV)
    {
        None
    } else {
        Some(cli_api_key_env.to_owned())
    };

    let configured_api_key_env = config.agent.api_key_env.trim();
    let provider_default_env = provider_enum.default_api_key_env();
    let resolved_api_key_env = if configured_api_key_env.is_empty()
        || configured_api_key_env.eq_ignore_ascii_case(defaults::DEFAULT_API_KEY_ENV)
    {
        provider_default_env.to_owned()
    } else {
        configured_api_key_env.to_owned()
    };

    let api_key_env = api_key_env_override.unwrap_or(resolved_api_key_env);
    let checkpointing_storage_dir = config.agent.checkpointing.storage_dir.as_ref().map(|dir| {
        let candidate = PathBuf::from(dir);
        if candidate.is_absolute() {
            candidate
        } else {
            workspace.join(candidate)
        }
    });

    CoreAgentConfig {
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
    }
}

pub fn check_prompt_cache_retention_compat(
    config: &VTCodeConfig,
    model: &str,
    provider: &str,
) -> Option<String> {
    if !provider.eq_ignore_ascii_case("openai") {
        return None;
    }

    if let Some(ref retention) = config.prompt_cache.providers.openai.prompt_cache_retention {
        if retention.trim().is_empty() {
            return None;
        }
        if !vtcode_core::config::constants::models::openai::RESPONSES_API_MODELS.contains(&model) {
            return Some(format!(
                "`prompt_cache_retention` is set but the selected model '{}' does not use the OpenAI Responses API. The setting will be ignored for this model. Run `vtcode models list --provider openai` to see supported Responses API models.",
                model
            ));
        }
    }

    None
}
