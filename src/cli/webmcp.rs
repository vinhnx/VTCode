use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::sync::Arc;
use vtcode_core::cli::args::{Cli, WebmcpCommand, WebmcpPairArgs, WebmcpServeArgs};
use vtcode_webmcp::{FilesystemWorkspace, RemoteMcpServerConfig, WebmcpServer, WebmcpServerConfig};

use crate::startup::{StartupContext, require_full_auto_workspace_trust};

/// Dispatch the standalone WebMCP command family.
pub(crate) async fn handle_webmcp_command(startup: &StartupContext, cli: &Cli, command: WebmcpCommand) -> Result<()> {
    match command {
        WebmcpCommand::Serve(args) => run_server(startup, cli, &args, None).await,
        WebmcpCommand::Stdio { companion } => run_stdio(startup, cli, companion).await,
        WebmcpCommand::Pair(args) => run_pair(startup, cli, &args).await,
        WebmcpCommand::Status => print_status(startup),
        WebmcpCommand::Tools => print_tools(),
        WebmcpCommand::Roots => print_roots(startup),
        WebmcpCommand::Unpair => {
            println!(
                "WebMCP pairing revocation is owned by the running server process; stop that process to revoke all in-memory sessions."
            );
            Ok(())
        }
    }
}

async fn companion_service(
    startup: &StartupContext,
    cli: &Cli,
    workspace: PathBuf,
    proxy_token_env: String,
) -> Result<Arc<super::companion::CompanionService>> {
    use vtcode_core::utils::dot_config::{WorkspaceTrustLevel, load_workspace_trust_level};
    if !startup.config.automation.full_auto.enabled {
        bail!("companion requires automation.full_auto.enabled; existing tool allowlists and denies still apply");
    }
    if load_workspace_trust_level(&workspace).await? != Some(WorkspaceTrustLevel::FullAuto) {
        bail!(
            "companion requires existing persisted full-auto workspace trust; grant it through VT Code before starting the service"
        );
    }
    Ok(Arc::new(super::companion::CompanionService::new(
        workspace,
        std::env::current_exe().context("failed to locate VT Code executable")?,
        proxy_token_env,
        super::companion::launch_args(cli, vtcode_config::loader::explicit_config_path()),
    )))
}

async fn run_stdio(startup: &StartupContext, cli: &Cli, enable_companion: bool) -> Result<()> {
    let workspace =
        vtcode_commons::fs::canonicalize_with_context_async(&startup.workspace, "local MCP workspace").await?;
    let companion = if enable_companion {
        Some(
            companion_service(
                startup,
                cli,
                workspace.clone(),
                startup.config.webmcp.remote_mcp.proxy_token_env.clone(),
            )
            .await?,
        )
    } else {
        None
    };
    let adapter = Arc::new(FilesystemWorkspace::new(&workspace, vec![workspace.clone()], false).await?);
    let handler = vtcode_webmcp::RemoteMcpHandler::local(
        adapter,
        companion
            .clone()
            .map(|service| service as Arc<dyn vtcode_webmcp::companion::CompanionTasks>),
    );
    let cancellation = tokio_util::sync::CancellationToken::new();
    let signal_cancellation = cancellation.clone();
    let signal = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal_cancellation.cancel();
        }
    });
    let result = handler.serve_stdio(cancellation).await;
    signal.abort();
    let _ = signal.await;
    if let Some(companion) = companion {
        companion.shutdown().await.map_err(anyhow::Error::msg)?;
    }
    result.context("local MCP service failed")
}

async fn run_pair(startup: &StartupContext, cli: &Cli, args: &WebmcpPairArgs) -> Result<()> {
    let serve_args = WebmcpServeArgs {
        host: args.host.clone(),
        port: args.port,
        origins: vec![args.origin.clone()],
        allowed_roots: Vec::new(),
        allow_remote: args.allow_remote,
        public_url: args.public_url.clone(),
        mcp: false,
        companion: false,
        mcp_public_url: None,
        mcp_authorization_server: None,
        mcp_proxy_token_env: None,
        mcp_citation_url_prefix: None,
    };
    run_server(startup, cli, &serve_args, Some(args.origin.as_str())).await
}

