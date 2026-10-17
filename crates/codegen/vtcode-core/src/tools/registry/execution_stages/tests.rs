use super::*;
use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::tools::registry::ToolRegistration;
use crate::tools::traits::Tool;
use async_trait::async_trait;
use futures::future::BoxFuture;
use serde_json::json;
use tempfile::TempDir;

const ECHO_NAME: &str = "stage_echo";
const ECHO_ALIAS: &str = "stage_echo_alias";

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        Ok(json!({"received": args}))
    }

    fn name(&self) -> &str {
        ECHO_NAME
    }

    fn description(&self) -> &str {
        "Records normalized handler arguments"
    }
}

fn echo_executor<'a>(_registry: &'a ToolRegistry, args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async move { Ok(json!({"received": args})) })
}

async fn register_echo(registry: &ToolRegistry, schema: Value) -> Result<()> {
    registry
        .register_tool(
            ToolRegistration::from_tool_instance(ECHO_NAME, CapabilityLevel::CodeSearch, EchoTool)
                .with_aliases([ECHO_ALIAS])
                .with_parameter_schema(schema),
        )
        .await?;
    registry.allow_all_tools().await?;
    Ok(())
}

#[tokio::test]
async fn names_resolve_registered_aliases_and_preserve_unknown_names() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    register_echo(&registry, json!({"type": "object"})).await?;
    let alias = registry.resolve_tool_name_with_display(ECHO_ALIAS);
    assert_eq!(alias.canonical, ECHO_NAME);
    assert_eq!(alias.display, "stage_echo_alias (alias for stage_echo)");
    assert!(alias.is_alias);
    let canonical = registry.resolve_tool_name_with_display(ECHO_NAME);
    assert_eq!(canonical.canonical, ECHO_NAME);
    assert_eq!(canonical.display, ECHO_NAME);
    assert!(!canonical.is_alias);
    let unknown = registry.resolve_tool_name_with_display("unknown_stage_name");
    assert_eq!(unknown.canonical, "unknown_stage_name");
    assert_eq!(unknown.display, "unknown_stage_name");
    assert!(!unknown.is_alias);
    Ok(())
}

#[tokio::test]
async fn base_routes_distinguish_registered_pty_mcp_and_unknown_tools() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    register_echo(&registry, json!({"type": "object"})).await?;
    registry
        .register_tool(ToolRegistration::new("stage_pty", CapabilityLevel::CodeSearch, true, echo_executor))
        .await?;
    let standard = registry.resolve_tool_route(ECHO_ALIAS);
    assert!(standard.tool_exists);
    assert!(!standard.needs_pty);
    assert!(!standard.is_mcp);
    assert!(standard.mcp_provider.is_none());
    assert!(standard.mcp_tool_name.is_none());
    let pty = registry.resolve_tool_route("stage_pty");
    assert!(pty.tool_exists);
    assert!(pty.needs_pty);
    assert!(!pty.is_mcp);
    let mcp = registry.resolve_tool_route("mcp::docs::lookup::nested");
    assert!(mcp.tool_exists);
    assert!(mcp.needs_pty);
    assert!(mcp.is_mcp);
    assert_eq!(mcp.mcp_provider.as_deref(), Some("docs"));
    assert_eq!(mcp.mcp_tool_name.as_deref(), Some("lookup::nested"));
    for name in ["unknown_stage_name", "mcp::docs::", "mcp::::lookup"] {
        let unknown = registry.resolve_tool_route(name);
        assert!(!unknown.tool_exists, "{name}");
        assert!(!unknown.needs_pty, "{name}");
        assert!(!unknown.is_mcp, "{name}");
    }
    Ok(())
}

#[tokio::test]
async fn canonical_mcp_route_takes_precedence_over_registration_pty_metadata() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    registry
        .register_tool(ToolRegistration::new("mcp::docs::lookup", CapabilityLevel::CodeSearch, false, echo_executor))
        .await?;
    let route = registry.resolve_tool_route("mcp::docs::lookup");
    assert!(route.tool_exists);
    assert!(route.needs_pty);
    assert!(route.is_mcp);
    assert_eq!(route.mcp_provider.as_deref(), Some("docs"));
    assert_eq!(route.mcp_tool_name.as_deref(), Some("lookup"));
    Ok(())
}

