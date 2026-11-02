use super::*;

pub(super) fn copilot_modal_subtitle(action: OAuthProviderAction, status: &CopilotAuthStatus) -> String {
    match action {
        OAuthProviderAction::Login => match status.kind {
            CopilotAuthStatusKind::Authenticated => {
                "Managed auth is connected; rerun the Copilot CLI login to replace the active session.".to_string()
            }
            CopilotAuthStatusKind::Unauthenticated => {
                "Sign in with your GitHub Copilot subscription through the official Copilot CLI.".to_string()
            }
            CopilotAuthStatusKind::ServerUnavailable => {
                format!(
                    "Copilot CLI unavailable; install `copilot` or configure `[auth.copilot].command`. See {COPILOT_AUTH_DOC_PATH}."
                )
            }
            CopilotAuthStatusKind::AuthFlowFailed => {
                "Authentication needs attention; rerun the Copilot CLI login flow.".to_string()
            }
        },
        OAuthProviderAction::Logout => match status.kind {
            CopilotAuthStatusKind::Authenticated => "Remove the active GitHub Copilot CLI session.".to_string(),
            _ => "No stored GitHub Copilot session to remove.".to_string(),
        },
        OAuthProviderAction::Refresh => "GitHub Copilot requires a new login instead of token refresh.".to_string(),
    }
}

pub(super) fn copilot_modal_badge(action: OAuthProviderAction, status: &CopilotAuthStatus) -> String {
    if matches!(action, OAuthProviderAction::Refresh) {
        return "Info".to_string();
    }

    match status.kind {
        CopilotAuthStatusKind::Authenticated => "Connected".to_string(),
        CopilotAuthStatusKind::ServerUnavailable => "Unavailable".to_string(),
        CopilotAuthStatusKind::AuthFlowFailed => "Attention".to_string(),
        CopilotAuthStatusKind::Unauthenticated => "Auth".to_string(),
    }
}

pub(super) fn openai_modal_subtitle(
    action: OAuthProviderAction,
    status: &OpenAIChatGptAuthStatus,
    overview: &vtcode_config::auth::OpenAICredentialOverview,
) -> String {
    match action {
        OAuthProviderAction::Login => match status {
            OpenAIChatGptAuthStatus::Authenticated { label, .. } => format!(
                "Connected{}; re-authenticate to replace the stored ChatGPT session.",
                label.as_deref().map(|value| format!(" as {value}")).unwrap_or_default()
            ),
            OpenAIChatGptAuthStatus::NotAuthenticated => {
                if overview.codex_fallback_available {
                    "Sign in with your ChatGPT subscription. Codex auth.json detected — VT Code can use it as a fallback."
                        .to_string()
                } else {
                    "Sign in with your ChatGPT subscription.".to_string()
                }
            }
        },
        OAuthProviderAction::Logout => match status {
            OpenAIChatGptAuthStatus::Authenticated { label, .. } => format!(
                "Remove the stored ChatGPT session{}.",
                label.as_deref().map(|value| format!(" for {value}")).unwrap_or_default()
            ),
            OpenAIChatGptAuthStatus::NotAuthenticated => "No stored ChatGPT session to remove.".to_string(),
        },
        OAuthProviderAction::Refresh => match status {
            OpenAIChatGptAuthStatus::Authenticated { .. } => {
                "Refresh the stored ChatGPT session using its refresh token.".to_string()
            }
            OpenAIChatGptAuthStatus::NotAuthenticated => "No stored ChatGPT session to refresh yet.".to_string(),
        },
    }
}

