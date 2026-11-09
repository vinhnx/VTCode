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

/// Credentials saved through `config/secret/store`, re-resolved at save time so
/// an in-app key save takes effect without a service restart. Applies to the
/// boot provider only; `config/select` overrides re-resolve on their own.
#[derive(Debug, Clone)]
pub(super) struct StoredSecretAuth {
    pub(super) provider: String,
    pub(super) auth: OverrideAuth,
}

/// Credentials from an in-app save that apply to `provider`: a recorded save
/// for the boot provider whose re-resolution succeeded.
fn stored_secret_for<'a>(state: &'a ServerState, provider: &str) -> Option<&'a OverrideAuth> {
    state
        .secret_store_auth
        .as_ref()
        .filter(|stored| stored.auth.error.is_none() && stored.provider.eq_ignore_ascii_case(provider))
        .map(|stored| &stored.auth)
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
    } else if let Some(auth) = stored_secret_for(&state, &config.provider) {
        // No override: a key saved in-app for the boot provider takes effect
        // for sessions started after the save, without a service restart.
        config.api_key.clone_from(&auth.api_key);
        config.openai_chatgpt_auth.clone_from(&auth.openai_chatgpt_auth);
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
    // while unauthenticated. Report the outcome honestly: override resolution
    // when one is active, otherwise an in-app boot-provider save when one
    // repaired it, else the boot auth error.
    // `vtcode login` only serves openai/openrouter/copilot/codex. Point every
    // other key-based provider at `vtcode secret add`, matching the CLI's own
    // missing-credential guidance, so desktop users are not sent to a command
    // that rejects their provider. Local providers need no credentials, so
    // they advertise no setup command at all.
    let provider = agent.provider.trim();
    let setup = if provider.is_empty() {
        "vtcode auth".to_string()
    } else if crate::cli::auth::supports_auth_provider(provider) {
        format!("vtcode login {provider}")
    } else {
        match crate::cli::secret::secret_target(&context.startup.config, provider, None) {
            Ok(target) if target.local => String::new(),
            _ => format!("vtcode secret add {provider}"),
        }
    };
    let secret_store = secret_store::snapshot_info(&context.startup.config, &agent.provider);
    let options = context.state.lock().await.session_options.clone();
    let authenticated = {
        let state = context.state.lock().await;
        match state.model_override_auth.as_ref() {
            Some(auth) => auth.error.is_none(),
            None => {
                stored_secret_for(&state, &agent.provider).is_some() || context.startup.provider_auth_error.is_none()
            }
        }
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
        secret_store,
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
    // The current-version accessor self-matches against the running version,
    // so it needs no extra pairing check.
    let (current_highlights, current_url) = match crate::updater::cached_current_release_highlights() {
        Some((version, items)) => (items, Some(crate::updater::Updater::release_url(&version))),
        None => (Vec::new(), None),
    };
    let current_release_seen = crate::updater::current_version_notes_seen();
    // Release notes are readable whether or not an update is being offered, but
    // `latest_version` must always name a release *newer* than the running one:
    // an equal or older cached release would make the reader claim backwards
    // ("latest known" pointing at the version you already run, or below it).
    // The client falls back to the running version's own notes then, which is
    // the honest label. `update_available` stays the separate offer gate, so a
    // dismissed update keeps its notes readable.
    let current_semver = semver::Version::parse(current).ok();
    let cached_release = crate::updater::cached_latest_release();
    let newer_cached_release = cached_release
        .as_ref()
        .filter(|(version, _)| current_semver.as_ref().is_some_and(|running| version > running));
    let update_available = check.cached_notice.is_some();
    let (latest_version, release_url, release_highlights) = match check.cached_notice {
        // An offered update wins the slot even when its notes are missing, so a
        // notes-less cache can never hide an update. Notes attach only when the
        // same release backed them (two cache reads, so a concurrent refresh
        // must not pair one release's version with another's changelog).
        Some(notice) => {
            let highlights = cached_release
                .as_ref()
                .filter(|(version, _)| *version == notice.latest_version)
                .map(|(_, items)| items.clone())
                .unwrap_or_default();
            let url = crate::updater::Updater::release_url(&notice.latest_version);
            (Some(notice.latest_version.to_string()), Some(url), highlights)
        }
        None => match newer_cached_release {
            Some((version, items)) => {
                let url = crate::updater::Updater::release_url(version);
                (Some(version.to_string()), Some(url), items.clone())
            }
            None => (None, None, Vec::new()),
        },
    };
    Ok(protocol::UpdateStatusSnapshot {
        current_version: current.to_string(),
        latest_version,
        update_available,
        release_url,
        release_highlights,
        current_release_highlights: current_highlights,
        current_release_url: current_url,
        current_release_seen,
        channel: Some(updater.config().channel.to_string()),
        check_due: check.should_refresh,
        install: install_state(),
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
