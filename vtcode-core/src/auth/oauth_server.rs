//! OAuth callback server for handling OpenRouter PKCE flow.
//!
//! This module provides a one-shot HTTP server that listens for OAuth callbacks,
//! exchanges the authorization code for an API key, and then shuts down.

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{Query, State},
    response::Html,
    routing::get,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

use vtcode_config::auth::{
    OpenRouterToken, PkceChallenge, exchange_code_for_token, save_oauth_token,
};

/// Default timeout for waiting for OAuth callback (5 minutes)
const DEFAULT_CALLBACK_TIMEOUT_SECS: u64 = 300;

/// Result of the OAuth flow.
#[derive(Debug, Clone)]
pub enum OAuthResult {
    /// Successfully obtained token
    Success(String),
    /// User cancelled or closed browser
    Cancelled,
    /// Error during OAuth flow
    Error(String),
}

/// State shared between server handler and main flow.
struct OAuthServerState {
    challenge: PkceChallenge,
    result_tx: mpsc::Sender<OAuthResult>,
}

/// Start the OAuth callback server and wait for the callback.
///
/// This function:
/// 1. Starts a local HTTP server on the specified port
/// 2. Waits for the OAuth callback with the authorization code
/// 3. Exchanges the code for an API key
/// 4. Saves the token and returns the result
///
/// # Arguments
/// * `challenge` - PKCE challenge used for the authorization request
/// * `port` - Port to listen on (default: 8484)
/// * `timeout_secs` - Timeout in seconds (default: 300)
///
/// # Returns
/// The OAuth result indicating success, cancellation, or error.
pub async fn run_oauth_callback_server(
    challenge: PkceChallenge,
    port: u16,
    timeout_secs: Option<u64>,
) -> Result<OAuthResult> {
    let timeout = timeout_secs.unwrap_or(DEFAULT_CALLBACK_TIMEOUT_SECS);

    // Channel to send result from handler to main flow
    let (result_tx, mut result_rx) = mpsc::channel::<OAuthResult>(1);

    // Shutdown signal
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let state = Arc::new(OAuthServerState {
        challenge,
        result_tx,
    });

    // Define routes
    let app = Router::new()
        .route("/callback", get(handle_callback))
        .route("/cancel", get(handle_cancel))
        .route("/health", get(|| async { "OK" }))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    tracing::info!("Starting OAuth callback server on http://{}", addr);

    // Start server with graceful shutdown
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind to port {}", port))?;

    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = shutdown_rx.await;
        tracing::debug!("OAuth server shutting down");
    });

    // Run server in background
    let server_handle = tokio::spawn(async move {
        if let Err(e) = server.await {
            tracing::error!("OAuth server error: {}", e);
        }
    });

    // Wait for result or timeout
    let result = tokio::select! {
        Some(result) = result_rx.recv() => {
            result
        }
        _ = tokio::time::sleep(std::time::Duration::from_secs(timeout)) => {
            OAuthResult::Error(format!("OAuth flow timed out after {} seconds", timeout))
        }
    };

    // Signal shutdown
    let _ = shutdown_tx.send(());

    // Wait for server to stop
    let _ = server_handle.await;

    Ok(result)
}