#[tokio::test]
async fn legacy_handler_strips_preview_metadata_and_retains_alias_history() -> Result<()> {
    let workspace = TempDir::new()?;
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    registry.optimization_config.tool_registry.use_optimized_registry = true;
    register_echo(
        &registry,
        json!({"type": "object", "properties": {"input": {"type": "string"}}, "additionalProperties": false}),
    )
    .await?;
    let args = json!({"input": "legacy", "max_output_tokens": 321});
    let prepared = registry.prepare_execution_args(ECHO_NAME, &args)?;
    assert_eq!(prepared.max_output_tokens, 321);
    assert!(!prepared.is_verification_command);
    assert_eq!(prepared.handler_args.as_ref(), &json!({"input": "legacy"}));
    for _ in 0..2 {
        let response = registry.execute_tool_ref(ECHO_ALIAS, &args).await?;
        assert_eq!(response["received"], json!({"input": "legacy"}));
    }
    let records = registry.execution_history.get_recent_records(10);
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .all(|record| record.tool_name == ECHO_NAME && record.requested_name == ECHO_ALIAS)
    );
    assert!(records.iter().all(|record| record.args == json!({"input": "legacy"})));
    assert!(registry.hot_tool_cache.read().peek(ECHO_NAME).is_some());
    assert!(registry.hot_tool_cache.read().peek(ECHO_ALIAS).is_none());
    assert_eq!(args, json!({"input": "legacy", "max_output_tokens": 321}));
    Ok(())
}

#[tokio::test]
async fn metadata_aware_handler_keeps_borrowed_and_owned_normalized_arguments() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    register_echo(
        &registry,
        json!({"type": "object", "properties": {"count": {"type": "integer"}, "max_output_tokens": {"type": "integer"}}, "additionalProperties": false}),
    )
    .await?;
    let unchanged = json!({"count": 7, "max_output_tokens": 234});
    let prepared = registry.prepare_execution_args(ECHO_NAME, &unchanged)?;
    assert!(matches!(prepared.handler_args, Cow::Borrowed(_)));
    assert_eq!(prepared.handler_args.as_ref(), &unchanged);
    let encoded = json!({"count": "7", "max_output_tokens": 234});
    let prepared = registry.prepare_execution_args(ECHO_NAME, &encoded)?;
    assert!(matches!(prepared.handler_args, Cow::Owned(_)));
    assert_eq!(prepared.handler_args.as_ref(), &json!({"count": 7, "max_output_tokens": 234}));
    assert_eq!(prepared.max_output_tokens, 234);
    let response = registry.execute_tool_ref(ECHO_ALIAS, &encoded).await?;
    assert_eq!(response["received"], json!({"count": 7, "max_output_tokens": 234}));
    assert_eq!(encoded, json!({"count": "7", "max_output_tokens": 234}));
    Ok(())
}

#[tokio::test]
async fn planning_budget_classifies_normalized_verifiers_before_stripping_metadata() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let inspection = json!({"query": "definition"});
    assert_eq!(
        registry
            .prepare_execution_args(tools::CODE_SEARCH, &inspection)?
            .max_output_tokens,
        6_000
    );
    registry.enable_planning();
    assert_eq!(
        registry
            .prepare_execution_args(tools::CODE_SEARCH, &inspection)?
            .max_output_tokens,
        2_000
    );
    let bounded_args = json!({"query": "definition", "max_output_tokens": 30_000});
    let bounded = registry.prepare_execution_args(tools::CODE_SEARCH, &bounded_args)?;
    assert_eq!(bounded.max_output_tokens, 4_000);
    assert!(!bounded.is_verification_command);
    let args = json!({"command": "cargo check --locked 2>&1 | tail -n 5", "max_output_tokens": 30_000});
    let verification = registry.prepare_execution_args(tools::UNIFIED_EXEC, &args)?;
    assert!(verification.is_verification_command);
    assert_eq!(verification.max_output_tokens, 30_000);
    assert_eq!(verification.handler_args["command"], "cargo check --locked 2>&1");
    assert_eq!(args["command"], "cargo check --locked 2>&1 | tail -n 5");
    Ok(())
}

#[tokio::test]
async fn malformed_preview_metadata_is_rejected_before_execution() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    register_echo(&registry, json!({"type": "object", "properties": {"max_output_tokens": {"type": "integer"}}}))
        .await?;
    for value in [json!(0), json!(50_001), json!("234"), json!(234.0)] {
        let args = json!({"max_output_tokens": value});
        assert!(registry.prepare_execution_args(ECHO_NAME, &args).is_err());
        let error = registry
            .execute_tool_ref(ECHO_ALIAS, &args)
            .await
            .expect_err("invalid metadata rejected");
        assert!(error.to_string().contains("max_output_tokens must be an integer"));
    }
    assert!(registry.execution_history.get_recent_records(10).is_empty());
    Ok(())
}

