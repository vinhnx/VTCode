use crate::agent::runloop::unified::state::CtrlCState;
use anyhow::{Context, Result, anyhow};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Notify;
use vtcode_auth::{
    AuthCallbackOutcome, AuthCodeCallbackServer, AuthCredentialsStoreMode, AuthStatus,
    OAuthCallbackPage, OAuthProvider, OpenAIChatGptAuthStatus, OpenAIChatGptSession,
    OpenRouterToken, PkceChallenge, clear_oauth_token_with_mode, clear_openai_chatgpt_session,
    exchange_code_for_token, exchange_openai_chatgpt_code_for_tokens, generate_openai_oauth_state,
    generate_pkce_challenge, get_auth_status_with_mode, get_auth_url,
    get_openai_chatgpt_auth_status_with_mode, get_openai_chatgpt_auth_url,
    load_openai_chatgpt_session_with_mode, parse_openai_chatgpt_manual_callback_input,
    save_oauth_token_with_mode, save_openai_chatgpt_session_with_mode,
    start_auth_code_callback_server,
};
use vtcode_config::VTCodeConfig;
use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::copilot::{
    COPILOT_AUTH_DOC_PATH, CopilotAuthEvent, CopilotAuthStatus, CopilotAuthStatusKind,
    login_with_events, logout_with_events, probe_auth_status,
};

use crate::agent::runloop::unified::url_guard::{UrlGuardPrompt, open_external_url};
use crate::codex_app_server::{
    CODEX_PROVIDER, CodexAccount, CodexAccountLoginCompleted, CodexAccountReadResponse,
    CodexAppServerClient, CodexLoginAccountResponse, CodexMcpServerStatus, ServerEvent,
    is_codex_cli_unavailable,
};

pub(crate) const OPENAI_PROVIDER: &str = "openai";
pub(crate) const OPENROUTER_PROVIDER: &str = "openrouter";
pub(crate) const COPILOT_PROVIDER: &str = "copilot";
const DEFAULT_OPENROUTER_CALLBACK_PORT: u16 = 8484;
const DEFAULT_FLOW_TIMEOUT_SECS: u64 = 300;
const OPENAI_MANUAL_PLACEHOLDER: &str = "http://localhost:1455/auth/callback?code=...&state=...";

