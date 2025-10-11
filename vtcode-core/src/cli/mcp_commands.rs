//! CLI commands for managing Model Context Protocol providers.

use crate::config::VTCodeConfig;
use crate::config::loader::ConfigManager;
use crate::config::mcp::{
    McpHttpServerConfig, McpProviderConfig, McpStdioServerConfig, McpTransportConfig,
};
use anyhow::{Context, Result, anyhow, bail};
use clap::{ArgGroup, Args, Subcommand};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

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

    /// Placeholder for OAuth login (not yet supported).
    Login(LoginArgs),

    /// Placeholder for OAuth logout (not yet supported).
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
        } => build_http_transport(http),
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
    write_global_config(&path, &config)?;

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

    write_global_config(&path, &config)?;
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
    let mut http_rows: Vec<[String; 5]> = Vec::new();

    for provider in &providers {
        match &provider.transport {
            McpTransportConfig::Stdio(stdio) => {
                let args_display = if stdio.args.is_empty() {
                    "-".to_string()
                } else {
                    stdio.args.join(" ")
                };
                let env_display = if provider.env.is_empty() {
                    "-".to_string()
                } else {
                    format_env_map(&provider.env)
                };
                let working_dir = stdio
                    .working_directory
                    .as_deref()
                    .unwrap_or("-")
                    .to_string();
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
                let api_key_env = http.api_key_env.clone().unwrap_or_else(|| "-".to_string());
                let protocol = http.protocol_version.clone();
                http_rows.push([
                    provider.name.clone(),
                    http.endpoint.clone(),
                    api_key_env,
                    protocol,
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
                "-".to_string()
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
            let env = http.api_key_env.as_deref().unwrap_or("-");
            println!("  api_key_env: {env}");
            println!("  protocol_version: {}", http.protocol_version);
            if !http.headers.is_empty() {
                println!("  headers: {}", format_env_map(&http.headers));
            }
        }
    }

    println!("  remove: vtcode mcp remove {}", provider.name);

    Ok(())
}

async fn run_login(login_args: LoginArgs) -> Result<()> {
    let _ = login_args;
    bail!("MCP OAuth login is not yet supported in VTCode.")
}

async fn run_logout(logout_args: LogoutArgs) -> Result<()> {
    let _ = logout_args;
    bail!("MCP OAuth logout is not yet supported in VTCode.")
}

fn build_stdio_transport(args: AddMcpStdioArgs) -> Result<McpTransportConfig> {
    let mut command_parts = args.command.into_iter();
    let command_bin = command_parts
        .next()
        .ok_or_else(|| anyhow!("command is required when using stdio transport"))?;
    let command_args: Vec<String> = command_parts.collect();

    let transport = McpStdioServerConfig {
        command: command_bin,
        args: command_args,
        working_directory: args.working_directory,
    };

    Ok(McpTransportConfig::Stdio(transport))
}

fn build_http_transport(args: AddMcpStreamableHttpArgs) -> McpTransportConfig {
    let headers = args.header.into_iter().collect::<HashMap<_, _>>();
    let transport = McpHttpServerConfig {
        endpoint: args.url,
        api_key_env: args.bearer_token_env_var,
        protocol_version: McpHttpServerConfig::default().protocol_version,
        headers,
    };

    McpTransportConfig::Http(transport)
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
            "protocol_version": http.protocol_version,
            "headers": http.headers,
        }),
    };

    json!({
        "name": provider.name,
        "enabled": provider.enabled,
        "transport": transport,
        "max_concurrent_requests": provider.max_concurrent_requests,
    })
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

fn write_global_config(path: &Path, config: &VTCodeConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    let contents = toml::to_string_pretty(config).context("failed to serialize configuration")?;
    fs::write(path, contents)
        .with_context(|| format!("failed to write configuration to {}", path.display()))?;
    Ok(())
}

fn global_config_path() -> Result<PathBuf> {
    let home_dir = dirs::home_dir().ok_or_else(|| anyhow!("failed to determine home directory"))?;
    Ok(home_dir.join(".vtcode").join("vtcode.toml"))
}

fn parse_env_pair(raw: &str) -> Result<(String, String), String> {
    let mut parts = raw.splitn(2, '=');
    let key = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "entries must be in KEY=VALUE form".to_string())?;
    let value = parts
        .next()
        .map(str::to_string)
        .ok_or_else(|| "entries must be in KEY=VALUE form".to_string())?;
    Ok((key.to_string(), value))
}

fn validate_provider_name(name: &str) -> Result<()> {
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

fn print_http_table(rows: &[[String; 5]]) {
    let mut widths = [
        "Name".len(),
        "Endpoint".len(),
        "API Key Env".len(),
        "Protocol".len(),
        "Status".len(),
    ];

    for row in rows {
        for (idx, cell) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(cell.len());
        }
    }

    println!(
        "{name:<name_w$}  {endpoint:<endpoint_w$}  {api:<api_w$}  {protocol:<protocol_w$}  {status:<status_w$}",
        name = "Name",
        endpoint = "Endpoint",
        api = "API Key Env",
        protocol = "Protocol",
        status = "Status",
        name_w = widths[0],
        endpoint_w = widths[1],
        api_w = widths[2],
        protocol_w = widths[3],
        status_w = widths[4],
    );

    for row in rows {
        println!(
            "{name:<name_w$}  {endpoint:<endpoint_w$}  {api:<api_w$}  {protocol:<protocol_w$}  {status:<status_w$}",
            name = row[0],
            endpoint = row[1],
            api = row[2],
            protocol = row[3],
            status = row[4],
            name_w = widths[0],
            endpoint_w = widths[1],
            api_w = widths[2],
            protocol_w = widths[3],
            status_w = widths[4],
        );
    }
}
