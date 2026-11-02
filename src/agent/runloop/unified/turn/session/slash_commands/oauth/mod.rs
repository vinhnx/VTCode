use crate::agent::runloop::ui_list;
use crate::agent::runloop::ui_list::Tone;
use crate::agent::runloop::unified::session_setup::configured_anthropic_config;
use anstyle::{AnsiColor, Color, Effects, Style as AnsiStyle};
use anyhow::Result;
use vtcode_auth::{
    AuthCredentialsStoreMode, AuthStatus, OpenAIChatGptAuthStatus, OpenAIChatGptSessionProvenance,
    OpenAIResolvedAuthSource,
};
use vtcode_core::config::api_keys::ApiKeySources;
use vtcode_core::copilot::{
    COPILOT_AUTH_DOC_PATH, CopilotAuthEvent, CopilotAuthStatus, CopilotAuthStatusKind, login_with_events,
    logout_with_events, probe_auth_status,
};
use vtcode_core::hooks::SessionEndReason;
use vtcode_core::llm::factory::{ProviderConfig, create_provider_with_config};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::ansi_codes::notify_attention;
use vtcode_ui::tui::app::{InlineListSelection, WizardModalMode, WizardStep};

use super::{SlashCommandContext, SlashCommandControl, ui};
use crate::agent::runloop::slash_commands::OAuthProviderAction;
use crate::agent::runloop::ui::build_inline_header_context;
use crate::agent::runloop::unified::external_url_guard::{
    ExternalUrlGuardContext, ExternalUrlOpenOutcome, request_external_url_open,
};
use crate::agent::runloop::unified::wizard_modal::{WizardModalOutcome, show_wizard_modal_and_wait};
use crate::cli::auth::{
    COPILOT_PROVIDER, OPENAI_PROVIDER, OPENROUTER_PROVIDER, begin_openai_login, begin_openrouter_login,
    clear_openai_login, clear_openrouter_login, complete_openai_login_from_manual_future,
    complete_openai_login_with_manual_future, complete_openrouter_login_with_tui_cancel, is_oauth_flow_cancelled,
    oauth_flow_cancelled_error, openai_auth_status, openai_manual_placeholder, openrouter_auth_status,
    prepare_openai_login, prepare_openrouter_login, refresh_openai_login, should_prompt_manual_openai_input,
    summarize_current_openai_credentials, supports_auth_provider,
};

const OAUTH_PROVIDER_PREFIX: &str = "oauth-provider:";
const OAUTH_PROVIDER_BACK: &str = "oauth-provider:back";
const OPENAI_MANUAL_PROMPT_ID: &str = "openai_manual_callback";

fn credential_storage_mode(ctx: &SlashCommandContext<'_>) -> AuthCredentialsStoreMode {
    ctx.vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.credential_storage_mode)
        .unwrap_or_default()
}

