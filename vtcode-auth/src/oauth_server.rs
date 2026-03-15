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
    provider: OAuthProvider,
}

impl OAuthCallbackPage {
    #[must_use]
    pub fn new(provider: OAuthProvider) -> Self {
        Self { provider }
    }
}

#[derive(Debug, Clone)]
pub enum AuthCallbackOutcome {
    Code(String),
    Cancelled,
    Error(String),
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

pub async fn run_auth_code_callback_server(
    port: u16,
    timeout_secs: u64,
    page: OAuthCallbackPage,
    expected_state: Option<String>,
) -> Result<AuthCallbackOutcome> {
    let timeout = if timeout_secs == 0 {
        DEFAULT_CALLBACK_TIMEOUT_SECS
    } else {
        timeout_secs
    };
    let (result_tx, mut result_rx) = mpsc::channel::<AuthCallbackOutcome>(1);
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

    let result = tokio::select! {
        Some(result) = result_rx.recv() => result,
        _ = tokio::time::sleep(std::time::Duration::from_secs(timeout)) => {
            AuthCallbackOutcome::Error(format!("OAuth flow timed out after {timeout} seconds"))
        }
    };

    let _ = shutdown_tx.send(());
    let _ = server_handle.await;
    Ok(result)
}

async fn handle_callback(
    State(state): State<Arc<AuthCallbackState>>,
    Query(params): Query<AuthCallbackParams>,
) -> Html<String> {
    if let Some(expected_state) = state.expected_state.as_deref() {
        match params.state.as_deref() {
            Some(actual_state) if actual_state == expected_state => {}
            _ => {
                let message = "OAuth error: state mismatch".to_string();
                let _ = state
                    .result_tx
                    .send(AuthCallbackOutcome::Error(message.clone()))
                    .await;
                return Html(error_html(state.page.provider, &message));
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
        return Html(error_html(state.page.provider, &message));
    }

    let Some(code) = params.code else {
        let message = "Missing authorization code".to_string();
        let _ = state
            .result_tx
            .send(AuthCallbackOutcome::Error(message.clone()))
            .await;
        return Html(error_html(state.page.provider, &message));
    };

    let _ = state.result_tx.send(AuthCallbackOutcome::Code(code)).await;
    Html(success_html(state.page.provider))
}

async fn handle_cancel(State(state): State<Arc<AuthCallbackState>>) -> Html<String> {
    let _ = state.result_tx.send(AuthCallbackOutcome::Cancelled).await;
    Html(cancelled_html(state.page.provider))
}

fn success_html(provider: OAuthProvider) -> String {
    base_html(
        "Authentication Successful",
        provider.subtitle(),
        Some("You may now close this window and return to VT Code."),
        "✓",
        "#22c55e",
        None,
    )
}

fn error_html(provider: OAuthProvider, error: &str) -> String {
    base_html(
        "Authentication Failed",
        provider.failure_subtitle(),
        None,
        "✕",
        "#ef4444",
        Some(error),
    )
}

fn cancelled_html(provider: OAuthProvider) -> String {
    base_html(
        "Authentication Cancelled",
        &provider.retry_hint(),
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
        .map(|value| {
            format!(
                r#"<div class="error">{}</div>"#,
                html_escape(value)
            )
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Query, State};

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
        let html = success_html(OAuthProvider::OpenAi);
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
}
