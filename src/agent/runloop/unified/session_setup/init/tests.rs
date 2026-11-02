#[test]
fn critical_path_avoids_registry_and_discovery() {
    let src = include_str!("mod.rs");
    // Use full source: a #[cfg(test)] helper sits above initialize_session_critical.
    let production = src;
    let start = production
        .find("pub(crate) async fn initialize_session_critical")
        .expect("critical present");
    let end = production
        .find("pub(crate) async fn complete_session_registry")
        .expect("completer present");
    let critical = &production[start..end];
    for forbidden in ["ToolRegistry::new", "discover_controller_subagents"] {
        assert!(!critical.contains(forbidden), "critical must not call {forbidden}");
    }
    assert!(production.contains("session_setup_registry"), "completer emits phase");
}

use std::collections::BTreeMap;

use clap::Parser;
use serde_json::json;
use tempfile::TempDir;
use vtcode_config::{
    AgentMode, SubagentMcpServer, SubagentSource, SubagentSpec, ToolProfile,
    core::permissions::{AgentPermissionsConfig, PermissionDefault},
};
use vtcode_core::cli::args::Cli;
use vtcode_core::config::constants::tools;
use vtcode_core::config::types::ModelSelectionSource;
use vtcode_core::core::agent::config::{RuntimeModelSelection, build_runtime_agent_config};

use super::*;

#[tokio::test]
async fn interactive_catalogue_uses_configured_tool_profile() {
    let temp = TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    let mut advanced = VTCodeConfig::default();
    advanced.tools.profile = ToolProfile::AdvancedVtCode;

    let advanced_tools = registry
        .model_tools(interactive_session_tools_config(
            "gpt-5",
            Some(&advanced),
            vtcode_core::config::ToolDocumentationMode::default(),
            DeferredToolPolicy::default(),
            false,
            registry.is_planning_active(),
        ))
        .await;
    let default_tools = registry
        .model_tools(interactive_session_tools_config(
            "gpt-5",
            None,
            vtcode_core::config::ToolDocumentationMode::default(),
            DeferredToolPolicy::default(),
            false,
            registry.is_planning_active(),
        ))
        .await;

    assert!(advanced_tools.iter().any(|tool| tool.function_name() == tools::CODE_SEARCH));
    assert!(default_tools.iter().all(|tool| tool.function_name() != tools::CODE_SEARCH));
}

#[tokio::test]
async fn planning_transition_exposes_planning_tools_from_attached_base() {
    let temp = TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    let base_tools = Arc::new(RwLock::new(
        registry
            .model_tools(interactive_session_base_tools_config(
                "gpt-5",
                None,
                vtcode_core::config::ToolDocumentationMode::default(),
                DeferredToolPolicy::default(),
                false,
            ))
            .await,
    ));
    let tool_catalog = registry.tool_catalog_state();

    let inactive = tool_catalog
        .filtered_snapshot_with_stats(&base_tools, registry.is_planning_active(), false)
        .await;
    let inactive_names = inactive.active_tool_names.as_ref();
    // `request_user_input` is gated by the interactive/planning capability
    // signal (passed as `request_user_input_enabled = false` here), so it is
    // absent until planning enables it. `code_search` is a read-only tool and
    // is intentionally exposed in both modes (planning-inactive exposes every
    // non-gated tool; planning keeps read-only tools) — see
    // `FeatureSet::tool_enabled_for_mode`.
    assert!(inactive_names.iter().any(|name| name == tools::CODE_SEARCH));
    assert!(!inactive_names.iter().any(|name| name == tools::REQUEST_USER_INPUT));

    registry.enable_planning();
    let active = tool_catalog
        .filtered_snapshot_with_stats(&base_tools, registry.is_planning_active(), true)
        .await;
    let active_names = active.active_tool_names.as_ref();
    assert!(active_names.iter().any(|name| name == tools::CODE_SEARCH));
    assert!(active_names.iter().any(|name| name == tools::REQUEST_USER_INPUT));
}

