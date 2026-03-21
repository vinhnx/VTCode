use anyhow::{Context, Result, anyhow};
use std::path::Path;
use vtcode_config::{read_workspace_env_value, resolve_openai_auth, write_workspace_env_value};

use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, UiSurfacePreference};
use vtcode_core::copilot::{CopilotAuthStatusKind, probe_auth_status};
use vtcode_core::llm::factory::{ProviderConfig, create_provider_with_config};
use vtcode_core::llm::provider::LLMProvider;
use vtcode_core::llm::rig_adapter::RigProviderCapabilities;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::app::{InlineHandle, InlineHeaderContext};

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
    header_context: &mut InlineHeaderContext,
    full_auto: bool,
    conversation_history_len: usize,
) -> Result<()> {
    let workspace = config.workspace.clone();
    let auth_cfg = vt_cfg.as_ref().cloned().unwrap_or_default();
    let (api_key, openai_chatgpt_auth) =
        resolve_runtime_api_key(&workspace, Some(&auth_cfg), &selection).await?;
    let using_chatgpt_auth =
        selection.provider_enum == Some(Provider::OpenAI) && openai_chatgpt_auth.is_some();
    let updated_cfg = picker.persist_selection(&workspace, &selection).await?;
    *vt_cfg = Some(updated_cfg);

    if let Some(provider_enum) = selection.provider_enum
        && let Err(err) =
            RigProviderCapabilities::new(provider_enum, &selection.model).validate_model(&api_key)
    {
        renderer.line(
            MessageStyle::Error,
            &format!(
                "Rig validation warning: unable to initialise {} via rig-core ({err}).",
                selection.model_display
            ),
        )?;
    }

    if let Some(provider_enum) = selection.provider_enum {
        let provider_name = selection.provider.clone();
        let new_client = create_provider_with_config(
            &provider_name,
            ProviderConfig {
                api_key: Some(api_key.clone()),
                openai_chatgpt_auth: openai_chatgpt_auth.clone(),
                copilot_auth: vt_cfg.as_ref().map(|cfg| cfg.auth.copilot.clone()),
                base_url: None,
                model: Some(selection.model.clone()),
                prompt_cache: Some(config.prompt_cache.clone()),
                timeouts: None,
                openai: vt_cfg.as_ref().map(|cfg| cfg.provider.openai.clone()),
                anthropic: None,
                model_behavior: config.model_behavior.clone(),
                workspace_root: Some(config.workspace.clone()),
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
    config.openai_chatgpt_auth = openai_chatgpt_auth;
    sync_runtime_custom_api_key(config, &selection);

    if let Some(provider_enum) = selection.provider_enum
        && selection.reasoning_supported
        && let Some(payload) = RigProviderCapabilities::new(provider_enum, &selection.model)
            .reasoning_parameters(selection.reasoning)
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
    let next_header_context = build_inline_header_context(
        config,
        session_bootstrap,
        runtime_provider_label(&selection, using_chatgpt_auth),
        selection.model.clone(),
        provider_client.effective_context_size(&selection.model),
        mode_label,
        reasoning_label.clone(),
    )
    .await?;
    header_context.clone_from(&next_header_context);
    handle.set_header_context(next_header_context);

    renderer.line(
        MessageStyle::Info,
        &format!(
            "Model set to {} ({}) via {}.",
            selection.model_display, selection.model, selection.provider_label
        ),
    )?;

    if conversation_history_len > 0 {
        renderer.line(
            MessageStyle::Warning,
            "Changing model mid-conversation may degrade performance due to context loss and token inefficiency. For best results, start a new conversation with /clear.",
        )?;
    }

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

    if using_chatgpt_auth {
        renderer.line(MessageStyle::Info, "Using ChatGPT subscription for OpenAI.")?;
    } else if selection.provider_enum == Some(Provider::Copilot) {
        renderer.line(
            MessageStyle::Info,
            "Using GitHub Copilot managed authentication.",
        )?;
    } else if selection.api_key.is_some() {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "API key saved to secure storage (keyring) and workspace .env as {}. The key will NOT appear in vtcode.toml.",
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

async fn resolve_runtime_api_key(
    workspace: &Path,
    vt_cfg: Option<&VTCodeConfig>,
    selection: &ModelSelectionResult,
) -> Result<(String, Option<vtcode_config::auth::OpenAIChatGptAuthHandle>)> {
    if let Some(key) = selection.api_key.as_ref() {
        write_workspace_env_value(workspace, &selection.env_key, key)?;
        return Ok((key.clone(), None));
    }

    if selection.provider_enum == Some(Provider::OpenAI)
        && let Some(cfg) = vt_cfg
    {
        let api_key = get_api_key(&selection.provider, &ApiKeySources::default()).ok();
        let resolved =
            resolve_openai_auth(&cfg.auth.openai, cfg.agent.credential_storage_mode, api_key)?;
        return Ok((resolved.api_key().to_string(), resolved.handle()));
    }

    if selection.provider_enum == Some(Provider::Copilot) {
        let Some(cfg) = vt_cfg else {
            return Err(anyhow!(
                "GitHub Copilot configuration is unavailable. Run `vtcode login copilot`."
            ));
        };
        let status = probe_auth_status(&cfg.auth.copilot, Some(workspace)).await;
        return match status.kind {
            CopilotAuthStatusKind::Authenticated => Ok((String::new(), None)),
            CopilotAuthStatusKind::Unauthenticated | CopilotAuthStatusKind::AuthFlowFailed => {
                Err(anyhow!(status.message.unwrap_or_else(|| {
                    "GitHub Copilot is not authenticated. Run `vtcode login copilot`."
                        .to_string()
                })))
            }
            CopilotAuthStatusKind::ServerUnavailable => Err(anyhow!(
                status.message.unwrap_or_else(|| {
                    "GitHub Copilot CLI is unavailable. Install `copilot`, set `VTCODE_COPILOT_COMMAND`, or configure `[auth.copilot].command`."
                        .to_string()
                })
            )),
        };
    }

    if let Some(key) = read_workspace_api_key(workspace, &selection.env_key)? {
        return Ok((key, None));
    }

    if selection.provider_enum.is_some() {
        return get_api_key(&selection.provider, &ApiKeySources::default())
            .with_context(|| format!("API key not found for provider '{}'", selection.provider))
            .map(|key| (key, None));
    }

    match std::env::var(&selection.env_key) {
        Ok(value) if !value.trim().is_empty() => Ok((value, None)),
        _ if selection.requires_api_key => Err(anyhow!(
            "API key not found for provider '{}'. Set {} or enter a key to continue.",
            selection.provider,
            selection.env_key
        )),
        _ => Ok((String::new(), None)),
    }
}

fn read_workspace_api_key(workspace: &Path, env_key: &str) -> Result<Option<String>> {
    read_workspace_env_value(workspace, env_key)
        .with_context(|| format!("Failed to read workspace .env value for {}", env_key))
}

fn sync_runtime_custom_api_key(config: &mut CoreAgentConfig, selection: &ModelSelectionResult) {
    if selection.provider_enum == Some(Provider::OpenAI) && selection.uses_chatgpt_auth {
        return;
    }

    if selection.api_key.is_some() {
        config
            .custom_api_keys
            .insert(selection.provider.clone(), String::new());
        return;
    }

    config.custom_api_keys.remove(&selection.provider);
}

fn runtime_provider_label(selection: &ModelSelectionResult, using_chatgpt_auth: bool) -> String {
    if selection.provider_enum == Some(Provider::OpenAI) && using_chatgpt_auth {
        "OpenAI (ChatGPT)".to_string()
    } else {
        selection.provider_label.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{read_workspace_api_key, resolve_runtime_api_key};
    use crate::agent::runloop::model_picker::ModelSelectionResult;
    use tempfile::tempdir;
    use vtcode_core::config::models::Provider;
    use vtcode_core::config::types::ReasoningEffortLevel;

    fn selection(
        provider: &str,
        provider_enum: Option<Provider>,
        env_key: &str,
        api_key: Option<&str>,
        requires_api_key: bool,
    ) -> ModelSelectionResult {
        ModelSelectionResult {
            provider: provider.to_string(),
            provider_label: provider.to_string(),
            provider_enum,
            model: "test-model".to_string(),
            model_display: "test-model".to_string(),
            known_model: false,
            reasoning_supported: false,
            reasoning: ReasoningEffortLevel::Medium,
            reasoning_changed: false,
            service_tier_supported: false,
            service_tier: None,
            service_tier_changed: false,
            api_key: api_key.map(ToString::to_string),
            env_key: env_key.to_string(),
            requires_api_key,
            uses_chatgpt_auth: false,
        }
    }

    #[test]
    fn resolve_runtime_api_key_prefers_workspace_env_file() {
        let dir = tempdir().expect("temp dir");
        std::fs::write(dir.path().join(".env"), "OPENAI_API_KEY=workspace-key\n")
            .expect("workspace env");
        let selection = selection(
            "openai",
            Some(Provider::OpenAI),
            "OPENAI_API_KEY",
            None,
            true,
        );

        let resolved = tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(resolve_runtime_api_key(dir.path(), None, &selection))
            .expect("workspace env should resolve");

        assert_eq!(resolved.0, "workspace-key");
    }

    #[test]
    fn resolve_runtime_api_key_writes_user_supplied_key_to_workspace_env() {
        let dir = tempdir().expect("temp dir");
        let selection = selection(
            "openai",
            Some(Provider::OpenAI),
            "OPENAI_API_KEY",
            Some("user-key"),
            true,
        );

        let resolved = tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(resolve_runtime_api_key(dir.path(), None, &selection))
            .expect("user key should resolve");
        let written =
            read_workspace_api_key(dir.path(), "OPENAI_API_KEY").expect("workspace env read");

        assert_eq!(resolved.0, "user-key");
        assert_eq!(written.as_deref(), Some("user-key"));
    }

    #[test]
    fn resolve_runtime_api_key_errors_for_missing_custom_provider_key() {
        let dir = tempdir().expect("temp dir");
        let selection = selection("custom", None, "CUSTOM_API_KEY", None, true);

        let err = tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(resolve_runtime_api_key(dir.path(), None, &selection))
            .expect_err("missing custom provider key should fail");

        assert!(err.to_string().contains("CUSTOM_API_KEY"));
    }
}
