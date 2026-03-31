//! CLI commands for managing Model Context Protocol providers.

use crate::cli::input_hardening::validate_agent_safe_text;
use crate::config::VTCodeConfig;
use crate::config::loader::ConfigManager;
use crate::config::mcp::{
    McpHttpServerConfig, McpProviderConfig, McpStdioServerConfig, McpTransportConfig,
};
use anyhow::{Context, Result, anyhow, bail};
use clap::{ArgGroup, Args, Subcommand};
use hashbrown::HashMap;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use tokio::fs;
use vtcode_config::auth::{
    AuthCallbackOutcome, McpOAuthConfig, McpOAuthService, OAuthCallbackPage,
    start_auth_code_callback_server,
};

static GLOBAL_CONFIG_PATH_OVERRIDE: LazyLock<Mutex<Option<PathBuf>>> =
    LazyLock::new(|| Mutex::new(None));

/// Subcommands exposed by the `vtcode mcp` entrypoint.
#[derive(Debug, Clone, Subcommand)]
pub enum McpCommands {
    /// List configured MCP providers.
    List(ListArgs),

    /// Show details for a single MCP provider.
    Get(GetArgs),

    /// Add or update an MCP provider definition.
    Add(AddArgs),

    /// Remove an MCP provider definition.
    Remove(RemoveArgs),

    /// Start OAuth login for an HTTP MCP provider.
    Login(LoginArgs),

    /// Clear stored OAuth credentials for an HTTP MCP provider.
    Logout(LogoutArgs),
}

/// Arguments for the `list` subcommand.
#[derive(Debug, Clone, Args)]
pub struct ListArgs {
    /// Output the configured providers as JSON.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for the `get` subcommand.
#[derive(Debug, Clone, Args)]
pub struct GetArgs {
    /// Name of the provider to display.
    pub name: String,

    /// Output the provider configuration as JSON.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for the `add` subcommand.
#[derive(Debug, Clone, Args)]
pub struct AddArgs {
    /// Name for the provider configuration.
    pub name: String,

    #[command(flatten)]
    pub transport_args: AddMcpTransportArgs,

    /// Maximum concurrent requests handled by the provider.
    #[arg(long)]
    pub max_concurrent_requests: Option<usize>,

    /// Persist the provider in a disabled state.
    #[arg(long)]
    pub disabled: bool,
}

/// Mutually exclusive transport arguments for MCP providers.
#[derive(Debug, Clone, Args)]
#[command(
    group(
        ArgGroup::new("transport")
            .args(["command", "url"])
            .required(true)
            .multiple(false)
    )
)]
pub struct AddMcpTransportArgs {
    #[command(flatten)]
    pub stdio: Option<AddMcpStdioArgs>,

    #[command(flatten)]
    pub streamable_http: Option<AddMcpStreamableHttpArgs>,
}

/// stdio transport arguments for MCP providers.
#[derive(Debug, Clone, Args)]
pub struct AddMcpStdioArgs {
    /// Command to launch the MCP server. Use `--url` for HTTP servers.
    #[arg(trailing_var_arg = true, num_args = 0..)]
    pub command: Vec<String>,

    /// Environment variables to export when launching the server.
    #[arg(long, value_parser = parse_env_pair, value_name = "KEY=VALUE")]
    pub env: Vec<(String, String)>,

    /// Optional working directory for the command.
    #[arg(long, value_name = "PATH")]
    pub working_directory: Option<String>,
}

/// Streamable HTTP transport arguments for MCP providers.
#[derive(Debug, Clone, Args)]
pub struct AddMcpStreamableHttpArgs {
    /// URL for the streamable HTTP MCP server.
    #[arg(long)]
    pub url: String,

