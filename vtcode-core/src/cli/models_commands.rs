//! Model management command handlers with concise, actionable output

use super::args::{Cli, ModelCommands};
use crate::llm::factory::{create_provider_with_config, get_factory};
use crate::utils::colors::{bold, cyan, dimmed, green, red, underline, yellow};
use crate::utils::dot_config::{DotConfig, get_dot_manager, load_user_config};
use anyhow::{Result, anyhow};

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

    let factory = get_factory().lock().unwrap();
    let config = load_user_config().await.unwrap_or_default();
    let providers = factory.list_providers();

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

        if let Ok(provider) =
            create_provider_with_config(provider_name, Some("dummy".to_string()), None, None, None)
        {
            let models = provider.supported_models();
            let current_model = &config.preferences.default_model;

            for model in models.iter().take(3) {
                let is_current_model = current_model == model;
                let model_status = if is_current_model { "⭐" } else { "  " };
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
    match provider {
        "openai" => config
            .providers
            .openai
            .as_ref()
            .map(|p| p.enabled)
            .unwrap_or(false),
        "anthropic" => config
            .providers
            .anthropic
            .as_ref()
            .map(|p| p.enabled)
            .unwrap_or(false),
        "gemini" => config
            .providers
            .gemini
            .as_ref()
            .map(|p| p.enabled)
            .unwrap_or(false),
        "deepseek" => config
            .providers
            .deepseek
            .as_ref()
            .map(|p| p.enabled)
            .unwrap_or(false),
        "openrouter" => config
            .providers
            .openrouter
            .as_ref()
            .map(|p| p.enabled)
            .unwrap_or(false),
        "xai" => config
            .providers
            .xai
            .as_ref()
            .map(|p| p.enabled)
            .unwrap_or(false),
        "ollama" => config
            .providers
            .ollama
            .as_ref()
            .map(|p| p.enabled)
            .unwrap_or(true),
        "lmstudio" => config
            .providers
            .lmstudio
            .as_ref()
            .map(|p| p.enabled)
            .unwrap_or(true),
        _ => false,
    }
}

/// Set default provider
async fn handle_set_provider(_cli: &Cli, provider: &str) -> Result<()> {
    let factory = get_factory().lock().unwrap();
    let available = factory.list_providers();

    if !available.contains(&provider.to_string()) {
        return Err(anyhow!(
            "Unknown provider '{}'. Available: {}",
            provider,
            available.join(", ")
        ));
    }

    let manager = get_dot_manager().lock().unwrap();
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
    let manager = get_dot_manager().lock().unwrap();
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
    let manager = get_dot_manager().lock().unwrap();
    let mut config = manager.load_config().await?;

    match provider {
        "openai" | "anthropic" | "gemini" | "openrouter" | "deepseek" | "xai" | "ollama"
        | "lmstudio" => {
            configure_standard_provider(&mut config, provider, api_key, base_url, model)?;
        }
        _ => return Err(anyhow!("Unsupported provider: {}", provider)),
    }

    manager.save_config(&config).await?;
    println!("{} {} configured!", green("✓"), green(&bold(provider)));

    if let Some(key) = api_key {
        let masked = mask_api_key(key);
        println!("  API Key: {}", dimmed(&masked));
    }
    if let Some(url) = base_url {
        println!("  Base URL: {}", dimmed(url));
    }
    if let Some(m) = model {
        println!("  Model: {}", dimmed(m));
    }

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
    let provider_config = match provider {
        "openai" => config.providers.openai.get_or_insert_with(Default::default),
        "anthropic" => config
            .providers
            .anthropic
            .get_or_insert_with(Default::default),
        "gemini" => config.providers.gemini.get_or_insert_with(Default::default),
        "deepseek" => config
            .providers
            .deepseek
            .get_or_insert_with(Default::default),
        "openrouter" => config
            .providers
            .openrouter
            .get_or_insert_with(Default::default),
        "xai" => config.providers.xai.get_or_insert_with(Default::default),
        "ollama" => config.providers.ollama.get_or_insert_with(Default::default),
        "lmstudio" => config
            .providers
            .lmstudio
            .get_or_insert_with(Default::default),
        "minimax" => config
            .providers
            .anthropic
            .get_or_insert_with(Default::default),
        _ => return Err(anyhow!("Unknown provider: {}", provider)),
    };

    if let Some(key) = api_key {
        provider_config.api_key = Some(key.to_string());
    }
    if let Some(url) = base_url {
        provider_config.base_url = Some(url.to_string());
    }
    if let Some(m) = model {
        provider_config.model = Some(m.to_string());
    }
    provider_config.enabled = if provider == "ollama" || provider == "lmstudio" {
        true
    } else {
        api_key.is_some() || provider_config.api_key.is_some()
    };

    Ok(())
}

/// Test provider connectivity
async fn handle_test_provider(_cli: &Cli, provider: &str) -> Result<()> {
    println!("{} Testing {}...", cyan("・"), bold(provider));

    let config = load_user_config().await?;
    let (api_key, base_url, model) = get_provider_credentials(&config, provider)?;

    let provider_instance =
        create_provider_with_config(provider, api_key, base_url, model.clone(), None)?;

    let test_request = crate::llm::provider::LLMRequest {
        messages: vec![crate::llm::provider::Message {
            role: crate::llm::provider::MessageRole::User,
            content: "Respond with 'OK' if you receive this message.".to_string(),
            reasoning: None,
            reasoning_details: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        system_prompt: None,
        tools: None,
        model: model.unwrap_or_else(|| "test".to_string()),
        max_tokens: Some(10),
        temperature: Some(0.1),
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
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
    let get_config = |p: Option<&crate::utils::dot_config::ProviderConfig>| {
        p.map(|c| (c.api_key.clone(), c.base_url.clone(), c.model.clone()))
            .unwrap_or((None, None, None))
    };

    match provider {
        "openai" => Ok(get_config(config.providers.openai.as_ref())),
        "anthropic" => Ok(get_config(config.providers.anthropic.as_ref())),
        "gemini" => Ok(get_config(config.providers.gemini.as_ref())),
        "deepseek" => Ok(get_config(config.providers.deepseek.as_ref())),
        "openrouter" => Ok(get_config(config.providers.openrouter.as_ref())),
        "xai" => Ok(get_config(config.providers.xai.as_ref())),
        "ollama" => Ok(get_config(config.providers.ollama.as_ref())),
        "lmstudio" => Ok(get_config(config.providers.lmstudio.as_ref())),
        _ => Err(anyhow!("Unknown provider: {}", provider)),
    }
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

/// Mask API key for display
fn mask_api_key(key: &str) -> String {
    if key.len() > 8 {
        format!("{}****{}", &key[..4], &key[key.len().saturating_sub(4)..])
    } else {
        "****".to_string()
    }
}
