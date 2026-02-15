use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;
use webbrowser;

use super::{SlashCommandContext, SlashCommandControl};

pub async fn handle_oauth_login(
    ctx: SlashCommandContext<'_>,
    provider: String,
) -> Result<SlashCommandControl> {
    if provider != "openrouter" {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("OAuth login not supported for provider: {}", provider),
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    ctx.renderer.line(
        MessageStyle::Info,
        "Starting OpenRouter OAuth authentication...",
    )?;

    // Get callback port from config or use default
    let callback_port = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.auth.openrouter.callback_port)
        .unwrap_or(8484);

    // Generate PKCE challenge
    let pkce = match vtcode_config::auth::generate_pkce_challenge() {
        Ok(c) => c,
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to generate PKCE challenge: {}", err),
            )?;
            return Ok(SlashCommandControl::Continue);
        }
    };

    // Generate auth URL
    let auth_url = vtcode_config::auth::get_auth_url(&pkce, callback_port);

    ctx.renderer
        .line(MessageStyle::Info, "Opening browser for authentication...")?;
    ctx.renderer
        .line(MessageStyle::Output, &format!("URL: {}", auth_url))?;

    // Try to open browser
    if let Err(err) = webbrowser::open(&auth_url) {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Failed to open browser: {}", err),
        )?;
        ctx.renderer.line(
            MessageStyle::Info,
            "Please open the URL manually in your browser.",
        )?;
    }

    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Waiting for callback on port {}...", callback_port),
    )?;

    // Get timeout from config or use default (5 minutes)
    let timeout_secs = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.auth.openrouter.flow_timeout_secs)
        .unwrap_or(300);

    // Start the OAuth callback server (it handles the full flow)
    #[cfg(feature = "a2a-server")]
    {
        use vtcode_core::auth::OAuthResult;

        match vtcode_core::auth::run_oauth_callback_server(pkce, callback_port, Some(timeout_secs))
            .await
        {
            Ok(OAuthResult::Success(api_key)) => {
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Successfully authenticated with OpenRouter!",
                )?;
                ctx.renderer.line(
                    MessageStyle::Output,
                    "Your API key has been securely stored and encrypted.",
                )?;
                ctx.renderer.line(
                    MessageStyle::Output,
                    &format!(
                        "Key preview: {}...",
                        &api_key[..std::cmp::min(8, api_key.len())]
                    ),
                )?;
            }
            Ok(OAuthResult::Cancelled) => {
                ctx.renderer
                    .line(MessageStyle::Info, "OAuth flow was cancelled by user.")?;
            }
            Ok(OAuthResult::Error(err)) => {
                ctx.renderer
                    .line(MessageStyle::Error, &format!("OAuth flow failed: {}", err))?;
            }
            Err(err) => {
                ctx.renderer
                    .line(MessageStyle::Error, &format!("OAuth server error: {}", err))?;
            }
        }
    }

    #[cfg(not(feature = "a2a-server"))]
    {
        ctx.renderer.line(
            MessageStyle::Error,
            "OAuth login requires the 'a2a-server' feature to be enabled.",
        )?;
        ctx.renderer.line(
            MessageStyle::Info,
            "Please rebuild with: cargo build --features a2a-server",
        )?;
    }

    Ok(SlashCommandControl::Continue)
}

pub async fn handle_oauth_logout(
    ctx: SlashCommandContext<'_>,
    provider: String,
) -> Result<SlashCommandControl> {
    if provider != "openrouter" {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("OAuth logout not supported for provider: {}", provider),
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    match vtcode_config::auth::clear_oauth_token() {
        Ok(()) => {
            ctx.renderer.line(
                MessageStyle::Info,
                "OpenRouter OAuth token cleared successfully.",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "You will need to authenticate again to use OAuth.",
            )?;
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to clear OAuth token: {}", err),
            )?;
        }
    }

    Ok(SlashCommandControl::Continue)
}

pub async fn handle_show_auth_status(
    ctx: SlashCommandContext<'_>,
    provider: Option<String>,
) -> Result<SlashCommandControl> {
    ctx.renderer
        .line(MessageStyle::Info, "Authentication Status")?;
    ctx.renderer.line(MessageStyle::Output, "")?;

    // Show OpenRouter status
    if provider.is_none() || provider.as_deref() == Some("openrouter") {
        match vtcode_config::auth::get_auth_status() {
            Ok(status) => match status {
                vtcode_config::auth::AuthStatus::Authenticated {
                    label,
                    age_seconds,
                    expires_in,
                } => {
                    ctx.renderer
                        .line(MessageStyle::Info, "OpenRouter: ✓ Authenticated (OAuth)")?;
                    if let Some(l) = label {
                        ctx.renderer
                            .line(MessageStyle::Output, &format!("  Label: {}", l))?;
                    }
                    let age_str = if age_seconds < 60 {
                        format!("{}s ago", age_seconds)
                    } else if age_seconds < 3600 {
                        format!("{}m ago", age_seconds / 60)
                    } else if age_seconds < 86400 {
                        format!("{}h ago", age_seconds / 3600)
                    } else {
                        format!("{}d ago", age_seconds / 86400)
                    };
                    ctx.renderer.line(
                        MessageStyle::Output,
                        &format!("  Token obtained: {}", age_str),
                    )?;
                    if let Some(expires) = expires_in {
                        let exp_str = if expires < 60 {
                            format!("{}s", expires)
                        } else if expires < 3600 {
                            format!("{}m", expires / 60)
                        } else if expires < 86400 {
                            format!("{}h", expires / 3600)
                        } else {
                            format!("{}d", expires / 86400)
                        };
                        ctx.renderer
                            .line(MessageStyle::Output, &format!("  Expires in: {}", exp_str))?;
                    }
                }
                vtcode_config::auth::AuthStatus::NotAuthenticated => {
                    // Check if using API key instead
                    if std::env::var("OPENROUTER_API_KEY").is_ok() {
                        ctx.renderer.line(
                            MessageStyle::Info,
                            "OpenRouter: Using API key from environment",
                        )?;
                    } else {
                        ctx.renderer
                            .line(MessageStyle::Info, "OpenRouter: Not authenticated")?;
                        ctx.renderer.line(
                            MessageStyle::Output,
                            "  Use /login openrouter to authenticate via OAuth",
                        )?;
                    }
                }
            },
            Err(err) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to check auth status: {}", err),
                )?;
            }
        }
    }

    // Show other provider status if needed
    if provider.is_none() {
        ctx.renderer.line(MessageStyle::Output, "")?;
        ctx.renderer.line(
            MessageStyle::Output,
            "Use /login <provider> to authenticate via OAuth",
        )?;
        ctx.renderer.line(
            MessageStyle::Output,
            "Use /logout <provider> to clear authentication",
        )?;
    }

    Ok(SlashCommandControl::Continue)
}