pub(super) fn openai_modal_badge(
    action: OAuthProviderAction,
    status: &OpenAIChatGptAuthStatus,
    overview: &vtcode_config::auth::OpenAICredentialOverview,
) -> String {
    if matches!(action, OAuthProviderAction::Refresh) {
        return if matches!(status, OpenAIChatGptAuthStatus::Authenticated { .. }) {
            "Refresh".to_string()
        } else {
            "Missing".to_string()
        };
    }

    if overview.active_source == Some(OpenAIResolvedAuthSource::ChatGpt) {
        return "Active".to_string();
    }

    match status {
        OpenAIChatGptAuthStatus::Authenticated { .. } => "Connected".to_string(),
        OpenAIChatGptAuthStatus::NotAuthenticated => "OAuth".to_string(),
    }
}

pub(super) fn openrouter_modal_subtitle(action: OAuthProviderAction, status: &AuthStatus) -> String {
    match action {
        OAuthProviderAction::Login => match status {
            AuthStatus::Authenticated { label, .. } => format!(
                "Connected{}; re-authenticate to replace the stored OpenRouter token.",
                label.as_deref().map(|value| format!(" as {value}")).unwrap_or_default()
            ),
            AuthStatus::NotAuthenticated => "Sign in with OpenRouter OAuth.".to_string(),
        },
        OAuthProviderAction::Logout => match status {
            AuthStatus::Authenticated { .. } => "Remove the stored OpenRouter OAuth token.".to_string(),
            AuthStatus::NotAuthenticated => "No stored OpenRouter OAuth token to remove.".to_string(),
        },
        OAuthProviderAction::Refresh => {
            "OpenRouter does not expose a refresh-token flow; reconnect with /login openrouter.".to_string()
        }
    }
}

pub(super) fn openrouter_modal_badge(action: OAuthProviderAction, status: &AuthStatus) -> String {
    if matches!(action, OAuthProviderAction::Refresh) {
        return "Info".to_string();
    }
    match status {
        AuthStatus::Authenticated { .. } => "Connected".to_string(),
        AuthStatus::NotAuthenticated => "OAuth".to_string(),
    }
}

pub(super) fn render_openrouter_auth_status(
    renderer: &mut AnsiRenderer,
    status: AuthStatus,
    storage_mode: AuthCredentialsStoreMode,
) -> Result<()> {
    match status {
        AuthStatus::Authenticated { label, age_seconds, expires_in } => {
            renderer.line(MessageStyle::Info, "OpenRouter: authenticated (OAuth)")?;
            if let Some(label) = label {
                renderer.line(MessageStyle::Output, &format!("  Label: {label}"))?;
            }
            renderer.line(MessageStyle::Output, &format!("  Token obtained: {}", format_auth_duration(age_seconds)))?;
            if let Some(expires_in) = expires_in {
                renderer.line(MessageStyle::Output, &format!("  Expires in: {}", format_auth_duration(expires_in)))?;
            }
        }
        AuthStatus::NotAuthenticated => {
            if vtcode_core::config::api_keys::get_api_key_with_mode(
                OPENROUTER_PROVIDER,
                &ApiKeySources::default(),
                storage_mode,
            )
            .is_ok()
            {
                renderer.line(MessageStyle::Info, "OpenRouter: using configured API key")?;
            } else {
                renderer.line(MessageStyle::Info, "OpenRouter: not authenticated")?;
            }
        }
    }
    Ok(())
}