    /// Optional environment variable containing the bearer token.
    #[arg(
        long = "bearer-token-env-var",
        value_name = "ENV_VAR",
        requires = "url"
    )]
    pub bearer_token_env_var: Option<String>,

    /// Additional headers to send with each request (KEY=VALUE form).
    #[arg(long, value_parser = parse_env_pair, value_name = "KEY=VALUE")]
    pub header: Vec<(String, String)>,

    /// Headers whose values are sourced from environment variables (KEY=ENV_VAR form).
    #[arg(long, value_parser = parse_env_pair, value_name = "KEY=ENV_VAR")]
    pub env_header: Vec<(String, String)>,
}

/// Arguments for the `remove` subcommand.
#[derive(Debug, Clone, Args)]
pub struct RemoveArgs {
    /// Name of the provider to remove.
    pub name: String,
}

/// Arguments for the `login` subcommand.
#[derive(Debug, Clone, Args)]
pub struct LoginArgs {
    /// Name of the provider to authenticate.
    pub name: String,
}

/// Arguments for the `logout` subcommand.
#[derive(Debug, Clone, Args)]
pub struct LogoutArgs {
    /// Name of the provider to deauthenticate.
    pub name: String,
}

/// Entry point for the `vtcode mcp` command group.
pub async fn handle_mcp_command(command: McpCommands) -> Result<()> {
    match command {
        McpCommands::List(args) => run_list(args).await,
        McpCommands::Get(args) => run_get(args).await,
        McpCommands::Add(args) => run_add(args).await,
        McpCommands::Remove(args) => run_remove(args).await,
        McpCommands::Login(args) => run_login(args).await,
        McpCommands::Logout(args) => run_logout(args).await,
    }
}

async fn run_add(add_args: AddArgs) -> Result<()> {
    validate_provider_name(&add_args.name)?;

    let (mut config, path) = load_global_config()?;

    let AddArgs {
        name,
        transport_args,
        max_concurrent_requests,
        disabled,
    } = add_args;

    let transport = match transport_args.clone() {
        AddMcpTransportArgs {
            stdio: Some(stdio), ..
        } => build_stdio_transport(stdio)?,
        AddMcpTransportArgs {
            streamable_http: Some(http),
            ..
        } => build_http_transport(http)?,
        _ => bail!("either --command or --url must be provided"),
    };

    let mut provider = McpProviderConfig::default();
    provider.name = name.clone();
    provider.transport = transport;
    provider.enabled = !disabled;
    provider.max_concurrent_requests =
        max_concurrent_requests.unwrap_or(provider.max_concurrent_requests);

    if let Some(stdio) = transport_args.stdio {
        provider.env = stdio.env.into_iter().collect();
    }

    let was_new = upsert_provider(&mut config, provider);
    write_global_config(&path, &config).await?;

    if was_new {
        println!("Added MCP provider '{}'.", name);
    } else {
        println!("Updated MCP provider '{}'.", name);
    }

    Ok(())
}

async fn run_remove(remove_args: RemoveArgs) -> Result<()> {
    validate_provider_name(&remove_args.name)?;

    let (mut config, path) = load_global_config()?;
    let original_len = config.mcp.providers.len();
    config
        .mcp
        .providers
        .retain(|provider| provider.name != remove_args.name);

    if config.mcp.providers.len() == original_len {
        println!("No MCP provider named '{}' found.", remove_args.name);
        return Ok(());
    }

    write_global_config(&path, &config).await?;
    println!("Removed MCP provider '{}'.", remove_args.name);
    Ok(())
}

