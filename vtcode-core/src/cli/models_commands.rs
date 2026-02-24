//! Model management command handlers with concise, actionable output

use super::args::{Cli, ModelCommands};
use crate::llm::factory::{create_provider_with_config, get_factory};
use crate::utils::colors::{bold, cyan, dimmed, green, red, underline, yellow};
use crate::utils::dot_config::{DotConfig, get_dot_manager, load_user_config};
use anyhow::{Context, Result, anyhow};

/// Handle model management commands with concise output
pub async fn handle_models_command(cli: &Cli, command: &ModelCommands) -> Result<()> {
    match command {
        ModelCommands::List => handle_list_models(cli).await,
        ModelCommands::SetProvider { provider } => handle_set_provider(cli, provider).await,
        ModelCommands::SetModel { model } => handle_set_model(cli, model).await,
        ModelCommands::Config {
            provider,
            api_key,
            base_url,
            model,
        } => {
            handle_config_provider(
                cli,
                provider,
                api_key.as_deref(),
                base_url.as_deref(),
                model.as_deref(),
            )
            .await
        }
        ModelCommands::Test { provider } => handle_test_provider(cli, provider).await,
        ModelCommands::Compare => handle_compare_models(cli).await,
        ModelCommands::Info { model } => handle_model_info(cli, model).await,
    }
}

/// Display available providers and models with status
async fn handle_list_models(_cli: &Cli) -> Result<()> {
    println!("{}", underline(&bold("Available Providers & Models")));
    println!();

    let config = load_user_config().await.unwrap_or_default();
    let factory = {
        let guard = get_factory()
            .lock()
            .map_err(|err| anyhow!("LLM factory lock poisoned while listing providers: {err}"))?;
        guard.list_providers()
    }; // Lock is released here when guard goes out of scope
    let providers = factory;

    for provider_name in &providers {
        let is_current = config.preferences.default_provider == *provider_name;
        let status = if is_current { "✦" } else { "  " };
        let provider_display = format!("{}{}", status, provider_name.to_uppercase());

        let colored_provider = if is_current {
            green(&bold(&provider_display))
        } else {
            bold(&provider_display)
        };
        println!("{}", colored_provider);

        if let Ok(provider) = create_provider_with_config(
            provider_name,
            Some("dummy".to_owned()),
            None,
            None,
            None,
            None,
            None,
            None,
        ) {
            let models = provider.supported_models();
            let current_model = &config.preferences.default_model;

            for model in models.iter().take(3) {
                let is_current_model = current_model == model;
                let model_status = if is_current_model { "*" } else { "  " };
                let colored_model = if is_current_model {
                    cyan(&bold(model))
                } else {
                    cyan(model)
                };
                println!("  {}{}", model_status, colored_model);
            }
            if models.len() > 3 {
                println!("  {} +{} more models", dimmed("..."), models.len() - 3);
            }
        } else {
            println!("  {}", yellow("・  Setup required"));
        }

        let configured = is_provider_configured(&config, provider_name);
        let config_status = if configured {
            green("✓ Configured")
        } else {
            yellow("・  Not configured")
        };
        println!("  {}", config_status);
        println!();
    }

    println!("{}", underline(&bold("・ Current Config")));
    println!("Provider: {}", cyan(&config.preferences.default_provider));
    println!("Model: {}", cyan(&config.preferences.default_model));

    Ok(())
}

/// Check if provider is configured
fn is_provider_configured(config: &DotConfig, provider: &str) -> bool {
    let (provider_config, default_enabled) = match provider {
        "openai" => (config.providers.openai.as_ref(), false),
        "anthropic" => (config.providers.anthropic.as_ref(), false),
        "gemini" => (config.providers.gemini.as_ref(), false),
        "deepseek" => (config.providers.deepseek.as_ref(), false),
        "openrouter" => (config.providers.openrouter.as_ref(), false),
        "xai" => (config.providers.xai.as_ref(), false),
        "ollama" => (config.providers.ollama.as_ref(), true),
        "lmstudio" => (config.providers.lmstudio.as_ref(), true),
        _ => return false,
    };
    provider_config
        .map(|p| p.enabled)
        .unwrap_or(default_enabled)
}

