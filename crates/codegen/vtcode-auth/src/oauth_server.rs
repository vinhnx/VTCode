use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{Query, State},
    response::Html,
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

const DEFAULT_CALLBACK_TIMEOUT_SECS: u64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OAuthProvider {
    OpenAi,
    OpenRouter,
}

impl OAuthProvider {
    #[must_use]
    pub fn slug(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::OpenRouter => "openrouter",
        }
    }

    #[must_use]
    pub fn display_name(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::OpenRouter => "OpenRouter",
        }
    }

    #[must_use]
    pub fn subtitle(self) -> &'static str {
        match self {
            Self::OpenAi => "Your ChatGPT subscription is now connected.",
            Self::OpenRouter => "Your OpenRouter account is now connected.",
        }
    }

    #[must_use]
    pub fn failure_subtitle(self) -> &'static str {
        match self {
            Self::OpenAi => "Unable to connect your ChatGPT subscription.",
            Self::OpenRouter => "Unable to connect your OpenRouter account.",
        }
    }

    #[must_use]
    pub fn retry_hint(self) -> String {
        format!("You can try again anytime using /login {}", self.slug())
    }

    #[must_use]
    pub fn supports_manual_refresh(self) -> bool {
        matches!(self, Self::OpenAi)
    }
}

impl fmt::Display for OAuthProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

impl FromStr for OAuthProvider {
    type Err = ();

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "openai" => Ok(Self::OpenAi),
            "openrouter" => Ok(Self::OpenRouter),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OAuthCallbackPage {
    provider_slug: &'static str,
    success_subtitle: &'static str,
    failure_subtitle: &'static str,
    retry_hint: &'static str,
}

impl OAuthCallbackPage {
    #[must_use]
    pub fn new(provider: OAuthProvider) -> Self {
        match provider {
            OAuthProvider::OpenAi => Self {
                provider_slug: "openai",
                success_subtitle: "Your ChatGPT subscription is now connected.",
                failure_subtitle: "Unable to connect your ChatGPT subscription.",
                retry_hint: "You can try again anytime using /login openai",
            },
            OAuthProvider::OpenRouter => Self {
                provider_slug: "openrouter",
                success_subtitle: "Your OpenRouter account is now connected.",
                failure_subtitle: "Unable to connect your OpenRouter account.",
                retry_hint: "You can try again anytime using /login openrouter",
            },
        }
    }

    #[must_use]
    pub fn custom(
        provider_slug: &'static str,
        success_subtitle: &'static str,
        failure_subtitle: &'static str,
        retry_hint: &'static str,
    ) -> Self {
        Self {
            provider_slug,
            success_subtitle,
            failure_subtitle,
            retry_hint,
        }
    }

    #[must_use]
    pub fn provider_slug(&self) -> &'static str {
        self.provider_slug
    }

    #[must_use]
    pub fn success_subtitle(&self) -> &'static str {
        self.success_subtitle
    }

    #[must_use]
    pub fn failure_subtitle(&self) -> &'static str {
        self.failure_subtitle
    }

    #[must_use]
    pub fn retry_hint(&self) -> &'static str {
        self.retry_hint
    }
}

#[derive(Debug, Clone)]
pub enum AuthCallbackOutcome {
    Code(String),
    Cancelled,
    Error(String),
}

pub struct AuthCodeCallbackServer {
    timeout: Duration,
    result_rx: mpsc::Receiver<AuthCallbackOutcome>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    server_handle: Option<tokio::task::JoinHandle<()>>,
}

