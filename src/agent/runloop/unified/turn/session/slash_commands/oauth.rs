use anstyle::{AnsiColor, Color, Effects, Style as AnsiStyle};
use anyhow::Result;
use vtcode_auth::{AuthStatus, OpenAIChatGptAuthStatus, OpenAIResolvedAuthSource};
use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::types::UiSurfacePreference;
use vtcode_core::copilot::{
    COPILOT_AUTH_DOC_PATH, CopilotAuthEvent, CopilotAuthStatus, CopilotAuthStatusKind,
    login_with_events, logout_with_events, probe_auth_status,
};
use vtcode_core::hooks::SessionEndReason;
use vtcode_core::llm::factory::{ProviderConfig, create_provider_with_config};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::ansi_codes::notify_attention;
use vtcode_tui::app::{InlineListItem, InlineListSelection, WizardModalMode, WizardStep};

use super::{SlashCommandContext, SlashCommandControl, ui};
use crate::agent::runloop::slash_commands::OAuthProviderAction;
use crate::agent::runloop::ui::build_inline_header_context;
use crate::agent::runloop::unified::external_url_guard::{
    ExternalUrlGuardContext, ExternalUrlOpenOutcome, request_external_url_open,
};
use crate::agent::runloop::unified::wizard_modal::{
    WizardModalOutcome, show_wizard_modal_and_wait,
};
use crate::cli::auth::{
    COPILOT_PROVIDER, OPENAI_PROVIDER, OPENROUTER_PROVIDER, begin_openai_login,
    begin_openrouter_login, clear_openai_login, clear_openrouter_login,
    complete_openai_login_from_manual_future, complete_openai_login_with_manual_future,
    complete_openrouter_login_with_tui_cancel, is_oauth_flow_cancelled, oauth_flow_cancelled_error,
    openai_auth_status, openai_manual_placeholder, openrouter_auth_status, prepare_openai_login,
    prepare_openrouter_login, refresh_openai_login, should_prompt_manual_openai_input,
    supports_auth_provider,
};

