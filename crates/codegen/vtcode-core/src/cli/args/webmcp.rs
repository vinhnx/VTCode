use clap::{Args, Subcommand, ValueHint};
use std::path::PathBuf;

/// WebMCP bridge command family.
#[derive(Debug, Clone, Subcommand)]
pub enum WebmcpCommand {
    /// Start the standalone WebMCP server.
    Serve(WebmcpServeArgs),
    /// Serve MCP over local stdin/stdout without opening a network listener.
    Stdio {
        /// Enable coding tasks using existing workspace trust and execution policies.
        #[arg(long)]
        companion: bool,
    },
    /// Start the server and print a one-time browser pairing code.
    Pair(WebmcpPairArgs),
    /// Show configured WebMCP listener status.
    Status,
    /// List the safe bridge operations.
    Tools,
    /// List configured headless workspace roots.
    Roots,
    /// Revoke in-process pairings when used by an embedding runtime.
    Unpair,
}

/// Options for the standalone WebMCP server.
#[derive(Debug, Clone, Args)]
pub struct WebmcpServeArgs {
    /// Literal loopback bind host. Remote clients require `--allow-remote` and a TLS-terminating reverse proxy.
    #[arg(long, value_name = "HOST")]
    pub host: Option<String>,
    /// Bind port, or zero for a random available port.
    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,
    /// Explicit browser origin allowlist. Repeat for multiple origins.
    #[arg(long = "origin", value_name = "ORIGIN", action = clap::ArgAction::Append)]
    pub origins: Vec<String>,
    /// Explicit root exposed by headless mode. One root is supported per bridge.
    #[arg(long = "allowed-root", value_name = "PATH", value_hint = ValueHint::DirPath, action = clap::ArgAction::Append)]
    pub allowed_roots: Vec<PathBuf>,
    /// Enable remote reverse-proxy mode (requires a `wss://` public URL; the listener remains loopback).
    #[arg(long)]
    pub allow_remote: bool,
    /// Public WSS URL advertised for remote pairing.
    #[arg(long, value_name = "URL")]
    pub public_url: Option<String>,
    /// Enable the read-only OpenAI-compatible remote MCP surface.
    #[arg(long)]
    pub mcp: bool,
    /// Enable companion coding tasks on the remote MCP service (one workspace, one active task).
    #[arg(long, requires = "mcp")]
    pub companion: bool,
    /// Canonical external HTTPS URL for the remote MCP `/sse/` endpoint.
    #[arg(long = "mcp-public-url", value_name = "URL")]
    pub mcp_public_url: Option<String>,
    /// External HTTPS OAuth authorization-server URL advertised to MCP clients.
    #[arg(long = "mcp-authorization-server", value_name = "URL")]
    pub mcp_authorization_server: Option<String>,
    /// Environment variable containing the internal bearer token from the proxy.
    #[arg(long = "mcp-proxy-token-env", value_name = "NAME")]
    pub mcp_proxy_token_env: Option<String>,
    /// Optional HTTP(S) URL prefix used for escaped file citation URLs.
    #[arg(long = "mcp-citation-url-prefix", value_name = "URL")]
    pub mcp_citation_url_prefix: Option<String>,
}

/// Options for starting a paired standalone server.
#[derive(Debug, Clone, Args)]
pub struct WebmcpPairArgs {
    /// Browser origin to bind to the one-time pairing code.
    #[arg(long, value_name = "ORIGIN")]
    pub origin: String,
    /// Literal bind host.
    #[arg(long, value_name = "HOST")]
    pub host: Option<String>,
    /// Bind port, or zero for a random available port.
    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,
    /// Enable remote reverse-proxy mode (requires a `wss://` public URL; the listener remains loopback).
    #[arg(long)]
    pub allow_remote: bool,
    /// Public WSS URL advertised for remote pairing.
    #[arg(long, value_name = "URL")]
    pub public_url: Option<String>,
}