#[tokio::test]
async fn raw_patch_preparation_normalizes_without_executing() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let patch = "*** Begin Patch\n*** Add File: unexecuted.txt\n+content\n*** End Patch\n";
    let raw = json!(patch);
    let prepared = registry.prepare_execution_args(tools::APPLY_PATCH, &raw)?;
    assert_eq!(prepared.handler_args["input"], patch);
    assert_eq!(raw, json!(patch));
    assert!(!workspace.path().join("unexecuted.txt").exists());
    assert!(registry.execution_history.get_recent_records(10).is_empty());
    Ok(())
}

fn disconnected_client() -> std::sync::Arc<crate::mcp::McpClient> {
    std::sync::Arc::new(crate::mcp::McpClient::new(vtcode_config::mcp::McpClientConfig {
        enabled: true,
        providers: vec![vtcode_config::mcp::McpProviderConfig {
            name: "offline_docs".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    }))
}

#[tokio::test]
async fn discovery_without_client_or_remote_match_preserves_standard_and_unknown_routes() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    register_echo(&registry, json!({"type": "object"})).await?;
    for has_client in [false, true] {
        if has_client {
            *registry.mcp_client.write() =
                Some(std::sync::Arc::new(crate::mcp::McpClient::new(vtcode_config::mcp::McpClientConfig {
                    enabled: false,
                    ..Default::default()
                })));
        }
        let standard = registry.resolve_execution_route(ECHO_ALIAS, ECHO_NAME).await;
        assert!(standard.route.tool_exists);
        assert!(!standard.route.is_mcp);
        assert!(!standard.route.needs_pty);
        assert!(standard.route.mcp_provider.is_none());
        assert!(standard.route.mcp_tool_name.is_none());
        assert!(standard.mcp_lookup_error.is_none());
        let unknown = registry.resolve_execution_route("missing_stage", "missing_stage").await;
        assert!(!unknown.route.tool_exists);
        assert!(!unknown.route.is_mcp);
        assert!(unknown.mcp_lookup_error.is_none());
    }
    Ok(())
}

#[tokio::test]
async fn discovery_failure_retains_standard_route_and_public_execution() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    register_echo(&registry, json!({"type": "object"})).await?;
    *registry.mcp_client.write() = Some(disconnected_client());
    let discovered = registry.resolve_execution_route(ECHO_ALIAS, ECHO_NAME).await;
    assert!(discovered.route.tool_exists);
    assert!(!discovered.route.is_mcp);
    assert!(!discovered.route.needs_pty);
    assert!(
        discovered
            .mcp_lookup_error
            .unwrap()
            .to_string()
            .contains("No MCP providers are currently connected")
    );
    let args = json!({"marker": "standard survives remote failure"});
    let result = registry.execute_tool_ref(ECHO_ALIAS, &args).await?;
    assert_eq!(result["received"], args);
    Ok(())
}

#[tokio::test]
async fn unknown_discovery_failure_preserves_error_payload_and_history() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    *registry.mcp_client.write() = Some(disconnected_client());
    registry
        .set_tool_policy("missing_stage", crate::tool_policy::ToolPolicy::Allow)
        .await?;
    let discovered = registry.resolve_execution_route("missing_stage", "missing_stage").await;
    assert!(!discovered.route.tool_exists);
    let original_error = discovered.mcp_lookup_error.unwrap().to_string();
    let args = json!({"marker": "unresolved remote"});
    let result = registry.execute_tool_ref("missing_stage", &args).await?;
    assert_eq!(result["error"]["error_type"], "ExecutionError");
    assert_eq!(result["error"]["original_error"], original_error);
    assert!(
        result["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Failed to resolve MCP tool 'missing_stage'")
    );
    let history = registry.execution_history.get_recent_records(10);
    assert_eq!(history.len(), 1);
    assert!(!history[0].success);
    assert_eq!(history[0].args, args);
    assert!(history[0].result.as_ref().unwrap_err().contains(&original_error));
    Ok(())
}

#[tokio::test]
async fn canonical_mcp_route_skips_disconnected_discovery() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    *registry.mcp_client.write() = Some(disconnected_client());
    let discovered = registry
        .resolve_execution_route("mcp_docs_lookup", "mcp::docs::lookup::nested")
        .await;
    assert!(discovered.route.tool_exists);
    assert!(discovered.route.is_mcp);
    assert!(discovered.route.needs_pty);
    assert_eq!(discovered.route.mcp_provider.as_deref(), Some("docs"));
    assert_eq!(discovered.route.mcp_tool_name.as_deref(), Some("lookup::nested"));
    assert!(discovered.mcp_lookup_error.is_none());
    Ok(())
}