async fn run_server(
    startup: &StartupContext,
    cli: &Cli,
    args: &WebmcpServeArgs,
    pairing_origin: Option<&str>,
) -> Result<()> {
    let configured = &startup.config.webmcp;
    let host = args.host.clone().unwrap_or_else(|| configured.host.clone());
    let port = args.port.unwrap_or(configured.port);
    let origins = if args.origins.is_empty() {
        configured.allowed_origins.clone()
    } else {
        args.origins.clone()
    };
    let mut remote_mcp = build_remote_mcp_config(startup, args, configured)?;
    let remote_endpoints = remote_mcp.as_ref().map(|remote| {
        let mut streamable_url = remote.public_url.clone();
        streamable_url.set_path("/mcp");
        streamable_url.set_query(None);
        streamable_url.set_fragment(None);
        let mut metadata_url = remote.public_url.clone();
        metadata_url.set_path("/.well-known/oauth-protected-resource");
        metadata_url.set_query(None);
        metadata_url.set_fragment(None);
        (remote.public_url.clone(), streamable_url, metadata_url)
    });
    if origins.is_empty() && remote_mcp.is_none() {
        bail!("WebMCP requires at least one explicit --origin (or webmcp.allowed_origins entry)");
    }
    if let Some(pairing_origin) = pairing_origin
        && !origins.iter().any(|origin| origin == pairing_origin)
    {
        bail!("pairing origin must be present in the explicit origin allowlist");
    }

    let allowed_roots = if args.allowed_roots.is_empty() {
        configured.allowed_roots.clone()
    } else {
        args.allowed_roots.clone()
    };
    if allowed_roots.len() > 1 {
        bail!(
            "headless WebMCP currently exposes one workspace root; run one bridge per root until root selection is available"
        );
    }
    let workspace_root = allowed_roots.first().cloned().unwrap_or_else(|| startup.workspace.clone());
    let workspace_root = vtcode_commons::fs::canonicalize_with_context_async(&workspace_root, "WebMCP workspace")
        .await
        .context("failed to resolve WebMCP workspace")?;
    let companion = if args.companion {
        let remote = remote_mcp.as_mut().context("companion requires remote MCP")?;
        let proxy_token_env = args
            .mcp_proxy_token_env
            .clone()
            .unwrap_or_else(|| configured.remote_mcp.proxy_token_env.clone());
        let service = companion_service(startup, cli, workspace_root.clone(), proxy_token_env).await?;
        remote.companion = Some(service.clone());
        Some(service)
    } else {
        None
    };
    let roots = if allowed_roots.is_empty() {
        vec![workspace_root.clone()]
    } else {
        allowed_roots
    };
    let full_auto_enabled = startup.full_auto_requested && startup.config.automation.full_auto.enabled;
    if full_auto_enabled
        && (full_auto_allows_tool(startup, vtcode_core::config::constants::tools::APPLY_PATCH)
            || full_auto_allows_tool(startup, vtcode_core::config::constants::tools::EXEC_COMMAND))
    {
        require_full_auto_workspace_trust(&workspace_root, "headless WebMCP mutations and checks", "webmcp serve")
            .await?;
    }
    let mutations_allowed = full_auto_allows_tool(startup, vtcode_core::config::constants::tools::APPLY_PATCH);
    let checks_allowed = full_auto_allows_tool(startup, vtcode_core::config::constants::tools::EXEC_COMMAND);
    let adapter = FilesystemWorkspace::new(&workspace_root, roots, mutations_allowed)
        .await
        .with_context(|| format!("failed to initialize WebMCP workspace {}", workspace_root.display()))?
        .with_checks_allowed(checks_allowed);

    let has_browser_origins = !origins.is_empty();
    let config = WebmcpServerConfig {
        host,
        port,
        allowed_origins: origins,
        pairing_ttl_secs: configured.pairing_ttl_secs,
        max_frame_bytes: configured.max_frame_bytes,
        max_in_flight_requests: configured.max_in_flight_requests,
        allow_remote: args.allow_remote,
        public_url: args.public_url.clone(),
        remote_mcp,
        ..Default::default()
    };
    let server = WebmcpServer::new(Arc::new(adapter), config)?;
    let pairing = if pairing_origin.is_some() || has_browser_origins {
        Some(match pairing_origin {
            Some(origin) => server.begin_pairing_for_origin(origin.to_string())?,
            None => server.begin_pairing(),
        })
    } else {
        None
    };
    let listener = server.bind().await?;
    let address = listener.local_addr().context("failed to determine WebMCP listener address")?;
    println!("WebMCP listening locally at ws://{address}/webmcp");
    if let Some((legacy_url, streamable_url, metadata_url)) = remote_endpoints {
        println!("Remote MCP legacy endpoint: {legacy_url}");
        println!("Remote MCP Streamable HTTP endpoint: {streamable_url}");
        println!("Remote MCP protected-resource metadata: {metadata_url}");
    }
    if companion.is_some() {
        println!(
            "Companion tasks enabled: one active exec task in this workspace; planning is read-only and implementation requires a completed plan."
        );
    } else {
        println!(
            "Agent turns are unavailable in standalone mode; use `/webmcp pair <origin>` inside an interactive `vtcode chat` session for VT Code turns."
        );
    }
    if let Some(public_url) = args.public_url.as_deref() {
        println!(
            "Remote access requires a TLS-terminating reverse proxy at {public_url} forwarding to this loopback listener."
        );
    }
    if let Some(pairing) = pairing.as_ref() {
        println!("Pairing code: {} (expires in {} seconds)", pairing.code(), pairing.expires_in().as_secs());
    }
    if !mutations_allowed {
        if companion.is_some() {
            println!(
                "Browser patch apply is disabled; companion implementation independently retains the exec runtime's configured permissions."
            );
        } else {
            println!(
                "Mutations remain terminal-approved; headless apply is disabled without explicit full-auto permissions."
            );
        }
    }
    let result = tokio::select! {
        result = server.serve_listener(listener) => result.map_err(anyhow::Error::from),
        signal = tokio::signal::ctrl_c() => signal.context("failed to listen for WebMCP shutdown")
    };
    if let Some(companion) = companion {
        companion.shutdown().await.map_err(anyhow::Error::msg)?;
    }
    result
}