/// Set default provider
async fn handle_set_provider(_cli: &Cli, provider: &str) -> Result<()> {
    let available = {
        let factory = get_factory()
            .lock()
            .map_err(|err| anyhow!("LLM factory lock poisoned while setting provider: {err}"))?;
        factory.list_providers()
    }; // Lock is released here when factory guard goes out of scope

    if !available.iter().any(|p| p == provider) {
        return Err(anyhow!(
            "Unknown provider '{}'. Available: {}",
            provider,
            available.join(", ")
        ));
    }

    let manager = {
        let guard = get_dot_manager()
            .context("Failed to initialize dot manager while setting provider")?
            .lock()
            .map_err(|err| anyhow!("Dot manager lock poisoned while setting provider: {err}"))?;
        guard.clone()
    }; // Lock is released here when guard goes out of scope
    manager
        .update_config(|config| {
            config.preferences.default_provider = provider.to_string();
        })
        .await?;

    println!("{} Provider set to: {}", green("✓"), green(&bold(provider)));
    println!(
        "{} Configure: {}",
        cyan("・"),
        dimmed(&format!(
            "vtcode models config {} --api-key YOUR_KEY",
            provider
        ))
    );

    Ok(())
}

/// Set default model
async fn handle_set_model(_cli: &Cli, model: &str) -> Result<()> {
    let manager = {
        let guard = get_dot_manager()
            .context("Failed to initialize dot manager while setting model")?
            .lock()
            .map_err(|err| anyhow!("Dot manager lock poisoned while setting model: {err}"))?;
        guard.clone()
    }; // Lock is released here when guard goes out of scope
    manager
        .update_config(|config| {
            config.preferences.default_model = model.to_string();
        })
        .await?;

    println!("{} Model set to: {}", green("✓"), green(&bold(model)));
    Ok(())
}

/// Configure provider settings
async fn handle_config_provider(
    _cli: &Cli,
    provider: &str,
    api_key: Option<&str>,
    base_url: Option<&str>,
    model: Option<&str>,
) -> Result<()> {
    // Clone manager once and reuse for both operations
    let manager = {
        let guard = get_dot_manager()
            .context("Failed to initialize dot manager while configuring provider")?
            .lock()
            .map_err(|err| {
                anyhow!("Dot manager lock poisoned while configuring provider: {err}")
            })?;
        guard.clone()
    };

    let mut config = manager.load_config().await?;

    match provider {
        "openai" | "anthropic" | "gemini" | "openrouter" | "deepseek" | "xai" | "ollama"
        | "lmstudio" => {
            configure_standard_provider(&mut config, provider, api_key, base_url, model)?;
        }
        _ => return Err(anyhow!("Unsupported provider: {}", provider)),
    }

    // Reuse the same manager instance
    manager.save_config(&config).await?;

    Ok(())
}

/// Configure standard providers
fn configure_standard_provider(
    config: &mut DotConfig,
    provider: &str,
    api_key: Option<&str>,
    base_url: Option<&str>,
    model: Option<&str>,
) -> Result<()> {
    // Helper macro to reduce boilerplate
    macro_rules! get_provider_config {
        ($field:ident) => {
            config.providers.$field.get_or_insert_with(Default::default)
        };
    }

    let provider_config = match provider {
        "openai" => get_provider_config!(openai),
        "anthropic" => get_provider_config!(anthropic),
        "gemini" => get_provider_config!(gemini),
        "deepseek" => get_provider_config!(deepseek),
        "openrouter" => get_provider_config!(openrouter),
        "xai" => get_provider_config!(xai),
        "ollama" => get_provider_config!(ollama),
        "lmstudio" => get_provider_config!(lmstudio),
        "minimax" => get_provider_config!(anthropic), // Note: maps to anthropic
        _ => return Err(anyhow!("Unknown provider: {}", provider)),
    };

    if let Some(key) = api_key {
        provider_config.api_key = Some(key.to_owned());
    }
    if let Some(url) = base_url {
        provider_config.base_url = Some(url.to_owned());
    }
    if let Some(m) = model {
        provider_config.model = Some(m.to_owned());
    }

    // Local providers are enabled by default; others require an API key
    provider_config.enabled = matches!(provider, "ollama" | "lmstudio")
        || api_key.is_some()
        || provider_config.api_key.is_some();

    Ok(())
}