impl AuthCodeCallbackServer {
    pub async fn start(
        port: u16,
        timeout_secs: u64,
        page: OAuthCallbackPage,
        expected_state: Option<String>,
    ) -> Result<Self> {
        let (result_tx, result_rx) = mpsc::channel::<AuthCallbackOutcome>(1);
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let state = Arc::new(AuthCallbackState {
            page,
            expected_state,
            result_tx,
        });

        let app = Router::new()
            .route("/callback", get(handle_callback))
            .route("/auth/callback", get(handle_callback))
            .route("/cancel", get(handle_cancel))
            .route("/health", get(|| async { "OK" }))
            .with_state(state);

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("failed to bind localhost callback server on port {port}"))?;

        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });
        let server_handle = tokio::spawn(async move {
            if let Err(err) = server.await {
                tracing::error!("OAuth callback server error: {}", err);
            }
        });

        Ok(Self {
            timeout: callback_timeout(timeout_secs),
            result_rx,
            shutdown_tx: Some(shutdown_tx),
            server_handle: Some(server_handle),
        })
    }

    pub async fn wait(mut self) -> Result<AuthCallbackOutcome> {
        let result = tokio::select! {
            Some(result) = self.result_rx.recv() => result,
            _ = tokio::time::sleep(self.timeout) => {
                AuthCallbackOutcome::Error(format!(
                    "OAuth flow timed out after {} seconds",
                    self.timeout.as_secs()
                ))
            }
        };

        self.shutdown().await;
        Ok(result)
    }

    async fn shutdown(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        if let Some(server_handle) = self.server_handle.take() {
            let _ = server_handle.await;
        }
    }
}

impl Drop for AuthCodeCallbackServer {
    fn drop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        if let Some(server_handle) = self.server_handle.take() {
            server_handle.abort();
        }
    }
}

#[derive(Debug, Deserialize)]
struct AuthCallbackParams {
    code: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
    state: Option<String>,
}

struct AuthCallbackState {
    page: OAuthCallbackPage,
    expected_state: Option<String>,
    result_tx: mpsc::Sender<AuthCallbackOutcome>,
}

pub async fn start_auth_code_callback_server(
    port: u16,
    timeout_secs: u64,
    page: OAuthCallbackPage,
    expected_state: Option<String>,
) -> Result<AuthCodeCallbackServer> {
    AuthCodeCallbackServer::start(port, timeout_secs, page, expected_state).await
}

pub async fn run_auth_code_callback_server(
    port: u16,
    timeout_secs: u64,
    page: OAuthCallbackPage,
    expected_state: Option<String>,
) -> Result<AuthCallbackOutcome> {
    start_auth_code_callback_server(port, timeout_secs, page, expected_state)
        .await?
        .wait()
        .await
}

async fn handle_callback(
    State(state): State<Arc<AuthCallbackState>>,
    Query(params): Query<AuthCallbackParams>,
) -> Html<String> {
    tracing::info!(
        provider = state.page.provider_slug(),
        has_code = params.code.is_some(),
        has_error = params.error.is_some(),
        "received oauth callback"
    );
    if let Some(expected_state) = state.expected_state.as_deref() {
        match params.state.as_deref() {
            Some(actual_state) if actual_state == expected_state => {}
            _ => {
                let message = "OAuth error: state mismatch".to_string();
                let _ = state
                    .result_tx
                    .send(AuthCallbackOutcome::Error(message.clone()))
                    .await;
                return Html(error_html(state.page, &message));
            }
        }
    }

    if let Some(error) = params.error {
        let message = match params.error_description {
            Some(description) if !description.trim().is_empty() => {
                format!("OAuth error: {error} - {description}")
            }
            _ => format!("OAuth error: {error}"),
        };
        let _ = state
            .result_tx
            .send(AuthCallbackOutcome::Error(message.clone()))
            .await;
        return Html(error_html(state.page, &message));
    }

    let Some(code) = params.code else {
        let message = "Missing authorization code".to_string();
        let _ = state
            .result_tx
            .send(AuthCallbackOutcome::Error(message.clone()))
            .await;
        return Html(error_html(state.page, &message));
    };

    let _ = state.result_tx.send(AuthCallbackOutcome::Code(code)).await;
    Html(success_html(state.page))
}