pub(crate) async fn handle_start_oauth_provider_picker(
    mut ctx: SlashCommandContext<'_>,
    action: OAuthProviderAction,
) -> Result<SlashCommandControl> {
    let activity = match action {
        OAuthProviderAction::Login => "opening authentication login",
        OAuthProviderAction::Logout => "opening authentication logout",
        OAuthProviderAction::Refresh => "opening authentication refresh",
    };
    if !ui::ensure_selection_ui_available(&mut ctx, activity)? {
        return Ok(SlashCommandControl::Continue);
    }

    show_oauth_provider_modal(&mut ctx, action).await?;
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
    // The OpenAI/OpenRouter device flows need a browser round-trip plus modal
    // input, neither of which exists without inline UI. Copilot stays
    // available: it shells out to the `copilot` CLI with text-driven events.
    if !ctx.renderer.supports_inline_ui() && provider.as_str() != COPILOT_PROVIDER {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("OAuth login for {provider} needs a browser flow. Run `vtcode login {provider}` in a terminal."),
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    let vt_cfg = ctx.vt_cfg.as_ref();

    match provider.as_str() {
        COPILOT_PROVIDER => {
            ctx.renderer
                .line(MessageStyle::Info, "Starting GitHub Copilot authentication via the official `copilot` CLI...")?;
            render_copilot_auth_intro(ctx.renderer, CopilotAuthAction::Login)?;
            let workspace = ctx.config.workspace.clone();
            let auth_cfg = ctx.vt_cfg.as_ref().map(|cfg| cfg.auth.copilot.clone()).unwrap_or_default();
            login_with_events(&auth_cfg, &workspace, |event| render_copilot_auth_event(ctx.renderer, event)).await?;
            sync_copilot_runtime_if_active(&mut ctx).await?;
            ctx.renderer
                .line(MessageStyle::Info, "Successfully authenticated with GitHub Copilot.")?;
            if ctx.config.provider.eq_ignore_ascii_case(COPILOT_PROVIDER) {
                ctx.renderer
                    .line(MessageStyle::Output, "Switched the current session to GitHub Copilot.")?;
            }
        }
        OPENROUTER_PROVIDER => {
            ctx.renderer
                .line(MessageStyle::Info, "Starting OpenRouter OAuth authentication...")?;
            let prepared = prepare_openrouter_login(vt_cfg)?;
            let auth_url = prepared.auth_url.clone();
            if let Some(control) = open_browser_with_guidance(&mut ctx, &auth_url).await? {
                return Ok(control);
            }
            let started = begin_openrouter_login(prepared).await?;
            ctx.renderer
                .line(MessageStyle::Info, "Waiting for OpenRouter OAuth callback...")?;
            let api_key =
                match complete_openrouter_login_with_tui_cancel(started, ctx.ctrl_c_state, ctx.ctrl_c_notify).await {
                    Ok(api_key) => api_key,
                    Err(err) if is_oauth_flow_cancelled(&err) => {
                        if ctx.ctrl_c_state.is_exit_requested() {
                            return Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit));
                        }
                        ctx.renderer
                            .line(MessageStyle::Info, "OpenRouter OAuth authentication cancelled.")?;
                        return Ok(SlashCommandControl::Continue);
                    }
                    Err(err) => return Err(err),
                };
            ctx.renderer
                .line(MessageStyle::Info, "Successfully authenticated with OpenRouter.")?;
            ctx.renderer
                .line(MessageStyle::Output, "Stored the OAuth token using your configured credential storage mode.")?;
            ctx.renderer
                .line(MessageStyle::Output, &format!("Key preview: {}...", &api_key[..api_key.len().min(8)]))?;
        }
        OPENAI_PROVIDER => {
            // Check if Codex's auth.json is available as a fallback before
            // starting the full OAuth flow. This lets users know they already
            // have credentials they can use without a new browser flow.
            if matches!(openai_auth_status(vt_cfg)?, OpenAIChatGptAuthStatus::NotAuthenticated) {
                let overview = summarize_current_openai_credentials(vt_cfg)?;
                if overview.codex_fallback_available {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        "Tip: Codex's auth.json was detected. VT Code can use it automatically at runtime.\n\
                         Continue below for a VT Code session with full auto-refresh, or cancel and\n\
                         run `vtcode login openai --from-codex` to validate the Codex credentials.",
                    )?;
                }
            }
            ctx.renderer
                .line(MessageStyle::Info, "Starting OpenAI ChatGPT authentication...")?;
            let prepared = prepare_openai_login(vt_cfg)?;
            let auth_url = prepared.auth_url.clone();
            let callback_port = prepared.callback_port;
            let started = begin_openai_login(prepared.clone()).await;
            if let Some(control) = open_browser_with_guidance(&mut ctx, &auth_url).await? {
                return Ok(control);
            }
            ctx.renderer.line(MessageStyle::Info, "Waiting for OpenAI OAuth callback...")?;
            let login_result = match started {
                Ok(started) => {
                    let manual_input = prompt_openai_manual_callback_input(&mut ctx, callback_port, &auth_url);
                    complete_openai_login_with_manual_future(started, Some(manual_input)).await
                }
                Err(err) if should_prompt_manual_openai_input(&err) => {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        "Local callback listener is unavailable. Waiting for manual redirect input...",
                    )?;
                    let manual_input = prompt_openai_manual_callback_input(&mut ctx, callback_port, &auth_url);
                    complete_openai_login_from_manual_future(prepared, manual_input).await
                }
                Err(err) => Err(err),
            };
            ctx.handle.close_modal();
            ctx.handle.force_redraw();
            let session = match login_result {
                Ok(session) => session,
                Err(err) if is_oauth_flow_cancelled(&err) => {
                    if ctx.ctrl_c_state.is_exit_requested() {
                        return Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit));
                    }
                    ctx.renderer
                        .line(MessageStyle::Info, "OpenAI ChatGPT authentication cancelled.")?;
                    return Ok(SlashCommandControl::Continue);
                }
                Err(err) => return Err(err),
            };
            sync_openai_runtime_if_active(&mut ctx).await?;
            ctx.renderer
                .line(MessageStyle::Info, "Successfully authenticated with OpenAI via ChatGPT.")?;
            if ctx.config.provider.eq_ignore_ascii_case(OPENAI_PROVIDER) && ctx.config.openai_chatgpt_auth.is_some() {
                ctx.renderer
                    .line(MessageStyle::Output, "Switched the current session to OpenAI (ChatGPT).")?;
            }
            if let Some(email) = session.email.as_deref() {
                ctx.renderer.line(MessageStyle::Output, &format!("Account: {email}"))?;
            }
            if let Some(plan) = session.plan.as_deref() {
                ctx.renderer.line(MessageStyle::Output, &format!("Plan: {plan}"))?;
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
        COPILOT_PROVIDER => {
            ctx.renderer
                .line(MessageStyle::Info, "Starting GitHub Copilot logout via the official `copilot` CLI...")?;
            render_copilot_auth_intro(ctx.renderer, CopilotAuthAction::Logout)?;
            let auth_cfg = vt_cfg.map(|cfg| cfg.auth.copilot.clone()).unwrap_or_default();
            logout_with_events(&auth_cfg, &ctx.config.workspace, |event| {
                render_copilot_auth_event(ctx.renderer, event)
            })
            .await?;
            sync_copilot_runtime_if_active(&mut ctx).await?;
            ctx.renderer
                .line(MessageStyle::Info, "GitHub Copilot authentication cleared successfully.")?;
            if ctx.config.provider.eq_ignore_ascii_case(COPILOT_PROVIDER) {
                ctx.renderer.line(
                    MessageStyle::Output,
                    "The current GitHub Copilot session no longer has active credentials.",
                )?;
            }
        }
        OPENROUTER_PROVIDER => {
            if matches!(openrouter_auth_status(vt_cfg)?, AuthStatus::NotAuthenticated) {
                if vtcode_core::config::api_keys::get_api_key_with_mode(
                    OPENROUTER_PROVIDER,
                    &ApiKeySources::default(),
                    credential_storage_mode(&ctx),
                )
                .is_ok()
                {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        "OpenRouter OAuth token already cleared; using OPENROUTER_API_KEY.",
                    )?;
                } else {
                    ctx.renderer
                        .line(MessageStyle::Info, "No stored OpenRouter OAuth token to clear.")?;
                }
                return Ok(SlashCommandControl::Continue);
            }
            clear_openrouter_login(vt_cfg)?;
            ctx.renderer
                .line(MessageStyle::Info, "OpenRouter OAuth token cleared successfully.")?;
        }
        OPENAI_PROVIDER => {
            let overview = summarize_current_openai_credentials(vt_cfg)?;
            if matches!(openai_auth_status(vt_cfg)?, OpenAIChatGptAuthStatus::NotAuthenticated) {
                if vtcode_core::config::api_keys::get_api_key_with_mode(
                    OPENAI_PROVIDER,
                    &ApiKeySources::default(),
                    credential_storage_mode(&ctx),
                )
                .is_ok()
                {
                    ctx.renderer
                        .line(MessageStyle::Info, "OpenAI ChatGPT session already cleared; using OPENAI_API_KEY.")?;
                } else if overview.codex_fallback_available {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        "OpenAI ChatGPT session not stored; Codex auth.json fallback is active.",
                    )?;
                    ctx.renderer
                        .line(MessageStyle::Output, "Run `codex logout` to remove the Codex credentials entirely.")?;
                    ctx.renderer.line(
                        MessageStyle::Output,
                        "For a VT Code session with full auto-refresh, run /login openai.",
                    )?;
                } else {
                    ctx.renderer
                        .line(MessageStyle::Info, "No stored OpenAI ChatGPT session to clear.")?;
                }
                return Ok(SlashCommandControl::Continue);
            }
            clear_openai_login(vt_cfg)?;
            sync_openai_runtime_if_active(&mut ctx).await?;
            ctx.renderer
                .line(MessageStyle::Info, "OpenAI ChatGPT session cleared successfully.")?;
            // Inform the user if Codex's auth.json fallback is still active.
            if overview.codex_fallback_available {
                ctx.renderer.line(
                    MessageStyle::Output,
                    "Note: Codex's auth.json was detected. VT Code will continue to use it as a\n\
                     fallback at runtime. Run `codex logout` to remove those credentials entirely.",
                )?;
            }
            if ctx.config.provider.eq_ignore_ascii_case(OPENAI_PROVIDER) {
                if ctx.config.api_key.trim().is_empty() {
                    ctx.renderer
                        .line(MessageStyle::Output, "The current OpenAI session no longer has active credentials.")?;
                } else {
                    ctx.renderer
                        .line(MessageStyle::Output, "The current OpenAI session fell back to OPENAI_API_KEY.")?;
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
        COPILOT_PROVIDER => {
            ctx.renderer.line(
                MessageStyle::Info,
                "GitHub Copilot does not expose a refresh-token flow. Use /login copilot to reconnect if needed.",
            )?;
        }
        OPENAI_PROVIDER => {
            ctx.renderer
                .line(MessageStyle::Info, "Refreshing the stored OpenAI ChatGPT session...")?;
            let session = refresh_openai_login(ctx.vt_cfg.as_ref()).await?;
            sync_openai_runtime_if_active(&mut ctx).await?;
            ctx.renderer
                .line(MessageStyle::Info, "OpenAI ChatGPT session refreshed successfully.")?;
            if let Some(email) = session.email.as_deref() {
                ctx.renderer.line(MessageStyle::Output, &format!("Account: {email}"))?;
            }
            if let Some(plan) = session.plan.as_deref() {
                ctx.renderer.line(MessageStyle::Output, &format!("Plan: {plan}"))?;
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
        && !supports_auth_provider(provider_name)
    {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!(
                "Authentication status not supported for provider: {provider_name}. Supported providers: openai, openrouter, copilot"
            ),
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    ctx.renderer.line(MessageStyle::Info, "Authentication Status")?;
    ctx.renderer.line(MessageStyle::Output, "")?;
    let vt_cfg = ctx.vt_cfg.as_ref();
    let storage_mode = credential_storage_mode(&ctx);

    if provider.is_none() || provider.as_deref() == Some(OPENROUTER_PROVIDER) {
        render_openrouter_auth_status(ctx.renderer, openrouter_auth_status(vt_cfg)?, storage_mode)?;
    }

    if provider.is_none() {
        ctx.renderer.line(MessageStyle::Output, "")?;
    }

    if provider.is_none() || provider.as_deref() == Some(OPENAI_PROVIDER) {
        let overview = summarize_current_openai_credentials(vt_cfg)?;
        render_openai_auth_status(ctx.renderer, openai_auth_status(vt_cfg)?, storage_mode, &overview)?;
        render_openai_credential_overview(
            ctx.renderer,
            vt_cfg,
            ctx.config.provider.eq_ignore_ascii_case(OPENAI_PROVIDER),
        )?;
    }

    if provider.is_none() {
        ctx.renderer.line(MessageStyle::Output, "")?;
    }

    if provider.is_none() || provider.as_deref() == Some(COPILOT_PROVIDER) {
        let auth_cfg = vt_cfg.map(|cfg| cfg.auth.copilot.clone()).unwrap_or_default();
        let status = probe_auth_status(&auth_cfg, Some(&ctx.config.workspace)).await;
        render_copilot_auth_status(ctx.renderer, status)?;
    }

    if provider.is_none() {
        ctx.renderer.line(MessageStyle::Output, "")?;
        ctx.renderer
            .line(MessageStyle::Output, "Use /login, /logout, or /refresh-oauth to manage stored authentication.")?;
    }

    Ok(SlashCommandControl::Continue)
}

fn ensure_supported_provider(renderer: &mut AnsiRenderer, provider: &str, action: &str) -> Result<bool> {
    if supports_auth_provider(provider) {
        return Ok(true);
    }

    renderer.line(
        MessageStyle::Error,
        &format!(
            "Authentication {action} not supported for provider: {provider}. Supported providers: openai, openrouter, copilot"
        ),
    )?;
    Ok(false)
}

async fn open_browser_with_guidance(
    ctx: &mut SlashCommandContext<'_>,
    auth_url: &str,
) -> Result<Option<SlashCommandControl>> {
    match request_external_url_open(
        ExternalUrlGuardContext::new(ctx.handle, ctx.session, ctx.ctrl_c_state, ctx.ctrl_c_notify),
        auth_url,
    )
    .await?
    {
        ExternalUrlOpenOutcome::Opened => {
            ctx.renderer.line(MessageStyle::Info, "Opening browser for authentication...")?;
            ctx.renderer.hyperlink_line(MessageStyle::Response, auth_url)?;
            Ok(None)
        }
        ExternalUrlOpenOutcome::OpenFailed(err) => {
            ctx.renderer.line(MessageStyle::Info, "Opening browser for authentication...")?;
            ctx.renderer.hyperlink_line(MessageStyle::Response, auth_url)?;
            ctx.renderer
                .line(MessageStyle::Error, &format!("Failed to open browser automatically: {err}"))?;
            ctx.renderer
                .line(MessageStyle::Info, "Please open the URL manually in your browser.")?;
            Ok(None)
        }
        ExternalUrlOpenOutcome::Cancelled => {
            ctx.renderer
                .line(MessageStyle::Info, "Cancelled opening authentication link.")?;
            Ok(Some(SlashCommandControl::Continue))
        }
        ExternalUrlOpenOutcome::Exit => Ok(Some(SlashCommandControl::BreakWithReason(SessionEndReason::Exit))),
        ExternalUrlOpenOutcome::Unsupported => {
            ctx.renderer
                .line(MessageStyle::Error, "Blocked unsupported authentication link target.")?;
            Ok(Some(SlashCommandControl::Continue))
        }
    }
}

async fn prompt_openai_manual_callback_input(
    ctx: &mut SlashCommandContext<'_>,
    callback_port: u16,
    auth_url: &str,
) -> Result<Option<String>> {
    let step = WizardStep {
        title: "Callback".to_string(),
        question: format!(
            "Waiting for browser callback. If it doesn't open automatically, copy this URL:\n\n{auth_url}\n\nOr paste the redirected URL / query string below."
        ),
        items: vec![
            ui_list::choice(
                "Submit",
                Some("Press Tab to type text, then Enter to submit.".to_string()),
                Some(InlineListSelection::RequestUserInputAnswer {
                    question_id: OPENAI_MANUAL_PROMPT_ID.to_string(),
                    selected: vec![],
                    other: Some(String::new()),
                }),
            )
            .with_search_value("submit callback redirect url query string".to_string()),
        ],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some("Redirect URL or query".to_string()),
        freeform_placeholder: Some(openai_manual_placeholder(callback_port)),
        freeform_default: None,
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
        WizardModalOutcome::Submitted(selections) => selections.into_iter().find_map(|selection| match selection {
            InlineListSelection::RequestUserInputAnswer { question_id, selected, other }
                if question_id == OPENAI_MANUAL_PROMPT_ID =>
            {
                other.or_else(|| selected.first().cloned())
            }
            _ => None,
        }),
        WizardModalOutcome::Cancelled { signal } => {
            if matches!(signal, Some("cancel")) {
                ctx.ctrl_c_state.mark_cancel_handled();
            }
            return Err(oauth_flow_cancelled_error());
        }
    };

    Ok(value.and_then(|value| {
        let trimmed = value.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    }))
}

async fn show_oauth_provider_modal(ctx: &mut SlashCommandContext<'_>, action: OAuthProviderAction) -> Result<()> {
    let vt_cfg = ctx.vt_cfg.as_ref();
    let openrouter_status = openrouter_auth_status(vt_cfg)?;
    let openai_status = openai_auth_status(vt_cfg)?;
    let openai_overview = summarize_current_openai_credentials(vt_cfg)?;
    let copilot_auth_cfg = vt_cfg.map(|cfg| cfg.auth.copilot.clone()).unwrap_or_default();
    let copilot_status = probe_auth_status(&copilot_auth_cfg, Some(&ctx.config.workspace)).await;

    let mut items = vec![
        ui_list::action(
            "GitHub Copilot",
            copilot_modal_subtitle(action, &copilot_status),
            Some(copilot_modal_badge(action, &copilot_status)),
            Tone::Accent,
            Some(InlineListSelection::ConfigAction(format!("{OAUTH_PROVIDER_PREFIX}{COPILOT_PROVIDER}"))),
        )
        .with_search_value("github copilot cli auth".to_string()),
        ui_list::action(
            "OpenAI ChatGPT",
            openai_modal_subtitle(action, &openai_status, &openai_overview),
            Some(openai_modal_badge(action, &openai_status, &openai_overview)),
            Tone::Accent,
            Some(InlineListSelection::ConfigAction(format!("{OAUTH_PROVIDER_PREFIX}{OPENAI_PROVIDER}"))),
        )
        .with_search_value("openai chatgpt oauth subscription".to_string()),
        ui_list::action(
            "OpenRouter",
            openrouter_modal_subtitle(action, &openrouter_status),
            Some(openrouter_modal_badge(action, &openrouter_status)),
            Tone::Accent,
            Some(InlineListSelection::ConfigAction(format!("{OAUTH_PROVIDER_PREFIX}{OPENROUTER_PROVIDER}"))),
        )
        .with_search_value("openrouter oauth".to_string()),
        ui_list::action(
            "Back",
            "Close this dialog".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(OAUTH_PROVIDER_BACK.to_string())),
        )
        .with_search_value("back close cancel".to_string()),
    ];

    if matches!(action, OAuthProviderAction::Refresh) {
        items[0].badge = Some("Info".to_string());
        items[1].badge = Some("Refresh".to_string());
        items[2].badge = Some("Info".to_string());
    }

    ctx.renderer.show_list_modal(
        oauth_modal_title(action),
        oauth_modal_lines(action),
        items,
        Some(InlineListSelection::ConfigAction(format!("{OAUTH_PROVIDER_PREFIX}{COPILOT_PROVIDER}"))),
        None,
    );
    Ok(())
}

fn oauth_modal_title(action: OAuthProviderAction) -> &'static str {
    match action {
        OAuthProviderAction::Login => "Authentication login",
        OAuthProviderAction::Logout => "Authentication logout",
        OAuthProviderAction::Refresh => "Refresh authentication",
    }
}

fn oauth_modal_lines(action: OAuthProviderAction) -> Vec<String> {
    match action {
        OAuthProviderAction::Login => vec![
            "Choose a provider to connect.".to_string(),
            "Credentials stay in secure storage; GitHub Copilot uses the official CLI.".to_string(),
        ],
        OAuthProviderAction::Logout => vec![
            "Choose a provider to disconnect.".to_string(),
            "This removes the stored authentication session for the selected provider.".to_string(),
        ],
        OAuthProviderAction::Refresh => vec![
            "Choose a provider to refresh.".to_string(),
            "OpenAI refreshes the stored ChatGPT session; OpenRouter and GitHub Copilot require a new login."
                .to_string(),
        ],
    }
}

mod status;

use status::{
    CopilotAuthAction, copilot_modal_badge, copilot_modal_subtitle, openai_modal_badge, openai_modal_subtitle,
    openrouter_modal_badge, openrouter_modal_subtitle, render_copilot_auth_event, render_copilot_auth_intro,
    render_copilot_auth_status, render_openai_auth_status, render_openai_credential_overview,
    render_openrouter_auth_status, sync_copilot_runtime_if_active, sync_openai_runtime_if_active,
};

#[cfg(test)]
mod tests;