const OAUTH_PROVIDER_PREFIX: &str = "oauth-provider:";
const OAUTH_PROVIDER_BACK: &str = "oauth-provider:back";
const OPENAI_MANUAL_PROMPT_ID: &str = "openai_manual_callback";

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
    let vt_cfg = ctx.vt_cfg.as_ref();

    match provider.as_str() {
        COPILOT_PROVIDER => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Starting GitHub Copilot authentication via the official `copilot` CLI...",
            )?;
            render_copilot_auth_intro(ctx.renderer, CopilotAuthAction::Login)?;
            let workspace = ctx.config.workspace.clone();
            let auth_cfg = ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.auth.copilot.clone())
                .unwrap_or_default();
            login_with_events(&auth_cfg, &workspace, |event| {
                render_copilot_auth_event(ctx.renderer, event)
            })
            .await?;
            sync_copilot_runtime_if_active(&mut ctx).await?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Successfully authenticated with GitHub Copilot.",
            )?;
            if ctx.config.provider.eq_ignore_ascii_case(COPILOT_PROVIDER) {
                ctx.renderer.line(
                    MessageStyle::Output,
                    "Switched the current session to GitHub Copilot.",
                )?;
            }
        }
        OPENROUTER_PROVIDER => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Starting OpenRouter OAuth authentication...",
            )?;
            let prepared = prepare_openrouter_login(vt_cfg)?;
            let auth_url = prepared.auth_url.clone();
            if let Some(control) = open_browser_with_guidance(&mut ctx, &auth_url).await? {
                return Ok(control);
            }
            let started = begin_openrouter_login(prepared).await?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Waiting for OpenRouter OAuth callback...",
            )?;
            let api_key = match complete_openrouter_login_with_tui_cancel(
                started,
                ctx.ctrl_c_state,
                ctx.ctrl_c_notify,
            )
            .await
            {
                Ok(api_key) => api_key,
                Err(err) if is_oauth_flow_cancelled(&err) => {
                    if ctx.ctrl_c_state.is_exit_requested() {
                        return Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit));
                    }
                    ctx.renderer.line(
                        MessageStyle::Info,
                        "OpenRouter OAuth authentication cancelled.",
                    )?;
                    return Ok(SlashCommandControl::Continue);
                }
                Err(err) => return Err(err),
            };
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
            let auth_url = prepared.auth_url.clone();
            let callback_port = prepared.callback_port;
            let started = begin_openai_login(prepared.clone()).await;
            if let Some(control) = open_browser_with_guidance(&mut ctx, &auth_url).await? {
                return Ok(control);
            }
            ctx.renderer
                .line(MessageStyle::Info, "Waiting for OpenAI OAuth callback...")?;
            let login_result = match started {
                Ok(started) => {
                    let manual_input =
                        prompt_openai_manual_callback_input(&mut ctx, callback_port, &auth_url);
                    complete_openai_login_with_manual_future(started, Some(manual_input)).await
                }
                Err(err) if should_prompt_manual_openai_input(&err) => {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        "Local callback listener is unavailable. Waiting for manual redirect input...",
                    )?;
                    let manual_input =
                        prompt_openai_manual_callback_input(&mut ctx, callback_port, &auth_url);
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
                    ctx.renderer.line(
                        MessageStyle::Info,
                        "OpenAI ChatGPT authentication cancelled.",
                    )?;
                    return Ok(SlashCommandControl::Continue);
                }
                Err(err) => return Err(err),
            };
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
        COPILOT_PROVIDER => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Starting GitHub Copilot logout via the official `copilot` CLI...",
            )?;
            render_copilot_auth_intro(ctx.renderer, CopilotAuthAction::Logout)?;
            let auth_cfg = vt_cfg
                .map(|cfg| cfg.auth.copilot.clone())
                .unwrap_or_default();
            logout_with_events(&auth_cfg, &ctx.config.workspace, |event| {
                render_copilot_auth_event(ctx.renderer, event)
            })
            .await?;
            sync_copilot_runtime_if_active(&mut ctx).await?;
            ctx.renderer.line(
                MessageStyle::Info,
                "GitHub Copilot authentication cleared successfully.",
            )?;
            if ctx.config.provider.eq_ignore_ascii_case(COPILOT_PROVIDER) {
                ctx.renderer.line(
                    MessageStyle::Output,
                    "The current GitHub Copilot session no longer has active credentials.",
                )?;
            }
        }
        OPENROUTER_PROVIDER => {
            if matches!(
                openrouter_auth_status(vt_cfg)?,
                AuthStatus::NotAuthenticated
            ) {
                if get_api_key(OPENROUTER_PROVIDER, &ApiKeySources::default()).is_ok() {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        "OpenRouter OAuth token already cleared; using OPENROUTER_API_KEY.",
                    )?;
                } else {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        "No stored OpenRouter OAuth token to clear.",
                    )?;
                }
                return Ok(SlashCommandControl::Continue);
            }
            clear_openrouter_login(vt_cfg)?;
            ctx.renderer.line(
                MessageStyle::Info,
                "OpenRouter OAuth token cleared successfully.",
            )?;
        }
        OPENAI_PROVIDER => {
            if matches!(
                openai_auth_status(vt_cfg)?,
                OpenAIChatGptAuthStatus::NotAuthenticated
            ) {
                if get_api_key(OPENAI_PROVIDER, &ApiKeySources::default()).is_ok() {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        "OpenAI ChatGPT session already cleared; using OPENAI_API_KEY.",
                    )?;
                } else {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        "No stored OpenAI ChatGPT session to clear.",
                    )?;
                }
                return Ok(SlashCommandControl::Continue);
            }
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
        COPILOT_PROVIDER => {
            ctx.renderer.line(
                MessageStyle::Info,
                "GitHub Copilot does not expose a refresh-token flow. Use /login copilot to reconnect if needed.",
            )?;
        }
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
        && !supports_auth_provider(provider_name)
    {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!(
                "Authentication status not supported for provider: {}. Supported providers: openai, openrouter, copilot",
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
    }

    if provider.is_none() || provider.as_deref() == Some(COPILOT_PROVIDER) {
        let auth_cfg = vt_cfg
            .map(|cfg| cfg.auth.copilot.clone())
            .unwrap_or_default();
        let status = probe_auth_status(&auth_cfg, Some(&ctx.config.workspace)).await;
        render_copilot_auth_status(ctx.renderer, status)?;
    }

    if provider.is_none() {
        ctx.renderer.line(MessageStyle::Output, "")?;
        ctx.renderer.line(
            MessageStyle::Output,
            "Use /login, /logout, or /refresh-oauth to manage stored authentication.",
        )?;
    }

    Ok(SlashCommandControl::Continue)
}