async fn run_list(list_args: ListArgs) -> Result<()> {
    let (config, _) = load_global_config()?;
    let mut providers = config.mcp.providers.clone();
    providers.sort_by(|a, b| a.name.cmp(&b.name));

    if list_args.json {
        let payload: Vec<_> = providers
            .into_iter()
            .map(|provider| json_provider(&provider))
            .collect();
        let output = serde_json::to_string_pretty(&payload)
            .context("failed to serialize MCP providers to JSON")?;
        println!("{output}");
        return Ok(());
    }

    if providers.is_empty() {
        println!(
            "No MCP providers configured. Use `vtcode mcp add <name> --command <binary>` to register one."
        );
        return Ok(());
    }

    let mut stdio_rows: Vec<[String; 6]> = Vec::new();
    let mut http_rows: Vec<[String; 6]> = Vec::new();

    for provider in &providers {
        match &provider.transport {
            McpTransportConfig::Stdio(stdio) => {
                let args_display = if stdio.args.is_empty() {
                    "-".to_owned()
                } else {
                    stdio.args.join(" ")
                };
                let env_display = if provider.env.is_empty() {
                    "-".to_owned()
                } else {
                    format_env_map(&provider.env)
                };
                let working_dir = stdio.working_directory.as_deref().unwrap_or("-").to_owned();
                let status = if provider.enabled {
                    "enabled"
                } else {
                    "disabled"
                };
                stdio_rows.push([
                    provider.name.clone(),
                    stdio.command.clone(),
                    args_display,
                    env_display,
                    working_dir,
                    format!(
                        "{status} (max {max_requests})",
                        max_requests = provider.max_concurrent_requests
                    ),
                ]);
            }
            McpTransportConfig::Http(http) => {
                let status = if provider.enabled {
                    "enabled"
                } else {
                    "disabled"
                };
                let protocol = http.protocol_version.clone();
                http_rows.push([
                    provider.name.clone(),
                    http.endpoint.clone(),
                    http_auth_label(&provider.name, http),
                    protocol,
                    http_oauth_status_label(&provider.name, http),
                    format!(
                        "{status} (max {max_requests})",
                        max_requests = provider.max_concurrent_requests
                    ),
                ]);
            }
        }
    }

    if !stdio_rows.is_empty() {
        print_stdio_table(&stdio_rows);
    }

    if !stdio_rows.is_empty() && !http_rows.is_empty() {
        println!();
    }

    if !http_rows.is_empty() {
        print_http_table(&http_rows);
    }

    Ok(())
}

async fn run_get(get_args: GetArgs) -> Result<()> {
    let (config, _) = load_global_config()?;
    let provider = config
        .mcp
        .providers
        .iter()
        .find(|provider| provider.name == get_args.name)
        .ok_or_else(|| anyhow!("No MCP provider named '{}' found.", get_args.name))?;

    if get_args.json {
        let output = serde_json::to_string_pretty(&json_provider(provider))
            .context("failed to serialize MCP provider to JSON")?;
        println!("{output}");
        return Ok(());
    }

    println!("{}", provider.name);
    println!("  enabled: {}", provider.enabled);
    println!(
        "  max_concurrent_requests: {}",
        provider.max_concurrent_requests
    );
    if !provider.env.is_empty() {
        println!("  env: {}", format_env_map(&provider.env));
    }

    match &provider.transport {
        McpTransportConfig::Stdio(stdio) => {
            println!("  transport: stdio");
            println!("  command: {}", stdio.command);
            let args_display = if stdio.args.is_empty() {
                "-".to_owned()
            } else {
                stdio.args.join(" ")
            };
            println!("  args: {args_display}");
            let working_directory = stdio.working_directory.as_deref().unwrap_or("-");
            println!("  working_directory: {working_directory}");
        }
        McpTransportConfig::Http(http) => {
            println!("  transport: http");
            println!("  endpoint: {}", http.endpoint);
            println!("  auth: {}", http_auth_label(&provider.name, http));
            println!(
                "  oauth_status: {}",
                http_oauth_status_label(&provider.name, http)
            );
            println!("  protocol_version: {}", http.protocol_version);
            if !http.http_headers.is_empty() {
                println!("  headers: {}", format_env_map(&http.http_headers));
            }
            if !http.env_http_headers.is_empty() {
                println!("  env_headers: {}", format_env_map(&http.env_http_headers));
            }
            if let Some(oauth) = &http.oauth {
                println!("  oauth.authorization_url: {}", oauth.authorization_url);
                println!("  oauth.token_url: {}", oauth.token_url);
                println!("  oauth.client_id: {}", oauth.client_id);
                if !oauth.scopes.is_empty() {
                    println!("  oauth.scopes: {}", oauth.scopes.join(", "));
                }
                println!("  oauth.callback_port: {}", oauth.callback_port);
            }
        }
    }

    println!("  remove: vtcode mcp remove {}", provider.name);

    Ok(())
}

