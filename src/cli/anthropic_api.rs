use anyhow::Result;

#[cfg(feature = "anthropic-api")]
pub(super) async fn handle_anthropic_api_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
    port: u16,
    host: String,
) -> Result<()> {
    use std::net::SocketAddr;
    use vtcode_core::llm::providers::anthropic::api::{AnthropicApiServerState, create_router};

    let provider = vtcode_core::llm::factory::create_provider_for_model(
        &core_cfg.model,
        core_cfg.api_key.clone(),
        None,
        core_cfg.model_behavior.clone(),
    )
    .map_err(|e| anyhow::anyhow!("Failed to create LLM provider: {}", e))?;

    let state =
        AnthropicApiServerState::new(std::sync::Arc::from(provider), core_cfg.model.clone());
    let app = create_router(state);

    let addr = format!("{}:{}", host, port)
        .parse::<SocketAddr>()
        .map_err(|e| anyhow::anyhow!("Invalid address {}:{}: {}", host, port, e))?;

    println!("Anthropic API server starting on http://{}", addr);
    println!("Compatible with Anthropic Messages API at /v1/messages");
    println!("Press Ctrl+C to stop the server");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to bind to address {}: {}", addr, e))?;

    ::axum::serve(listener, app)
        .with_graceful_shutdown(vtcode_core::shutdown::shutdown_signal_logged("server"))
        .await
        .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;

    Ok(())
}

#[cfg(not(feature = "anthropic-api"))]
pub(super) async fn handle_anthropic_api_command(
    _core_cfg: vtcode_core::config::types::AgentConfig,
    _port: u16,
    _host: String,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Anthropic API server is not enabled. Recompile with --features anthropic-api"
    ))
}