fn ensure_supported_provider(
    renderer: &mut AnsiRenderer,
    provider: &str,
    action: &str,
) -> Result<bool> {
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
            ctx.renderer
                .line(MessageStyle::Info, "Opening browser for authentication...")?;
            ctx.renderer
                .hyperlink_line(MessageStyle::Response, auth_url)?;
            Ok(None)
        }
        ExternalUrlOpenOutcome::OpenFailed(err) => {
            ctx.renderer
                .line(MessageStyle::Info, "Opening browser for authentication...")?;
            ctx.renderer
                .hyperlink_line(MessageStyle::Response, auth_url)?;
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to open browser automatically: {}", err),
            )?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Please open the URL manually in your browser.",
            )?;
            Ok(None)
        }
        ExternalUrlOpenOutcome::Cancelled => {
            ctx.renderer
                .line(MessageStyle::Info, "Cancelled opening authentication link.")?;
            Ok(Some(SlashCommandControl::Continue))
        }
        ExternalUrlOpenOutcome::Exit => Ok(Some(SlashCommandControl::BreakWithReason(
            SessionEndReason::Exit,
        ))),
        ExternalUrlOpenOutcome::Unsupported => {
            ctx.renderer.line(
                MessageStyle::Error,
                "Blocked unsupported authentication link target.",
            )?;
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

async fn show_oauth_provider_modal(
    ctx: &mut SlashCommandContext<'_>,
    action: OAuthProviderAction,
) -> Result<()> {
    let vt_cfg = ctx.vt_cfg.as_ref();
    let openrouter_status = openrouter_auth_status(vt_cfg)?;
    let openai_status = openai_auth_status(vt_cfg)?;
    let openai_overview = summarize_current_openai_credentials(vt_cfg)?;
    let copilot_auth_cfg = vt_cfg
        .map(|cfg| cfg.auth.copilot.clone())
        .unwrap_or_default();
    let copilot_status = probe_auth_status(&copilot_auth_cfg, Some(&ctx.config.workspace)).await;

    let mut items = vec![
        InlineListItem {
            title: "GitHub Copilot".to_string(),
            subtitle: Some(copilot_modal_subtitle(action, &copilot_status)),
            badge: Some(copilot_modal_badge(action, &copilot_status)),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}",
                OAUTH_PROVIDER_PREFIX, COPILOT_PROVIDER
            ))),
            search_value: Some("github copilot cli auth".to_string()),
        },
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
        items[0].badge = Some("Info".to_string());
        items[1].badge = Some("Refresh".to_string());
        items[2].badge = Some("Info".to_string());
    }

    ctx.renderer.show_list_modal(
        oauth_modal_title(action),
        oauth_modal_lines(action),
        items,
        Some(InlineListSelection::ConfigAction(format!(
            "{}{}",
            OAUTH_PROVIDER_PREFIX, COPILOT_PROVIDER
        ))),
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
            "VT Code stores OpenAI/OpenRouter credentials securely and uses the official `copilot` CLI for GitHub Copilot.".to_string(),
        ],
        OAuthProviderAction::Logout => vec![
            "Choose a provider to disconnect.".to_string(),
            "This removes the stored authentication session for the selected provider.".to_string(),
        ],
        OAuthProviderAction::Refresh => vec![
            "Choose a provider to refresh.".to_string(),
            "OpenAI refreshes the stored ChatGPT session; OpenRouter and GitHub Copilot require a new login.".to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashbrown::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::llm::provider as uni;
    use vtcode_tui::app::InlineHeaderContext;

    use crate::agent::runloop::model_picker::ModelPickerState;
    use crate::agent::runloop::unified::palettes::ActivePalette;
    use crate::agent::runloop::unified::session_setup::IdeContextBridge;
    use crate::agent::runloop::unified::state::CtrlCSignal;
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
    use crate::agent::runloop::unified::workspace_links::LinkedDirectory;
    use crate::agent::runloop::welcome::SessionBootstrap;
    use vtcode_core::skills::types::Skill;

    #[tokio::test]
    async fn openrouter_login_returns_continue_when_ctrl_c_is_pending() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let turn = backing.turn_loop_context();
        assert!(matches!(
            turn.ctrl_c_state.register_signal(),
            CtrlCSignal::Cancel
        ));

        let header_context = Box::leak(Box::new(InlineHeaderContext::default()));
        let ide_context_bridge = Box::leak(Box::new(None::<IdeContextBridge>));
        let model_picker_state = Box::leak(Box::new(None::<ModelPickerState>));
        let palette_state = Box::leak(Box::new(None::<ActivePalette>));
        let conversation_history = Box::leak(Box::new(Vec::<uni::Message>::new()));
        let loaded_skills = Box::leak(Box::new(Arc::new(RwLock::new(
            HashMap::<String, Skill>::new(),
        ))));
        let linked_directories = Box::leak(Box::new(Vec::<LinkedDirectory>::new()));
        let session_bootstrap = Box::leak(Box::new(SessionBootstrap::default()));
        let vt_cfg = Box::leak(Box::new(None::<VTCodeConfig>));
        let async_mcp_manager = None;
        let checkpoint_manager = None;
        let lifecycle_hooks = None;
        let harness_emitter = None;
        let thread_handle = Box::leak(Box::new(
            vtcode_core::core::threads::ThreadManager::new().start_thread_with_identifier(
                "test-thread",
                vtcode_core::core::threads::ThreadBootstrap::new(None),
            ),
        ));

        let ctx = SlashCommandContext {
            thread_id: "test-thread",
            active_thread_label: "main",
            thread_handle,
            renderer: turn.renderer,
            handle: turn.handle,
            session: turn.session,
            header_context,
            ide_context_bridge,
            config: turn.config,
            vt_cfg,
            provider_client: turn.provider_client,
            session_bootstrap,
            model_picker_state,
            palette_state,
            tool_registry: turn.tool_registry,
            conversation_history,
            decision_ledger: turn.decision_ledger,
            context_manager: turn.context_manager,
            session_stats: turn.session_stats,
            input_status_state: turn.input_status_state,
            tools: turn.tools,
            tool_catalog: turn.tool_catalog,
            async_mcp_manager,
            mcp_panel_state: turn.mcp_panel_state,
            linked_directories,
            ctrl_c_state: turn.ctrl_c_state,
            ctrl_c_notify: turn.ctrl_c_notify,
            full_auto: turn.full_auto,
            loaded_skills,
            checkpoint_manager,
            lifecycle_hooks,
            harness_emitter,
        };

        let control = handle_oauth_login(ctx, OPENROUTER_PROVIDER.to_string())
            .await
            .expect("openrouter login should continue after cancel");

        assert!(matches!(control, SlashCommandControl::Continue));
    }
}

