use anyhow::{Context, Result, anyhow};
use vtcode_config::write_workspace_env_value;

use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, UiSurfacePreference};
use vtcode_core::llm::factory::{ProviderConfig, create_provider_with_config};
use vtcode_core::llm::provider::LLMProvider;
use vtcode_core::llm::rig_adapter::{reasoning_parameters_for, verify_model_with_rig};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::InlineHandle;

use crate::agent::runloop::model_picker::{ModelPickerState, ModelSelectionResult};
use crate::agent::runloop::welcome::SessionBootstrap;

use crate::agent::runloop::ui::build_inline_header_context;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn finalize_model_selection(
    renderer: &mut AnsiRenderer,
    picker: &ModelPickerState,
    selection: ModelSelectionResult,
    config: &mut CoreAgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
    provider_client: &mut Box<dyn LLMProvider>,
    session_bootstrap: &SessionBootstrap,
    handle: &InlineHandle,
    full_auto: bool,
) -> Result<()> {
    let workspace = config.workspace.clone();

    let api_key = if let Some(key) = selection.api_key.as_ref() {
        write_workspace_env_value(&workspace, &selection.env_key, key)?;
        unsafe {
            // SAFETY: we only write ASCII-alphanumeric keys derived from known providers or
            // sanitized user input, and values are supplied directly by the user.
            std::env::set_var(&selection.env_key, key);
        }
        key.clone()
    } else if selection.provider_enum.is_some() {
        let key = get_api_key(&selection.provider, &ApiKeySources::default())
            .with_context(|| format!("API key not found for provider '{}'", selection.provider))?;
        unsafe {
            // SAFETY: see above. Keys are sanitized and values come from configuration sources.
            std::env::set_var(&selection.env_key, &key);
        }
        key
    } else {
        match std::env::var(&selection.env_key) {
            Ok(value) if !value.trim().is_empty() => value,
            _ if selection.requires_api_key => {
                return Err(anyhow!(
                    "API key not found for provider '{}'. Set {} or enter a key to continue.",
                    selection.provider,
                    selection.env_key
                ));
            }
            _ => String::new(),
        }
    };

    if let Some(provider_enum) = selection.provider_enum
        && let Err(err) = verify_model_with_rig(provider_enum, &selection.model, &api_key)
    {
        renderer.line(
            MessageStyle::Error,
            &format!(
                "Rig validation warning: unable to initialise {} via rig-core ({err}).",
                selection.model_display
            ),
        )?;
    }

    let updated_cfg = picker.persist_selection(&workspace, &selection).await?;
    *vt_cfg = Some(updated_cfg);

    if let Some(provider_enum) = selection.provider_enum {
        let provider_name = selection.provider.clone();
        let new_client = create_provider_with_config(
            &provider_name,
            ProviderConfig {
                api_key: Some(api_key.clone()),
                base_url: None,
                model: Some(selection.model.clone()),
                prompt_cache: Some(config.prompt_cache.clone()),
                timeouts: None,
                openai: vt_cfg.as_ref().map(|cfg| cfg.provider.openai.clone()),
                anthropic: None,
                model_behavior: config.model_behavior.clone(),
            },
        )
        .context("Failed to initialize provider for the selected model")?;
        *provider_client = new_client;
        config.provider = provider_enum.to_string();
    } else {
        renderer.line(
            MessageStyle::Info,
            "Saved selection, but custom providers require manual configuration before taking effect.",
        )?;
        config.provider = selection.provider.clone();
    }

    config.model = selection.model.clone();
    config.api_key = api_key;
    config.reasoning_effort = selection.reasoning;
    config.api_key_env = selection.env_key.clone();
    sync_runtime_custom_api_key(config, &selection);

    if let Some(provider_enum) = selection.provider_enum
        && selection.reasoning_supported
        && let Some(payload) = reasoning_parameters_for(provider_enum, selection.reasoning)
    {
        renderer.line(
            MessageStyle::Info,
            &format!("Rig reasoning configuration prepared: {}", payload),
        )?;
    }

    let reasoning_label = selection.reasoning.as_str().to_string();
    let mode_label = match (config.ui_surface, full_auto) {
        (UiSurfacePreference::Inline, true) => "auto".to_string(),
        (UiSurfacePreference::Inline, false) => "inline".to_string(),
        (UiSurfacePreference::Alternate, _) => "alt".to_string(),
        (UiSurfacePreference::Auto, true) => "auto".to_string(),
        (UiSurfacePreference::Auto, false) => "std".to_string(),
    };
    let header_context = build_inline_header_context(
        config,
        session_bootstrap,
        selection.provider_label.clone(),
        selection.model.clone(),
        mode_label,
        reasoning_label.clone(),
    )
    .await?;
    handle.set_header_context(header_context);

    renderer.line(
        MessageStyle::Info,
        &format!(
            "Model set to {} ({}) via {}.",
            selection.model_display, selection.model, selection.provider_label
        ),
    )?;

    if !selection.known_model {
        renderer.line(
            MessageStyle::Info,
            "The selected model is not part of VT Code's curated list; capabilities may vary.",
        )?;
    }

    if selection.reasoning_supported {
        let message = if selection.reasoning_changed {
            format!("Reasoning effort updated to '{}'.", selection.reasoning)
        } else {
            format!("Reasoning effort remains '{}'.", selection.reasoning)
        };
        renderer.line(MessageStyle::Info, &message)?;
    }

    if selection.service_tier_supported {
        let service_tier_label = match selection.service_tier {
            Some(_) => "priority",
            None => "project default",
        };
        let message = if selection.service_tier_changed {
            format!("Service tier updated to '{}'.", service_tier_label)
        } else {
            format!("Service tier remains '{}'.", service_tier_label)
        };
        renderer.line(MessageStyle::Info, &message)?;
    }

    if selection.api_key.is_some() {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "API key saved to secure storage (keyring) and environment variable {}. The key will NOT appear in vtcode.toml.",
                selection.env_key
            ),
        )?;
    } else if selection.requires_api_key {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Using environment variable {} for authentication.",
                selection.env_key
            ),
        )?;
    }

    Ok(())
}

fn sync_runtime_custom_api_key(config: &mut CoreAgentConfig, selection: &ModelSelectionResult) {
    if selection.api_key.is_some() {
        config
            .custom_api_keys
            .insert(selection.provider.clone(), String::new());
        return;
    }

    config.custom_api_keys.remove(&selection.provider);
}