async fn run_login(login_args: LoginArgs) -> Result<()> {
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
    let prepared = service.prepare_login(&provider.name, oauth)?;
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
    println!(
        "Waiting for the OAuth callback on localhost:{}...",
        prepared.callback_port
    );

    let completion = match callback_server.wait().await? {
        AuthCallbackOutcome::Code(code) => {
            service
                .complete_login(&provider.name, oauth, &prepared, &code)
                .await?
        }
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

async fn run_logout(logout_args: LogoutArgs) -> Result<()> {
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

fn build_stdio_transport(args: AddMcpStdioArgs) -> Result<McpTransportConfig> {
    let mut command_parts = args.command.into_iter();
    let command_bin = command_parts
        .next()
        .ok_or_else(|| anyhow!("command is required when using stdio transport"))?;
    validate_agent_safe_text("command", &command_bin)?;
    let command_args: Vec<String> = command_parts.collect();
    for arg in &command_args {
        validate_agent_safe_text("command argument", arg)?;
    }
    if let Some(working_directory) = args.working_directory.as_deref() {
        validate_agent_safe_text("working_directory", working_directory)?;
    }

    let transport = McpStdioServerConfig {
        command: command_bin,
        args: command_args,
        working_directory: args.working_directory,
    };

    Ok(McpTransportConfig::Stdio(transport))
}

fn build_http_transport(args: AddMcpStreamableHttpArgs) -> Result<McpTransportConfig> {
    validate_agent_safe_text("url", &args.url)?;
    if let Some(env_var) = args.bearer_token_env_var.as_deref() {
        validate_agent_safe_text("bearer_token_env_var", env_var)?;
    }
    let headers = args.header.into_iter().collect::<HashMap<_, _>>();
    let env_headers = args.env_header.into_iter().collect::<HashMap<_, _>>();
    let default_config = McpHttpServerConfig::default();
    let transport = McpHttpServerConfig {
        endpoint: args.url,
        api_key_env: args.bearer_token_env_var,
        oauth: None,
        protocol_version: default_config.protocol_version,
        http_headers: headers,
        env_http_headers: env_headers,
    };

    Ok(McpTransportConfig::Http(transport))
}

fn upsert_provider(config: &mut VTCodeConfig, provider: McpProviderConfig) -> bool {
    if let Some(existing) = config
        .mcp
        .providers
        .iter_mut()
        .find(|entry| entry.name == provider.name)
    {
        *existing = provider;
        false
    } else {
        config.mcp.providers.push(provider);
        true
    }
}

fn json_provider(provider: &McpProviderConfig) -> serde_json::Value {
    let transport = match &provider.transport {
        McpTransportConfig::Stdio(stdio) => json!({
            "type": "stdio",
            "command": stdio.command,
            "args": stdio.args,
            "working_directory": stdio.working_directory,
            "env": provider.env,
        }),
        McpTransportConfig::Http(http) => json!({
            "type": "http",
            "endpoint": http.endpoint,
            "api_key_env": http.api_key_env,
            "oauth": http.oauth,
            "protocol_version": http.protocol_version,
            "headers": http.http_headers,
            "env_headers": http.env_http_headers,
        }),
    };

    json!({
        "name": provider.name,
        "enabled": provider.enabled,
        "transport": transport,
        "max_concurrent_requests": provider.max_concurrent_requests,
    })
}

fn provider_http_oauth_config(provider: &McpProviderConfig) -> Result<&McpOAuthConfig> {
    match &provider.transport {
        McpTransportConfig::Http(http) => http.oauth.as_ref().ok_or_else(|| {
            anyhow!(
                "MCP provider '{}' does not have HTTP OAuth configured.",
                provider.name
            )
        }),
        McpTransportConfig::Stdio(_) => Err(anyhow!(
            "MCP provider '{}' uses stdio transport and does not support HTTP OAuth login.",
            provider.name
        )),
    }
}

fn http_auth_label(_provider_name: &str, http: &McpHttpServerConfig) -> String {
    if http.oauth.is_some() {
        "oauth".to_string()
    } else {
        http.api_key_env
            .clone()
            .map(|env| format!("env:{env}"))
            .unwrap_or_else(|| "none".to_string())
    }
}

fn http_oauth_status_label(provider_name: &str, http: &McpHttpServerConfig) -> String {
    let Some(oauth) = http.oauth.as_ref() else {
        return "-".to_string();
    };

    match McpOAuthService::new().status(provider_name, oauth.credentials_store_mode) {
        Ok(vtcode_config::auth::McpOAuthStatus::Authenticated { .. }) => {
            "authenticated".to_string()
        }
        Ok(vtcode_config::auth::McpOAuthStatus::NotAuthenticated) => {
            "not authenticated".to_string()
        }
        Err(error) => format!("error: {error}"),
    }
}

fn open_browser_or_print_url(url: &str) -> Result<()> {
    println!("Open this URL to continue OAuth:\n{url}");
    if let Err(error) = webbrowser::open(url) {
        println!("Automatic browser open failed: {error}");
    }
    Ok(())
}

fn format_env_map(map: &HashMap<String, String>) -> String {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by(|(a, _), (b, _)| a.cmp(b));
    entries
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn load_global_config() -> Result<(VTCodeConfig, PathBuf)> {
    let path = global_config_path()?;
    if path.exists() {
        let manager = ConfigManager::load_from_file(&path)
            .with_context(|| format!("failed to load configuration from {}", path.display()))?;
        Ok((manager.config().clone(), path))
    } else {
        Ok((VTCodeConfig::default(), path))
    }
}

async fn write_global_config(path: &Path, config: &VTCodeConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    let contents = toml::to_string_pretty(config).context("failed to serialize configuration")?;
    fs::write(path, contents)
        .await
        .with_context(|| format!("failed to write configuration to {}", path.display()))?;
    Ok(())
}

fn global_config_path() -> Result<PathBuf> {
    if let Some(path) = GLOBAL_CONFIG_PATH_OVERRIDE
        .lock()
        .map_err(|_| anyhow!("global config path override mutex poisoned"))?
        .clone()
    {
        return Ok(path);
    }

    let home_dir = dirs::home_dir().ok_or_else(|| anyhow!("failed to determine home directory"))?;
    Ok(home_dir.join(".vtcode").join("vtcode.toml"))
}

#[doc(hidden)]
pub fn set_global_config_path_override_for_tests(path: Option<PathBuf>) -> Result<()> {
    let mut guard = GLOBAL_CONFIG_PATH_OVERRIDE
        .lock()
        .map_err(|_| anyhow!("global config path override mutex poisoned"))?;
    *guard = path;
    Ok(())
}

fn parse_env_pair(raw: &str) -> Result<(String, String), String> {
    let mut parts = raw.splitn(2, '=');
    let key = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "entries must be in KEY=VALUE form".to_owned())?;
    let value = parts
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "entries must be in KEY=VALUE form".to_owned())?;
    validate_agent_safe_text("env key", key).map_err(|err| err.to_string())?;
    validate_agent_safe_text("env value", &value).map_err(|err| err.to_string())?;
    Ok((key.to_owned(), value))
}

fn validate_provider_name(name: &str) -> Result<()> {
    validate_agent_safe_text("provider name", name)?;
    let is_valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

    if is_valid {
        Ok(())
    } else {
        bail!("invalid provider name '{name}' (use letters, numbers, '-', '_')");
    }
}

fn print_stdio_table(rows: &[[String; 6]]) {
    let mut widths = [
        "Name".len(),
        "Command".len(),
        "Args".len(),
        "Env".len(),
        "Working Dir".len(),
        "Status".len(),
    ];

    for row in rows {
        for (idx, cell) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(cell.len());
        }
    }

    println!(
        "{name:<name_w$}  {command:<command_w$}  {args:<args_w$}  {env:<env_w$}  {workdir:<workdir_w$}  {status:<status_w$}",
        name = "Name",
        command = "Command",
        args = "Args",
        env = "Env",
        workdir = "Working Dir",
        status = "Status",
        name_w = widths[0],
        command_w = widths[1],
        args_w = widths[2],
        env_w = widths[3],
        workdir_w = widths[4],
        status_w = widths[5],
    );

    for row in rows {
        println!(
            "{name:<name_w$}  {command:<command_w$}  {args:<args_w$}  {env:<env_w$}  {workdir:<workdir_w$}  {status:<status_w$}",
            name = row[0],
            command = row[1],
            args = row[2],
            env = row[3],
            workdir = row[4],
            status = row[5],
            name_w = widths[0],
            command_w = widths[1],
            args_w = widths[2],
            env_w = widths[3],
            workdir_w = widths[4],
            status_w = widths[5],
        );
    }
}

fn print_http_table(rows: &[[String; 6]]) {
    let mut widths = [
        "Name".len(),
        "Endpoint".len(),
        "Auth".len(),
        "Protocol".len(),
        "OAuth Status".len(),
        "Status".len(),
    ];

    for row in rows {
        for (idx, cell) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(cell.len());
        }
    }

    println!(
        "{name:<name_w$}  {endpoint:<endpoint_w$}  {auth:<auth_w$}  {protocol:<protocol_w$}  {oauth:<oauth_w$}  {status:<status_w$}",
        name = "Name",
        endpoint = "Endpoint",
        auth = "Auth",
        protocol = "Protocol",
        oauth = "OAuth Status",
        status = "Status",
        name_w = widths[0],
        endpoint_w = widths[1],
        auth_w = widths[2],
        protocol_w = widths[3],
        oauth_w = widths[4],
        status_w = widths[5],
    );

    for row in rows {
        println!(
            "{name:<name_w$}  {endpoint:<endpoint_w$}  {auth:<auth_w$}  {protocol:<protocol_w$}  {oauth:<oauth_w$}  {status:<status_w$}",
            name = row[0],
            endpoint = row[1],
            auth = row[2],
            protocol = row[3],
            oauth = row[4],
            status = row[5],
            name_w = widths[0],
            endpoint_w = widths[1],
            auth_w = widths[2],
            protocol_w = widths[3],
            oauth_w = widths[4],
            status_w = widths[5],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{GLOBAL_CONFIG_PATH_OVERRIDE, parse_env_pair, validate_provider_name};
    use std::path::PathBuf;

    #[test]
    fn parse_env_pair_accepts_valid_input() {
        let parsed = parse_env_pair("FOO=bar").expect("valid env pair");
        assert_eq!(parsed.0, "FOO");
        assert_eq!(parsed.1, "bar");
    }

    #[test]
    fn parse_env_pair_rejects_control_chars() {
        let err = parse_env_pair("FOO=bad\u{0000}value").expect_err("nul must be rejected");
        assert!(err.contains("U+0000"));
    }

    #[test]
    fn validate_provider_name_rejects_control_chars() {
        let err = validate_provider_name("bad\u{0007}name").expect_err("control chars rejected");
        assert!(err.to_string().contains("U+0007"));
    }

    #[test]
    fn global_config_path_uses_test_override() {
        let override_path = PathBuf::from("/tmp/vtcode-mcp-test.toml");
        *GLOBAL_CONFIG_PATH_OVERRIDE
            .lock()
            .expect("override mutex should be available") = Some(override_path.clone());
        let resolved = super::global_config_path().expect("global config path");
        assert_eq!(resolved, override_path);
        *GLOBAL_CONFIG_PATH_OVERRIDE
            .lock()
            .expect("override mutex should be available") = None;
    }
}