fn copilot_modal_subtitle(action: OAuthProviderAction, status: &CopilotAuthStatus) -> String {
    match action {
        OAuthProviderAction::Login => match status.kind {
            CopilotAuthStatusKind::Authenticated => {
                "Managed auth is connected; rerun the Copilot CLI login to replace the active session."
                    .to_string()
            }
            CopilotAuthStatusKind::Unauthenticated => {
                "Sign in with your GitHub Copilot subscription through the official Copilot CLI."
                    .to_string()
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
            CopilotAuthStatusKind::Authenticated => {
                "Remove the active GitHub Copilot CLI session.".to_string()
            }
            _ => "No stored GitHub Copilot session to remove.".to_string(),
        },
        OAuthProviderAction::Refresh => {
            "GitHub Copilot requires a new login instead of token refresh.".to_string()
        }
    }
}

fn copilot_modal_badge(action: OAuthProviderAction, status: &CopilotAuthStatus) -> String {
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

fn render_copilot_auth_status(
    renderer: &mut AnsiRenderer,
    status: CopilotAuthStatus,
) -> Result<()> {
    match status.kind {
        CopilotAuthStatusKind::Authenticated => {
            renderer.line(
                MessageStyle::Info,
                "GitHub Copilot: authenticated (managed auth via Copilot CLI)",
            )?;
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
        renderer.line(MessageStyle::Output, &format!("  Details: {}", message))?;
    }

    if matches!(status.kind, CopilotAuthStatusKind::ServerUnavailable) {
        renderer.line(
            MessageStyle::Output,
            &format!(
                "  Help: install `copilot` or configure `[auth.copilot].command`; see {COPILOT_AUTH_DOC_PATH}."
            ),
        )?;
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
            copilot_auth: ctx.vt_cfg.as_ref().map(|cfg| cfg.auth.copilot.clone()),
            base_url: None,
            model: Some(ctx.config.model.clone()),
            prompt_cache: Some(ctx.config.prompt_cache.clone()),
            timeouts: None,
            openai: ctx.vt_cfg.as_ref().map(|cfg| cfg.provider.openai.clone()),
            anthropic: None,
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

async fn sync_copilot_runtime_if_active(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
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
            anthropic: None,
            model_behavior: ctx.config.model_behavior.clone(),
            workspace_root: Some(ctx.config.workspace.clone()),
        },
    )?;
    *ctx.provider_client = provider;
    ctx.config.api_key.clear();
    ctx.config.openai_chatgpt_auth = None;

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
        "GitHub Copilot".to_string(),
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

#[derive(Clone, Copy)]
enum CopilotAuthAction {
    Login,
    Logout,
}

fn render_copilot_auth_intro(renderer: &mut AnsiRenderer, action: CopilotAuthAction) -> Result<()> {
    renderer.line(MessageStyle::Info, "Managed auth via GitHub Copilot CLI.")?;
    renderer.line(
        MessageStyle::Info,
        "`gh` is optional fallback only; login/logout require the official `copilot` CLI.",
    )?;
    renderer.line(
        MessageStyle::Info,
        match action {
            CopilotAuthAction::Login => {
                "Waiting for the official Copilot CLI to start the managed login flow."
            }
            CopilotAuthAction::Logout => "Clearing the managed GitHub Copilot CLI session.",
        },
    )?;
    Ok(())
}

fn render_copilot_auth_event(renderer: &mut AnsiRenderer, event: CopilotAuthEvent) -> Result<()> {
    match event {
        CopilotAuthEvent::VerificationCode { url, user_code } => {
            renderer.line(
                MessageStyle::Info,
                "Your GitHub device code — copy it before the browser opens:",
            )?;
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
