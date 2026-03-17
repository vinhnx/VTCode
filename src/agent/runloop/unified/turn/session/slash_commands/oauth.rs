use anyhow::Result;
use vtcode_auth::{AuthStatus, OpenAIChatGptAuthStatus, OpenAIResolvedAuthSource};
use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::types::UiSurfacePreference;
use vtcode_core::llm::factory::{ProviderConfig, create_provider_with_config};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::{InlineListItem, InlineListSelection, WizardModalMode, WizardStep};

use super::{SlashCommandContext, SlashCommandControl, ui};
use crate::agent::runloop::slash_commands::OAuthProviderAction;
use crate::agent::runloop::ui::build_inline_header_context;
use crate::agent::runloop::unified::wizard_modal::{
    WizardModalOutcome, show_wizard_modal_and_wait,
};
use crate::cli::auth::{
    OPENAI_PROVIDER, OPENROUTER_PROVIDER, clear_openai_login, clear_openrouter_login,
    complete_openai_login_with_manual_future, complete_openrouter_login, openai_auth_status,
    openai_manual_placeholder, openrouter_auth_status, prepare_openai_login,
    prepare_openrouter_login, refresh_openai_login, supports_oauth_provider,
};

const OAUTH_PROVIDER_PREFIX: &str = "oauth-provider:";
const OAUTH_PROVIDER_BACK: &str = "oauth-provider:back";
const OPENAI_MANUAL_PROMPT_ID: &str = "openai_manual_callback";

pub(crate) async fn handle_start_oauth_provider_picker(
    mut ctx: SlashCommandContext<'_>,
    action: OAuthProviderAction,
) -> Result<SlashCommandControl> {
    let activity = match action {
        OAuthProviderAction::Login => "opening OAuth login",
        OAuthProviderAction::Logout => "opening OAuth logout",
        OAuthProviderAction::Refresh => "opening OAuth refresh",
    };
    if !ui::ensure_selection_ui_available(&mut ctx, activity)? {
        return Ok(SlashCommandControl::Continue);
    }

    show_oauth_provider_modal(&mut ctx, action)?;
    let Some(selection) = ui::wait_for_list_modal_selection(&mut ctx).await else {
        return Ok(SlashCommandControl::Continue);
    };

    let InlineListSelection::ConfigAction(action_key) = selection else {
        return Ok(SlashCommandControl::Continue);
    };
    if action_key == OAUTH_PROVIDER_BACK {
        return Ok(SlashCommandControl::Continue);
    }
    let Some(provider) = action_key.strip_prefix(OAUTH_PROVIDER_PREFIX) else {
        return Ok(SlashCommandControl::Continue);
    };

    match action {
        OAuthProviderAction::Login => handle_oauth_login(ctx, provider.to_string()).await,
        OAuthProviderAction::Logout => handle_oauth_logout(ctx, provider.to_string()).await,
        OAuthProviderAction::Refresh => handle_refresh_oauth(ctx, provider.to_string()).await,
    }
}

