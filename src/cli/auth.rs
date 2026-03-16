use anyhow::{Result, anyhow};
use std::future::Future;
use vtcode_auth::{
    AuthCallbackOutcome, AuthCredentialsStoreMode, AuthStatus, OAuthCallbackPage, OAuthProvider,
    OpenAIChatGptAuthStatus, OpenAIChatGptSession, OpenRouterToken, PkceChallenge,
    clear_oauth_token_with_mode, clear_openai_chatgpt_session_with_mode, exchange_code_for_token,
    exchange_openai_chatgpt_code_for_tokens, generate_openai_oauth_state, generate_pkce_challenge,
    get_auth_status_with_mode, get_auth_url, get_openai_chatgpt_auth_status_with_mode,
    get_openai_chatgpt_auth_url, load_openai_chatgpt_session_with_mode,
    parse_openai_chatgpt_manual_callback_input, run_auth_code_callback_server,
    save_oauth_token_with_mode, save_openai_chatgpt_session_with_mode,
};
use vtcode_config::VTCodeConfig;
use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};

pub(crate) const OPENAI_PROVIDER: &str = "openai";
pub(crate) const OPENROUTER_PROVIDER: &str = "openrouter";
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

pub(crate) fn supports_oauth_provider(provider: &str) -> bool {
    provider.parse::<OAuthProvider>().is_ok()
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

pub(crate) async fn complete_openrouter_login(prepared: PreparedOpenRouterLogin) -> Result<String> {
    match run_auth_code_callback_server(
        prepared.callback_port,
        prepared.timeout_secs,
        OAuthCallbackPage::new(OAuthProvider::OpenRouter),
        None,
    )
    .await?
    {
        AuthCallbackOutcome::Code(code) => {
            let api_key = exchange_code_for_token(&code, &prepared.pkce).await?;
            let token = OpenRouterToken {
                api_key: api_key.clone(),
                obtained_at: now_secs(),
                expires_at: None,
                label: Some("VT Code OAuth".to_string()),
            };
            save_oauth_token_with_mode(&token, prepared.storage_mode)?;
            Ok(api_key)
        }
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
    prepared: PreparedOpenAiLogin,
) -> Result<OpenAIChatGptSession> {
    complete_openai_login_with_manual_future(
        prepared,
        None::<std::future::Ready<Result<Option<String>>>>,
    )
    .await
}

pub(crate) async fn complete_openai_login_with_manual_future<ManualFut>(
    prepared: PreparedOpenAiLogin,
    manual_input: Option<ManualFut>,
) -> Result<OpenAIChatGptSession>
where
    ManualFut: Future<Output = Result<Option<String>>>,
{
    let code = resolve_openai_authorization_code(
        run_auth_code_callback_server(
            prepared.callback_port,
            prepared.timeout_secs,
            OAuthCallbackPage::new(OAuthProvider::OpenAi),
            Some(prepared.state.clone()),
        ),
        &prepared.state,
        manual_input,
    )
    .await?;
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

enum OpenAiAuthRace {
    Callback(Result<AuthCallbackOutcome>),
    Manual(Result<Option<String>>),
}

fn callback_outcome_to_openai_code(outcome: AuthCallbackOutcome) -> Result<String> {
    match outcome {
        AuthCallbackOutcome::Code(code) => Ok(code),
        AuthCallbackOutcome::Cancelled => Err(anyhow!("OAuth flow was cancelled")),
        AuthCallbackOutcome::Error(error) => Err(anyhow!(error)),
    }
}

pub(crate) fn clear_openrouter_login(vt_cfg: Option<&VTCodeConfig>) -> Result<()> {
    clear_oauth_token_with_mode(credential_storage_mode(vt_cfg))
}

pub(crate) fn clear_openai_login(vt_cfg: Option<&VTCodeConfig>) -> Result<()> {
    clear_openai_chatgpt_session_with_mode(credential_storage_mode(vt_cfg))
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
    match provider.parse::<OAuthProvider>() {
        Ok(OAuthProvider::OpenRouter) => {
            let prepared = prepare_openrouter_login(vt_cfg)?;
            println!("Starting OpenRouter OAuth authentication...");
            open_browser_or_print_url(&prepared.auth_url);
            let api_key = complete_openrouter_login(prepared).await?;
            println!("OpenRouter authentication complete.");
            println!(
                "Stored secure OAuth token. Key preview: {}...",
                &api_key[..api_key.len().min(8)]
            );
            Ok(())
        }
        Ok(OAuthProvider::OpenAi) => {
            let prepared = prepare_openai_login(vt_cfg)?;
            println!("Starting OpenAI ChatGPT authentication...");
            open_browser_or_print_url(&prepared.auth_url);
            print_openai_manual_guidance();
            let session = complete_openai_login_with_cli_fallback(prepared).await?;
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
            "OAuth authentication is not supported for provider '{}'. Supported providers: openai, openrouter",
            provider
        )),
    }
}

pub(crate) fn handle_logout_command(vt_cfg: Option<&VTCodeConfig>, provider: &str) -> Result<()> {
    match provider.parse::<OAuthProvider>() {
        Ok(OAuthProvider::OpenRouter) => {
            clear_openrouter_login(vt_cfg)?;
            println!("OpenRouter OAuth token cleared.");
            Ok(())
        }
        Ok(OAuthProvider::OpenAi) => {
            clear_openai_login(vt_cfg)?;
            println!("OpenAI ChatGPT session cleared.");
            Ok(())
        }
        Err(()) => Err(anyhow!(
            "OAuth authentication is not supported for provider '{}'. Supported providers: openai, openrouter",
            provider
        )),
    }
}

pub(crate) fn handle_show_auth_command(
    vt_cfg: Option<&VTCodeConfig>,
    provider: Option<&str>,
) -> Result<()> {
    println!("Authentication Status");
    println!();

    match provider.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => match value.parse::<OAuthProvider>() {
            Ok(OAuthProvider::OpenRouter) => {
                render_openrouter_auth_status(openrouter_auth_status(vt_cfg)?)
            }
            Ok(OAuthProvider::OpenAi) => render_openai_auth_status(openai_auth_status(vt_cfg)?),
            Err(()) => {
                return Err(anyhow!(
                    "OAuth authentication is not supported for provider '{}'. Supported providers: openai, openrouter",
                    value
                ));
            }
        },
        None => {
            render_openrouter_auth_status(openrouter_auth_status(vt_cfg)?);
            println!();
            render_openai_auth_status(openai_auth_status(vt_cfg)?);
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
) -> Result<OpenAIChatGptSession> {
    match complete_openai_login(prepared.clone()).await {
        Ok(session) => Ok(session),
        Err(err) if should_prompt_manual_openai_input(&err) => {
            let Some(input) = prompt_openai_manual_input_cli_once()? else {
                return Err(err);
            };
            let code = parse_openai_chatgpt_manual_callback_input(&input, &prepared.state)?;
            persist_openai_login_code(prepared, &code).await
        }
        Err(err) => Err(err),
    }
}

fn should_prompt_manual_openai_input(err: &anyhow::Error) -> bool {
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

fn open_browser_or_print_url(url: &str) {
    println!("Open this URL to continue:");
    println!("{url}");
    if let Err(err) = webbrowser::open(url) {
        eprintln!("warning: failed to open browser automatically: {}", err);
    }
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
    use super::{clear_openai_login, clear_openrouter_login, resolve_openai_authorization_code};
    use anyhow::anyhow;
    use serial_test::serial;
    use std::future;
    use vtcode_auth::{
        AuthCallbackOutcome, AuthCredentialsStoreMode, OpenAIChatGptSession, OpenRouterToken,
        clear_oauth_token_with_mode, clear_openai_chatgpt_session_with_mode,
        load_oauth_token_with_mode, load_openai_chatgpt_session_with_mode,
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
            expires_at: Some(2),
            label: Some("test-token".to_string()),
        }
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

    #[test]
    #[serial]
    fn openai_logout_clears_only_configured_storage_mode() {
        let _ = clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File);
        let _ = clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::Keyring);

        let file_session = sample_openai_session("file-api-key");
        let keyring_session = sample_openai_session("keyring-api-key");
        save_openai_chatgpt_session_with_mode(&file_session, AuthCredentialsStoreMode::File)
            .expect("save file session");

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

        assert_eq!(
            load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
                .expect("load file session")
                .expect("keyring session should remain as file-mode fallback")
                .openai_api_key,
            "keyring-api-key"
        );
        assert_eq!(
            load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::Keyring)
                .expect("load keyring session")
                .expect("keyring session should remain")
                .openai_api_key,
            "keyring-api-key"
        );

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
        save_oauth_token_with_mode(&file_token, AuthCredentialsStoreMode::File)
            .expect("save file token");

        if save_oauth_token_with_mode(&keyring_token, AuthCredentialsStoreMode::Keyring).is_err() {
            let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::File);
            return;
        }

        let config = config_with_storage_mode(AuthCredentialsStoreMode::File);
        clear_openrouter_login(Some(&config)).expect("clear openrouter login");

        assert_eq!(
            load_oauth_token_with_mode(AuthCredentialsStoreMode::File)
                .expect("load file token")
                .expect("keyring token should remain as file-mode fallback")
                .api_key,
            "keyring-token"
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
