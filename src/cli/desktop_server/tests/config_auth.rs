use super::*;

fn unauthenticated_startup_context(workspace: &Path) -> StartupContext {
    let mut startup = test_startup_context(workspace);
    startup.provider_auth_error = Some("API key not found for provider 'gemini'.".to_string());
    startup.agent_config.api_key.clear();
    startup
}

#[tokio::test]
async fn config_discover_reports_boot_auth_failure_without_disconnecting() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, _) = test_service_context(unauthenticated_startup_context(&workspace), workspace);
    context.state.lock().await.initialized = true;
    let request = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String("discover-unauthenticated".into()),
        method: protocol::method::CONFIG_DISCOVER.into(),
        params: json!({}),
    };
    let (result, shutdown) = handle_request(&context, &request).await.expect("config discover");
    assert!(!shutdown);
    let snapshot: ConfigurationSnapshot = serde_json::from_value(result).expect("snapshot");
    assert!(!snapshot.authenticated);
    assert_eq!(snapshot.provider, "gemini");
    assert_eq!(snapshot.credential_setup_command, "vtcode login gemini");
}

#[tokio::test]
async fn config_select_to_local_provider_recovers_authentication() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, _) = test_service_context(unauthenticated_startup_context(&workspace), workspace);
    context.state.lock().await.initialized = true;
    let local = catalog_models("", "")
        .into_iter()
        .find(|model| model.provider == "ollama" && !model.id.to_ascii_lowercase().contains("cloud"))
        .expect("local ollama catalog entry");
    let request = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String("select-ollama".into()),
        method: protocol::method::CONFIG_SELECT.into(),
        params: serde_json::to_value(ModelSelectParams {
            provider: local.provider.clone(),
            model: local.id.clone(),
        })
        .expect("select params"),
    };
    let (result, shutdown) = handle_request(&context, &request).await.expect("config select");
    assert!(!shutdown);
    let snapshot: ConfigurationSnapshot = serde_json::from_value(result).expect("snapshot");
    assert!(snapshot.authenticated);
    assert_eq!(snapshot.provider, local.provider);
    assert_eq!(snapshot.model, local.id);

    let agent = active_agent_config(&context).await;
    assert_eq!(agent.provider, local.provider);
    assert!(agent.api_key.is_empty(), "local providers resolve no remote credential");
}

#[tokio::test]
async fn failed_override_resolution_keeps_selection_but_reports_unauthenticated() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, _) = test_service_context(test_startup_context(&workspace), workspace.clone());
    // A provider identity that cannot exist in the environment or secure
    // storage, so resolution deterministically fails without touching env.
    let auth = catalog::resolve_override_auth(
        &context,
        "vtcode-test-unconfigured-provider",
        "vtcode-test-model",
        "VTCODE_TEST_UNCONFIGURED_PROVIDER_KEY",
    )
    .await;
    assert!(auth.error.is_some());
    assert!(auth.api_key.is_empty());

    {
        let mut state = context.state.lock().await;
        state.model_override = Some(ModelSelectParams {
            provider: "vtcode-test-unconfigured-provider".to_string(),
            model: "vtcode-test-model".to_string(),
        });
        state.model_override_auth = Some(auth);
    }
    let agent = active_agent_config(&context).await;
    assert!(agent.api_key.is_empty(), "a failed override must not leak the boot key");
    let snapshot = configuration_snapshot(&context, &agent).await.expect("snapshot");
    assert!(!snapshot.authenticated);
}