pub(crate) async fn handle_oauth_login(
    mut ctx: SlashCommandContext<'_>,
    provider: String,
) -> Result<SlashCommandControl> {
    let provider = provider.trim().to_ascii_lowercase();
    if !ensure_supported_provider(ctx.renderer, &provider, "login")? {
        return Ok(SlashCommandControl::Continue);
    }
    let vt_cfg = ctx.vt_cfg.as_ref();

    match provider.as_str() {
        OPENROUTER_PROVIDER => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Starting OpenRouter OAuth authentication...",
            )?;
            let prepared = prepare_openrouter_login(vt_cfg)?;
            open_browser_with_guidance(ctx.renderer, &prepared.auth_url)?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Waiting for OpenRouter OAuth callback...",
            )?;
            let api_key = complete_openrouter_login(prepared).await?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Successfully authenticated with OpenRouter.",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "Stored the OAuth token using your configured credential storage mode.",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                &format!("Key preview: {}...", &api_key[..api_key.len().min(8)]),
            )?;
        }
        OPENAI_PROVIDER => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Starting OpenAI ChatGPT authentication...",
            )?;
            let prepared = prepare_openai_login(vt_cfg)?;
            open_browser_with_guidance(ctx.renderer, &prepared.auth_url)?;
            ctx.renderer.line(
                MessageStyle::Output,
                "If localhost is unavailable, paste the redirected URL or raw query string into the inline prompt.",
            )?;
            ctx.renderer
                .line(MessageStyle::Info, "Waiting for OpenAI OAuth callback...")?;
            let manual_input =
                prompt_openai_manual_callback_input(&mut ctx, prepared.callback_port);
            let login_result =
                complete_openai_login_with_manual_future(prepared, Some(manual_input)).await;
            ctx.handle.close_modal();
            ctx.handle.force_redraw();
            let session = login_result?;
            sync_openai_runtime_if_active(&mut ctx).await?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Successfully authenticated with OpenAI via ChatGPT.",
            )?;
            if ctx.config.provider.eq_ignore_ascii_case(OPENAI_PROVIDER)
                && ctx.config.openai_chatgpt_auth.is_some()
            {
                ctx.renderer.line(
                    MessageStyle::Output,
                    "Switched the current session to OpenAI (ChatGPT).",
                )?;
            }
            if let Some(email) = session.email.as_deref() {
                ctx.renderer
                    .line(MessageStyle::Output, &format!("Account: {}", email))?;
            }
            if let Some(plan) = session.plan.as_deref() {
                ctx.renderer
                    .line(MessageStyle::Output, &format!("Plan: {}", plan))?;
            }
        }
        _ => {}
    }

    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_oauth_logout(
    mut ctx: SlashCommandContext<'_>,
    provider: String,
) -> Result<SlashCommandControl> {
    let provider = provider.trim().to_ascii_lowercase();
    if !ensure_supported_provider(ctx.renderer, &provider, "logout")? {
        return Ok(SlashCommandControl::Continue);
    }
    let vt_cfg = ctx.vt_cfg.as_ref();

    match provider.as_str() {
        OPENROUTER_PROVIDER => {
            clear_openrouter_login(vt_cfg)?;
            ctx.renderer.line(
                MessageStyle::Info,
                "OpenRouter OAuth token cleared successfully.",
            )?;
        }
        OPENAI_PROVIDER => {
            clear_openai_login(vt_cfg)?;
            sync_openai_runtime_if_active(&mut ctx).await?;
            ctx.renderer.line(
                MessageStyle::Info,
                "OpenAI ChatGPT session cleared successfully.",
            )?;
            if ctx.config.provider.eq_ignore_ascii_case(OPENAI_PROVIDER) {
                if ctx.config.api_key.trim().is_empty() {
                    ctx.renderer.line(
                        MessageStyle::Output,
                        "The current OpenAI session no longer has active credentials.",
                    )?;
                } else {
                    ctx.renderer.line(
                        MessageStyle::Output,
                        "The current OpenAI session fell back to OPENAI_API_KEY.",
                    )?;
                }
            }
        }
        _ => {}
    }

    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_refresh_oauth(
    mut ctx: SlashCommandContext<'_>,
    provider: String,
) -> Result<SlashCommandControl> {
    let provider = provider.trim().to_ascii_lowercase();
    if !ensure_supported_provider(ctx.renderer, &provider, "refresh")? {
        return Ok(SlashCommandControl::Continue);
    }

    match provider.as_str() {
        OPENAI_PROVIDER => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Refreshing the stored OpenAI ChatGPT session...",
            )?;
            let session = refresh_openai_login(ctx.vt_cfg.as_ref()).await?;
            sync_openai_runtime_if_active(&mut ctx).await?;
            ctx.renderer.line(
                MessageStyle::Info,
                "OpenAI ChatGPT session refreshed successfully.",
            )?;
            if let Some(email) = session.email.as_deref() {
                ctx.renderer
                    .line(MessageStyle::Output, &format!("Account: {}", email))?;
            }
            if let Some(plan) = session.plan.as_deref() {
                ctx.renderer
                    .line(MessageStyle::Output, &format!("Plan: {}", plan))?;
            }
        }
        OPENROUTER_PROVIDER => {
            ctx.renderer.line(
                MessageStyle::Info,
                "OpenRouter OAuth does not expose a refresh-token flow. Use /login openrouter to reconnect if needed.",
            )?;
        }
        _ => {}
    }

    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_show_auth_status(
    ctx: SlashCommandContext<'_>,
    provider: Option<String>,
) -> Result<SlashCommandControl> {
    let provider = provider.map(|value| value.trim().to_ascii_lowercase());
    if let Some(provider_name) = provider.as_deref()
        && !supports_oauth_provider(provider_name)
    {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!(
                "OAuth status not supported for provider: {}. Supported providers: openai, openrouter",
                provider_name
            ),
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    ctx.renderer
        .line(MessageStyle::Info, "Authentication Status")?;
    ctx.renderer.line(MessageStyle::Output, "")?;
    let vt_cfg = ctx.vt_cfg.as_ref();

    if provider.is_none() || provider.as_deref() == Some(OPENROUTER_PROVIDER) {
        render_openrouter_auth_status(ctx.renderer, openrouter_auth_status(vt_cfg)?)?;
    }

    if provider.is_none() {
        ctx.renderer.line(MessageStyle::Output, "")?;
    }

    if provider.is_none() || provider.as_deref() == Some(OPENAI_PROVIDER) {
        render_openai_auth_status(ctx.renderer, openai_auth_status(vt_cfg)?)?;
        render_openai_credential_overview(
            ctx.renderer,
            vt_cfg,
            ctx.config.provider.eq_ignore_ascii_case(OPENAI_PROVIDER),
        )?;
    }

    if provider.is_none() {
        ctx.renderer.line(MessageStyle::Output, "")?;
        ctx.renderer.line(
            MessageStyle::Output,
            "Use /login, /logout, or /refresh-oauth to manage OAuth-backed credentials.",
        )?;
    }

    Ok(SlashCommandControl::Continue)
}

fn ensure_supported_provider(
    renderer: &mut AnsiRenderer,
    provider: &str,
    action: &str,
) -> Result<bool> {
    if supports_oauth_provider(provider) {
        return Ok(true);
    }

    renderer.line(
        MessageStyle::Error,
        &format!(
            "OAuth {action} not supported for provider: {provider}. Supported providers: openai, openrouter"
        ),
    )?;
    Ok(false)
}

fn open_browser_with_guidance(renderer: &mut AnsiRenderer, auth_url: &str) -> Result<()> {
    renderer.line(MessageStyle::Info, "Opening browser for authentication...")?;
    renderer.line(MessageStyle::Info, "URL:")?;
    renderer.line(MessageStyle::Info, auth_url)?;
    if let Err(err) = webbrowser::open(auth_url) {
        renderer.line(
            MessageStyle::Error,
            &format!("Failed to open browser automatically: {}", err),
        )?;
        renderer.line(
            MessageStyle::Info,
            "Please open the URL manually in your browser.",
        )?;
    }
    Ok(())
}

async fn prompt_openai_manual_callback_input(
    ctx: &mut SlashCommandContext<'_>,
    callback_port: u16,
) -> Result<Option<String>> {
    let step = WizardStep {
        title: "Callback".to_string(),
        question: "Paste the redirected URL or raw query string while VT Code keeps waiting for the browser callback.".to_string(),
        items: vec![InlineListItem {
            title: "Submit".to_string(),
            subtitle: Some("Press Tab to type text, then Enter to submit.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: OPENAI_MANUAL_PROMPT_ID.to_string(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("submit callback redirect url query string".to_string()),
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some("Redirect URL or query".to_string()),
        freeform_placeholder: Some(openai_manual_placeholder(callback_port)),
    };

    let outcome = show_wizard_modal_and_wait(
        ctx.handle,
        ctx.session,
        "OpenAI manual callback".to_string(),
        vec![step],
        0,
        None,
        WizardModalMode::MultiStep,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
    )
    .await?;

    let value = match outcome {
        WizardModalOutcome::Submitted(selections) => {
            selections
                .into_iter()
                .find_map(|selection| match selection {
                    InlineListSelection::RequestUserInputAnswer {
                        question_id,
                        selected,
                        other,
                    } if question_id == OPENAI_MANUAL_PROMPT_ID => {
                        other.or_else(|| selected.first().cloned())
                    }
                    _ => None,
                })
        }
        WizardModalOutcome::Cancelled { .. } => None,
    };

    Ok(value.and_then(|value| {
        let trimmed = value.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    }))
}

fn show_oauth_provider_modal(
    ctx: &mut SlashCommandContext<'_>,
    action: OAuthProviderAction,
) -> Result<()> {
    let vt_cfg = ctx.vt_cfg.as_ref();
    let openrouter_status = openrouter_auth_status(vt_cfg)?;
    let openai_status = openai_auth_status(vt_cfg)?;
    let openai_overview = summarize_current_openai_credentials(vt_cfg)?;

    let mut items = vec![
        InlineListItem {
            title: "OpenAI ChatGPT".to_string(),
            subtitle: Some(openai_modal_subtitle(action, &openai_status)),
            badge: Some(openai_modal_badge(action, &openai_status, &openai_overview)),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}",
                OAUTH_PROVIDER_PREFIX, OPENAI_PROVIDER
            ))),
            search_value: Some("openai chatgpt oauth subscription".to_string()),
        },
        InlineListItem {
            title: "OpenRouter".to_string(),
            subtitle: Some(openrouter_modal_subtitle(action, &openrouter_status)),
            badge: Some(openrouter_modal_badge(action, &openrouter_status)),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}",
                OAUTH_PROVIDER_PREFIX, OPENROUTER_PROVIDER
            ))),
            search_value: Some("openrouter oauth".to_string()),
        },
        InlineListItem {
            title: "Back".to_string(),
            subtitle: Some("Close this dialog".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                OAUTH_PROVIDER_BACK.to_string(),
            )),
            search_value: Some("back close cancel".to_string()),
        },
    ];

    if matches!(action, OAuthProviderAction::Refresh) {
        items[0].badge = Some("Refresh".to_string());
        items[1].badge = Some("Info".to_string());
    }

    ctx.renderer.show_list_modal(
        oauth_modal_title(action),
        oauth_modal_lines(action),
        items,
        Some(InlineListSelection::ConfigAction(format!(
            "{}{}",
            OAUTH_PROVIDER_PREFIX, OPENAI_PROVIDER
        ))),
        None,
    );
    Ok(())
}