#[tokio::test]
async fn mcp_refresh_route_retains_configured_tool_profile() {
    let temp = TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    let tool_catalog = registry.tool_catalog_state();
    let active_tools = Arc::new(RwLock::new(Vec::new()));
    let mut advanced = VTCodeConfig::default();
    advanced.tools.profile = ToolProfile::AdvancedVtCode;
    let cli = Cli::parse_from(["vtcode"]);
    let runtime_config = build_runtime_agent_config(
        &cli,
        &advanced,
        temp.path().to_path_buf(),
        RuntimeModelSelection {
            model: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            model_source: ModelSelectionSource::WorkspaceConfig,
        },
        "test-key".to_string(),
        vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
    );

    refresh_tool_snapshot(
        &registry,
        &active_tools,
        tool_catalog.as_ref(),
        &runtime_config,
        Some(&advanced),
        vtcode_core::config::ToolDocumentationMode::default(),
        &DeferredToolPolicy::default(),
    )
    .await;

    assert!(
        active_tools
            .read()
            .await
            .iter()
            .any(|tool| tool.function_name() == tools::CODE_SEARCH)
    );
}

#[tokio::test]
async fn enabled_session_mcp_manager_starts_initialization_task() {
    let mut cfg = VTCodeConfig::default();
    cfg.mcp.enabled = true;

    let manager = create_async_mcp_manager(Some(&cfg), None, Path::new("/tmp")).expect("manager should exist");

    assert!(manager.has_initialization_task(), "enabled session MCP manager should start in the background");
}

#[tokio::test]
async fn critical_path_defers_mcp_init_until_hydrate() {
    let temp = TempDir::new().expect("temp dir");
    let mut cfg = VTCodeConfig::default();
    cfg.mcp.enabled = true;
    let cli = Cli::parse_from(["vtcode"]);
    let runtime_config = build_runtime_agent_config(
        &cli,
        &cfg,
        temp.path().to_path_buf(),
        RuntimeModelSelection {
            model: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            model_source: ModelSelectionSource::WorkspaceConfig,
        },
        "test-key".to_string(),
        vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
    );

    let mut critical =
        initialize_session_critical(&runtime_config, Some(&cfg), false, false, None, "test-mcp-defer", None)
            .await
            .expect("critical session");
    let manager = critical.async_mcp_manager.clone().expect("MCP manager when enabled");
    assert!(
        !manager.has_initialization_task(),
        "critical path must not start MCP background init before first paint"
    );

    let mut context_manager = ContextManager::new(
        critical.base_system_prompt.clone(),
        (),
        critical.loaded_skills.clone(),
        Some(cfg.agent.clone()),
    );
    context_manager.set_workspace_root(&runtime_config.workspace);
    complete_session_registry(&mut critical, &runtime_config, Some(&cfg), false, false, None, "test-hydrate", None)
        .await
        .expect("complete registry");
    hydrate_session_runtime(
        &mut critical,
        &mut context_manager,
        &runtime_config,
        Some(&cfg),
        false,
        false,
        None,
        "test-mcp-defer",
        None,
    )
    .await
    .expect("hydrate session");
    assert!(manager.has_initialization_task(), "hydration must start MCP init after primary-agent reconfigure");
}

#[tokio::test]
async fn initialize_session_restarts_mcp_background_task_after_merge() {
    let temp = TempDir::new().expect("temp dir");
    let mut cfg = VTCodeConfig::default();
    cfg.mcp.enabled = true;
    let cli = Cli::parse_from(["vtcode"]);
    let runtime_config = build_runtime_agent_config(
        &cli,
        &cfg,
        temp.path().to_path_buf(),
        RuntimeModelSelection {
            model: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            model_source: ModelSelectionSource::WorkspaceConfig,
        },
        "test-key".to_string(),
        vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
    );

    let state = initialize_session(&runtime_config, Some(&cfg), false, false, None, "test-session", None)
        .await
        .expect("initialize session");

    let manager = state.async_mcp_manager.expect("MCP manager should exist when enabled");
    assert!(
        manager.has_initialization_task(),
        "session MCP background task must survive primary-agent merge without manual /mcp repair"
    );
}