pub(super) fn render_openai_auth_status(
    renderer: &mut AnsiRenderer,
    status: OpenAIChatGptAuthStatus,
    _storage_mode: AuthCredentialsStoreMode,
    overview: &vtcode_config::auth::OpenAICredentialOverview,
) -> Result<()> {
    use vtcode_config::auth::OpenAIResolvedAuthSource;
    // Drive the headline from overview.active_source — the same field runtime
    // resolution uses — so TUI status never disagrees with actual auth.
    match overview.active_source {
        Some(OpenAIResolvedAuthSource::ChatGpt) => match status {
            OpenAIChatGptAuthStatus::Authenticated { label, age_seconds, expires_in } => {
                renderer.line(MessageStyle::Info, "OpenAI: authenticated (ChatGPT)")?;
                if let Some(label) = label {
                    renderer.line(MessageStyle::Output, &format!("  Label: {label}"))?;
                }
                renderer.line(
                    MessageStyle::Output,
                    &format!("  Session obtained: {}", format_auth_duration(age_seconds)),
                )?;
                if let Some(expires_in) = expires_in {
                    renderer
                        .line(MessageStyle::Output, &format!("  Expires in: {}", format_auth_duration(expires_in)))?;
                }
                if overview.chatgpt_session_provenance == Some(OpenAIChatGptSessionProvenance::CodexFallback) {
                    renderer.line(
                        MessageStyle::Output,
                        "  Source: Codex auth.json fallback (run /login openai for a VT Code session with auto-refresh)",
                    )?;
                }
            }
            OpenAIChatGptAuthStatus::NotAuthenticated => {
                // active_source says ChatGpt but native status is NotAuthenticated
                // → this is a Codex fallback session.
                renderer.line(MessageStyle::Info, "OpenAI: using Codex auth.json fallback")?;
                if let Some(email) = overview.chatgpt_email.as_deref() {
                    renderer.line(MessageStyle::Output, &format!("  Account: {email}"))?;
                }
                if let Some(plan) = overview.chatgpt_plan.as_deref() {
                    renderer.line(MessageStyle::Output, &format!("  Plan: {plan}"))?;
                }
                renderer.line(
                    MessageStyle::Output,
                    "  Run /login openai for a VT Code session with full auto-refresh support.",
                )?;
            }
        },
        Some(OpenAIResolvedAuthSource::ApiKey) => {
            renderer.line(MessageStyle::Info, "OpenAI: using configured API key")?;
        }
        None => {
            renderer.line(MessageStyle::Info, "OpenAI: not authenticated")?;
            renderer
                .line(MessageStyle::Output, "  Run /login openai to authenticate with your ChatGPT subscription.")?;
        }
    }
    // Show dual-credential notice when both sources are available.
    if let Some(notice) = &overview.notice {
        renderer.line(MessageStyle::Output, &format!("  Note: {notice}"))?;
    }
    if let Some(rec) = &overview.recommendation {
        renderer.line(MessageStyle::Output, &format!("  {rec}"))?;
    }
    Ok(())
}

pub(super) fn render_copilot_auth_status(renderer: &mut AnsiRenderer, status: CopilotAuthStatus) -> Result<()> {
    match status.kind {
        CopilotAuthStatusKind::Authenticated => {
            renderer.line(MessageStyle::Info, "GitHub Copilot: authenticated (managed auth via Copilot CLI)")?;
        }
        CopilotAuthStatusKind::Unauthenticated => {
            renderer.line(MessageStyle::Info, "GitHub Copilot: not authenticated")?;
        }
        CopilotAuthStatusKind::ServerUnavailable => {
            renderer.line(MessageStyle::Warning, "GitHub Copilot: CLI unavailable")?;
        }
        CopilotAuthStatusKind::AuthFlowFailed => {
            renderer.line(MessageStyle::Warning, "GitHub Copilot: auth flow failed")?;
        }
    }

    if let Some(message) = status.message.as_deref()
        && !message.trim().is_empty()
    {
        renderer.line(MessageStyle::Output, &format!("  Details: {message}"))?;
    }

    if matches!(status.kind, CopilotAuthStatusKind::ServerUnavailable) {
        renderer.line(
            MessageStyle::Output,
            &format!("  Help: install `copilot` or configure `[auth.copilot].command`; see {COPILOT_AUTH_DOC_PATH}."),
        )?;
    }

    Ok(())
}