fn oauth_modal_title(action: OAuthProviderAction) -> &'static str {
    match action {
        OAuthProviderAction::Login => "OAuth login",
        OAuthProviderAction::Logout => "OAuth logout",
        OAuthProviderAction::Refresh => "Refresh OAuth",
    }
}

fn oauth_modal_lines(action: OAuthProviderAction) -> Vec<String> {
    match action {
        OAuthProviderAction::Login => vec![
            "Choose an OAuth-capable provider to connect.".to_string(),
            "VT Code stores credentials securely using your configured credential storage mode."
                .to_string(),
        ],
        OAuthProviderAction::Logout => vec![
            "Choose an OAuth-capable provider to disconnect.".to_string(),
            "This removes the stored OAuth session for the selected provider.".to_string(),
        ],
        OAuthProviderAction::Refresh => vec![
            "Choose an OAuth-capable provider to refresh.".to_string(),
            "OpenAI refreshes the stored ChatGPT session; OpenRouter requires a new login."
                .to_string(),
        ],
    }
}

fn openai_modal_subtitle(action: OAuthProviderAction, status: &OpenAIChatGptAuthStatus) -> String {
    match action {
        OAuthProviderAction::Login => match status {
            OpenAIChatGptAuthStatus::Authenticated { label, .. } => format!(
                "Connected{}; re-authenticate to replace the stored ChatGPT session.",
                label
                    .as_deref()
                    .map(|value| format!(" as {}", value))
                    .unwrap_or_default()
            ),
            OpenAIChatGptAuthStatus::NotAuthenticated => {
                "Sign in with your ChatGPT subscription.".to_string()
            }
        },
        OAuthProviderAction::Logout => match status {
            OpenAIChatGptAuthStatus::Authenticated { label, .. } => format!(
                "Remove the stored ChatGPT session{}.",
                label
                    .as_deref()
                    .map(|value| format!(" for {}", value))
                    .unwrap_or_default()
            ),
            OpenAIChatGptAuthStatus::NotAuthenticated => {
                "No stored ChatGPT session to remove.".to_string()
            }
        },
        OAuthProviderAction::Refresh => match status {
            OpenAIChatGptAuthStatus::Authenticated { .. } => {
                "Refresh the stored ChatGPT session using its refresh token.".to_string()
            }
            OpenAIChatGptAuthStatus::NotAuthenticated => {
                "No stored ChatGPT session to refresh yet.".to_string()
            }
        },
    }
}