#[tokio::test]
async fn hydrate_replaces_critical_seed_prompt_and_fills_tools() {
    let temp = TempDir::new().expect("temp dir");
    let cfg = VTCodeConfig::default();
    let cli = Cli::parse_from(["vtcode"]);
    let runtime_config = build_runtime_agent_config(
        &cli,
        &cfg,
        temp.path().to_path_buf(),
        RuntimeModelSelection {
            model: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            model_source: ModelSelectionSource::WorkspaceConfig,
        },
        "test-key".to_string(),
        vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
    );

    let mut critical =
        initialize_session_critical(&runtime_config, Some(&cfg), false, false, None, "test-hydrate", None)
            .await
            .expect("critical session");
    assert!(
        critical.tools.read().await.is_empty(),
        "critical path must leave model tools empty for first-frame deferral"
    );
    let seed_prompt = critical.base_system_prompt.clone();
    assert!(!seed_prompt.trim().is_empty(), "critical path seeds a fallback system prompt");

    let mut context_manager = ContextManager::new(
        critical.base_system_prompt.clone(),
        (),
        critical.loaded_skills.clone(),
        Some(cfg.agent.clone()),
    );
    context_manager.set_workspace_root(&runtime_config.workspace);
    complete_session_registry(&mut critical, &runtime_config, Some(&cfg), false, false, None, "test-hydrate", None)
        .await
        .expect("complete registry");
    hydrate_session_runtime(
        &mut critical,
        &mut context_manager,
        &runtime_config,
        Some(&cfg),
        false,
        false,
        None,
        "test-hydrate",
        None,
    )
    .await
    .expect("hydrate session");
    assert!(
        !critical.tools.read().await.is_empty(),
        "hydration must project model tools before the first model turn"
    );
    assert_ne!(
        critical.base_system_prompt, seed_prompt,
        "hydration must replace the seed system prompt with the composed workspace prompt"
    );
}

#[tokio::test]
async fn hydrate_reuses_critical_discovery_for_controller() {
    let temp = TempDir::new().expect("temp dir");
    let cfg = VTCodeConfig::default();
    let cli = Cli::parse_from(["vtcode"]);
    let runtime_config = build_runtime_agent_config(
        &cli,
        &cfg,
        temp.path().to_path_buf(),
        RuntimeModelSelection {
            model: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            model_source: ModelSelectionSource::WorkspaceConfig,
        },
        "test-key".to_string(),
        vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
    );

    let mut critical = initialize_session_critical(&runtime_config, Some(&cfg), false, false, None, "test-reuse", None)
        .await
        .expect("critical session");
    complete_session_registry(&mut critical, &runtime_config, Some(&cfg), false, false, None, "test-reuse", None)
        .await
        .expect("complete registry");
    let critical_names = critical
        .discovered_subagents
        .as_ref()
        .expect("complete_session_registry must cache discovery")
        .effective
        .iter()
        .map(|spec| spec.name.clone())
        .collect::<Vec<_>>();

    let mut context_manager = ContextManager::new(
        critical.base_system_prompt.clone(),
        (),
        critical.loaded_skills.clone(),
        Some(cfg.agent.clone()),
    );
    context_manager.set_workspace_root(&runtime_config.workspace);
    hydrate_session_runtime(
        &mut critical,
        &mut context_manager,
        &runtime_config,
        Some(&cfg),
        false,
        false,
        None,
        "test-reuse",
        None,
    )
    .await
    .expect("hydrate session");

    let controller = critical
        .tool_registry
        .as_ref()
        .expect("registry")
        .subagent_controller()
        .expect("controller when subagents enabled");
    let controller_names = controller
        .effective_specs()
        .await
        .into_iter()
        .map(|spec| spec.name)
        .collect::<Vec<_>>();
    for name in critical_names {
        assert!(
            controller_names.contains(&name),
            "hydrated controller must reuse critical discovery, missing {name}"
        );
    }
}