pub(super) fn render_openai_credential_overview(
    renderer: &mut AnsiRenderer,
    vt_cfg: Option<&vtcode_config::VTCodeConfig>,
    current_provider_is_openai: bool,
) -> Result<()> {
    let overview = summarize_current_openai_credentials(vt_cfg)?;
    renderer.line(
        MessageStyle::Output,
        &format!(
            "  API key: {}",
            if overview.api_key_available {
                "available"
            } else {
                "not found"
            }
        ),
    )?;
    renderer.line(
        MessageStyle::Output,
        &format!(
            "  ChatGPT session: {}",
            if overview.chatgpt_session_present {
                "connected"
            } else {
                "not connected"
            }
        ),
    )?;

    let usage_status = match overview.active_source {
        Some(OpenAIResolvedAuthSource::ChatGpt) => "using ChatGPT subscription",
        Some(OpenAIResolvedAuthSource::ApiKey) => "using OPENAI_API_KEY",
        None => "no active OpenAI credential",
    };
    renderer.line(
        MessageStyle::Output,
        &format!("  Usage status: {} (preferred_method = {})", usage_status, overview.preferred_method.as_str()),
    )?;

    if current_provider_is_openai {
        renderer.line(MessageStyle::Output, &format!("  Current session: {usage_status}"))?;
    }

    if let Some(notice) = overview.notice.as_deref() {
        renderer.line(MessageStyle::Info, &format!("  Notice: {notice}"))?;
    }
    if let Some(recommendation) = overview.recommendation.as_deref() {
        renderer.line(MessageStyle::Output, &format!("  Recommendation: {recommendation}"))?;
    }
    Ok(())
}

pub(super) async fn sync_openai_runtime_if_active(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    if !ctx.config.provider.eq_ignore_ascii_case(OPENAI_PROVIDER) {
        return Ok(());
    }

    let storage_mode = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.credential_storage_mode)
        .unwrap_or_default();
    let api_key =
        vtcode_core::config::api_keys::get_api_key_with_mode(OPENAI_PROVIDER, &ApiKeySources::default(), storage_mode)
            .ok();
    let (runtime_api_key, runtime_auth) = match ctx.vt_cfg.as_ref() {
        Some(cfg) => {
            match vtcode_config::auth::resolve_openai_auth(&cfg.auth.openai, cfg.agent.credential_storage_mode, api_key)
            {
                Ok(resolved) => (resolved.api_key().to_string(), resolved.handle()),
                Err(_) => (String::new(), None),
            }
        }
        None => (api_key.unwrap_or_default(), None),
    };

    let provider = create_provider_with_config(
        OPENAI_PROVIDER,
        ProviderConfig {
            api_key: Some(runtime_api_key.clone()),
            openai_chatgpt_auth: runtime_auth.clone(),
            copilot_auth: ctx.vt_cfg.as_ref().map(|cfg| cfg.auth.copilot.clone()),
            base_url: None,
            model: Some(ctx.config.model.clone()),
            prompt_cache: Some(ctx.config.prompt_cache.clone()),
            timeouts: None,
            openai: ctx.vt_cfg.as_ref().map(|cfg| cfg.provider.openai.clone()),
            anthropic: configured_anthropic_config(ctx.vt_cfg.as_ref()),
            openrouter: None,
            model_behavior: ctx.config.model_behavior.clone(),
            workspace_root: Some(ctx.config.workspace.clone()),
        },
    )?;
    *ctx.provider_client = provider;
    ctx.config.api_key = runtime_api_key;
    ctx.config.openai_chatgpt_auth = runtime_auth;

    let provider_label = if ctx.config.openai_chatgpt_auth.is_some() {
        "OpenAI (ChatGPT)".to_string()
    } else {
        "openai".to_string()
    };
    let next_header_context = build_inline_header_context(
        ctx.config,
        ctx.vt_cfg.as_ref(),
        ctx.session_bootstrap,
        provider_label,
        ctx.config.model.clone(),
        ctx.provider_client.effective_context_size(&ctx.config.model),
        ctx.config.reasoning_effort.as_str().to_string(),
    )
    .await?;
    ctx.header_context.clone_from(&next_header_context);
    ctx.handle.set_header_context(next_header_context);

    Ok(())
}

