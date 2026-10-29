use super::*;

#[tokio::test]
async fn execute_public_repo_browser_alias_is_rejected() -> Result<()> {
    let temp_dir = TempDir::new()?;
    fs::write(temp_dir.path().join("public-route.txt"), "public route\n")?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;

    let err = registry
        .execute_public_tool_ref("repo_browser.read_file", &json!({"path": "public-route.txt"}))
        .await
        .expect_err("repo_browser.read_file should not resolve publicly");

    assert!(err.to_string().contains("Unknown tool"));

    Ok(())
}
#[tokio::test]
async fn set_tool_policy_accepts_current_public_names() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let policy_path = temp_dir.path().join("tool-policy.json");
    let policy_manager = crate::tool_policy::ToolPolicyManager::new_with_config_path(&policy_path).await?;
    let registry = ToolRegistry::new_with_custom_policy(temp_dir.path().to_path_buf(), policy_manager).await;

    registry.set_tool_policy(tools::EXEC_COMMAND, ToolPolicy::Deny).await?;

    assert_eq!(registry.get_tool_policy(tools::EXEC_COMMAND).await, ToolPolicy::Deny);
    assert_eq!(registry.get_tool_policy(tools::WRITE_STDIN).await, ToolPolicy::Allow);

    Ok(())
}
#[tokio::test]
async fn apply_config_policies_applies_current_public_names() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let policy_path = temp_dir.path().join("tool-policy.json");
    let policy_manager = crate::tool_policy::ToolPolicyManager::new_with_config_path(&policy_path).await?;
    let registry = ToolRegistry::new_with_custom_policy(temp_dir.path().to_path_buf(), policy_manager).await;

    let mut config = ToolsConfig::default();
    config.policies.clear();
    config.policies.insert(tools::EXEC_COMMAND.to_string(), ToolPolicy::Allow);
    config.policies.insert(tools::APPLY_PATCH.to_string(), ToolPolicy::Deny);

    registry.apply_config_policies(&config).await?;

    assert_eq!(registry.get_tool_policy(tools::EXEC_COMMAND).await, ToolPolicy::Allow);
    assert_eq!(registry.get_tool_policy(tools::APPLY_PATCH).await, ToolPolicy::Deny);

    Ok(())
}
#[tokio::test]
async fn apply_config_policies_includes_advanced_profile_tools() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let policy_path = temp_dir.path().join("tool-policy.json");
    let policy_manager = crate::tool_policy::ToolPolicyManager::new_with_config_path(&policy_path).await?;
    let registry = ToolRegistry::new_with_custom_policy(temp_dir.path().to_path_buf(), policy_manager).await;

    let mut config = ToolsConfig {
        profile: ToolProfile::AdvancedVtCode,
        ..ToolsConfig::default()
    };
    config.policies.insert(tools::CODE_SEARCH.to_string(), ToolPolicy::Deny);

    registry.apply_config_policies(&config).await?;

    assert_eq!(registry.get_tool_policy(tools::CODE_SEARCH).await, ToolPolicy::Deny);

    Ok(())
}
#[tokio::test]
async fn persisted_approval_cache_round_trips_through_registry() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let policy_path = temp_dir.path().join("tool-policy.json");
    let policy_manager = crate::tool_policy::ToolPolicyManager::new_with_config_path(&policy_path).await?;
    let registry = ToolRegistry::new_with_custom_policy(temp_dir.path().to_path_buf(), policy_manager).await;

    registry.persist_approval_cache_key("read_file").await?;

    assert!(registry.has_persisted_approval("read_file").await);

    let manager = crate::tool_policy::ToolPolicyManager::new_with_config_path(&policy_path).await?;
    assert!(manager.has_approval_cache_key("read_file"));

    Ok(())
}
#[tokio::test]
async fn public_alias_resolution_stays_consistent_across_execution_preflight_and_policy() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry
        .register_tool(
            ToolRegistration::from_tool_instance(CUSTOM_TOOL_NAME, CapabilityLevel::CodeSearch, CustomEchoTool)
                .with_description("Custom echo tool for routing parity tests")
                .with_parameter_schema(json!({
                    "type": "object",
                    "properties": {
                        "input": {"type": "string"}
                    }
                }))
                .with_permission(ToolPolicy::Allow)
                .with_aliases(["custom tool"]),
        )
        .await?;

    let preflight = registry.preflight_validate_call("Custom Tool", &json!({"input": "value"}))?;
    assert_eq!(preflight.normalized_tool_name, CUSTOM_TOOL_NAME);

    assert_eq!(registry.evaluate_tool_policy("Custom Tool").await?, ToolPermissionDecision::Allow);

    let response = registry
        .execute_public_tool_ref("Custom Tool", &json!({"input": "value"}))
        .await?;
    assert_eq!(response["success"].as_bool(), Some(true));

    Ok(())
}
#[tokio::test]
async fn safe_mode_prompt_uses_behavior_metadata() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    registry.set_enforce_safe_mode_prompts(true).await;

    assert_eq!(registry.evaluate_tool_policy(tools::CODE_SEARCH).await?, ToolPermissionDecision::Allow);
    assert_eq!(registry.evaluate_tool_policy(tools::EXEC_COMMAND).await?, ToolPermissionDecision::Prompt);
    assert_eq!(registry.evaluate_tool_policy(tools::APPLY_PATCH).await?, ToolPermissionDecision::Prompt);

    Ok(())
}
#[tokio::test]
async fn mcp_policy_paths_resolve_model_visible_aliases() -> Result<()> {
    fn noop_executor<'a>(_registry: &'a ToolRegistry, _args: Value) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async { Ok(json!({"success": true})) })
    }

    let temp_dir = TempDir::new()?;
    let policy_path = temp_dir.path().join("tool-policy.json");
    let policy_manager = crate::tool_policy::ToolPolicyManager::new_with_config_path(&policy_path).await?;
    let registry = ToolRegistry::new_with_custom_policy(temp_dir.path().to_path_buf(), policy_manager).await;

    let public_name = crate::tools::mcp::model_visible_mcp_tool_name("context7", "search");
    registry
        .register_tool(
            ToolRegistration::new("mcp::context7::search", CapabilityLevel::Basic, false, noop_executor)
                .with_description("Fake MCP search tool")
                .with_parameter_schema(json!({"type": "object"}))
                .with_permission(ToolPolicy::Prompt)
                .with_aliases([public_name.clone()])
                .with_llm_visibility(false),
        )
        .await?;

    registry
        .mcp_tool_index
        .write()
        .await
        .insert("context7".to_string(), vec!["search".to_string()]);
    registry
        .mcp_reverse_index
        .write()
        .await
        .insert("search".to_string(), "context7".to_string());

    registry.persist_mcp_tool_policy(&public_name, ToolPolicy::Allow).await?;

    let manager = crate::tool_policy::ToolPolicyManager::new_with_config_path(&policy_path).await?;
    assert_eq!(manager.get_mcp_tool_policy("context7", "search"), ToolPolicy::Allow);

    assert_eq!(registry.evaluate_tool_policy(&public_name).await?, ToolPermissionDecision::Allow);
    assert_eq!(registry.evaluate_tool_policy("mcp::context7::search").await?, ToolPermissionDecision::Allow);

    Ok(())
}
#[tokio::test]
async fn mcp_alias_prevalidation_preserves_canonical_registration_name() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let canonical_name = "mcp::deepwiki::ask_question";
    let alias_name = crate::tools::mcp::model_visible_mcp_tool_name("deepwiki", "ask_question");
    let tool = crate::mcp::McpToolInfo {
        name: "ask_question".to_string(),
        description: "Ask a DeepWiki question".to_string(),
        provider: "deepwiki".to_string(),
        input_schema: json!({"type": "object"}),
        output_schema: None,
    };
    let registration = crate::tools::mcp::build_mcp_registration(
        Arc::new(McpClient::new(vtcode_config::mcp::McpClientConfig::default())),
        "deepwiki",
        &tool,
        None,
    )?;

    registry.allow_all_tools().await?;

    registry.register_tool(registration).await?;
    registry
        .mcp_tool_index
        .write()
        .await
        .insert("deepwiki".to_string(), vec!["ask_question".to_string()]);
    registry
        .mcp_reverse_index
        .write()
        .await
        .insert("ask_question".to_string(), "deepwiki".to_string());
    registry.persist_mcp_tool_policy(&alias_name, ToolPolicy::Allow).await?;

    let raw_tool = registry.get_tool(&alias_name).expect("MCP alias should resolve to a tool");
    assert_eq!(raw_tool.name(), canonical_name);
    assert_eq!(
        registry
            .evaluate_tool_policy(&alias_name)
            .await
            .expect("MCP alias policy should resolve"),
        ToolPermissionDecision::Allow
    );
    assert_eq!(
        registry
            .resolve_public_tool_name(&alias_name)
            .expect("MCP alias should be public"),
        canonical_name
    );

    let preflight = registry.preflight_validate_harness_call(&alias_name, &json!({}))?;
    assert_eq!(preflight.normalized_tool_name, canonical_name);

    let response = registry.execute_public_tool_ref(&alias_name, &json!({})).await?;
    assert!(response.is_object(), "MCP tool should execute successfully");

    Ok(())
}
#[tokio::test]
async fn mcp_proxy_harness_fallback_resolves_single_mcp_tool() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let canonical_name = "mcp::deepwiki::ask_question";
    let alias_name = crate::tools::mcp::model_visible_mcp_tool_name("deepwiki", "ask_question");
    let tool = vtcode_mcp::McpToolInfo {
        name: "ask_question".to_string(),
        description: "Ask a DeepWiki question".to_string(),
        provider: "deepwiki".to_string(),
        input_schema: json!({"type": "object"}),
        output_schema: None,
    };
    let registration = crate::tools::mcp::build_mcp_registration(
        Arc::new(McpClient::new(vtcode_config::mcp::McpClientConfig::default())),
        "deepwiki",
        &tool,
        None,
    )?;

    registry.register_tool(registration).await?;

    let preflight = registry.preflight_validate_harness_call("mcp_proxy", &json!({}))?;
    assert_eq!(preflight.normalized_tool_name, canonical_name);
    assert_eq!(
        registry
            .resolve_public_tool_name(&alias_name)
            .expect("MCP alias should be public"),
        canonical_name
    );

    Ok(())
}
#[tokio::test]
async fn mcp_proxy_harness_fallback_rejects_ambiguous_mcp_tools() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let tools = [("deepwiki", "ask_question"), ("context7", "search-docs")];
    for (provider, remote_name) in tools {
        let tool = vtcode_mcp::McpToolInfo {
            name: remote_name.to_string(),
            description: "MCP tool".to_string(),
            provider: provider.to_string(),
            input_schema: json!({"type": "object"}),
            output_schema: None,
        };
        let registration = crate::tools::mcp::build_mcp_registration(
            Arc::new(McpClient::new(vtcode_config::mcp::McpClientConfig::default())),
            provider,
            &tool,
            None,
        )?;
        registry.register_tool(registration).await?;
    }

    let error = registry
        .preflight_validate_harness_call("mcp_proxy", &json!({}))
        .expect_err("ambiguous MCP proxy call should fail");
    let message = error.to_string();
    assert!(message.contains("Ambiguous MCP proxy call"));
    assert!(message.contains("mcp__deepwiki__ask_question"));
    assert!(message.contains("mcp__context7__search-docs"));

    Ok(())
}
#[tokio::test]
async fn clear_mcp_client_removes_stale_proxy_tools() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let tool = vtcode_mcp::McpToolInfo {
        name: "ask_question".to_string(),
        description: "Ask a DeepWiki question".to_string(),
        provider: "deepwiki".to_string(),
        input_schema: json!({"type": "object"}),
        output_schema: None,
    };
    let client = Arc::new(McpClient::new(vtcode_config::mcp::McpClientConfig::default()));
    let registration = crate::tools::mcp::build_mcp_registration(Arc::clone(&client), "deepwiki", &tool, None)?;
    registry.register_tool(registration).await?;
    registry
        .mcp_tool_index
        .write()
        .await
        .insert("deepwiki".to_string(), vec!["ask_question".to_string()]);
    registry.set_mcp_client(Arc::clone(&client)).await;
    // `set_mcp_client` drops proxies bound to the previous client; re-register
    // against the newly attached client to simulate a completed refresh.
    let registration = crate::tools::mcp::build_mcp_registration(Arc::clone(&client), "deepwiki", &tool, None)?;
    registry.register_tool(registration).await?;
    registry
        .mcp_tool_index
        .write()
        .await
        .insert("deepwiki".to_string(), vec!["ask_question".to_string()]);
    assert!(registry.inventory.get_registration("mcp::deepwiki::ask_question").is_some());

    registry.clear_mcp_client().await;

    assert!(registry.mcp_client().is_none());
    assert!(registry.mcp_tool_index.read().await.is_empty());
    assert!(
        registry.inventory.get_registration("mcp::deepwiki::ask_question").is_none(),
        "stale MCP proxy must not survive client detach"
    );
    let err = registry
        .execute_mcp_tool("ask_question", json!({}))
        .await
        .expect_err("execution without a client should fail");
    assert!(err.to_string().contains("MCP client not available"));
    assert!(err.to_string().contains("/mcp repair"));

    Ok(())
}
#[tokio::test]
async fn set_mcp_client_drops_proxies_bound_to_previous_client() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let tool = vtcode_mcp::McpToolInfo {
        name: "ask_question".to_string(),
        description: "Ask a DeepWiki question".to_string(),
        provider: "deepwiki".to_string(),
        input_schema: json!({"type": "object"}),
        output_schema: None,
    };
    let first = Arc::new(McpClient::new(vtcode_config::mcp::McpClientConfig::default()));
    let registration = crate::tools::mcp::build_mcp_registration(Arc::clone(&first), "deepwiki", &tool, None)?;
    registry.register_tool(registration).await?;
    assert!(registry.inventory.get_registration("mcp::deepwiki::ask_question").is_some());

    let second = Arc::new(McpClient::new(vtcode_config::mcp::McpClientConfig::default()));
    registry.set_mcp_client(second).await;

    assert!(
        registry.inventory.get_registration("mcp::deepwiki::ask_question").is_none(),
        "proxies bound to the old client must not survive a client swap"
    );

    Ok(())
}