#[tokio::test]
async fn critical_path_defers_policy_manager_to_hydrate() {
    let temp = TempDir::new().expect("temp dir");
    let cfg = VTCodeConfig::default();
    let cli = Cli::parse_from(["vtcode"]);
    let runtime_config = build_runtime_agent_config(
        &cli,
        &cfg,
        temp.path().to_path_buf(),
        RuntimeModelSelection {
            model: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            model_source: ModelSelectionSource::WorkspaceConfig,
        },
        "test-key".to_string(),
        vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
    );

    let mut critical =
        initialize_session_critical(&runtime_config, Some(&cfg), false, false, None, "test-defer-policy", None)
            .await
            .expect("critical session");
    assert!(critical.tool_registry.is_none(), "critical path must not build a tool registry before first paint");

    let mut context_manager = ContextManager::new(
        critical.base_system_prompt.clone(),
        (),
        critical.loaded_skills.clone(),
        Some(cfg.agent.clone()),
    );
    context_manager.set_workspace_root(&runtime_config.workspace);
    complete_session_registry(&mut critical, &runtime_config, Some(&cfg), false, false, None, "test-hydrate", None)
        .await
        .expect("complete registry");
    hydrate_session_runtime(
        &mut critical,
        &mut context_manager,
        &runtime_config,
        Some(&cfg),
        false,
        false,
        None,
        "test-defer-policy",
        None,
    )
    .await
    .expect("hydrate session");
    assert!(
        critical
            .tool_registry
            .as_ref()
            .expect("hydrated registry")
            .has_policy_manager()
            .await,
        "hydration must attach the policy manager before the first turn"
    );
}

#[tokio::test]
async fn critical_path_defers_update_and_release_notes_to_hydrate() {
    let temp = TempDir::new().expect("temp dir");
    let cfg = VTCodeConfig::default();
    let cli = Cli::parse_from(["vtcode"]);
    let runtime_config = build_runtime_agent_config(
        &cli,
        &cfg,
        temp.path().to_path_buf(),
        RuntimeModelSelection {
            model: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            model_source: ModelSelectionSource::WorkspaceConfig,
        },
        "test-key".to_string(),
        vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
    );

    let mut critical =
        initialize_session_critical(&runtime_config, Some(&cfg), false, false, None, "test-defer-update", None)
            .await
            .expect("critical session");
    // First-paint path must not pay update-cache / release-notes file I/O.
    // Only an in-memory preflight notice (if present) may appear here.
    let preflight = crate::updater::get_preflight_notice();
    assert_eq!(
        critical.startup_update_check.cached_notice, preflight,
        "critical update check must be preflight-only, no disk read"
    );
    assert!(
        critical.session_bootstrap.release_highlights.is_none(),
        "critical path must defer release-notes read to hydration"
    );

    let mut context_manager = ContextManager::new(
        critical.base_system_prompt.clone(),
        (),
        critical.loaded_skills.clone(),
        Some(cfg.agent.clone()),
    );
    context_manager.set_workspace_root(&runtime_config.workspace);
    complete_session_registry(&mut critical, &runtime_config, Some(&cfg), false, false, None, "test-hydrate", None)
        .await
        .expect("complete registry");
    hydrate_session_runtime(
        &mut critical,
        &mut context_manager,
        &runtime_config,
        Some(&cfg),
        false,
        false,
        None,
        "test-defer-update",
        None,
    )
    .await
    .expect("hydrate session");
    // Hydration completes the deferred reads without breaking parity:
    // cached notice (if any) must have a matching header highlight.
    if let Some(notice) = critical.startup_update_check.cached_notice.as_ref() {
        assert!(
            !critical.session_bootstrap.header_highlights.is_empty(),
            "hydrated update notice must surface a header highlight"
        );
        let _ = notice;
    }
}

