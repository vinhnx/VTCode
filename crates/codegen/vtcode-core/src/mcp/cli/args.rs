//! Clap argument types for the `vtcode mcp` subcommands.

use crate::cli::input_hardening::validate_agent_safe_text;
use anyhow::{Result, bail};
use clap::{ArgGroup, Args};

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
    #[arg(long = "command", num_args = 1.., allow_hyphen_values = true, value_name = "COMMAND")]
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

    /// Optional environment variable containing the Bearer [REDACTED]
    #[arg(long = "bearer-token-env-var", value_name = "ENV_VAR", requires = "url")]
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

/// Arguments for the `resources` subcommand.
#[derive(Debug, Clone, Args)]
pub struct ResourcesArgs {
    /// Only show resources from this provider.
    #[arg(long)]
    pub provider: Option<String>,

    /// Output the resources as JSON.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for the `app` subcommand.
#[derive(Debug, Clone, Args)]
pub struct AppArgs {
    /// Resource URI to inspect (e.g. `ui://server/page.html`).
    pub uri: String,

    /// Print the raw resource body (e.g. HTML source) instead of the summary.
    #[arg(long, conflicts_with = "json")]
    pub html: bool,

    /// Output the inspection as JSON.
    #[arg(long)]
    pub json: bool,
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

pub(crate) fn validate_provider_name(name: &str) -> Result<()> {
    validate_agent_safe_text("provider name", name)?;
    let is_valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

    if is_valid {
        Ok(())
    } else {
        bail!("invalid provider name '{name}' (use letters, numbers, '-', '_')");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AddArgs, AddMcpStdioArgs, AddMcpTransportArgs, AppArgs, ResourcesArgs, parse_env_pair, validate_provider_name,
    };
    use clap::{Args, Command, FromArgMatches};

    #[test]
    fn add_args_accepts_documented_command_flag() {
        let command = AddArgs::augment_args(Command::new("add"));
        let matches = command
            .try_get_matches_from([
                "add",
                "test-provider",
                "--command",
                "uvx",
                "mcp-server-time",
                "--local-timezone=Asia/Ho_Chi_Minh",
            ])
            .expect("documented --command form should parse");
        let args = AddArgs::from_arg_matches(&matches).expect("parsed matches should map to AddArgs");

        let AddMcpTransportArgs { stdio: Some(stdio), streamable_http: None } = args.transport_args else {
            panic!("stdio transport should be selected");
        };
        let AddMcpStdioArgs { command, env, working_directory } = stdio;
        assert_eq!(command, ["uvx", "mcp-server-time", "--local-timezone=Asia/Ho_Chi_Minh"]);
        assert!(env.is_empty());
        assert_eq!(working_directory, None);
    }

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
    fn resources_args_accepts_provider_filter_and_json() {
        let command = ResourcesArgs::augment_args(Command::new("resources"));
        let matches = command
            .try_get_matches_from(["resources", "--provider", "demo", "--json"])
            .expect("documented resources flags should parse");
        let args = ResourcesArgs::from_arg_matches(&matches).expect("parsed matches should map to ResourcesArgs");
        assert_eq!(args.provider.as_deref(), Some("demo"));
        assert!(args.json);
    }

    #[test]
    fn app_args_accepts_uri_with_html() {
        let command = AppArgs::augment_args(Command::new("app"));
        let matches = command
            .try_get_matches_from(["app", "ui://demo/app.html", "--html"])
            .expect("documented app flags should parse");
        let args = AppArgs::from_arg_matches(&matches).expect("parsed matches should map to AppArgs");
        assert_eq!(args.uri, "ui://demo/app.html");
        assert!(args.html);
        assert!(!args.json);
    }

    #[test]
    fn app_args_rejects_html_with_json() {
        let command = AppArgs::augment_args(Command::new("app"));
        command
            .try_get_matches_from(["app", "ui://demo/app.html", "--html", "--json"])
            .expect_err("html and json conflict");
    }
}