fn full_auto_allows_tool(startup: &StartupContext, tool: &str) -> bool {
    startup.full_auto_requested
        && startup.config.automation.full_auto.enabled
        && startup.config.automation.full_auto.allowed_tools.iter().any(|allowed| {
            let allowed = allowed.trim();
            allowed == tool || allowed == vtcode_core::config::constants::tools::WILDCARD_ALL
        })
}

fn print_status(startup: &StartupContext) -> Result<()> {
    let config = &startup.config.webmcp;
    println!("WebMCP enabled: {}", config.enabled);
    println!("Bind: {}:{}", config.host, config.port);
    println!("Allowed origins: {}", config.allowed_origins.len());
    println!("Allowed roots: {}", config.allowed_roots.len());
    println!("Pairing TTL: {} seconds", config.pairing_ttl_secs);
    println!("Remote MCP enabled: {}", config.remote_mcp.enabled);
    if let Some(public_url) = config.remote_mcp.public_url.as_deref() {
        println!("Remote MCP public URL: {public_url}");
    }
    if let Some(authorization_server) = config.remote_mcp.authorization_server.as_deref() {
        println!("Remote MCP authorization server: {authorization_server}");
    }
    println!("Remote MCP proxy token environment: {}", config.remote_mcp.proxy_token_env);
    println!("Remote MCP allowed origins: {}", config.remote_mcp.allowed_origins.len());
    Ok(())
}

fn print_tools() -> Result<()> {
    println!("WebMCP tools:");
    for tool in [
        "workspace.list_files",
        "workspace.read_file",
        "patch.propose",
        "patch.apply (terminal approval required)",
        "checks.run (allowlisted argv only)",
        "patch.revert (current-file validation required)",
        "turn.request",
        "cancel",
        "search (remote MCP, read-only)",
        "fetch (remote MCP, read-only)",
    ] {
        println!("  {tool}");
    }
    Ok(())
}

fn build_remote_mcp_config(
    startup: &StartupContext,
    args: &WebmcpServeArgs,
    configured: &vtcode_config::WebmcpConfig,
) -> Result<Option<RemoteMcpServerConfig>> {
    let enabled = args.mcp || configured.remote_mcp.enabled;
    if !enabled {
        return Ok(None);
    }
    let public_url = args
        .mcp_public_url
        .as_deref()
        .or(configured.remote_mcp.public_url.as_deref())
        .context("remote MCP requires --mcp-public-url or webmcp.remote_mcp.public_url")?;
    let authorization_server = args
        .mcp_authorization_server
        .as_deref()
        .or(configured.remote_mcp.authorization_server.as_deref())
        .context("remote MCP requires --mcp-authorization-server or webmcp.remote_mcp.authorization_server")?;
    let token_env = args
        .mcp_proxy_token_env
        .as_deref()
        .unwrap_or(&configured.remote_mcp.proxy_token_env);
    let token = std::env::var(token_env)
        .with_context(|| format!("remote MCP proxy token environment variable {token_env} is not set"))?;
    let citation_prefix = args
        .mcp_citation_url_prefix
        .as_deref()
        .or(configured.remote_mcp.citation_url_prefix.as_deref())
        .map(url::Url::parse)
        .transpose()
        .context("remote MCP citation URL prefix is invalid")?;
    let mut remote = RemoteMcpServerConfig::new(public_url, authorization_server, token)?;
    remote.citation_url_prefix = citation_prefix;
    remote.allowed_origins = configured.remote_mcp.allowed_origins.clone();
    remote.max_results = configured.remote_mcp.max_results;
    remote.max_scan_files = configured.remote_mcp.max_scan_files;
    remote.max_scan_bytes = configured.remote_mcp.max_scan_bytes;
    remote.session_ttl = std::time::Duration::from_secs(configured.remote_mcp.session_ttl_secs);
    remote.max_request_body_bytes = configured.max_frame_bytes;
    remote.validate()?;
    let _ = startup;
    Ok(Some(remote))
}

fn print_roots(startup: &StartupContext) -> Result<()> {
    let roots: &[PathBuf] = &startup.config.webmcp.allowed_roots;
    if roots.is_empty() {
        println!(
            "No configured headless roots. `vtcode webmcp serve --allowed-root <path>` is required for root changes."
        );
    } else {
        for root in roots {
            println!("{}", root.display());
        }
    }
    Ok(())
}