#[tokio::test]
async fn async_mcp_manager_uses_primary_agent_merged_mcp_config() {
    let mut cfg = VTCodeConfig::default();
    cfg.mcp.enabled = true;
    cfg.mcp.providers.push(
        serde_json::from_value(json!({
            "name": "global",
            "command": "global-mcp",
            "args": []
        }))
        .expect("global provider"),
    );
    let mut spec = test_primary_agent_spec("mcp-primary");
    spec.mcp_servers = vec![SubagentMcpServer::Inline(BTreeMap::from([
        (
            "global".to_string(),
            json!({
                "type": "stdio",
                "command": "duplicate-mcp"
            }),
        ),
        (
            "local".to_string(),
            json!({
                "type": "stdio",
                "command": "local-mcp"
            }),
        ),
    ]))];
    let active = ActivePrimaryAgent::from_spec(&spec);

    let manager = create_async_mcp_manager(Some(&cfg), Some(&active), Path::new("/tmp")).expect("manager should exist");
    let manager_config = manager.config();
    // `session_mcp_config` appends providers discovered from the user's home
    // plugin root (`~/.agents/plugins`), which is machine-dependent. Subtract
    // exactly that environment-dependent set so the assertion stays hermetic
    // while still checking the primary-agent merge result in order.
    let plugin_provider_names = discover_plugin_mcp_providers(Path::new("/tmp"))
        .into_iter()
        .map(|provider| provider.name)
        .collect::<std::collections::HashSet<_>>();
    let merged_provider_names = manager_config
        .providers
        .iter()
        .map(|provider| provider.name.as_str())
        .filter(|name| !plugin_provider_names.contains(*name))
        .collect::<Vec<_>>();

    assert_eq!(merged_provider_names, vec!["global", "local"]);
    assert!(!matches!(manager.get_status().await, McpInitStatus::Disabled));
}

#[test]
fn session_mcp_config_includes_plugin_providers() {
    use std::fs;

    let tmp = TempDir::new().expect("temp dir");
    let workspace = tmp.path();
    let mut cfg = VTCodeConfig::default();
    cfg.mcp.enabled = true;
    cfg.mcp.providers.push(
        serde_json::from_value(json!({
            "name": "global",
            "command": "global-mcp",
            "args": []
        }))
        .expect("global provider"),
    );

    // A plugin with an mcp.json stdio server under the workspace plugin root.
    let plugin_root = workspace.join(".agents/plugins/test-plugin");
    fs::create_dir_all(plugin_root.join("skills")).expect("create plugin dirs");
    fs::write(
        plugin_root.join("plugin.json"),
        r#"{
            "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name": "test-plugin",
            "version": "1.0.0",
            "description": "Test plugin"
        }"#,
    )
    .expect("write plugin.json");
    fs::write(
        plugin_root.join("mcp.json"),
        r#"{
            "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
            "mcpServers": {
                "search": {
                    "type": "stdio",
                    "command": "./bin/search",
                    "args": []
                }
            }
        }"#,
    )
    .expect("write mcp.json");

    let config = session_mcp_config(Some(&cfg), None, workspace);
    let provider_names = config
        .providers
        .iter()
        .map(|provider| provider.name.as_str())
        .collect::<Vec<_>>();

    assert!(
        provider_names.contains(&"test-plugin.search"),
        "expected plugin provider in session config, got: {provider_names:?}"
    );
    assert!(provider_names.contains(&"global"), "expected configured provider retained, got: {provider_names:?}");
}

#[test]
fn startup_primary_agent_defaults_to_build_without_config() {
    let active =
        active_primary_agent_from_specs(&[test_primary_agent_spec("builder")], None).expect("default primary agent");

    assert_eq!(active.active().identity.name, "build");
    assert_eq!(active.active().identity.source, SubagentSource::Builtin);
}

#[test]
fn startup_primary_agent_uses_default_primary_agent_config() {
    let mut cfg = VTCodeConfig {
        default_primary_agent: "builder".to_string(),
        ..VTCodeConfig::default()
    };
    let active = active_primary_agent_from_specs(&[test_primary_agent_spec("builder")], Some(&cfg))
        .expect("configured primary agent");

    assert_eq!(active.active().identity.name, "builder");

    cfg.default_primary_agent = "missing".to_string();
    let fallback = active_primary_agent_from_specs(&[test_primary_agent_spec("builder")], Some(&cfg))
        .expect("fallback primary agent");

    assert_eq!(fallback.active().identity.name, "build");
    assert_eq!(fallback.active().identity.source, SubagentSource::Builtin);
}