/// Test provider connectivity
async fn handle_test_provider(_cli: &Cli, provider: &str) -> Result<()> {
    println!("{} Testing {}...", cyan("・"), bold(provider));

    let config = load_user_config().await?;
    let (api_key, base_url, model) = get_provider_credentials(&config, provider)?;

    let provider_instance = create_provider_with_config(
        provider,
        api_key,
        base_url,
        model.clone(),
        None,
        None,
        None,
        None,
    )?;

    let test_request = crate::llm::provider::LLMRequest {
        messages: vec![crate::llm::provider::Message::user("test".to_owned())],
        model: model.clone().unwrap_or_else(|| "test".to_owned()),
        max_tokens: Some(10),
        temperature: Some(0.0),
        ..Default::default()
    };

    match provider_instance.generate(test_request).await {
        Ok(response) => {
            let content = response.content.unwrap_or_default();
            if content.to_lowercase().contains("ok") {
                println!("{} {} test successful!", green("✓"), green(&bold(provider)));
            } else {
                println!(
                    "{} {} responded unexpectedly",
                    yellow("・"),
                    yellow(&bold(provider))
                );
            }
        }
        Err(e) => {
            println!("{} {} test failed: {}", red("✦"), red(&bold(provider)), e);
        }
    }

    Ok(())
}

/// Get provider credentials
fn get_provider_credentials(
    config: &DotConfig,
    provider: &str,
) -> Result<(Option<String>, Option<String>, Option<String>)> {
    let provider_config = match provider {
        "openai" => config.providers.openai.as_ref(),
        "anthropic" => config.providers.anthropic.as_ref(),
        "gemini" => config.providers.gemini.as_ref(),
        "deepseek" => config.providers.deepseek.as_ref(),
        "openrouter" => config.providers.openrouter.as_ref(),
        "xai" => config.providers.xai.as_ref(),
        "ollama" => config.providers.ollama.as_ref(),
        "lmstudio" => config.providers.lmstudio.as_ref(),
        _ => return Err(anyhow!("Unknown provider: {}", provider)),
    };

    Ok(provider_config
        .map(|c| (c.api_key.clone(), c.base_url.clone(), c.model.clone()))
        .unwrap_or((None, None, None)))
}

/// Compare model performance (placeholder)
async fn handle_compare_models(_cli: &Cli) -> Result<()> {
    println!("{}", underline(&bold("✦ Model Performance Comparison")));
    println!();
    println!("{} Coming soon! Will compare:", yellow("✦"));
    println!("• Response times • Token usage • Cost • Quality");
    println!();
    println!(
        "{} Use 'vtcode models list' for available models",
        cyan("・")
    );

    Ok(())
}

/// Show model information
async fn handle_model_info(_cli: &Cli, model: &str) -> Result<()> {
    println!("{} Model Info: {}", cyan("・"), underline(&bold(model)));
    println!();

    println!("Model: {}", cyan(model));
    println!("Provider: {}", infer_provider_from_model(model));
    println!("Status: {}", green("Available"));
    println!();
    println!("{} Check docs/models.json for specs", cyan("・"));

    Ok(())
}

/// Infer provider from model name
fn infer_provider_from_model(model: &str) -> &'static str {
    crate::llm::factory::infer_provider(None, model)
        .map(|provider| provider.label())
        .unwrap_or("Unknown")
}
