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

/// Credentials resolved when a `config/select` override was applied. A provider
/// switch must re-resolve: reusing the boot provider's key would authenticate
/// every later turn as the wrong provider (or with an empty key when the
/// service booted without credentials).
#[derive(Debug, Clone, Default)]
pub(super) struct OverrideAuth {
    pub(super) api_key: String,
    pub(super) openai_chatgpt_auth: Option<vtcode_config::auth::OpenAIChatGptAuthHandle>,
    /// Resolution failure for the override provider; `None` means authenticated.
    pub(super) error: Option<String>,
}

/// Resolve credentials for a `config/select` provider/model without failing
/// the selection. Callers report the outcome through `authenticated` so the
/// desktop can guide recovery instead of disconnecting.
pub(super) async fn resolve_override_auth(
    context: &ServiceContext,
    provider: &str,
    model: &str,
    api_key_env: &str,
) -> OverrideAuth {
    let selection = vtcode_core::core::agent::config::RuntimeModelSelection {
        model: model.to_string(),
        provider: provider.to_string(),
        api_key_env: api_key_env.to_string(),
        model_source: vtcode_core::config::types::ModelSelectionSource::WorkspaceConfig,
    };
    match crate::startup::resolve_runtime_provider_auth(
        &context.startup.config,
        &context.workspace,
        &selection,
        false,
        None,
    )
    .await
    {
        Ok((api_key, handle)) => OverrideAuth { api_key, openai_chatgpt_auth: handle, error: None },
        Err(error) => OverrideAuth {
            api_key: String::new(),
            openai_chatgpt_auth: None,
            error: Some(format!("{error:#}")),
        },
    }
}

/// The startup agent config with any `config/select` override applied.
pub(super) async fn active_agent_config(context: &ServiceContext) -> vtcode_core::config::types::AgentConfig {
    let mut config = context.startup.agent_config.clone();
    let state = context.state.lock().await;
    if let Some(selection) = state.model_override.as_ref() {
        config.provider.clone_from(&selection.provider);
        config.model.clone_from(&selection.model);
        match state.model_override_auth.as_ref() {
            Some(auth) if auth.error.is_none() => {
                config.api_key.clone_from(&auth.api_key);
                config.openai_chatgpt_auth.clone_from(&auth.openai_chatgpt_auth);
            }
            Some(_) => {
                config.api_key.clear();
                config.openai_chatgpt_auth = None;
            }
            None => {}
        }
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
    // The desktop service boots without credentials so the app stays connected
    // while unauthenticated. Report the outcome honestly: the boot auth error
    // when no override is active, otherwise the select-time resolution result.
    let setup = if agent.provider.trim().is_empty() {
        "vtcode auth".to_string()
    } else {
        format!("vtcode login {}", agent.provider)
    };
    let options = context.state.lock().await.session_options.clone();
    let authenticated = match context.state.lock().await.model_override_auth.as_ref() {
        Some(auth) => auth.error.is_none(),
        None => context.startup.provider_auth_error.is_none(),
    };
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
        authenticated,
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

/// Cached updater status for the desktop client. Read-only: the startup
/// check only inspects the local update cache, so serving it performs no
/// network I/O and never blocks the request loop on GitHub.
pub(super) fn update_status_snapshot() -> Result<protocol::UpdateStatusSnapshot> {
    let current = env!("CARGO_PKG_VERSION");
    let updater = crate::updater::Updater::new(current)?;
    let check = updater.startup_update_check()?;
    let (latest_version, release_url, update_available) = match check.cached_notice {
        Some(notice) => {
            let url = crate::updater::Updater::release_url(&notice.latest_version);
            (Some(notice.latest_version.to_string()), Some(url), true)
        }
        None => (None, None, false),
    };
    Ok(protocol::UpdateStatusSnapshot {
        current_version: current.to_string(),
        latest_version,
        update_available,
        release_url,
        channel: Some(updater.config().channel.to_string()),
        check_due: check.should_refresh,
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