#[test]
fn full_auto_with_defaulted_primary_agent_selects_effective_auto() {
    let mut auto = test_primary_agent_spec("auto");
    auto.prompt = "Custom auto instructions".to_string();

    let active = active_primary_agent_from_specs_for_mode(&[auto], None, true, false, None).expect("auto");

    assert_eq!(active.active().identity.name, "auto");
    assert_eq!(active.active().instructions, "Custom auto instructions");
}

#[test]
fn resumed_session_mode_wins_over_config_default() {
    let cfg = VTCodeConfig {
        default_primary_agent: "builder".to_string(),
        ..VTCodeConfig::default()
    };
    // A resumed "plan" session should restore "plan", not the config default "builder".
    let active = active_primary_agent_from_specs_for_mode(
        &[test_primary_agent_spec("plan"), test_primary_agent_spec("builder")],
        Some(&cfg),
        false,
        false,
        Some("plan".to_string()),
    )
    .expect("resumed primary agent");

    assert_eq!(active.active().identity.name, "plan");
}

#[test]
fn explicit_configured_primary_agent_overrides_resumed_mode() {
    // When the user explicitly configures a primary agent, it wins even on resume.
    let cfg = VTCodeConfig {
        default_primary_agent: "builder".to_string(),
        ..VTCodeConfig::default()
    };
    let active = active_primary_agent_from_specs_for_mode(
        &[test_primary_agent_spec("plan"), test_primary_agent_spec("builder")],
        Some(&cfg),
        false,
        true,
        Some("plan".to_string()),
    )
    .expect("configured primary agent");

    assert_eq!(active.active().identity.name, "builder");
}

#[test]
fn full_auto_honours_explicit_primary_agent_config() {
    let cfg = VTCodeConfig {
        default_primary_agent: "builder".to_string(),
        ..VTCodeConfig::default()
    };
    let specs = [test_primary_agent_spec("auto"), test_primary_agent_spec("builder")];

    let active =
        active_primary_agent_from_specs_for_mode(&specs, Some(&cfg), true, true, None).expect("explicit builder");

    assert_eq!(active.active().identity.name, "builder");
}

#[test]
fn full_auto_honours_explicit_build_primary_agent_config() {
    let cfg = VTCodeConfig {
        default_primary_agent: "build".to_string(),
        ..VTCodeConfig::default()
    };
    let specs = [test_primary_agent_spec("auto")];

    let active =
        active_primary_agent_from_specs_for_mode(&specs, Some(&cfg), true, true, None).expect("explicit build");

    assert_eq!(active.active().identity.name, "build");
    assert_eq!(active.active().identity.source, SubagentSource::Builtin);
}

#[test]
fn full_auto_missing_defaulted_auto_fails_fast() {
    let err = active_primary_agent_from_specs_for_mode(&[test_primary_agent_spec("builder")], None, true, false, None)
        .expect_err("missing auto should fail");

    assert!(
        err.to_string()
            .contains("no effective primary agent named 'auto' was discovered")
    );
}

fn test_primary_agent_spec(name: &str) -> SubagentSpec {
    SubagentSpec {
        name: name.to_string(),
        description: format!("{name} description"),
        prompt: format!("{name} instructions"),
        tools: None,
        disallowed_tools: Vec::new(),
        model: None,
        color: None,
        reasoning_effort: None,
        permissions: AgentPermissionsConfig::new(PermissionDefault::Deny),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: AgentMode::Primary,
        max_turns: None,
        nickname_candidates: Vec::new(),
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: Vec::new(),
        source: SubagentSource::ProjectVtcode,
        file_path: None,
        warnings: Vec::new(),
        tool_policy_overrides: BTreeMap::new(),
    }
}
