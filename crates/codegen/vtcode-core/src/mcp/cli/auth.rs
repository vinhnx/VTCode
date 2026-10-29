//! OAuth subcommands for HTTP MCP providers: `login`, `logout`.

use super::args::{LoginArgs, LogoutArgs, validate_provider_name};
use super::config_store::load_global_config;
use crate::config::mcp::{McpProviderConfig, McpTransportConfig};
use anyhow::{Result, anyhow, bail};
use vtcode_config::auth::{
    AuthCallbackOutcome, McpOAuthConfig, McpOAuthService, OAuthCallbackPage, start_auth_code_callback_server,
};

pub(crate) async fn run_login(login_args: LoginArgs) -> Result<()> {
    validate_provider_name(&login_args.name)?;

    let (config, _) = load_global_config()?;
    let provider = config
        .mcp
        .providers
        .iter()
        .find(|provider| provider.name == login_args.name)
        .ok_or_else(|| anyhow!("No MCP provider named '{}' found.", login_args.name))?;
    let oauth = provider_http_oauth_config(provider)?;
    let service = McpOAuthService::new();
    let McpTransportConfig::Http(http) = &provider.transport else {
        bail!("MCP OAuth login requires HTTP transport");
    };
    let prepared = service.prepare_login_for_resource(&provider.name, oauth, &http.endpoint)?;
    let callback_server = start_auth_code_callback_server(
        prepared.callback_port,
        prepared.timeout_secs,
        OAuthCallbackPage::custom(
            "mcp",
            "The MCP provider is now connected.",
            "Unable to connect this MCP provider.",
            "You can try again anytime using `vtcode mcp login <name>`.",
        ),
        Some(prepared.expected_state().to_string()),
    )
    .await?;

    println!("Starting MCP OAuth login for '{}'...", provider.name);
    open_browser_or_print_url(&prepared.auth_url)?;
    println!("Waiting for the OAuth callback on localhost:{}...", prepared.callback_port);

    let completion = match callback_server.wait().await? {
        AuthCallbackOutcome::Code(code) => service.complete_login(&provider.name, oauth, &prepared, &code).await?,
        AuthCallbackOutcome::Cancelled => {
            bail!("OAuth flow was cancelled")
        }
        AuthCallbackOutcome::Error(error) => {
            bail!(error)
        }
    };

    println!("MCP OAuth login complete for '{}'.", completion.name);
    Ok(())
}

pub(crate) async fn run_logout(logout_args: LogoutArgs) -> Result<()> {
    validate_provider_name(&logout_args.name)?;

    let (config, _) = load_global_config()?;
    let provider = config
        .mcp
        .providers
        .iter()
        .find(|provider| provider.name == logout_args.name)
        .ok_or_else(|| anyhow!("No MCP provider named '{}' found.", logout_args.name))?;
    let oauth = provider_http_oauth_config(provider)?;
    let service = McpOAuthService::new();
    service.logout(&provider.name, oauth.credentials_store_mode)?;
    println!("Cleared MCP OAuth credentials for '{}'.", provider.name);
    Ok(())
}

fn provider_http_oauth_config(provider: &McpProviderConfig) -> Result<&McpOAuthConfig> {
    match &provider.transport {
        McpTransportConfig::Http(http) => http
            .oauth
            .as_ref()
            .ok_or_else(|| anyhow!("MCP provider '{}' does not have HTTP OAuth configured.", provider.name)),
        McpTransportConfig::Stdio(_) => Err(anyhow!(
            "MCP provider '{}' uses stdio transport and does not support HTTP OAuth login.",
            provider.name
        )),
    }
}

fn open_browser_or_print_url(url: &str) -> Result<()> {
    println!("Open this URL to continue OAuth:\n{url}");
    if let Err(error) = webbrowser::open(url) {
        vtcode_commons::diagnostics::stderr(format_args!("Automatic browser open failed: {error}"));
    }
    Ok(())
}
