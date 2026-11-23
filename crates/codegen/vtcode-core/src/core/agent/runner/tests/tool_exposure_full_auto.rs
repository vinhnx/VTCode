#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]

use super::*;

#[tokio::test]
async fn exec_full_auto_default_allowlist_exposes_tools_without_native_allowed_tools() {
    // Regression probe for the 2026-11-23 exec zero-tools bug on local
    // routes: mirror exec setup (auto primary + full-auto default
    // allowlist) on a provider without native allowed-tools support and
    // require a non-empty tool snapshot.
    let temp = TempDir::new().expect("workspace");
    let config = VTCodeConfig::default();
    let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-exec-local-tools".to_string(),
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(config.clone()),
        None,
    ))
    .await
    .expect("runner");
    runner.provider_client = Box::new(RecordingQueuedProvider::with_name("llama-cpp", Vec::new()));
    runner.set_active_primary_agent(ActivePrimaryAgent::from_spec(&vtcode_config::builtin_primary_auto_agent()));
    runner.enable_full_auto(&config.automation.full_auto.allowed_tools).await;
    let mut table = Vec::new();
    for name in [
        tools::EXEC_COMMAND,
        tools::WRITE_STDIN,
        tools::APPLY_PATCH,
        tools::SEARCH_TOOLS,
        tools::CODE_SEARCH,
        tools::RECORD_DECISION,
        tools::READ_FILE,
        tools::LIST_FILES,
    ] {
        let permitted = runner.tool_permitted_for_advertisement_inner(name, true).await;
        let exposed = runner.is_tool_exposed(name).await;
        table.push(format!("{name}: permitted={permitted} exposed={exposed}"));
    }
    let snapshot = runner.build_universal_tool_snapshot().await.expect("snapshot");
    let mut defs: Vec<String> = snapshot
        .snapshot
        .as_ref()
        .map(|defs| defs.iter().map(|tool| tool.function_name().to_string()).collect())
        .unwrap_or_default();
    defs.sort();
    // Modern default allowlist admits the three execution tools plus the
    // allowlisted union tool code_search; search_tools/record_decision stay
    // hidden (not allowlisted). Pin the exact set: drift in either direction
    // (silent collapse or allowlist bypass) must fail loudly.
    let mut expected = vec![
        tools::EXEC_COMMAND.to_string(),
        tools::WRITE_STDIN.to_string(),
        tools::APPLY_PATCH.to_string(),
        tools::CODE_SEARCH.to_string(),
    ];
    expected.sort();
    assert_eq!(
        defs,
        expected,
        "exec-equivalent local route must expose exactly the allowlisted catalog; active={:?}\n{}",
        snapshot.active_tool_names,
        table.join("\n")
    );
}

#[tokio::test]
async fn stale_full_auto_allowlist_falls_back_to_full_catalog_on_local_like_routes() {
    // A non-empty allowlist matching zero emitted tools is config drift
    // (2026-11-23: a global allowlist of legacy file-tool names collapsed
    // exec to a silent zero-tool catalog on local routes). Advertisement
    // must degrade to the full catalog; execution must keep denying.
    let temp = TempDir::new().expect("workspace");
    let mut config = VTCodeConfig::default();
    // Neutralize the orthogonal hosted-tool-search dimension: the default
    // provider is Anthropic with tool search on, which injects a
    // `tool_search_*` definition into the catalog. This test pins the
    // allowlist fallback set, not deferral policy.
    config.provider.anthropic.tool_search.enabled = false;
    let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-stale-allowlist".to_string(),
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(config),
        None,
    ))
    .await
    .expect("runner");
    runner.provider_client = Box::new(RecordingQueuedProvider::with_name("llama-cpp", Vec::new()));
    runner.set_active_primary_agent(ActivePrimaryAgent::from_spec(&vtcode_config::builtin_primary_auto_agent()));
    runner
        .enable_full_auto(&[
            tools::READ_FILE.to_string(),
            tools::LIST_FILES.to_string(),
            tools::GREP_FILE.to_string(),
        ])
        .await;
    let snapshot = runner.build_universal_tool_snapshot().await.expect("snapshot");
    let mut defs: Vec<String> = snapshot
        .snapshot
        .as_ref()
        .map(|defs| defs.iter().map(|tool| tool.function_name().to_string()).collect())
        .unwrap_or_default();
    defs.sort();
    let mut expected = vec![
        tools::EXEC_COMMAND.to_string(),
        tools::WRITE_STDIN.to_string(),
        tools::APPLY_PATCH.to_string(),
        tools::SEARCH_TOOLS.to_string(),
        tools::RECORD_DECISION.to_string(),
    ];
    expected.sort();
    assert_eq!(defs, expected, "stale allowlist must fall back to exactly the full VtCode catalog");
    assert!(
        runner.tool_registry.is_denied_in_full_auto(tools::EXEC_COMMAND).await,
        "execution must keep enforcing the allowlist despite the advertisement fallback"
    );
}

#[tokio::test]
async fn empty_full_auto_allowlist_still_advertises_no_tools() {
    // An intentionally empty allowlist means "no tools allowed": the
    // stale-allowlist fallback must not rescue a deliberate deny-all.
    let temp = TempDir::new().expect("workspace");
    let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-empty-allowlist".to_string(),
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");
    runner.provider_client = Box::new(RecordingQueuedProvider::with_name("llama-cpp", Vec::new()));
    runner.set_active_primary_agent(ActivePrimaryAgent::from_spec(&vtcode_config::builtin_primary_auto_agent()));
    runner.enable_full_auto(&[]).await;
    let snapshot = runner.build_universal_tool_snapshot().await.expect("snapshot");
    let defs: Vec<String> = snapshot
        .snapshot
        .as_ref()
        .map(|defs| defs.iter().map(|tool| tool.function_name().to_string()).collect())
        .unwrap_or_default();
    assert!(defs.is_empty(), "empty allowlist must advertise no tools; defs={defs:?}");
}

#[tokio::test]
async fn narrow_valid_allowlist_filters_without_triggering_stale_fallback() {
    // A partial match is a deliberate restriction, not drift: only the
    // allowlisted tool is advertised on local-like routes.
    let temp = TempDir::new().expect("workspace");
    let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-narrow-allowlist".to_string(),
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");
    runner.provider_client = Box::new(RecordingQueuedProvider::with_name("llama-cpp", Vec::new()));
    runner.set_active_primary_agent(ActivePrimaryAgent::from_spec(&vtcode_config::builtin_primary_auto_agent()));
    runner.enable_full_auto(&[tools::EXEC_COMMAND.to_string()]).await;
    let snapshot = runner.build_universal_tool_snapshot().await.expect("snapshot");
    let defs: Vec<String> = snapshot
        .snapshot
        .as_ref()
        .map(|defs| defs.iter().map(|tool| tool.function_name().to_string()).collect())
        .unwrap_or_default();
    assert_eq!(defs, vec![tools::EXEC_COMMAND.to_string()], "narrow allowlist must filter, not fall back");
}