#[derive(Debug, Clone)]
pub(crate) struct PreparedOpenRouterLogin {
    pub(crate) auth_url: String,
    callback_port: u16,
    timeout_secs: u64,
    storage_mode: AuthCredentialsStoreMode,
    pkce: PkceChallenge,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedOpenAiLogin {
    pub(crate) auth_url: String,
    pub(crate) callback_port: u16,
    timeout_secs: u64,
    storage_mode: AuthCredentialsStoreMode,
    pkce: PkceChallenge,
    state: String,
}

pub(crate) struct StartedOpenRouterLogin {
    prepared: PreparedOpenRouterLogin,
    callback_server: AuthCodeCallbackServer,
}

pub(crate) struct StartedOpenAiLogin {
    prepared: PreparedOpenAiLogin,
    callback_server: AuthCodeCallbackServer,
}

pub(crate) fn supports_oauth_provider(provider: &str) -> bool {
    provider.parse::<OAuthProvider>().is_ok()
}

pub(crate) fn supports_auth_provider(provider: &str) -> bool {
    provider.eq_ignore_ascii_case(COPILOT_PROVIDER)
        || provider.eq_ignore_ascii_case(CODEX_PROVIDER)
        || supports_oauth_provider(provider)
}

pub(crate) fn prepare_openrouter_login(
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<PreparedOpenRouterLogin> {
    let callback_port = vt_cfg
        .map(|cfg| cfg.auth.openrouter.callback_port)
        .unwrap_or(DEFAULT_OPENROUTER_CALLBACK_PORT);
    let timeout_secs = vt_cfg
        .map(|cfg| cfg.auth.openrouter.flow_timeout_secs)
        .unwrap_or(DEFAULT_FLOW_TIMEOUT_SECS);
    let storage_mode = credential_storage_mode(vt_cfg);
    let pkce = generate_pkce_challenge()?;
    let auth_url = get_auth_url(&pkce, callback_port);

    Ok(PreparedOpenRouterLogin {
        auth_url,
        callback_port,
        timeout_secs,
        storage_mode,
        pkce,
    })
}

pub(crate) async fn begin_openrouter_login(
    prepared: PreparedOpenRouterLogin,
) -> Result<StartedOpenRouterLogin> {
    let callback_server = start_auth_code_callback_server(
        prepared.callback_port,
        prepared.timeout_secs,
        OAuthCallbackPage::new(OAuthProvider::OpenRouter),
        None,
    )
    .await?;
    Ok(StartedOpenRouterLogin {
        prepared,
        callback_server,
    })
}

pub(crate) async fn complete_openrouter_login(started: StartedOpenRouterLogin) -> Result<String> {
    let StartedOpenRouterLogin {
        prepared,
        callback_server,
    } = started;

    match callback_server.wait().await? {
        AuthCallbackOutcome::Code(code) => persist_openrouter_login_code(prepared, &code).await,
        AuthCallbackOutcome::Cancelled => Err(anyhow!("OAuth flow was cancelled")),
        AuthCallbackOutcome::Error(error) => Err(anyhow!(error)),
    }
}

pub(crate) async fn complete_openrouter_login_with_tui_cancel(
    started: StartedOpenRouterLogin,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<String> {
    let StartedOpenRouterLogin {
        prepared,
        callback_server,
    } = started;

    let outcome =
        wait_for_callback_or_cancel(callback_server.wait(), ctrl_c_state, ctrl_c_notify).await?;

    match outcome {
        AuthCallbackOutcome::Code(code) => persist_openrouter_login_code(prepared, &code).await,
        AuthCallbackOutcome::Cancelled => Err(anyhow!("OAuth flow was cancelled")),
        AuthCallbackOutcome::Error(error) => Err(anyhow!(error)),
    }
}

pub(crate) fn prepare_openai_login(vt_cfg: Option<&VTCodeConfig>) -> Result<PreparedOpenAiLogin> {
    let callback_port = vt_cfg
        .map(|cfg| cfg.auth.openai.callback_port)
        .unwrap_or(vtcode_auth::OpenAIAuthConfig::default().callback_port);
    let timeout_secs = vt_cfg
        .map(|cfg| cfg.auth.openai.flow_timeout_secs)
        .unwrap_or(DEFAULT_FLOW_TIMEOUT_SECS);
    let storage_mode = credential_storage_mode(vt_cfg);
    let pkce = generate_pkce_challenge()?;
    let state = generate_openai_oauth_state()?;
    let auth_url = get_openai_chatgpt_auth_url(&pkce, callback_port, &state);

    Ok(PreparedOpenAiLogin {
        auth_url,
        callback_port,
        timeout_secs,
        storage_mode,
        pkce,
        state,
    })
}

pub(crate) async fn complete_openai_login(
    started: StartedOpenAiLogin,
) -> Result<OpenAIChatGptSession> {
    complete_openai_login_with_manual_future(
        started,
        None::<std::future::Ready<Result<Option<String>>>>,
    )
    .await
}

pub(crate) async fn begin_openai_login(
    prepared: PreparedOpenAiLogin,
) -> Result<StartedOpenAiLogin> {
    let callback_server = start_auth_code_callback_server(
        prepared.callback_port,
        prepared.timeout_secs,
        OAuthCallbackPage::new(OAuthProvider::OpenAi),
        Some(prepared.state.clone()),
    )
    .await?;
    Ok(StartedOpenAiLogin {
        prepared,
        callback_server,
    })
}

pub(crate) async fn complete_openai_login_with_manual_future<ManualFut>(
    started: StartedOpenAiLogin,
    manual_input: Option<ManualFut>,
) -> Result<OpenAIChatGptSession>
where
    ManualFut: Future<Output = Result<Option<String>>>,
{
    let StartedOpenAiLogin {
        prepared,
        callback_server,
    } = started;
    let code =
        resolve_openai_authorization_code(callback_server.wait(), &prepared.state, manual_input)
            .await?;
    persist_openai_login_code(prepared, &code).await
}

pub(crate) async fn complete_openai_login_with_tui_cancel(
    started: StartedOpenAiLogin,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<OpenAIChatGptSession> {
    let StartedOpenAiLogin {
        prepared,
        callback_server,
    } = started;

    let outcome =
        wait_for_callback_or_cancel(callback_server.wait(), ctrl_c_state, ctrl_c_notify).await?;

    let code = callback_outcome_to_openai_code(outcome)?;
    persist_openai_login_code(prepared, &code).await
}

pub(crate) async fn complete_openai_login_from_manual_future<ManualFut>(
    prepared: PreparedOpenAiLogin,
    manual_input: ManualFut,
) -> Result<OpenAIChatGptSession>
where
    ManualFut: Future<Output = Result<Option<String>>>,
{
    let input = manual_input
        .await?
        .ok_or_else(|| anyhow!("No redirected URL or query string provided"))?;
    let code = parse_openai_chatgpt_manual_callback_input(&input, &prepared.state)?;
    persist_openai_login_code(prepared, &code).await
}

async fn persist_openai_login_code(
    prepared: PreparedOpenAiLogin,
    code: &str,
) -> Result<OpenAIChatGptSession> {
    tracing::info!("received openai oauth authorization code; exchanging tokens");
    let session =
        exchange_openai_chatgpt_code_for_tokens(code, &prepared.pkce, prepared.callback_port)
            .await?;
    tracing::info!("openai oauth token exchange completed; persisting session");
    save_openai_chatgpt_session_with_mode(&session, prepared.storage_mode)?;
    tracing::info!("openai oauth session persisted; verifying load");
    load_openai_chatgpt_session_with_mode(prepared.storage_mode)?
        .ok_or_else(|| anyhow!("OpenAI ChatGPT session was not persisted correctly"))
}

async fn persist_openrouter_login_code(
    prepared: PreparedOpenRouterLogin,
    code: &str,
) -> Result<String> {
    let api_key = exchange_code_for_token(code, &prepared.pkce).await?;
    let token = OpenRouterToken {
        api_key: api_key.clone(),
        obtained_at: now_secs(),
        expires_at: None,
        label: Some("VT Code OAuth".to_string()),
    };
    save_oauth_token_with_mode(&token, prepared.storage_mode)?;
    Ok(api_key)
}

async fn resolve_openai_authorization_code<CallbackFut, ManualFut>(
    callback_future: CallbackFut,
    expected_state: &str,
    manual_input: Option<ManualFut>,
) -> Result<String>
where
    CallbackFut: Future<Output = Result<AuthCallbackOutcome>>,
    ManualFut: Future<Output = Result<Option<String>>>,
{
    tokio::pin!(callback_future);

    let Some(manual_input) = manual_input else {
        return callback_outcome_to_openai_code(callback_future.await?);
    };

    tokio::pin!(manual_input);

    match tokio::select! {
        callback = &mut callback_future => OpenAiAuthRace::Callback(callback),
        manual = &mut manual_input => OpenAiAuthRace::Manual(manual),
    } {
        OpenAiAuthRace::Manual(Ok(Some(input))) => {
            match parse_openai_chatgpt_manual_callback_input(&input, expected_state) {
                Ok(code) => Ok(code),
                Err(manual_err) => match callback_future.await {
                    Ok(AuthCallbackOutcome::Code(code)) => Ok(code),
                    Ok(_) | Err(_) => Err(manual_err),
                },
            }
        }
        OpenAiAuthRace::Manual(Ok(None)) => callback_outcome_to_openai_code(callback_future.await?),
        OpenAiAuthRace::Manual(Err(err)) => Err(err),
        OpenAiAuthRace::Callback(Ok(outcome)) => match callback_outcome_to_openai_code(outcome) {
            Ok(code) => Ok(code),
            Err(callback_err) => match manual_input.await? {
                Some(input) => parse_openai_chatgpt_manual_callback_input(&input, expected_state),
                None => Err(callback_err),
            },
        },
        OpenAiAuthRace::Callback(Err(callback_err)) => match manual_input.await? {
            Some(input) => parse_openai_chatgpt_manual_callback_input(&input, expected_state),
            None => Err(callback_err),
        },
    }
}

async fn wait_for_callback_or_cancel<CallbackFut>(
    callback_future: CallbackFut,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<AuthCallbackOutcome>
where
    CallbackFut: Future<Output = Result<AuthCallbackOutcome>>,
{
    let cancel_future = wait_for_ctrl_c_cancel(ctrl_c_state, ctrl_c_notify);
    tokio::pin!(callback_future);
    tokio::pin!(cancel_future);

    tokio::select! {
        biased;
        _ = &mut cancel_future => {
            if ctrl_c_state.is_cancel_requested() {
                ctrl_c_state.mark_cancel_handled();
            }
            Ok(AuthCallbackOutcome::Cancelled)
        }
        outcome = &mut callback_future => outcome,
    }
}

async fn wait_for_ctrl_c_cancel(ctrl_c_state: &Arc<CtrlCState>, ctrl_c_notify: &Arc<Notify>) {
    if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
        return;
    }
    ctrl_c_notify.notified().await;
}

enum OpenAiAuthRace {
    Callback(Result<AuthCallbackOutcome>),
    Manual(Result<Option<String>>),
}

fn callback_outcome_to_openai_code(outcome: AuthCallbackOutcome) -> Result<String> {
    match outcome {
        AuthCallbackOutcome::Code(code) => Ok(code),
        AuthCallbackOutcome::Cancelled => Err(oauth_flow_cancelled_error()),
        AuthCallbackOutcome::Error(error) => Err(anyhow!(error)),
    }
}

#[derive(Debug, thiserror::Error)]
#[error("OAuth flow was cancelled")]
struct OAuthFlowCancelled;

pub(crate) fn oauth_flow_cancelled_error() -> anyhow::Error {
    anyhow!(OAuthFlowCancelled)
}

pub(crate) fn is_oauth_flow_cancelled(err: &anyhow::Error) -> bool {
    err.is::<OAuthFlowCancelled>()
}

pub(crate) fn clear_openrouter_login(vt_cfg: Option<&VTCodeConfig>) -> Result<()> {
    clear_oauth_token_with_mode(credential_storage_mode(vt_cfg))
}

pub(crate) fn clear_openai_login(_vt_cfg: Option<&VTCodeConfig>) -> Result<()> {
    clear_openai_chatgpt_session()
}

pub(crate) fn openrouter_auth_status(vt_cfg: Option<&VTCodeConfig>) -> Result<AuthStatus> {
    get_auth_status_with_mode(credential_storage_mode(vt_cfg))
}

pub(crate) fn openai_auth_status(vt_cfg: Option<&VTCodeConfig>) -> Result<OpenAIChatGptAuthStatus> {
    get_openai_chatgpt_auth_status_with_mode(credential_storage_mode(vt_cfg))
}

pub(crate) fn load_openai_session(
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<Option<OpenAIChatGptSession>> {
    load_openai_chatgpt_session_with_mode(credential_storage_mode(vt_cfg))
}

pub(crate) async fn refresh_openai_login(
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<OpenAIChatGptSession> {
    vtcode_auth::refresh_openai_chatgpt_session_with_mode(credential_storage_mode(vt_cfg)).await
}

pub(crate) async fn refresh_openai_login_if_available(
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<Option<OpenAIChatGptSession>> {
    if load_openai_session(vt_cfg)?.is_none() {
        return Ok(None);
    }

    refresh_openai_login(vt_cfg).await.map(Some)
}

pub(crate) async fn handle_login_command(
    vt_cfg: Option<&VTCodeConfig>,
    provider: &str,
) -> Result<()> {
    let provider = provider.trim().to_ascii_lowercase();
    if provider == CODEX_PROVIDER {
        let client = CodexAppServerClient::connect(vt_cfg).await?;
        let status = client.account_read().await?;
        if status.account.is_some() {
            print_existing_codex_auth_summary(&status);
            return Ok(());
        }
        if !status.requires_openai_auth {
            println!("Codex does not currently require OpenAI authentication for this setup.");
            return Ok(());
        }

        let response = client.account_login_chatgpt().await?;
        let (auth_url, login_id) = match response {
            CodexLoginAccountResponse::ChatGpt { auth_url, login_id } => (auth_url, login_id),
            CodexLoginAccountResponse::ApiKey => {
                println!("Codex is configured with API key authentication.");
                return Ok(());
            }
            CodexLoginAccountResponse::ChatGptAuthTokens => {
                return Err(anyhow!(
                    "Codex requested externally managed ChatGPT tokens; `vtcode login codex` supports only browser-based ChatGPT login"
                ));
            }
        };

        println!("Starting Codex ChatGPT authentication...");
        open_browser_or_print_url(&auth_url)?;
        let mut events = client.subscribe();
        let completion =
            wait_for_codex_account_login_completion(&mut events, Some(&login_id)).await?;
        if !completion.success {
            return Err(anyhow!(
                completion
                    .error
                    .unwrap_or_else(|| "Codex ChatGPT login failed".to_string())
            ));
        }
        let updated = client.account_read().await?;
        print_codex_login_summary(&updated);
        return Ok(());
    }

    if provider == COPILOT_PROVIDER {
        let workspace = current_auth_workspace();
        let auth_cfg = vt_cfg
            .map(|cfg| cfg.auth.copilot.clone())
            .unwrap_or_default();
        println!("Starting GitHub Copilot authentication via the official `copilot` CLI...");
        login_with_events(&auth_cfg, &workspace, print_copilot_auth_event).await?;
        println!("GitHub Copilot authentication complete.");
        return Ok(());
    }

    match provider.parse::<OAuthProvider>() {
        Ok(OAuthProvider::OpenRouter) => {
            let prepared = prepare_openrouter_login(vt_cfg)?;
            let auth_url = prepared.auth_url.clone();
            let started = begin_openrouter_login(prepared).await?;
            println!("Starting OpenRouter OAuth authentication...");
            open_browser_or_print_url(&auth_url)?;
            let api_key = complete_openrouter_login(started).await?;
            println!("OpenRouter authentication complete.");
            println!(
                "Stored secure OAuth token. Key preview: {}...",
                &api_key[..api_key.len().min(8)]
            );
            Ok(())
        }
        Ok(OAuthProvider::OpenAi) => {
            let prepared = prepare_openai_login(vt_cfg)?;
            let started = begin_openai_login(prepared.clone()).await;
            println!("Starting OpenAI ChatGPT authentication...");
            open_browser_or_print_url(&prepared.auth_url)?;
            print_openai_manual_guidance();
            let session = complete_openai_login_with_cli_fallback(prepared, started).await?;
            println!("OpenAI ChatGPT authentication complete.");
            if let Some(email) = session.email.as_deref() {
                println!("Account: {}", email);
            }
            if let Some(plan) = session.plan.as_deref() {
                println!("Plan: {}", plan);
            }
            Ok(())
        }
        Err(()) => Err(anyhow!(
            "Authentication is not supported for provider '{}'. Supported providers: openai, openrouter, copilot, codex",
            provider
        )),
    }
}

pub(crate) async fn handle_logout_command(
    vt_cfg: Option<&VTCodeConfig>,
    provider: &str,
) -> Result<()> {
    let provider = provider.trim().to_ascii_lowercase();
    if provider == CODEX_PROVIDER {
        let client = CodexAppServerClient::connect(vt_cfg).await?;
        let status = client.account_read().await?;
        if status.account.is_none() {
            println!("Codex account credentials already cleared.");
            return Ok(());
        }
        client.account_logout().await?;
        println!("Codex account credentials cleared.");
        return Ok(());
    }

    if provider == COPILOT_PROVIDER {
        let workspace = current_auth_workspace();
        let auth_cfg = vt_cfg
            .map(|cfg| cfg.auth.copilot.clone())
            .unwrap_or_default();
        logout_with_events(&auth_cfg, &workspace, print_copilot_auth_event).await?;
        println!("GitHub Copilot authentication cleared.");
        return Ok(());
    }

    match provider.parse::<OAuthProvider>() {
        Ok(OAuthProvider::OpenRouter) => {
            if matches!(
                openrouter_auth_status(vt_cfg)?,
                AuthStatus::NotAuthenticated
            ) {
                println!("OpenRouter OAuth token already cleared.");
                return Ok(());
            }
            clear_openrouter_login(vt_cfg)?;
            println!("OpenRouter OAuth token cleared.");
            Ok(())
        }
        Ok(OAuthProvider::OpenAi) => {
            if matches!(
                openai_auth_status(vt_cfg)?,
                OpenAIChatGptAuthStatus::NotAuthenticated
            ) {
                println!("OpenAI ChatGPT session already cleared.");
                return Ok(());
            }
            clear_openai_login(vt_cfg)?;
            println!("OpenAI ChatGPT session cleared.");
            Ok(())
        }
        Err(()) => Err(anyhow!(
            "Authentication is not supported for provider '{}'. Supported providers: openai, openrouter, copilot, codex",
            provider
        )),
    }
}

pub(crate) async fn handle_show_auth_command(
    vt_cfg: Option<&VTCodeConfig>,
    provider: Option<&str>,
) -> Result<()> {
    println!("Authentication Status");
    println!();

    match provider.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case(CODEX_PROVIDER) => {
            let client = CodexAppServerClient::connect(vt_cfg).await?;
            render_codex_auth_status(
                client.account_read().await?,
                Some(client.mcp_server_status_list().await?.data.as_slice()),
            );
        }
        Some(value) if value.eq_ignore_ascii_case(COPILOT_PROVIDER) => {
            let workspace = current_auth_workspace();
            let auth_cfg = vt_cfg
                .map(|cfg| cfg.auth.copilot.clone())
                .unwrap_or_default();
            render_copilot_auth_status(probe_auth_status(&auth_cfg, Some(&workspace)).await);
        }
        Some(value) => match value.parse::<OAuthProvider>() {
            Ok(OAuthProvider::OpenRouter) => {
                render_openrouter_auth_status(openrouter_auth_status(vt_cfg)?)
            }
            Ok(OAuthProvider::OpenAi) => render_openai_auth_status(openai_auth_status(vt_cfg)?),
            Err(()) => {
                return Err(anyhow!(
                    "Authentication is not supported for provider '{}'. Supported providers: openai, openrouter, copilot, codex",
                    value
                ));
            }
        },
        None => {
            match CodexAppServerClient::connect(vt_cfg).await {
                Ok(client) => {
                    let mcp_statuses = client.mcp_server_status_list().await?;
                    render_codex_auth_status(
                        client.account_read().await?,
                        Some(mcp_statuses.data.as_slice()),
                    );
                }
                Err(err) if is_codex_cli_unavailable(&err) => {
                    render_codex_unavailable_status();
                }
                Err(err) => return Err(err),
            }
            println!();
            render_openrouter_auth_status(openrouter_auth_status(vt_cfg)?);
            println!();
            render_openai_auth_status(openai_auth_status(vt_cfg)?);
            println!();
            let workspace = current_auth_workspace();
            let auth_cfg = vt_cfg
                .map(|cfg| cfg.auth.copilot.clone())
                .unwrap_or_default();
            render_copilot_auth_status(probe_auth_status(&auth_cfg, Some(&workspace)).await);
            println!();
            println!("Use `vtcode login <provider>` to authenticate.");
            println!("Use `vtcode logout <provider>` to clear stored credentials.");
        }
    }

    Ok(())
}

fn credential_storage_mode(vt_cfg: Option<&VTCodeConfig>) -> AuthCredentialsStoreMode {
    vt_cfg
        .map(|cfg| cfg.agent.credential_storage_mode)
        .unwrap_or_default()
}

fn current_auth_workspace() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub(crate) fn openai_manual_placeholder(callback_port: u16) -> String {
    OPENAI_MANUAL_PLACEHOLDER.replace("1455", &callback_port.to_string())
}

fn print_openai_manual_guidance() {
    println!(
        "If the browser cannot reach localhost or you are on SSH/headless, paste the redirected URL or raw query string here."
    );
}

async fn complete_openai_login_with_cli_fallback(
    prepared: PreparedOpenAiLogin,
    started: Result<StartedOpenAiLogin>,
) -> Result<OpenAIChatGptSession> {
    match started {
        Ok(started) => match complete_openai_login(started).await {
            Ok(session) => Ok(session),
            Err(err) if should_prompt_manual_openai_input(&err) => {
                let Some(input) = prompt_openai_manual_input_cli_once()? else {
                    return Err(err);
                };
                complete_openai_login_from_manual_future(
                    prepared,
                    std::future::ready(Ok(Some(input))),
                )
                .await
            }
            Err(err) => Err(err),
        },
        Err(err) if should_prompt_manual_openai_input(&err) => {
            let Some(input) = prompt_openai_manual_input_cli_once()? else {
                return Err(err);
            };
            complete_openai_login_from_manual_future(prepared, std::future::ready(Ok(Some(input))))
                .await
        }
        Err(err) => Err(err),
    }
}

pub(crate) fn should_prompt_manual_openai_input(err: &anyhow::Error) -> bool {
    let message = err.to_string();
    message.contains("failed to bind localhost callback server") || message.contains("timed out")
}

fn prompt_openai_manual_input_cli_once() -> Result<Option<String>> {
    tokio::task::block_in_place(|| -> Result<Option<String>> {
        println!("Paste the redirected URL/query and press Enter when ready.");
        print!("Redirect URL or query: ");
        vtcode_core::ui::terminal::flush_stdout();
        let input = vtcode_core::ui::terminal::read_line()
            .map_err(|err| anyhow!("stdin read failed: {err}"))?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed.to_string()))
        }
    })
}

pub(crate) fn open_browser_or_print_url(url: &str) -> Result<()> {
    println!("Open this URL to continue:");
    println!(
        "{}{}{}",
        vtcode_commons::ansi_codes::hyperlink_open(url),
        url,
        vtcode_commons::ansi_codes::hyperlink_close(),
    );

    if !confirm_cli_browser_open(url)? {
        println!(
            "Automatic browser launch skipped. Open the URL manually if you want to continue."
        );
        return Ok(());
    }

    if let Err(err) = open_external_url(url) {
        eprintln!("warning: {}", err);
    }

    Ok(())
}

async fn wait_for_codex_account_login_completion(
    events: &mut tokio::sync::broadcast::Receiver<ServerEvent>,
    login_id: Option<&str>,
) -> Result<CodexAccountLoginCompleted> {
    loop {
        let event = match events.recv().await {
            Ok(event) => event,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                return Err(anyhow!(
                    "lost connection while waiting for Codex login completion"
                ));
            }
        };

        if event.method != "account/login/completed" {
            continue;
        }

        let completion: CodexAccountLoginCompleted =
            serde_json::from_value(event.params).context("invalid Codex login completion event")?;
        if login_id.is_none() || completion.login_id.as_deref() == login_id {
            return Ok(completion);
        }
    }
}

fn render_codex_auth_status(
    status: CodexAccountReadResponse,
    mcp_statuses: Option<&[CodexMcpServerStatus]>,
) {
    println!("Codex");
    match status.account {
        Some(CodexAccount::ApiKey) => {
            println!("  Auth: API key");
        }
        Some(CodexAccount::ChatGpt { email, plan_type }) => {
            println!("  Auth: ChatGPT");
            println!("  Account: {email}");
            println!("  Plan: {plan_type}");
        }
        None if status.requires_openai_auth => {
            println!("  Auth: not authenticated");
        }
        None => {
            println!("  Auth: not required");
        }
    }

    if let Some(mcp_statuses) = mcp_statuses
        && !mcp_statuses.is_empty()
    {
        println!("  MCP:");
        for server in mcp_statuses {
            println!("    {}: {}", server.name, server.auth_status);
        }
    }
}

fn render_codex_unavailable_status() {
    println!("Codex");
    println!("  Auth: CLI unavailable");
    println!("  Help: install `codex` or configure `[agent.codex_app_server].command`.");
}

fn print_codex_login_summary(status: &CodexAccountReadResponse) {
    println!("Codex authentication complete.");
    if let Some(CodexAccount::ChatGpt { email, plan_type }) = &status.account {
        println!("Account: {email}");
        println!("Plan: {plan_type}");
    }
}

fn print_existing_codex_auth_summary(status: &CodexAccountReadResponse) {
    match &status.account {
        Some(CodexAccount::ApiKey) => println!("Codex is already authenticated with an API key."),
        Some(CodexAccount::ChatGpt { email, plan_type }) => {
            println!("Codex is already authenticated.");
            println!("Account: {email}");
            println!("Plan: {plan_type}");
        }
        None => {}
    }
}

fn confirm_cli_browser_open(url: &str) -> Result<bool> {
    let Some(prompt) = UrlGuardPrompt::parse(url.to_string()) else {
        return Ok(false);
    };

    if vtcode_core::ui::terminal::is_piped_input() || vtcode_core::ui::terminal::is_piped_output() {
        println!(
            "Automatic browser launch skipped because approval requires an interactive terminal."
        );
        return Ok(false);
    }

    for line in prompt.cli_lines() {
        println!("{}", line);
    }

    print!("Open in browser now? [y/N]: ");
    vtcode_core::ui::terminal::flush_stdout();
    let input = vtcode_core::ui::terminal::read_line()
        .map_err(|err| anyhow!("stdin read failed: {err}"))?;
    Ok(cli_browser_open_approved(&input))
}

fn cli_browser_open_approved(input: &str) -> bool {
    matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn render_openrouter_auth_status(status: AuthStatus) {
    match status {
        AuthStatus::Authenticated {
            label,
            age_seconds,
            expires_in,
        } => {
            println!("OpenRouter: authenticated (OAuth)");
            if let Some(label) = label {
                println!("  Label: {}", label);
            }
            println!("  Token obtained: {}", format_auth_duration(age_seconds));
            if let Some(expires_in) = expires_in {
                println!("  Expires in: {}", format_auth_duration(expires_in));
            }
        }
        AuthStatus::NotAuthenticated => {
            if get_api_key(OPENROUTER_PROVIDER, &ApiKeySources::default()).is_ok() {
                println!("OpenRouter: using OPENROUTER_API_KEY");
            } else {
                println!("OpenRouter: not authenticated");
            }
        }
    }
}

fn render_openai_auth_status(status: OpenAIChatGptAuthStatus) {
    match status {
        OpenAIChatGptAuthStatus::Authenticated {
            label,
            age_seconds,
            expires_in,
        } => {
            println!("OpenAI: authenticated (ChatGPT)");
            if let Some(label) = label {
                println!("  Label: {}", label);
            }
            println!("  Session obtained: {}", format_auth_duration(age_seconds));
            if let Some(expires_in) = expires_in {
                println!("  Expires in: {}", format_auth_duration(expires_in));
            }
        }
        OpenAIChatGptAuthStatus::NotAuthenticated => {
            if get_api_key(OPENAI_PROVIDER, &ApiKeySources::default()).is_ok() {
                println!("OpenAI: using OPENAI_API_KEY");
            } else {
                println!("OpenAI: not authenticated");
            }
        }
    }
}

fn render_copilot_auth_status(status: CopilotAuthStatus) {
    match status.kind {
        CopilotAuthStatusKind::Authenticated => {
            println!("GitHub Copilot: authenticated (managed auth via Copilot CLI)")
        }
        CopilotAuthStatusKind::Unauthenticated => println!("GitHub Copilot: not authenticated"),
        CopilotAuthStatusKind::ServerUnavailable => println!("GitHub Copilot: CLI unavailable"),
        CopilotAuthStatusKind::AuthFlowFailed => println!("GitHub Copilot: auth flow failed"),
    }

    if let Some(message) = status.message.as_deref()
        && !message.trim().is_empty()
    {
        println!("  Details: {}", message);
    }

    if matches!(status.kind, CopilotAuthStatusKind::ServerUnavailable) {
        println!(
            "  Help: install `copilot` or configure `[auth.copilot].command`; see {}",
            COPILOT_AUTH_DOC_PATH
        );
    }
}

fn print_copilot_auth_event(event: CopilotAuthEvent) -> Result<()> {
    match event {
        CopilotAuthEvent::VerificationCode { url, user_code } => {
            println!("GitHub device code (copy this): {}", user_code);
            vtcode_commons::ansi_codes::notify_attention(
                true,
                Some(&format!("GitHub device code: {user_code}")),
            );
            println!("Open this URL to continue:");
            println!("{}", url);
        }
        CopilotAuthEvent::Progress { message } => println!("{}", message),
        CopilotAuthEvent::Success { account } => {
            if let Some(account) = account {
                println!("GitHub Copilot account: {}", account);
            }
        }
        CopilotAuthEvent::Failure { message } => {
            eprintln!("GitHub Copilot auth failed: {}", message)
        }
    }

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

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{
        clear_openai_login, clear_openrouter_login, cli_browser_open_approved,
        resolve_openai_authorization_code, wait_for_callback_or_cancel,
    };
    use crate::agent::runloop::unified::state::{CtrlCSignal, CtrlCState};
    use anyhow::anyhow;
    use serial_test::serial;
    use std::future;
    use std::sync::Arc;
    use tokio::sync::Notify;
    use vtcode_auth::{
        AuthCallbackOutcome, AuthCredentialsStoreMode, OpenAIAuthConfig, OpenAIChatGptSession,
        OpenAIResolvedAuth, OpenRouterToken, clear_oauth_token_with_mode,
        clear_openai_chatgpt_session, clear_openai_chatgpt_session_with_mode,
        load_oauth_token_with_mode, load_openai_chatgpt_session_with_mode, resolve_openai_auth,
        save_oauth_token_with_mode, save_openai_chatgpt_session_with_mode,
    };
    use vtcode_config::VTCodeConfig;

    fn config_with_storage_mode(mode: AuthCredentialsStoreMode) -> VTCodeConfig {
        let mut config = VTCodeConfig::default();
        config.agent.credential_storage_mode = mode;
        config
    }

    fn sample_openai_session(api_key: &str) -> OpenAIChatGptSession {
        OpenAIChatGptSession {
            openai_api_key: api_key.to_string(),
            id_token: "id-token".to_string(),
            access_token: "access-token".to_string(),
            refresh_token: "refresh-token".to_string(),
            account_id: Some("account-123".to_string()),
            email: Some("user@example.com".to_string()),
            plan: Some("plus".to_string()),
            obtained_at: 1,
            refreshed_at: 1,
            expires_at: Some(2),
        }
    }

    fn sample_openrouter_token(api_key: &str) -> OpenRouterToken {
        OpenRouterToken {
            api_key: api_key.to_string(),
            obtained_at: 1,
            expires_at: None,
            label: Some("test-token".to_string()),
        }
    }

    #[test]
    fn cli_browser_open_approved_accepts_yes_only() {
        assert!(cli_browser_open_approved("y"));
        assert!(cli_browser_open_approved("YES"));
        assert!(!cli_browser_open_approved("n"));
        assert!(!cli_browser_open_approved(""));
    }

    #[tokio::test]
    async fn openai_code_resolution_accepts_manual_query_input() {
        let code = resolve_openai_authorization_code(
            future::pending::<Result<AuthCallbackOutcome, anyhow::Error>>(),
            "test-state",
            Some(future::ready(Ok(Some(
                "code=manual-code&state=test-state".to_string(),
            )))),
        )
        .await
        .expect("manual input should resolve");
        assert_eq!(code, "manual-code");
    }

    #[tokio::test]
    async fn openai_code_resolution_falls_back_to_manual_when_callback_fails() {
        let code = resolve_openai_authorization_code(
            future::ready(Err(anyhow!("failed to bind localhost callback server"))),
            "test-state",
            Some(future::ready(Ok(Some(
                "http://localhost:1455/auth/callback?code=manual-code&state=test-state".to_string(),
            )))),
        )
        .await
        .expect("manual fallback should resolve");
        assert_eq!(code, "manual-code");
    }

    #[tokio::test]
    async fn openai_code_resolution_rejects_manual_bare_code() {
        let error = resolve_openai_authorization_code(
            future::ready(Err(anyhow!("OAuth flow timed out after 300 seconds"))),
            "test-state",
            Some(future::ready(Ok(Some("manual-code".to_string())))),
        )
        .await
        .expect_err("bare code should fail");
        assert!(
            error
                .to_string()
                .contains("full redirect url or query string")
        );
    }

    #[tokio::test]
    async fn openai_code_resolution_uses_callback_when_manual_input_is_invalid() {
        let code = resolve_openai_authorization_code(
            future::ready(Ok(AuthCallbackOutcome::Code("callback-code".to_string()))),
            "test-state",
            Some(future::ready(Ok(Some("manual-code".to_string())))),
        )
        .await
        .expect("callback code should win after invalid manual input");
        assert_eq!(code, "callback-code");
    }

    #[tokio::test]
    async fn callback_wait_returns_cancelled_when_ctrl_c_is_pending() {
        let ctrl_c_state = Arc::new(CtrlCState::new());
        assert!(matches!(
            ctrl_c_state.register_signal(),
            CtrlCSignal::Cancel
        ));
        let ctrl_c_notify = Arc::new(Notify::new());

        let outcome = wait_for_callback_or_cancel(
            future::pending::<Result<AuthCallbackOutcome, anyhow::Error>>(),
            &ctrl_c_state,
            &ctrl_c_notify,
        )
        .await
        .expect("cancelled callback wait");

        assert!(matches!(outcome, AuthCallbackOutcome::Cancelled));
        assert!(!ctrl_c_state.is_cancel_requested());
    }

    #[test]
    #[serial]
    fn openai_logout_clears_all_sessions_and_falls_back_to_api_key() {
        let _ = clear_openai_chatgpt_session();
        let _ = clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File);
        let _ = clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::Keyring);

        let file_session = sample_openai_session("file-api-key");
        let keyring_session = sample_openai_session("keyring-api-key");
        if save_openai_chatgpt_session_with_mode(&file_session, AuthCredentialsStoreMode::File)
            .is_err()
        {
            return;
        }

        if save_openai_chatgpt_session_with_mode(
            &keyring_session,
            AuthCredentialsStoreMode::Keyring,
        )
        .is_err()
        {
            let _ = clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File);
            return;
        }

        let config = config_with_storage_mode(AuthCredentialsStoreMode::File);
        clear_openai_login(Some(&config)).expect("clear openai login");

        assert!(
            load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
                .expect("load file session")
                .is_none()
        );
        assert!(
            load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::Keyring)
                .expect("load keyring session")
                .is_none()
        );
        assert!(matches!(
            resolve_openai_auth(
                &OpenAIAuthConfig::default(),
                AuthCredentialsStoreMode::File,
                Some("env-api-key".to_string())
            )
            .expect("resolve auth after logout"),
            OpenAIResolvedAuth::ApiKey { api_key } if api_key == "env-api-key"
        ));

