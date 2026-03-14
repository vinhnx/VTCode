use anyhow::Result;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::{SlashCommandContext, SlashCommandControl};

const OPENROUTER_PROVIDER: &str = "openrouter";

pub(crate) async fn handle_oauth_login(
    ctx: SlashCommandContext<'_>,
    provider: String,
) -> Result<SlashCommandControl> {
    if !ensure_openrouter_provider(ctx.renderer, provider.as_str(), "login")? {
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

    run_openrouter_oauth_login(ctx.renderer, pkce, callback_port, timeout_secs).await?;

    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_oauth_logout(
    ctx: SlashCommandContext<'_>,
    provider: String,
) -> Result<SlashCommandControl> {
    if !ensure_openrouter_provider(ctx.renderer, provider.as_str(), "logout")? {
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

pub(crate) async fn handle_show_auth_status(
    ctx: SlashCommandContext<'_>,
    provider: Option<String>,
) -> Result<SlashCommandControl> {
    ctx.renderer
        .line(MessageStyle::Info, "Authentication Status")?;
    ctx.renderer.line(MessageStyle::Output, "")?;

    // Show OpenRouter status
    if provider.is_none() || provider.as_deref() == Some(OPENROUTER_PROVIDER) {
        match vtcode_config::auth::get_auth_status() {
            Ok(status) => render_openrouter_auth_status(ctx.renderer, status)?,
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

fn ensure_openrouter_provider(
    renderer: &mut AnsiRenderer,
    provider: &str,
    action: &str,
) -> Result<bool> {
    if provider == OPENROUTER_PROVIDER {
        return Ok(true);
    }

    renderer.line(
        MessageStyle::Error,
        &format!("OAuth {action} not supported for provider: {provider}"),
    )?;
    Ok(false)
}

async fn run_openrouter_oauth_login(
    renderer: &mut AnsiRenderer,
    pkce: vtcode_core::auth::PkceChallenge,
    callback_port: u16,
    timeout_secs: u64,
) -> Result<()> {
    #[cfg(feature = "a2a-server")]
    {
        use vtcode_core::auth::OAuthResult;

        match vtcode_core::auth::run_oauth_callback_server(pkce, callback_port, Some(timeout_secs))
            .await
        {
            Ok(OAuthResult::Success(api_key)) => {
                renderer.line(
                    MessageStyle::Info,
                    "Successfully authenticated with OpenRouter!",
                )?;
                renderer.line(
                    MessageStyle::Output,
                    "Your API key has been securely stored and encrypted.",
                )?;
                renderer.line(
                    MessageStyle::Output,
                    &format!(
                        "Key preview: {}...",
                        &api_key[..std::cmp::min(8, api_key.len())]
                    ),
                )?;
            }
            Ok(OAuthResult::Cancelled) => {
                renderer.line(MessageStyle::Info, "OAuth flow was cancelled by user.")?;
            }
            Ok(OAuthResult::Error(err)) => {
                renderer.line(MessageStyle::Error, &format!("OAuth flow failed: {}", err))?;
            }
            Err(err) => {
                renderer.line(MessageStyle::Error, &format!("OAuth server error: {}", err))?;
            }
        }
    }

    #[cfg(not(feature = "a2a-server"))]
    {
        let _ = (pkce, callback_port, timeout_secs);

        renderer.line(
            MessageStyle::Error,
            "OAuth login requires the 'a2a-server' feature to be enabled.",
        )?;
        renderer.line(
            MessageStyle::Info,
            "Please rebuild with: cargo build --features a2a-server",
        )?;
    }

    Ok(())
}

fn render_openrouter_auth_status(
    renderer: &mut AnsiRenderer,
    status: vtcode_config::auth::AuthStatus,
) -> Result<()> {
    match status {
        vtcode_config::auth::AuthStatus::Authenticated {
            label,
            age_seconds,
            expires_in,
        } => {
            renderer.line(MessageStyle::Info, "OpenRouter: ✓ Authenticated (OAuth)")?;
            if let Some(label) = label {
                renderer.line(MessageStyle::Output, &format!("  Label: {}", label))?;
            }
            renderer.line(
                MessageStyle::Output,
                &format!(
                    "  Token obtained: {} ago",
                    format_auth_duration(age_seconds)
                ),
            )?;
            if let Some(expires_in) = expires_in {
                renderer.line(
                    MessageStyle::Output,
                    &format!("  Expires in: {}", format_auth_duration(expires_in)),
                )?;
            }
        }
        vtcode_config::auth::AuthStatus::NotAuthenticated => {
            if std::env::var("OPENROUTER_API_KEY").is_ok() {
                renderer.line(
                    MessageStyle::Info,
                    "OpenRouter: Using API key from environment",
                )?;
            } else {
                renderer.line(MessageStyle::Info, "OpenRouter: Not authenticated")?;
                renderer.line(
                    MessageStyle::Output,
                    "  Use /login openrouter to authenticate via OAuth",
                )?;
            }
        }
    }

    Ok(())
}

fn format_auth_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}d", seconds / 86400)
    }
}