pub(super) async fn sync_copilot_runtime_if_active(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    if !ctx.config.provider.eq_ignore_ascii_case(COPILOT_PROVIDER) {
        return Ok(());
    }

    let provider = create_provider_with_config(
        COPILOT_PROVIDER,
        ProviderConfig {
            api_key: Some(String::new()),
            openai_chatgpt_auth: None,
            copilot_auth: ctx.vt_cfg.as_ref().map(|cfg| cfg.auth.copilot.clone()),
            base_url: None,
            model: Some(ctx.config.model.clone()),
            prompt_cache: Some(ctx.config.prompt_cache.clone()),
            timeouts: None,
            openai: ctx.vt_cfg.as_ref().map(|cfg| cfg.provider.openai.clone()),
            anthropic: configured_anthropic_config(ctx.vt_cfg.as_ref()),
            openrouter: None,
            model_behavior: ctx.config.model_behavior.clone(),
            workspace_root: Some(ctx.config.workspace.clone()),
        },
    )?;
    *ctx.provider_client = provider;
    ctx.config.api_key.clear();
    ctx.config.openai_chatgpt_auth = None;

    let next_header_context = build_inline_header_context(
        ctx.config,
        ctx.vt_cfg.as_ref(),
        ctx.session_bootstrap,
        "GitHub Copilot".to_string(),
        ctx.config.model.clone(),
        ctx.provider_client.effective_context_size(&ctx.config.model),
        ctx.config.reasoning_effort.as_str().to_string(),
    )
    .await?;
    ctx.header_context.clone_from(&next_header_context);
    ctx.handle.set_header_context(next_header_context);

    Ok(())
}

fn format_auth_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else if seconds < 86_400 {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    } else {
        format!("{}d {}h", seconds / 86_400, (seconds % 86_400) / 3600)
    }
}

#[derive(Clone, Copy)]
pub(super) enum CopilotAuthAction {
    Login,
    Logout,
}

pub(super) fn render_copilot_auth_intro(renderer: &mut AnsiRenderer, action: CopilotAuthAction) -> Result<()> {
    renderer.line(MessageStyle::Info, "Managed auth via GitHub Copilot CLI.")?;
    renderer.line(
        MessageStyle::Info,
        "`gh` is optional fallback only; login/logout require the official `copilot` CLI.",
    )?;
    renderer.line(
        MessageStyle::Info,
        match action {
            CopilotAuthAction::Login => "Waiting for the official Copilot CLI to start the managed login flow.",
            CopilotAuthAction::Logout => "Clearing the managed GitHub Copilot CLI session.",
        },
    )?;
    Ok(())
}

pub(super) fn render_copilot_auth_event(renderer: &mut AnsiRenderer, event: CopilotAuthEvent) -> Result<()> {
    match event {
        CopilotAuthEvent::VerificationCode { url, user_code } => {
            renderer.line(MessageStyle::Info, "Your GitHub device code — copy it before the browser opens:")?;
            let device_code_style = AnsiStyle::new()
                .fg_color(Some(Color::Ansi(AnsiColor::BrightYellow)))
                .effects(Effects::BOLD);
            renderer.line_with_style(device_code_style, &user_code)?;
            notify_attention(true, Some(&format!("GitHub device code: {user_code}")));
            renderer.line(MessageStyle::Info, "Opening browser to:")?;
            renderer.hyperlink_line(MessageStyle::Info, &url)?;
            renderer.line(MessageStyle::Info, "Enter the code above when prompted.")?;
        }
        CopilotAuthEvent::Progress { message } => renderer.line(MessageStyle::Info, &message)?,
        CopilotAuthEvent::Success { account } => {
            if let Some(account) = account.as_deref() {
                renderer.line(MessageStyle::Output, &format!("Account: {account}"))?;
            }
        }
        CopilotAuthEvent::Failure { message } => {
            renderer.line(MessageStyle::Info, &format!("Failure: {message}"))?;
        }
    }

    Ok(())
}