        let _ = clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File);
        let _ = clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::Keyring);
    }

    #[test]
    #[serial]
    fn openrouter_logout_clears_only_configured_storage_mode() {
        let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::File);
        let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::Keyring);

        let file_token = sample_openrouter_token("file-token");
        let keyring_token = sample_openrouter_token("keyring-token");
        if save_oauth_token_with_mode(&file_token, AuthCredentialsStoreMode::File).is_err() {
            return;
        }

        if save_oauth_token_with_mode(&keyring_token, AuthCredentialsStoreMode::Keyring).is_err() {
            let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::File);
            return;
        }

        let Some(loaded_keyring_token) =
            load_oauth_token_with_mode(AuthCredentialsStoreMode::Keyring)
                .expect("load keyring token after save")
        else {
            let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::File);
            let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::Keyring);
            return;
        };
        if loaded_keyring_token.api_key != "keyring-token" {
            let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::File);
            let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::Keyring);
            return;
        }

        let config = config_with_storage_mode(AuthCredentialsStoreMode::File);
        clear_openrouter_login(Some(&config)).expect("clear openrouter login");

        assert!(
            load_oauth_token_with_mode(AuthCredentialsStoreMode::File)
                .expect("load file token")
                .is_none()
        );
        assert_eq!(
            load_oauth_token_with_mode(AuthCredentialsStoreMode::Keyring)
                .expect("load keyring token")
                .expect("keyring token should remain")
                .api_key,
            "keyring-token"
        );

        let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::File);
        let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::Keyring);
    }
}