fn openai_modal_badge(
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

fn openrouter_modal_subtitle(action: OAuthProviderAction, status: &AuthStatus) -> String {
    match action {
        OAuthProviderAction::Login => match status {
            AuthStatus::Authenticated { label, .. } => format!(
                "Connected{}; re-authenticate to replace the stored OpenRouter token.",
                label
                    .as_deref()
                    .map(|value| format!(" as {}", value))
                    .unwrap_or_default()
            ),
            AuthStatus::NotAuthenticated => "Sign in with OpenRouter OAuth.".to_string(),
        },
        OAuthProviderAction::Logout => match status {
            AuthStatus::Authenticated { .. } => {
                "Remove the stored OpenRouter OAuth token.".to_string()
            }
            AuthStatus::NotAuthenticated => {
                "No stored OpenRouter OAuth token to remove.".to_string()
            }
        },
        OAuthProviderAction::Refresh => {
            "OpenRouter does not expose a refresh-token flow; reconnect with /login openrouter."
                .to_string()
        }
    }
}

fn openrouter_modal_badge(action: OAuthProviderAction, status: &AuthStatus) -> String {
    if matches!(action, OAuthProviderAction::Refresh) {
        return "Info".to_string();
    }
    match status {
        AuthStatus::Authenticated { .. } => "Connected".to_string(),
        AuthStatus::NotAuthenticated => "OAuth".to_string(),
    }
}

fn render_openrouter_auth_status(renderer: &mut AnsiRenderer, status: AuthStatus) -> Result<()> {
    match status {
        AuthStatus::Authenticated {
            label,
            age_seconds,
            expires_in,
        } => {
            renderer.line(MessageStyle::Info, "OpenRouter: authenticated (OAuth)")?;
            if let Some(label) = label {
                renderer.line(MessageStyle::Output, &format!("  Label: {}", label))?;
            }
            renderer.line(
                MessageStyle::Output,
                &format!("  Token obtained: {}", format_auth_duration(age_seconds)),
            )?;
            if let Some(expires_in) = expires_in {
                renderer.line(
                    MessageStyle::Output,
                    &format!("  Expires in: {}", format_auth_duration(expires_in)),
                )?;
            }
        }
        AuthStatus::NotAuthenticated => {
            if get_api_key(OPENROUTER_PROVIDER, &ApiKeySources::default()).is_ok() {
                renderer.line(MessageStyle::Info, "OpenRouter: using OPENROUTER_API_KEY")?;
            } else {
                renderer.line(MessageStyle::Info, "OpenRouter: not authenticated")?;
            }
        }
    }
    Ok(())
}

fn render_openai_auth_status(
    renderer: &mut AnsiRenderer,
    status: OpenAIChatGptAuthStatus,
) -> Result<()> {
    match status {
        OpenAIChatGptAuthStatus::Authenticated {
            label,
            age_seconds,
            expires_in,
        } => {
            renderer.line(MessageStyle::Info, "OpenAI: authenticated (ChatGPT)")?;
            if let Some(label) = label {
                renderer.line(MessageStyle::Output, &format!("  Label: {}", label))?;
            }
            renderer.line(
                MessageStyle::Output,
                &format!("  Session obtained: {}", format_auth_duration(age_seconds)),
            )?;
            if let Some(expires_in) = expires_in {
                renderer.line(
                    MessageStyle::Output,
                    &format!("  Expires in: {}", format_auth_duration(expires_in)),
                )?;
            }
        }
        OpenAIChatGptAuthStatus::NotAuthenticated => {
            if get_api_key(OPENAI_PROVIDER, &ApiKeySources::default()).is_ok() {
                renderer.line(MessageStyle::Info, "OpenAI: using OPENAI_API_KEY")?;
            } else {
                renderer.line(MessageStyle::Info, "OpenAI: not authenticated")?;
            }
        }
    }
    Ok(())
}

fn render_openai_credential_overview(
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
            if overview.chatgpt_session.is_some() {
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
        &format!(
            "  Usage status: {} (preferred_method = {})",
            usage_status,
            overview.preferred_method.as_str()
        ),
    )?;

    if current_provider_is_openai {
        renderer.line(
            MessageStyle::Output,
            &format!("  Current session: {}", usage_status),
        )?;
    }

    if let Some(notice) = overview.notice.as_deref() {
        renderer.line(MessageStyle::Info, &format!("  Notice: {}", notice))?;
    }
    if let Some(recommendation) = overview.recommendation.as_deref() {
        renderer.line(
            MessageStyle::Output,
            &format!("  Recommendation: {}", recommendation),
        )?;
    }
    Ok(())
}

fn summarize_current_openai_credentials(
    vt_cfg: Option<&vtcode_config::VTCodeConfig>,
) -> Result<vtcode_config::auth::OpenAICredentialOverview> {
    let default_auth = vtcode_auth::OpenAIAuthConfig::default();
    let auth_cfg = vt_cfg.map(|cfg| &cfg.auth.openai).unwrap_or(&default_auth);
    let storage_mode = vt_cfg
        .map(|cfg| cfg.agent.credential_storage_mode)
        .unwrap_or_default();
    let api_key = get_api_key(OPENAI_PROVIDER, &ApiKeySources::default()).ok();
    vtcode_config::auth::summarize_openai_credentials(auth_cfg, storage_mode, api_key)
}

async fn sync_openai_runtime_if_active(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    if !ctx.config.provider.eq_ignore_ascii_case(OPENAI_PROVIDER) {
        return Ok(());
    }

    let api_key = get_api_key(OPENAI_PROVIDER, &ApiKeySources::default()).ok();
    let (runtime_api_key, runtime_auth) = match ctx.vt_cfg.as_ref() {
        Some(cfg) => match vtcode_config::auth::resolve_openai_auth(
            &cfg.auth.openai,
            cfg.agent.credential_storage_mode,
            api_key,
        ) {
            Ok(resolved) => (resolved.api_key().to_string(), resolved.handle()),
            Err(_) => (String::new(), None),
        },
        None => (api_key.unwrap_or_default(), None),
    };

    let provider = create_provider_with_config(
        OPENAI_PROVIDER,
        ProviderConfig {
            api_key: Some(runtime_api_key.clone()),
            openai_chatgpt_auth: runtime_auth.clone(),
            base_url: None,
            model: Some(ctx.config.model.clone()),
            prompt_cache: Some(ctx.config.prompt_cache.clone()),
            timeouts: None,
            openai: ctx.vt_cfg.as_ref().map(|cfg| cfg.provider.openai.clone()),
            anthropic: None,
            model_behavior: ctx.config.model_behavior.clone(),
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
    let mode_label = match (ctx.config.ui_surface, ctx.full_auto) {
        (UiSurfacePreference::Inline, true) => "auto".to_string(),
        (UiSurfacePreference::Inline, false) => "inline".to_string(),
        (UiSurfacePreference::Alternate, _) => "alt".to_string(),
        (UiSurfacePreference::Auto, true) => "auto".to_string(),
        (UiSurfacePreference::Auto, false) => "std".to_string(),
    };
    let next_header_context = build_inline_header_context(
        ctx.config,
        ctx.session_bootstrap,
        provider_label,
        ctx.config.model.clone(),
        ctx.provider_client
            .effective_context_size(&ctx.config.model),
        mode_label,
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