async fn handle_cancel(State(state): State<Arc<AuthCallbackState>>) -> Html<String> {
    let _ = state.result_tx.send(AuthCallbackOutcome::Cancelled).await;
    Html(cancelled_html(state.page))
}

fn success_html(page: OAuthCallbackPage) -> String {
    base_html(
        "Authentication Successful",
        page.success_subtitle(),
        Some("You may now close this window and return to VT Code."),
        "✓",
        "#22c55e",
        None,
    )
}

fn error_html(page: OAuthCallbackPage, error: &str) -> String {
    base_html(
        "Authentication Failed",
        page.failure_subtitle(),
        None,
        "✕",
        "#ef4444",
        Some(error),
    )
}

fn cancelled_html(page: OAuthCallbackPage) -> String {
    base_html(
        "Authentication Cancelled",
        page.retry_hint(),
        None,
        "—",
        "#71717a",
        None,
    )
}

fn base_html(
    title: &str,
    subtitle: &str,
    close_note: Option<&str>,
    icon: &str,
    accent: &str,
    error: Option<&str>,
) -> String {
    let close_note_html = close_note
        .map(|value| format!(r#"<p class="close-note">{}</p>"#, html_escape(value)))
        .unwrap_or_default();
    let error_html = error
        .map(|value| format!(r#"<div class="error">{}</div>"#, html_escape(value)))
        .unwrap_or_default();
    let auto_close = if close_note.is_some() {
        r#"<script>setTimeout(() => window.close(), 3000);</script>"#
    } else {
        ""
    };

    format!(
        r##"<!DOCTYPE html>
<html>
<head>
    <title>VT Code - {title}</title>
    <style>
        @font-face {{
            font-family: 'SF Pro Display';
            src: local('SF Pro Display'), local('.SF NS Display'), local('Helvetica Neue');
        }}
        @font-face {{
            font-family: 'SF Mono';
            src: local('SF Mono'), local('Menlo'), local('Monaco');
        }}
        :root {{
            color-scheme: dark;
            --bg: #0a0a0a;
            --panel: #111111;
            --panel-border: #262626;
            --text: #fafafa;
            --muted: #a1a1aa;
            --subtle: #52525b;
            --code-bg: #18181b;
            --code-border: #27272a;
            --accent: {accent};
        }}
        * {{ box-sizing: border-box; }}
        body {{
            font-family: 'SF Pro Display', -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            min-height: 100vh;
            margin: 0;
            background:
                radial-gradient(circle at top, rgba(255,255,255,0.04), transparent 32%),
                linear-gradient(180deg, var(--bg), #050505);
            color: var(--text);
            padding: 24px;
        }}
        .container {{
            text-align: center;
            padding: 2.75rem 3rem;
            border: 1px solid var(--panel-border);
            border-radius: 14px;
            background: rgba(17, 17, 17, 0.92);
            max-width: 460px;
            width: 100%;
            box-shadow: 0 30px 90px rgba(0, 0, 0, 0.35);
        }}
        .logo {{
            margin-bottom: 1.5rem;
        }}
        .logo-mark {{
            display: inline-flex;
            align-items: center;
            justify-content: center;
            font-size: 0.95rem;
            letter-spacing: 0.24em;
            text-transform: uppercase;
            color: var(--muted);
        }}
        .status-icon {{
            width: 52px;
            height: 52px;
            margin: 0 auto 1.25rem;
            border: 2px solid var(--accent);
            border-radius: 50%;
            display: flex;
            align-items: center;
            justify-content: center;
            font-size: 1.25rem;
            color: var(--accent);
        }}
        h1 {{
            margin: 0 0 0.75rem 0;
            font-size: 1.25rem;
            font-weight: 600;
            letter-spacing: -0.02em;
        }}
        p {{
            color: var(--muted);
            margin: 0;
            font-size: 0.92rem;
            line-height: 1.55;
        }}
        .close-note {{
            margin-top: 1.25rem;
            font-size: 0.78rem;
            color: var(--subtle);
        }}
        .error {{
            margin-top: 1.35rem;
            padding: 0.95rem 1rem;
            background: var(--code-bg);
            border: 1px solid var(--code-border);
            border-radius: 10px;
            font-family: 'SF Mono', Menlo, Monaco, monospace;
            font-size: 0.75rem;
            color: #d4d4d8;
            word-break: break-word;
            text-align: left;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="logo">
            <div class="logo-mark">&gt; VT Code</div>
        </div>
        <div class="status-icon">{icon}</div>
        <h1>{title}</h1>
        <p>{subtitle}</p>
        {close_note_html}
        {error_html}
    </div>
    {auto_close}
</body>
</html>"##,
        title = html_escape(title),
        subtitle = html_escape(subtitle),
        icon = icon,
        accent = accent,
        close_note_html = close_note_html,
        error_html = error_html,
        auto_close = auto_close,
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn callback_timeout(timeout_secs: u64) -> Duration {
    Duration::from_secs(if timeout_secs == 0 {
        DEFAULT_CALLBACK_TIMEOUT_SECS
    } else {
        timeout_secs
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Query, State};
    use reqwest::Client;

    #[test]
    fn oauth_provider_parses_known_providers() {
        assert_eq!("openai".parse::<OAuthProvider>(), Ok(OAuthProvider::OpenAi));
        assert_eq!(
            "openrouter".parse::<OAuthProvider>(),
            Ok(OAuthProvider::OpenRouter)
        );
        assert!("other".parse::<OAuthProvider>().is_err());
    }

    #[test]
    fn success_html_mentions_vtcode_and_autoclose() {
        let html = success_html(OAuthCallbackPage::new(OAuthProvider::OpenAi));
        assert!(html.contains("VT Code"));
        assert!(html.contains("Authentication Successful"));
        assert!(html.contains("window.close"));
    }

    #[tokio::test]
    async fn callback_rejects_state_mismatch() {
        let (result_tx, mut result_rx) = mpsc::channel(1);
        let state = Arc::new(AuthCallbackState {
            page: OAuthCallbackPage::new(OAuthProvider::OpenAi),
            expected_state: Some("expected-state".to_string()),
            result_tx,
        });

        let html = handle_callback(
            State(state),
            Query(AuthCallbackParams {
                code: Some("auth-code".to_string()),
                error: None,
                error_description: None,
                state: Some("wrong-state".to_string()),
            }),
        )
        .await;

        let outcome = result_rx.recv().await.expect("callback outcome");
        match outcome {
            AuthCallbackOutcome::Error(message) => {
                assert!(message.contains("state mismatch"));
            }
            _ => panic!("expected error outcome"),
        }
        assert!(html.0.contains("Authentication Failed"));
    }

    #[tokio::test]
    async fn callback_server_starts_listening_before_wait() {
        let listener = match std::net::TcpListener::bind(("127.0.0.1", 0)) {
            Ok(listener) => listener,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(err) => panic!("bind temp port: {err}"),
        };
        let port = listener.local_addr().expect("local addr").port();
        drop(listener);

        let server = start_auth_code_callback_server(
            port,
            5,
            OAuthCallbackPage::new(OAuthProvider::OpenAi),
            None,
        )
        .await
        .expect("start callback server");
        let client = Client::builder()
            .no_proxy()
            .build()
            .expect("build http client");

        let health = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await
            .expect("health request should succeed");
        assert!(health.status().is_success());

        let cancel = client
            .get(format!("http://127.0.0.1:{port}/cancel"))
            .send()
            .await
            .expect("cancel request should succeed");
        assert!(cancel.status().is_success());

        assert!(matches!(
            server.wait().await.expect("wait for callback outcome"),
            AuthCallbackOutcome::Cancelled
        ));
    }
}