/// Query parameters from OAuth callback.
#[derive(Debug, serde::Deserialize)]
struct CallbackParams {
    code: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Handle the OAuth callback.
async fn handle_callback(
    State(state): State<Arc<OAuthServerState>>,
    Query(params): Query<CallbackParams>,
) -> Html<String> {
    // Check for errors first
    if let Some(error) = params.error {
        let description = params.error_description.unwrap_or_default();
        let error_msg = format!("OAuth error: {} - {}", error, description);
        tracing::error!("{}", error_msg);

        let _ = state
            .result_tx
            .send(OAuthResult::Error(error_msg.clone()))
            .await;

        return Html(error_html(&error_msg));
    }

    // Get the authorization code
    let code = match params.code {
        Some(c) => c,
        None => {
            let error_msg = "Missing authorization code";
            let _ = state
                .result_tx
                .send(OAuthResult::Error(error_msg.to_string()))
                .await;
            return Html(error_html(error_msg));
        }
    };

    tracing::info!("Received OAuth callback with authorization code");

    // Exchange code for token
    match exchange_code_for_token(&code, &state.challenge).await {
        Ok(api_key) => {
            // Save the token
            let token = OpenRouterToken {
                api_key: api_key.clone(),
                obtained_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                expires_at: None, // OpenRouter tokens typically don't expire
                label: Some("VT Code OAuth".to_string()),
            };

            if let Err(e) = save_oauth_token(&token) {
                tracing::error!("Failed to save OAuth token: {}", e);
                let _ = state
                    .result_tx
                    .send(OAuthResult::Error(format!("Failed to save token: {}", e)))
                    .await;
                return Html(error_html(&format!("Failed to save token: {}", e)));
            }

            let _ = state.result_tx.send(OAuthResult::Success(api_key)).await;
            Html(success_html())
        }
        Err(e) => {
            let error_msg = format!("Failed to exchange code: {}", e);
            tracing::error!("{}", error_msg);
            let _ = state
                .result_tx
                .send(OAuthResult::Error(error_msg.clone()))
                .await;
            Html(error_html(&error_msg))
        }
    }
}

/// Handle cancel request.
async fn handle_cancel(State(state): State<Arc<OAuthServerState>>) -> Html<String> {
    let _ = state.result_tx.send(OAuthResult::Cancelled).await;
    Html(cancelled_html())
}

/// Generate success HTML page.
fn success_html() -> String {
    r##"<!DOCTYPE html>
<html>
<head>
    <title>VT Code - Authentication Successful</title>
    <style>
        @font-face {
            font-family: 'SF Pro Display';
            src: local('SF Pro Display'), local('.SF NS Display'), local('Helvetica Neue');
        }
        * { box-sizing: border-box; }
        body {
            font-family: 'SF Pro Display', -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background: #0a0a0a;
            color: #fafafa;
        }
        .container {
            text-align: center;
            padding: 3rem 4rem;
            border: 1px solid #262626;
            border-radius: 12px;
            max-width: 440px;
        }
        .logo {
            margin-bottom: 1.5rem;
        }
        .logo svg {
            height: 32px;
            width: auto;
        }
        .status-icon {
            width: 48px;
            height: 48px;
            margin: 0 auto 1.5rem;
            border: 2px solid #22c55e;
            border-radius: 50%;
            display: flex;
            align-items: center;
            justify-content: center;
            font-size: 1.25rem;
            color: #22c55e;
        }
        h1 {
            margin: 0 0 0.75rem 0;
            font-size: 1.25rem;
            font-weight: 500;
            letter-spacing: -0.02em;
        }
        p {
            color: #a1a1aa;
            margin: 0;
            font-size: 0.875rem;
            line-height: 1.5;
        }
        .close-note {
            margin-top: 1.5rem;
            font-size: 0.75rem;
            color: #52525b;
        }
    </style>
</head>
<body>
    <div class="container">
        <div class="logo">
            <svg viewBox="0 0 800 160" xmlns="http://www.w3.org/2000/svg">
                <text x="60" y="100" font-family="SF Pro Display,-apple-system,BlinkMacSystemFont,sans-serif" font-size="72" font-weight="600" letter-spacing="2" fill="#fafafa">&gt; VT Code</text>
            </svg>
        </div>
        <div class="status-icon">✓</div>
        <h1>Authentication Successful</h1>
        <p>Your OpenRouter account is now connected.</p>
        <p class="close-note">This window will close automatically.</p>
    </div>
    <script>
        setTimeout(() => window.close(), 3000);
    </script>
</body>
</html>"##.to_string()
}

/// Generate error HTML page.
fn error_html(error: &str) -> String {
    format!(r##"<!DOCTYPE html>
<html>
<head>
    <title>VT Code - Authentication Failed</title>
    <style>
        @font-face {{
            font-family: 'SF Pro Display';
            src: local('SF Pro Display'), local('.SF NS Display'), local('Helvetica Neue');
        }}
        @font-face {{
            font-family: 'SF Mono';
            src: local('SF Mono'), local('Menlo'), local('Monaco');
        }}
        * {{ box-sizing: border-box; }}
        body {{
            font-family: 'SF Pro Display', -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background: #0a0a0a;
            color: #fafafa;
        }}
        .container {{
            text-align: center;
            padding: 3rem 4rem;
            border: 1px solid #262626;
            border-radius: 12px;
            max-width: 480px;
        }}
        .logo {{
            margin-bottom: 1.5rem;
        }}
        .logo svg {{
            height: 32px;
            width: auto;
        }}
        .status-icon {{
            width: 48px;
            height: 48px;
            margin: 0 auto 1.5rem;
            border: 2px solid #ef4444;
            border-radius: 50%;
            display: flex;
            align-items: center;
            justify-content: center;
            font-size: 1.25rem;
            color: #ef4444;
        }}
        h1 {{
            margin: 0 0 0.75rem 0;
            font-size: 1.25rem;
            font-weight: 500;
            letter-spacing: -0.02em;
        }}
        p {{
            color: #a1a1aa;
            margin: 0;
            font-size: 0.875rem;
            line-height: 1.5;
        }}
        .error {{
            margin-top: 1.5rem;
            padding: 1rem;
            background: #18181b;
            border: 1px solid #27272a;
            border-radius: 8px;
            font-family: 'SF Mono', Menlo, Monaco, monospace;
            font-size: 0.75rem;
            color: #71717a;
            word-break: break-word;
            text-align: left;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="logo">
            <svg viewBox="0 0 800 160" xmlns="http://www.w3.org/2000/svg">
                <text x="60" y="100" font-family="SF Pro Display,-apple-system,BlinkMacSystemFont,sans-serif" font-size="72" font-weight="600" letter-spacing="2" fill="#fafafa">&gt; VT Code</text>
            </svg>
        </div>
        <div class="status-icon">✕</div>
        <h1>Authentication Failed</h1>
        <p>Unable to connect your OpenRouter account.</p>
        <div class="error">{}</div>
    </div>
</body>
</html>"##, error)
}

/// Generate cancelled HTML page.
fn cancelled_html() -> String {
    r##"<!DOCTYPE html>
<html>
<head>
    <title>VT Code - Authentication Cancelled</title>
    <style>
        @font-face {
            font-family: 'SF Pro Display';
            src: local('SF Pro Display'), local('.SF NS Display'), local('Helvetica Neue');
        }
        * { box-sizing: border-box; }
        body {
            font-family: 'SF Pro Display', -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background: #0a0a0a;
            color: #fafafa;
        }
        .container {
            text-align: center;
            padding: 3rem 4rem;
            border: 1px solid #262626;
            border-radius: 12px;
            max-width: 440px;
        }
        .logo {
            margin-bottom: 1.5rem;
        }
        .logo svg {
            height: 32px;
            width: auto;
        }
        .status-icon {
            width: 48px;
            height: 48px;
            margin: 0 auto 1.5rem;
            border: 2px solid #71717a;
            border-radius: 50%;
            display: flex;
            align-items: center;
            justify-content: center;
            font-size: 1.25rem;
            color: #71717a;
        }
        h1 {
            margin: 0 0 0.75rem 0;
            font-size: 1.25rem;
            font-weight: 500;
            letter-spacing: -0.02em;
        }
        p {
            color: #a1a1aa;
            margin: 0;
            font-size: 0.875rem;
            line-height: 1.5;
        }
    </style>
</head>
<body>
    <div class="container">
        <div class="logo">
            <svg viewBox="0 0 800 160" xmlns="http://www.w3.org/2000/svg">
                <text x="60" y="100" font-family="SF Pro Display,-apple-system,BlinkMacSystemFont,sans-serif" font-size="72" font-weight="600" letter-spacing="2" fill="#fafafa">&gt; VT Code</text>
            </svg>
        </div>
        <div class="status-icon">—</div>
        <h1>Authentication Cancelled</h1>
        <p>You can try again anytime using /login openrouter</p>
    </div>
</body>
</html>"##.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_success_html_valid() {
        let html = success_html();
        assert!(html.contains("Authentication Successful"));
        assert!(html.contains("OpenRouter"));
        assert!(html.contains("VT Code"));
        // Verify mono minimal theme (no gradients)
        assert!(!html.contains("linear-gradient"));
        // Verify SF Pro font
        assert!(html.contains("SF Pro Display"));
    }

    #[test]
    fn test_error_html_valid() {
        let html = error_html("Test error message");
        assert!(html.contains("Authentication Failed"));
        assert!(html.contains("Test error message"));
        assert!(html.contains("VT Code"));
        // Verify mono minimal theme (no gradients)
        assert!(!html.contains("linear-gradient"));
        // Verify SF Mono font for error
        assert!(html.contains("SF Mono"));
    }

    #[test]
    fn test_cancelled_html_valid() {
        let html = cancelled_html();
        assert!(html.contains("Authentication Cancelled"));
        assert!(html.contains("/login openrouter"));
        assert!(html.contains("VT Code"));
        // Verify mono minimal theme (no gradients)
        assert!(!html.contains("linear-gradient"));
    }
}
