//! Provider registry subcommands: `add`, `remove`, `list`, `get`.

use super::args::validate_provider_name;
use super::args::{
    AddArgs, AddMcpStdioArgs, AddMcpStreamableHttpArgs, AddMcpTransportArgs, GetArgs, ListArgs, RemoveArgs,
};
use super::config_store::{load_global_config, write_global_config};
use crate::cli::input_hardening::validate_agent_safe_text;
use crate::config::VTCodeConfig;
use crate::config::mcp::{
    McpHttpHandshakeMode, McpHttpServerConfig, McpProviderConfig, McpStdioServerConfig, McpTransportConfig,
};
use anyhow::{Context, Result, anyhow, bail};
use hashbrown::HashMap;
use serde_json::json;

pub(crate) async fn run_add(add_args: AddArgs) -> Result<()> {
    validate_provider_name(&add_args.name)?;

    let (mut config, path) = load_global_config()?;

    let AddArgs {
        name,
        transport_args,
        max_concurrent_requests,
        disabled,
    } = add_args;

    let transport = match transport_args.clone() {
        AddMcpTransportArgs { stdio: Some(stdio), .. } => build_stdio_transport(stdio)?,
        AddMcpTransportArgs { streamable_http: Some(http), .. } => build_http_transport(http)?,
        _ => bail!("either --command or --url must be provided"),
    };

    let mut provider = McpProviderConfig::default();
    provider.name = name.clone();
    provider.transport = transport;
    provider.enabled = !disabled;
    provider.max_concurrent_requests = max_concurrent_requests.unwrap_or(provider.max_concurrent_requests);

    if let Some(stdio) = transport_args.stdio {
        provider.env = stdio.env.into_iter().collect();
    }

    let was_new = upsert_provider(&mut config, provider);
    write_global_config(&path, &config).await?;

    if was_new {
        println!("Added MCP provider '{name}'.");
    } else {
        println!("Updated MCP provider '{name}'.");
    }

    Ok(())
}

pub(crate) async fn run_remove(remove_args: RemoveArgs) -> Result<()> {
    validate_provider_name(&remove_args.name)?;

    let (mut config, path) = load_global_config()?;
    let original_len = config.mcp.providers.len();
    config.mcp.providers.retain(|provider| provider.name != remove_args.name);

    if config.mcp.providers.len() == original_len {
        println!("No MCP provider named '{}' found.", remove_args.name);
        return Ok(());
    }

    write_global_config(&path, &config).await?;
    println!("Removed MCP provider '{}'.", remove_args.name);
    Ok(())
}

pub(crate) async fn run_list(list_args: ListArgs) -> Result<()> {
    let (config, _) = load_global_config()?;
    let mut providers = config.mcp.providers.clone();
    providers.sort_by(|a, b| a.name.cmp(&b.name));

    if list_args.json {
        let payload: Vec<_> = providers.into_iter().map(|provider| json_provider(&provider)).collect();
        let output = serde_json::to_string_pretty(&payload).context("failed to serialize MCP providers to JSON")?;
        println!("{output}");
        return Ok(());
    }

    if providers.is_empty() {
        println!("No MCP providers configured. Use `vtcode mcp add <name> --command <binary>` to register one.");
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
                let status = if provider.enabled { "enabled" } else { "disabled" };
                stdio_rows.push([
                    provider.name.clone(),
                    stdio.command.clone(),
                    args_display,
                    env_display,
                    working_dir,
                    format!("{status} (max {max_requests})", max_requests = provider.max_concurrent_requests),
                ]);
            }
            McpTransportConfig::Http(http) => {
                let status = if provider.enabled { "enabled" } else { "disabled" };
                let protocol = http.protocol_version.clone();
                http_rows.push([
                    provider.name.clone(),
                    http.endpoint.clone(),
                    http_auth_label(http),
                    protocol,
                    http_oauth_status_label(&provider.name, http),
                    format!("{status} (max {max_requests})", max_requests = provider.max_concurrent_requests),
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

pub(crate) async fn run_get(get_args: GetArgs) -> Result<()> {
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
    println!("  max_concurrent_requests: {}", provider.max_concurrent_requests);
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
            println!("  auth: {}", http_auth_label(http));
            println!("  oauth_status: {}", http_oauth_status_label(&provider.name, http));
            println!("  protocol_version: {}", http.protocol_version);
            println!(
                "  handshake: {}",
                match http.handshake {
                    McpHttpHandshakeMode::Legacy => "legacy",
                    McpHttpHandshakeMode::Auto => "auto",
                }
            );
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
        handshake: default_config.handshake,
        http_headers: headers,
        env_http_headers: env_headers,
    };

    Ok(McpTransportConfig::Http(transport))
}

fn upsert_provider(config: &mut VTCodeConfig, provider: McpProviderConfig) -> bool {
    if let Some(existing) = config.mcp.providers.iter_mut().find(|entry| entry.name == provider.name) {
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
            "handshake": http.handshake,
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

fn http_auth_label(http: &McpHttpServerConfig) -> String {
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

    match vtcode_config::auth::McpOAuthService::new().status_for_resource(provider_name, oauth, &http.endpoint) {
        Ok(vtcode_config::auth::McpOAuthStatus::Authenticated { .. }) => "authenticated".to_string(),
        Ok(vtcode_config::auth::McpOAuthStatus::NotAuthenticated) => "not authenticated".to_string(),
        Err(error) => format!("error: {error}"),
    }
}

fn format_env_map(map: &HashMap<String, String>) -> String {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by_key(|(a, _)| *a);
    entries
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ")
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
        for (width, cell) in widths.iter_mut().zip(row.iter()) {
            *width = (*width).max(cell.len());
        }
    }

    let [name_w, command_w, args_w, env_w, workdir_w, status_w] = widths;

    println!(
        "{name:<name_w$}  {command:<command_w$}  {args:<args_w$}  {env:<env_w$}  {workdir:<workdir_w$}  {status:<status_w$}",
        name = "Name",
        command = "Command",
        args = "Args",
        env = "Env",
        workdir = "Working Dir",
        status = "Status",
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
        for (width, cell) in widths.iter_mut().zip(row.iter()) {
            *width = (*width).max(cell.len());
        }
    }

    let [name_w, endpoint_w, auth_w, protocol_w, oauth_w, status_w] = widths;

    println!(
        "{name:<name_w$}  {endpoint:<endpoint_w$}  {auth:<auth_w$}  {protocol:<protocol_w$}  {oauth:<oauth_w$}  {status:<status_w$}",
        name = "Name",
        endpoint = "Endpoint",
        auth = "Auth",
        protocol = "Protocol",
        oauth = "OAuth Status",
        status = "Status",
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
        );
    }
}

#[cfg(test)]
mod tests {
    use super::json_provider;
    use crate::config::mcp::{McpHttpHandshakeMode, McpHttpServerConfig, McpProviderConfig, McpTransportConfig};

    #[test]
    fn json_provider_reports_http_handshake_mode() {
        let provider = McpProviderConfig {
            name: "modern".to_string(),
            transport: McpTransportConfig::Http(McpHttpServerConfig {
                endpoint: "https://example.com/mcp".to_string(),
                handshake: McpHttpHandshakeMode::Auto,
                ..McpHttpServerConfig::default()
            }),
            ..McpProviderConfig::default()
        };

        assert_eq!(json_provider(&provider)["transport"]["handshake"], "auto");
    }
}
