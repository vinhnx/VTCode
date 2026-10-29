use super::*;

pub(super) fn listing_matches_workspace(listing: &SessionListing, workspace: &Path) -> bool {
    let Some(listing_workspace) = session_workspace_path(listing) else {
        return false;
    };
    vtcode_commons::paths::canonicalize(listing_workspace).is_ok_and(|path| path == workspace)
}

pub(super) fn workspace_label(workspace: &Path) -> String {
    workspace
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or("workspace")
        .to_string()
}

/// The startup agent config with any `config/select` override applied.
pub(super) async fn active_agent_config(context: &ServiceContext) -> vtcode_core::config::types::AgentConfig {
    let mut config = context.startup.agent_config.clone();
    let state = context.state.lock().await;
    if let Some(selection) = state.model_override.as_ref() {
        config.provider.clone_from(&selection.provider);
        config.model.clone_from(&selection.model);
    }
    if let Some(effort) = state
        .session_options
        .reasoning_effort
        .as_deref()
        .and_then(vtcode_commons::reasoning::ReasoningEffortLevel::parse)
    {
        config.reasoning_effort = effort;
    }
    if let Some(theme) = state.theme_override.as_ref() {
        config.theme.clone_from(theme);
    }
    config
}

pub(super) async fn effective_speed_config(
    context: &ServiceContext,
    agent: &vtcode_core::config::types::AgentConfig,
) -> vtcode_core::config::loader::VTCodeConfig {
    let mut config = context.startup.config.clone();
    let enabled = context
        .state
        .lock()
        .await
        .fast_mode_overrides
        .get(&agent.provider.to_ascii_lowercase())
        .copied();
    if let Some(enabled) = enabled
        && crate::agent::runloop::unified::fast_mode::supports_fast_mode(agent, &config)
    {
        crate::agent::runloop::unified::fast_mode::set_fast_mode_preference(agent, &mut config, enabled)
            .expect("supported speed route accepts its fast-mode preference");
    }
    config
}

pub(super) async fn configuration_snapshot(
    context: &ServiceContext,
    agent: &vtcode_core::config::types::AgentConfig,
) -> Result<ConfigurationSnapshot> {
    // Tool-capable startup already ran the shared provider-auth preflight.
    // Reaching the config surface means required credentials are available,
    // or the selected provider does not require a credential.
    let setup = if agent.provider.trim().is_empty() {
        "vtcode auth".to_string()
    } else {
        format!("vtcode login {}", agent.provider)
    };
    let options = context.state.lock().await.session_options.clone();
    let speed_config = effective_speed_config(context, agent).await;
    let fast_mode_supported = crate::agent::runloop::unified::fast_mode::supports_fast_mode(agent, &speed_config);
    let fast_mode_enabled =
        fast_mode_supported && crate::agent::runloop::unified::fast_mode::fast_mode_selected(agent, &speed_config);
    let skip_confirmations = session_options::skip_confirmations(&options, context.startup.skip_confirmations);
    let modes = session_options::discover_modes(context.workspace.clone()).await?;
    let primary = vtcode_core::primary_agent::ActivePrimaryAgentState::from_specs_with_default(
        &modes,
        options.mode.as_deref().unwrap_or(&context.startup.config.default_primary_agent),
    );
    Ok(ConfigurationSnapshot {
        session_options: Some(protocol::SessionOptionsSnapshot {
            permission_mode: if options.permission_mode == Some(protocol::PermissionMode::FullAccess) {
                protocol::PermissionMode::FullAccess
            } else if skip_confirmations {
                protocol::PermissionMode::AutoApproveCommands
            } else {
                protocol::PermissionMode::Ask
            },
            mode: primary.active().name().to_owned(),
            available_modes: modes
                .iter()
                .map(|spec| {
                    let agent = vtcode_core::primary_agent::ActivePrimaryAgent::from_spec(spec);
                    protocol::AgentModeInfo {
                        name: spec.name.clone(),
                        display_name: agent.display_name,
                        description: spec.description.chars().take(500).collect(),
                    }
                })
                .collect(),
            reasoning_effort: agent.reasoning_effort.as_str().into(),
            supported_efforts: session_options::supported_efforts(&agent.provider, &agent.model),
            speed: crate::agent::runloop::effective_speed_label(agent, &speed_config).map(str::to_owned),
            fast_mode_supported,
            fast_mode_enabled,
            fast_mode_label: if fast_mode_supported {
                crate::agent::runloop::unified::fast_mode::fast_mode_label(agent).map(str::to_owned)
            } else {
                None
            },
        }),
        provider: agent.provider.clone(),
        model: agent.model.clone(),
        authenticated: true,
        credential_setup_command: setup,
        sandbox: Some(
            if options.permission_mode == Some(protocol::PermissionMode::FullAccess) {
                "Full access"
            } else if skip_confirmations {
                "Auto-approved commands"
            } else {
                "Ask before protected actions"
            }
            .to_string(),
        ),
        shared_ui: Some(shared_preferences::snapshot(&context.startup.config.ui.keybindings, &agent.theme)),
        context_window: model_catalog_entry(&agent.provider, &agent.model).map(|entry| entry.context_window as u64),
    })
}

/// The selectable model catalog, mirroring the TUI picker's source.
pub(super) fn catalog_models(active_provider: &str, active_model: &str) -> Vec<ModelInfo> {
    let recommended_models = ModelId::fallback_models();
    ModelId::all_models()
        .into_iter()
        .map(|model| {
            let provider = model.provider().as_ref().to_string();
            let id = model.as_str().to_string();
            let recommended_rank = recommended_models
                .iter()
                .position(|recommended| recommended == &model)
                .and_then(|rank| u8::try_from(rank).ok());
            let catalog = model_catalog_entry(&provider, &id);
            ModelInfo {
                context_window: catalog.map(|entry| entry.context_window as u64),
                current: provider == active_provider && id == active_model,
                recommended_rank,
                capabilities: catalog.map(|entry| ModelCapabilities {
                    reasoning: entry.reasoning,
                    vision: entry.vision,
                    tool_call: entry.tool_call,
                    structured_output: entry.structured_output,
                    is_pro: entry.is_pro,
                    input_modalities: entry.input_modalities.iter().map(|modality| (*modality).to_string()).collect(),
                }),
                display_name: model.display_name().to_string(),
                provider,
                id,
            }
        })
        .collect()
}

pub(super) fn sanitize_public_error(message: &str) -> String {
    let sanitized = vtcode_commons::sanitizer::sanitize_provider_diagnostic(message)
        .chars()
        .filter(|character| !character.is_control())
        .take(240)
        .collect::<String>();
    if sanitized.trim().is_empty() {
        "desktop service request failed".to_string()
    } else {
        sanitized
    }
}
