use super::*;
use crate::config::TimeoutsConfig;
use crate::config::ToolDocumentationMode as ConfigToolDocumentationMode;
use crate::config::ToolsConfig;
use crate::constants::tools;
use crate::tool_policy::{ToolConstraints, ToolPolicy, ToolPolicyConfig};
use crate::tools::handlers::{
    PlanningWorkflowState, SessionSurface, SessionToolsConfig, TaskTrackerTool, ToolModelCapabilities, ToolProfile,
};
use crate::tools::registry::mcp_helpers::normalize_mcp_tool_identifier;
use crate::tools::traits::Tool;
use anyhow::Result;
use async_trait::async_trait;
use futures::future::BoxFuture;
use serde_json::Value;
use serde_json::json;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tempfile::TempDir;
use vtcode_commons::canonicalize;

const CUSTOM_TOOL_NAME: &str = "custom_test_tool";

#[tokio::test]
async fn public_decision_requires_canonical_task_and_validates_even_without_evidence() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let args = json!({"summary":"Reuse existing parser","rationale":"Preserve validated input handling"});
    assert!(registry.record_decision_executor(args.clone()).await.is_err());
    registry.set_harness_task(Some("task-test".into()));
    assert!(registry.record_decision_executor(args.clone()).await.is_err());
    let called = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&called);
    registry.set_decision_evidence_validator(Arc::new(move |task, ids| {
        assert_eq!(task, "task-test");
        assert!(ids.is_empty());
        observed.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }));
    assert_eq!(registry.record_decision_executor(args.clone()).await?["recorded"], true);
    assert_eq!(called.load(Ordering::SeqCst), 1);
    let mut budgeted_args = args.clone();
    budgeted_args["max_output_tokens"] = json!(100);
    assert_eq!(registry.execute_tool(tools::RECORD_DECISION, budgeted_args.clone()).await?["recorded"], true);
    assert_eq!(called.load(Ordering::SeqCst), 2);
    budgeted_args["max_output_tokens"] = json!(0);
    assert!(registry.execute_tool(tools::RECORD_DECISION, budgeted_args).await.is_err());
    assert_eq!(called.load(Ordering::SeqCst), 2);
    registry.set_tool_policy(tools::RECORD_DECISION, ToolPolicy::Deny).await?;
    assert!(registry.execute_tool(tools::RECORD_DECISION, args.clone()).await.is_err());
    assert_eq!(called.load(Ordering::SeqCst), 2);
    registry.set_tool_policy(tools::RECORD_DECISION, ToolPolicy::Allow).await?;
    registry.set_harness_task(Some("next-task".into()));
    let observed = Arc::clone(&called);
    registry.set_decision_evidence_validator(Arc::new(move |task, _| {
        assert_eq!(task, "next-task");
        observed.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { anyhow::bail!("canonical persistence failed") })
    }));
    let failed = registry.execute_tool(tools::RECORD_DECISION, args).await?;
    assert!(failed.get("error").is_some(), "{failed}");
    assert_eq!(called.load(Ordering::SeqCst), 3);
    Ok(())
}

#[tokio::test]
async fn rejected_exec_policy_is_not_a_lost_execution_result() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    let mut commands = registry.commands_config();
    commands.deny_list.push("rustc".to_string());
    registry.apply_commands_config(&commands);
    let response = registry
        .execute_tool(tools::EXEC_COMMAND, json!({"cmd": "rustc --version"}))
        .await?;
    assert_eq!(response["error"]["error_type"], "PolicyViolation");
    assert!(response.get("exit_code").is_none());
    assert!(registry.in_progress_exec_sessions(32).await.is_empty());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn filtered_verifier_reports_checker_and_filter_failures_without_masking() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;
    fs::create_dir(temp_dir.path().join("scripts"))?;
    for (checker_exit, filter, expected_exit, raw_alias) in [
        (7, "grep README", 7, false),
        (0, "grep README", 0, false),
        (0, "grep missing-pattern", 1, false),
        (0, "grep -E '['", 2, false),
        (7, "grep README", 7, true),
        (0, "grep README", 0, true),
    ] {
        fs::write(
            temp_dir.path().join("scripts/check_markdown.py"),
            format!(
                "import sys\nprint('README: asymmetric evidence')\nprint('unrelated evidence')\nsys.exit({checker_exit})\n"
            ),
        )?;
        let command = format!("python3 scripts/check_markdown.py 2>&1 | {filter}");
        let mut args = json!({"cmd":command, "yield_time_ms":1000});
        if raw_alias {
            args["raw_command"] = json!(command);
        }
        let result = registry.execute_tool(tools::EXEC_COMMAND, args).await?;
        assert_eq!(result["exit_code"], expected_exit, "{result}");
        let output = result["output"].as_str().unwrap();
        assert!(!output.contains("unrelated evidence"), "{output}");
        assert_eq!(output.contains("README: asymmetric evidence"), filter == "grep README", "{output}");
    }
    let mut commands = registry.commands_config();
    commands.deny_list.push("python3".into());
    registry.apply_commands_config(&commands);
    let denied = registry
        .execute_tool(tools::EXEC_COMMAND, json!({"cmd":"python3 scripts/check_markdown.py | grep README"}))
        .await?;
    assert_eq!(denied["error"]["error_type"], "PolicyViolation");
    assert!(denied.get("exit_code").is_none());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn public_pipe_stdin_defaults_to_eof_and_opt_in_accepts_input() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;
    let eof = registry
        .execute_tool(
            tools::EXEC_COMMAND,
            json!({
                "cmd": "if read value; then printf 'unexpected:%s' \"$value\"; else printf 'stdin-eof'; fi",
                "shell": "/bin/sh", "yield_time_ms": 1000,
            }),
        )
        .await?;
    assert_eq!(eof["exit_code"], 0);
    assert!(eof["output"].as_str().unwrap().contains("stdin-eof"));

    let active = registry
        .execute_tool(
            tools::EXEC_COMMAND,
            json!({
                "cmd": "read value; printf 'received:%s' \"$value\"",
                "shell": "/bin/sh", "stdin": true, "yield_time_ms": 250,
            }),
        )
        .await?;
    assert!(active.get("exit_code").is_none());
    let session_id = active["session_id"].as_str().unwrap();
    let written = registry
        .execute_tool(
            tools::WRITE_STDIN,
            json!({
                "session_id": session_id, "chars": "hello\n", "yield_time_ms": 1000,
            }),
        )
        .await?;
    assert_eq!(written["exit_code"], 0);
    assert!(written["output"].as_str().unwrap().contains("received:hello"));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn public_session_controls_inspect_terminate_and_close() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;
    let active = registry
        .execute_tool(
            tools::EXEC_COMMAND,
            json!({
                "cmd": "printf 'retained-evidence\\n'; sleep 60", "shell": "/bin/sh",
                "background": true, "yield_time_ms": 250,
            }),
        )
        .await?;
    let session_id = active["session_id"].as_str().unwrap();
    let inspection = registry
        .execute_tool(
            tools::WRITE_STDIN,
            json!({
                "session_id": session_id, "action": "inspect",
            }),
        )
        .await?;
    assert_eq!(inspection["content_type"], "exec_inspect");
    let terminated = registry
        .execute_tool(
            tools::WRITE_STDIN,
            json!({
                "session_id": session_id, "action": "terminate", "yield_time_ms": 1000,
            }),
        )
        .await?;
    assert!(terminated.get("exit_code").is_some(), "{terminated}");
    let closed = registry
        .execute_tool(
            tools::WRITE_STDIN,
            json!({
                "session_id": session_id, "action": "close",
            }),
        )
        .await?;
    assert_eq!(closed["success"], true);
    assert!(registry.exec_sessions.snapshot_session(session_id).await.is_err());
    let missing = registry
        .execute_tool(
            tools::WRITE_STDIN,
            json!({
                "session_id": session_id, "action": "close",
            }),
        )
        .await?;
    assert!(missing["error"]["message"].as_str().unwrap().contains("not found"));
    Ok(())
}
const SLOW_TIMEOUT_TOOL_NAME: &str = "slow_timeout_test_tool";
const REENTRANT_TOOL_NAME: &str = "reentrant_guard_test_tool";
const MUTUAL_REENTRANT_TOOL_A: &str = "mutual_reentrant_tool_a";
const MUTUAL_REENTRANT_TOOL_B: &str = "mutual_reentrant_tool_b";
const REPLACE_DISPATCH_TOOL_NAME: &str = "replace_dispatch_test_tool";

struct CustomEchoTool;
struct SlowTimeoutTool;

#[async_trait]
impl Tool for CustomEchoTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        Ok(json!({
            "success": true,
            "args": args,
        }))
    }

    fn name(&self) -> &str {
        CUSTOM_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Custom echo tool for testing"
    }
}

#[async_trait]
impl Tool for SlowTimeoutTool {
    async fn execute(&self, _args: Value) -> Result<Value> {
        tokio::time::sleep(Duration::from_millis(1_100)).await;
        Ok(json!({
            "ok": true,
        }))
    }

    fn name(&self) -> &str {
        SLOW_TIMEOUT_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Tool that intentionally exceeds low timeout ceilings"
    }
}

fn reentrant_tool_executor<'a>(registry: &'a ToolRegistry, args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async move { registry.execute_tool_ref(REENTRANT_TOOL_NAME, &args).await })
}

fn mutual_reentrant_tool_a_executor<'a>(registry: &'a ToolRegistry, args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async move { registry.execute_tool_ref(MUTUAL_REENTRANT_TOOL_B, &args).await })
}

fn mutual_reentrant_tool_b_executor<'a>(registry: &'a ToolRegistry, args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async move { registry.execute_tool_ref(MUTUAL_REENTRANT_TOOL_A, &args).await })
}

fn replacement_first_executor<'a>(_registry: &'a ToolRegistry, _args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async move { Ok(json!({"version": 1})) })
}

fn replacement_second_executor<'a>(_registry: &'a ToolRegistry, _args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async move { Ok(json!({"version": 2})) })
}

fn catalogue_race_executor<'a>(_registry: &'a ToolRegistry, _args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async { Ok(json!({"status": "ok"})) })
}

fn advanced_session_tools_config() -> SessionToolsConfig {
    SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ConfigToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    )
    .with_tool_profile(ToolProfile::AdvancedVtCode)
}

async fn policy_catalogue_test_hooks(registry: &ToolRegistry) -> Arc<policy::PolicyCatalogueTestHooks> {
    registry.policy_gateway.full_auto_catalogue_test_hooks()
}

async fn wait_for_catalogue_pause(pause: &policy::PolicyCatalogueTestPause) {
    tokio::time::timeout(Duration::from_secs(5), pause.wait_until_reached())
        .await
        .expect("catalogue operation reached the controlled pause");
}

async fn assert_catalogue_task_remains_pending<T>(task: &mut tokio::task::JoinHandle<T>, task_name: &str) {
    tokio::select! {
        outcome = &mut *task => match outcome {
            Ok(_) => panic!("{task_name} completed while catalogue refresh was paused"),
            Err(error) => panic!(
                "{task_name} failed while catalogue refresh was paused: {error}"
            ),
        },
        () = tokio::time::sleep(Duration::from_millis(50)) => {}
    }
}

#[tokio::test]
async fn registers_builtin_tools() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let available = registry.available_tools().await;

    assert!(available.contains(&tools::EXEC_COMMAND.to_string()));
    assert!(available.contains(&tools::WRITE_STDIN.to_string()));
    assert!(available.contains(&tools::APPLY_PATCH.to_string()));
    assert!(available.contains(&tools::SEARCH_TOOLS.to_string()));
    assert!(!available.contains(&tools::CODE_SEARCH.to_string()));
    assert!(!available.contains(&tools::UNIFIED_SEARCH.to_string()));
    assert!(!available.contains(&tools::UNIFIED_FILE.to_string()));
    assert!(!available.contains(&tools::UNIFIED_EXEC.to_string()));
    assert!(!available.contains(&tools::READ_FILE.to_string()));
    assert!(!available.contains(&tools::WRITE_FILE.to_string()));
    assert!(!available.contains(&tools::DELETE_FILE.to_string()));
    assert!(!available.contains(&tools::MOVE_FILE.to_string()));
    assert!(!available.contains(&tools::COPY_FILE.to_string()));
    assert!(!available.contains(&tools::RUN_PTY_CMD.to_string()));
    Ok(())
}

#[tokio::test]
async fn request_user_input_aliases_are_not_registered() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    assert!(registry.get_tool(tools::REQUEST_USER_INPUT).is_some());
    assert!(registry.get_tool(tools::ASK_QUESTIONS).is_none());
    assert!(registry.get_tool(tools::ASK_USER_QUESTION).is_none());

    Ok(())
}

#[tokio::test]
async fn public_tool_projections_stay_in_sync() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let config = SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ConfigToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    );

    let names = registry
        .public_tool_names(SessionSurface::Interactive, CapabilityLevel::CodeSearch)
        .await;
    let schema_names = registry
        .schema_entries(config.clone())
        .await
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    let declaration_names = registry
        .function_declarations(config.clone())
        .await
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    let model_tool_names = registry
        .model_tools(config)
        .await
        .into_iter()
        .map(|tool| tool.function_name().to_string())
        .collect::<Vec<_>>();

    assert_eq!(schema_names, names);
    assert_eq!(declaration_names, names);
    assert_eq!(model_tool_names, names);
    assert_eq!(
        names,
        vec![
            tools::APPLY_PATCH.to_string(),
            tools::EXEC_COMMAND.to_string(),
            tools::WRITE_STDIN.to_string(),
            tools::SEARCH_TOOLS.to_string(),
            tools::RECORD_DECISION.to_string(),
        ]
    );
    for removed_tool in [
        tools::UNIFIED_EXEC,
        tools::UNIFIED_FILE,
        tools::UNIFIED_SEARCH,
        tools::LIST_FILES,
        tools::READ_FILE,
        tools::WRITE_FILE,
        tools::DELETE_FILE,
        tools::MOVE_FILE,
        tools::COPY_FILE,
    ] {
        assert!(
            registry.get_tool_schema(removed_tool).await.is_none(),
            "{removed_tool} schema should not be discoverable"
        );
        assert!(!registry.has_tool(removed_tool).await, "{removed_tool} should not be reported as available");
    }

    let code_search_schema = registry
        .get_tool_schema(tools::CODE_SEARCH)
        .await
        .expect("code_search schema should be discoverable on request");
    let code_search_parameters = &code_search_schema["parameters"];
    assert_eq!(code_search_parameters["required"], json!(["query"]));
    assert_eq!(code_search_parameters["additionalProperties"], false);
    let mut property_names = code_search_parameters["properties"]
        .as_object()
        .expect("code_search properties")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    property_names.sort_unstable();
    assert_eq!(property_names, ["file_types", "max_results", "path", "query", "result_types"]);

    Ok(())
}

#[tokio::test]
async fn advanced_profile_exposes_code_search_without_removed_public_names() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let config = SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ConfigToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    )
    .with_tool_profile(ToolProfile::AdvancedVtCode);

    let names = registry
        .schema_entries(config.clone())
        .await
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    assert!(names.contains(&tools::CODE_SEARCH.to_string()), "advanced profile should expose code_search");
    for removed_tool in [
        tools::UNIFIED_EXEC,
        tools::UNIFIED_FILE,
        tools::UNIFIED_SEARCH,
        tools::READ_FILE,
        tools::WRITE_FILE,
        tools::DELETE_FILE,
        tools::MOVE_FILE,
        tools::COPY_FILE,
    ] {
        assert!(
            !names.contains(&removed_tool.to_string()),
            "{removed_tool} must not be exposed in the advanced profile"
        );
    }

    let model_tool_names = registry
        .model_tools(config)
        .await
        .into_iter()
        .map(|tool| tool.function_name().to_string())
        .collect::<Vec<_>>();
    assert!(model_tool_names.contains(&tools::CODE_SEARCH.to_string()));
    assert!(!model_tool_names.contains(&tools::UNIFIED_SEARCH.to_string()));

    Ok(())
}

#[tokio::test]
async fn registered_planning_task_tracker_exposes_planning_index_contract() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let planning_config = SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ConfigToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    )
    .with_tool_profile(ToolProfile::AdvancedVtCode)
    .with_planning_active(true);
    let planning_entry = registry
        .schema_entries(planning_config)
        .await
        .into_iter()
        .find(|entry| entry.name == tools::TASK_TRACKER)
        .expect("planning task tracker schema");

    assert_eq!(planning_entry.parameters["properties"]["index"]["minimum"], 1);
    assert_eq!(planning_entry.parameters["properties"]["index_path"]["pattern"], "^[1-9][0-9]*(\\.[1-9][0-9]*)*$");

    let standard_state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
    let standard_tool = TaskTrackerTool::new(temp_dir.path().to_path_buf(), standard_state);
    let standard_parameters = standard_tool.parameter_schema().expect("standard task tracker schema");
    assert_eq!(standard_parameters["properties"]["index"]["minimum"], 0);
    assert_eq!(standard_parameters["properties"]["index_path"]["pattern"], "^[1-9][0-9]*$");
    Ok(())
}

#[tokio::test]
async fn get_tool_schema_uses_runtime_planning_state_for_task_tracker() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let standard_schema = registry
        .get_tool_schema(tools::TASK_TRACKER)
        .await
        .expect("standard task_tracker schema");
    assert_eq!(
        standard_schema["description"].as_str(),
        Some(crate::tools::handlers::task_tracker::task_tracker_description_for_workflow(false))
    );
    assert_eq!(standard_schema["parameters"]["properties"]["index"]["minimum"], 0);
    assert_eq!(standard_schema["parameters"]["properties"]["index_path"]["pattern"], "^[1-9][0-9]*$");

    registry.enable_planning();
    registry.planning_workflow_state().enable();

    let planning_schema = registry
        .get_tool_schema(tools::TASK_TRACKER)
        .await
        .expect("planning task_tracker schema");
    assert_eq!(
        planning_schema["description"].as_str(),
        Some(crate::tools::handlers::task_tracker::task_tracker_description_for_workflow(true))
    );
    assert_eq!(planning_schema["parameters"]["properties"]["index"]["minimum"], 1);
    assert_eq!(
        planning_schema["parameters"]["properties"]["index_path"]["pattern"],
        "^[1-9][0-9]*(\\.[1-9][0-9]*)*$"
    );

    Ok(())
}

#[tokio::test]
async fn public_routing_keeps_aliases_private_and_rebuilds_on_dynamic_updates() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let test_file = temp_dir.path().join("alias-read.txt");
    fs::write(&test_file, "via alias\n")?;

    let err = registry
        .execute_public_tool_ref(tools::READ_FILE, &json!({"path": test_file.to_string_lossy().to_string()}))
        .await
        .expect_err("read_file should not resolve through public routing");
    assert!(err.to_string().contains("Unknown tool"));

    registry
        .register_tool(
            ToolRegistration::from_tool_instance(CUSTOM_TOOL_NAME, CapabilityLevel::CodeSearch, CustomEchoTool)
                .with_description("Custom echo tool for testing")
                .with_parameter_schema(json!({
                    "type": "object",
                    "properties": {
                        "input": {"type": "string"}
                    }
                }))
                .with_aliases(["custom_tool_alias"]),
        )
        .await?;

    registry.allow_all_tools().await?;

    let public_names = registry
        .public_tool_names(SessionSurface::Interactive, CapabilityLevel::CodeSearch)
        .await;
    assert!(!public_names.contains(&CUSTOM_TOOL_NAME.to_string()));
    assert!(!public_names.contains(&"custom_tool_alias".to_string()));

    let schema_names = registry
        .schema_entries(SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ConfigToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        ))
        .await
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    assert!(!schema_names.contains(&CUSTOM_TOOL_NAME.to_string()));
    assert!(!schema_names.contains(&"custom_tool_alias".to_string()));

    let dynamic_result = registry
        .execute_public_tool_ref("custom_tool_alias", &json!({"input": "value"}))
        .await?;
    assert_eq!(dynamic_result["success"].as_bool(), Some(true));

    registry.unregister_tool(CUSTOM_TOOL_NAME).await?;

    let public_names = registry
        .public_tool_names(SessionSurface::Interactive, CapabilityLevel::CodeSearch)
        .await;
    assert!(!public_names.contains(&CUSTOM_TOOL_NAME.to_string()));

    let err = registry
        .execute_public_tool_ref("custom_tool_alias", &json!({"input": "value"}))
        .await
        .expect_err("alias should be removed with the registration");
    assert!(err.to_string().contains("Unknown tool"));

    Ok(())
}

#[tokio::test]
async fn allows_registering_custom_tools() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry
        .register_tool(
            ToolRegistration::from_tool_instance(CUSTOM_TOOL_NAME, CapabilityLevel::CodeSearch, CustomEchoTool)
                .with_parameter_schema(json!({
                    "type": "object",
                    "properties": {
                        "input": {"type": "string"}
                    }
                })),
        )
        .await?;

    registry.allow_all_tools().await?;

    let available = registry.available_tools().await;
    assert!(!available.contains(&CUSTOM_TOOL_NAME.to_string()));

    let response = registry.execute_tool(CUSTOM_TOOL_NAME, json!({"input": "value"})).await?;
    assert!(response["success"].as_bool().unwrap_or(false));
    Ok(())
}

#[tokio::test]
async fn dynamic_tool_registration_keeps_policy_catalog_in_sync() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let policy_path = temp_dir.path().join("tool-policy.json");
    let policy_manager = crate::tool_policy::ToolPolicyManager::new_with_config_path(&policy_path).await?;
    let registry = ToolRegistry::new_with_custom_policy(temp_dir.path().to_path_buf(), policy_manager).await;

    registry
        .register_tool(ToolRegistration::from_tool_instance(
            CUSTOM_TOOL_NAME,
            CapabilityLevel::CodeSearch,
            CustomEchoTool,
        ))
        .await?;

    let config: ToolPolicyConfig = serde_json::from_str(&fs::read_to_string(&policy_path)?)?;
    assert!(config.available_tools.contains(&CUSTOM_TOOL_NAME.to_string()));

    registry.unregister_tool(CUSTOM_TOOL_NAME).await?;

    let config: ToolPolicyConfig = serde_json::from_str(&fs::read_to_string(&policy_path)?)?;
    assert!(!config.available_tools.contains(&CUSTOM_TOOL_NAME.to_string()));

    Ok(())
}

#[tokio::test]
async fn duplicate_registration_replaces_schema_and_runtime_dispatch() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry
        .register_tool(
            ToolRegistration::new(
                REPLACE_DISPATCH_TOOL_NAME,
                CapabilityLevel::CodeSearch,
                false,
                replacement_first_executor,
            )
            .with_description("first version")
            .with_parameter_schema(json!({
                "type": "object",
                "properties": {
                    "old": {"type": "string"}
                }
            })),
        )
        .await?;
    registry
        .register_tool(
            ToolRegistration::new(
                REPLACE_DISPATCH_TOOL_NAME,
                CapabilityLevel::CodeSearch,
                false,
                replacement_second_executor,
            )
            .with_description("second version")
            .with_parameter_schema(json!({
                "type": "object",
                "properties": {
                    "new": {"type": "string"}
                }
            })),
        )
        .await?;
    registry.allow_all_tools().await?;

    let schema = registry
        .get_tool_schema(REPLACE_DISPATCH_TOOL_NAME)
        .await
        .expect("replacement schema should be present");
    assert_eq!(schema.pointer("/parameters/properties/new/type"), Some(&json!("string")));
    assert!(schema.pointer("/parameters/properties/old").is_none());

    let response = registry.execute_tool(REPLACE_DISPATCH_TOOL_NAME, json!({})).await?;
    assert_eq!(response.get("version"), Some(&json!(2)));

    Ok(())
}

#[tokio::test]
async fn executes_prevalidated_tool_path() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry
        .register_tool(ToolRegistration::from_tool_instance(
            CUSTOM_TOOL_NAME,
            CapabilityLevel::CodeSearch,
            CustomEchoTool,
        ))
        .await?;
    registry.allow_all_tools().await?;

    let args = json!({"input": "value"});
    let response = registry.execute_tool_ref_prevalidated(CUSTOM_TOOL_NAME, &args).await?;
    assert!(response["success"].as_bool().unwrap_or(false));

    Ok(())
}

#[tokio::test]
async fn harness_exec_reuses_public_output_normalization() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let response = registry
        .execute_harness_command_session(json!({
            "action": "run",
            "command": "printf vtcode",
            "tty": false,
            "yield_time_ms": 1000
        }))
        .await?;

    assert_eq!(response["output"].as_str(), Some("vtcode"));
    assert!(response.get("stdout").is_none());

    Ok(())
}

#[tokio::test]
async fn harness_terminal_runs_retain_completed_sessions_until_close() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    let active_pty_sessions = Arc::new(AtomicUsize::new(0));
    registry.set_active_pty_sessions(Arc::clone(&active_pty_sessions));

    let response = registry
        .execute_harness_command_session_terminal_run(json!({
            "action": "run",
            "command": ["/bin/sh", "-lc", "printf vtcode-terminal"],
            "tty": true,
            "yield_time_ms": 200,
        }))
        .await?;

    let session_id = response["session_id"]
        .as_str()
        .expect("terminal run should expose session_id")
        .to_string();
    assert_eq!(response["exit_code"], 0);
    assert_eq!(response["output"].as_str(), Some("vtcode-terminal"));
    // A completed command releases its foreground-PTY count immediately; the
    // retention contract applies to the session record, not the live count.
    assert_eq!(active_pty_sessions.load(Ordering::Relaxed), 0);
    assert_eq!(registry.harness_exec_session_completed(&session_id).await?, Some(0));

    registry.close_harness_exec_session(&session_id).await?;
    assert_eq!(active_pty_sessions.load(Ordering::Relaxed), 0);
    registry.harness_exec_session_completed(&session_id).await.unwrap_err();

    Ok(())
}

#[tokio::test]
async fn background_terminal_runs_do_not_mark_the_ui_busy() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    let active_pty_sessions = Arc::new(AtomicUsize::new(0));
    registry.set_active_pty_sessions(Arc::clone(&active_pty_sessions));

    let response = registry
        .execute_harness_command_session_terminal_run(json!({
            "action": "run",
            "command": ["/bin/sh", "-lc", "sleep 5"],
            "tty": true,
            "background": true,
            "yield_time_ms": 200,
        }))
        .await?;
    let session_id = response["session_id"]
        .as_str()
        .expect("background terminal run should expose session_id");

    assert_eq!(response["background"], true);
    assert_eq!(active_pty_sessions.load(Ordering::Relaxed), 0);

    registry.close_harness_exec_session(session_id).await?;
    assert_eq!(active_pty_sessions.load(Ordering::Relaxed), 0);
    Ok(())
}

#[tokio::test]
async fn background_exec_returns_reusable_session_metadata_and_drains_on_wait() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let initial = tokio::time::timeout(
        Duration::from_secs(3),
        registry.execute_harness_command_session(json!({
            "action": "run",
            "command": "printf first; sleep 0.4; printf second",
            "tty": false,
            "background": true,
            "yield_time_ms": 250,
        })),
    )
    .await??;
    let session_id = initial["session_id"]
        .as_str()
        .expect("background run should expose session_id")
        .to_string();

    assert_eq!(initial["background"], true);
    assert_eq!(initial["lifecycle_state"], "running");
    assert!(initial["child_pid"].as_u64().is_some_and(|pid| pid > 0));
    assert_eq!(initial["next_wait_args"]["session_id"], session_id);
    assert_eq!(initial["next_continue_args"]["session_id"], session_id);
    assert!(initial["output"].as_str().unwrap_or_default().contains("first"));

    let completed = tokio::time::timeout(
        Duration::from_secs(5),
        registry.execute_harness_command_session(json!({
            "action": "wait",
            "session_id": session_id,
            "wait_timeout_seconds": 3,
        })),
    )
    .await??;
    assert_eq!(completed["background"], true);
    assert_eq!(completed["exit_code"], 0);
    assert!(completed["output"].as_str().unwrap_or_default().contains("second"));

    registry
        .execute_harness_command_session(json!({
            "action": "close",
            "session_id": session_id,
        }))
        .await?;
    Ok(())
}

fn delayed_exec_args(tty: bool, yield_time_ms: u64) -> Value {
    json!({
        "cmd": "printf first && sleep 0.2 && printf second",
        "tty": tty,
        "yield_time_ms": yield_time_ms,
    })
}

fn long_running_exec_args(tty: bool, yield_time_ms: u64) -> Value {
    json!({
        "cmd": "sleep 0.4 && printf second && sleep 0.4 && printf third && sleep 0.4 && printf done",
        "tty": tty,
        "yield_time_ms": yield_time_ms,
    })
}

#[tokio::test]
async fn prevalidated_exec_mode_settles_noninteractive_run() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let response = registry
        .execute_public_tool_ref_prevalidated_with_mode(
            tools::EXEC_COMMAND,
            &delayed_exec_args(false, 50),
            ExecSettlementMode::SettleNonInteractive,
        )
        .await?;

    let output = response["output"].as_str().expect("settled exec output should be text");
    assert!(output.contains("first"));
    assert!(output.contains("second"));
    assert_eq!(response["exit_code"], 0);
    assert!(response.get("next_continue_args").is_none());

    Ok(())
}

#[tokio::test]
async fn prevalidated_exec_mode_settles_pipe_poll_until_exit() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let initial = registry.execute_harness_command_session(delayed_exec_args(false, 50)).await?;
    let session_id = initial["session_id"]
        .as_str()
        .expect("partial run should expose session_id")
        .to_string();
    let initial_output = initial["output"].as_str().unwrap_or_default().to_string();
    if initial.get("next_continue_args").is_none() {
        assert_eq!(initial["exit_code"], 0);
        assert!(initial_output.contains("first"));
        assert!(initial_output.contains("second"));
        return Ok(());
    }
    assert!(initial.get("exit_code").is_none());

    let response = registry
        .execute_public_tool_ref_prevalidated_with_mode(
            tools::WRITE_STDIN,
            &json!({
                "session_id": session_id,
                "chars": "",
                "yield_time_ms": 50,
            }),
            ExecSettlementMode::SettleNonInteractive,
        )
        .await?;

    assert_eq!(response["exit_code"], 0);
    let settled_output = response["output"].as_str().expect("settled poll output should be text");
    assert!(initial_output.contains("second") || settled_output.contains("second"));
    assert!(response.get("next_continue_args").is_none());

    Ok(())
}

#[tokio::test]
async fn prevalidated_exec_mode_keeps_interactive_runs_manual() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let response = registry
        .execute_public_tool_ref_prevalidated_with_mode(
            tools::EXEC_COMMAND,
            &long_running_exec_args(true, 50),
            ExecSettlementMode::SettleNonInteractive,
        )
        .await?;

    assert!(response.get("next_continue_args").is_some());
    assert!(response.get("exit_code").is_none());

    let session_id = response["session_id"]
        .as_str()
        .expect("interactive run should expose session_id")
        .to_string();
    registry
        .execute_harness_command_session(json!({
            "action": "close",
            "session_id": session_id,
        }))
        .await?;

    Ok(())
}

#[tokio::test]
async fn command_session_run_preserves_requested_session_id_for_follow_up_calls() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let mut run_args = long_running_exec_args(true, 10);
    run_args
        .as_object_mut()
        .expect("run args should be an object")
        .insert("session_id".to_string(), json!("check_sh"));

    let initial = registry.execute_harness_command_session(run_args).await?;
    assert_eq!(initial["session_id"], "check_sh");
    assert_eq!(initial["next_continue_args"], json!({ "session_id": "check_sh" }));

    let response = registry
        .execute_harness_command_session(json!({
            "action": "poll",
            "session_id": "check_sh",
            "yield_time_ms": 10,
        }))
        .await?;

    assert!(response.get("output").is_some());
    assert!(response.get("exit_code").is_some() || response.get("next_continue_args").is_some());

    if response.get("exit_code").is_none() {
        registry
            .execute_harness_command_session(json!({
                "action": "close",
                "session_id": "check_sh",
            }))
            .await?;
    }

    Ok(())
}

#[tokio::test]
async fn active_exec_continuations_bypass_identical_call_loop_detection() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;
    registry.execution_history.set_loop_detection_limits(5, 2);

    let initial = registry
        .execute_harness_command_session(long_running_exec_args(false, 10))
        .await?;
    let session_id = initial["session_id"]
        .as_str()
        .expect("partial run should expose session_id")
        .to_string();
    let continue_args = json!({
        "session_id": session_id,
        "chars": "",
        "yield_time_ms": 10,
    });

    let first = registry
        .execute_public_tool_ref_prevalidated(tools::WRITE_STDIN, &continue_args)
        .await?;
    assert_ne!(first.get("loop_detected"), Some(&json!(true)));

    let second = registry
        .execute_public_tool_ref_prevalidated(tools::WRITE_STDIN, &continue_args)
        .await?;
    assert_ne!(second.get("loop_detected"), Some(&json!(true)));

    let third = registry
        .execute_public_tool_ref_prevalidated(tools::WRITE_STDIN, &continue_args)
        .await?;
    assert_ne!(third.get("loop_detected"), Some(&json!(true)));
    assert!(
        third.get("exit_code").is_some() || third.get("next_continue_args").is_some(),
        "continuation should either remain active or complete cleanly"
    );

    Ok(())
}

#[tokio::test]
async fn command_session_accepts_compact_session_alias_for_poll() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let initial = registry
        .execute_harness_command_session(long_running_exec_args(true, 10))
        .await?;
    let session_id = initial["session_id"]
        .as_str()
        .expect("partial run should expose session_id")
        .to_string();

    let response = registry
        .execute_harness_command_session(json!({
            "s": session_id,
            "yield_time_ms": 10
        }))
        .await?;

    assert!(response.get("output").is_some());
    assert!(response.get("exit_code").is_some() || response.get("next_continue_args").is_some());

    Ok(())
}

#[tokio::test]
async fn command_session_inspect_accepts_compact_session_alias() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let initial = registry
        .execute_harness_command_session(long_running_exec_args(true, 10))
        .await?;
    let session_id = initial["session_id"]
        .as_str()
        .expect("partial run should expose session_id")
        .to_string();

    let response = registry
        .execute_harness_command_session(json!({
            "action": "inspect",
            "s": session_id,
            "head_lines": 1,
            "tail_lines": 0
        }))
        .await?;

    assert_eq!(response["content_type"], "exec_inspect");
    assert!(response["output"].is_string());
    assert!(response.get("session_id").is_some());

    Ok(())
}

#[tokio::test]
async fn command_session_inspect_rejects_missing_session_without_waiting() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        registry.execute_harness_command_session(json!({
            "action": "inspect",
            "session_id": "run-missing",
            "yield_time_ms": 5000,
        })),
    )
    .await
    .expect("missing session must fail before the five-second yield");
    let source = result.expect_err("missing session must not look like successful empty output");
    let error = ToolExecutionError::from_anyhow(tools::UNIFIED_EXEC, &source, 0, false, false, None);
    assert!(error.is_exec_session_not_found());
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn command_session_inspect_distinguishes_empty_and_closed_sessions() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry
        .exec_sessions
        .create_pipe_session(
            "run-empty".into(),
            vec!["/bin/sh".into(), "-c".into(), "exit 0".into()],
            temp_dir.path().to_path_buf(),
            Default::default(),
        )
        .await?;
    let args = json!({"action": "inspect", "s": "run-empty", "yield_time_ms": 0});
    let empty = registry.execute_harness_command_session(args.clone()).await?;
    assert_eq!(empty["success"], true);
    assert_eq!(empty["output"], "");
    registry.close_exec_session("run-empty").await?;
    let source = registry
        .execute_harness_command_session(args)
        .await
        .expect_err("closed session is unavailable");
    let error = ToolExecutionError::from_anyhow(tools::UNIFIED_EXEC, &source, 0, false, false, None);
    assert!(error.is_exec_session_not_found());
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn command_session_wait_returns_partial_output_when_session_closes_mid_wait() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;

    // The fixture emits output three times over ~600ms, so the wait loop's
    // 50ms drain cycle provably buffers it long before the fixture closes
    // the session — the same concurrent-removal actor as a TUI force-cancel
    // closing a foreground session under an in-flight wait.
    registry
        .exec_sessions
        .create_pipe_session(
            "run-mid-wait-close".into(),
            vec![
                "/bin/sh".into(),
                "-c".into(),
                "for i in 1 2 3; do echo mid-wait-output; sleep 0.2; done; sleep 30".into(),
            ],
            temp_dir.path().to_path_buf(),
            Default::default(),
        )
        .await?;

    let (close_side, wait_side) = tokio::join!(
        async {
            // 2s leaves a 30x+ margin over the fixture's last emission on a
            // loaded CI runner while keeping the test fast (wait deadline
            // is 30s, so the close always lands mid-wait).
            tokio::time::sleep(Duration::from_millis(2000)).await;
            registry.close_exec_session("run-mid-wait-close").await?;
            anyhow::Ok(())
        },
        registry.execute_harness_command_session(json!({
            "action": "wait",
            "s": "run-mid-wait-close",
            "wait_timeout_seconds": 30,
        }))
    );
    close_side.expect("close side must succeed");

    // join! completes only when both sides settle: the wait either observes
    // the mid-wait close or runs out its own deadline.
    let response = wait_side.expect("a session closed mid-wait must return its buffered output, not an error");
    let output = response["output"].as_str().unwrap_or_default();
    assert!(output.contains("mid-wait-output"), "output buffered before the close must survive: {response}");
    assert!(
        output.contains("closed while waiting"),
        "the response must disclose that the capture is partial: {response}"
    );

    Ok(())
}

#[tokio::test]
async fn mutating_tools_clear_recent_read_reuse_history() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;
    registry.execution_history.set_loop_detection_limits(5, 2);

    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "original")?;

    let read_args = json!({
        "path": test_file.to_string_lossy(),
        "max_bytes": 1000,
    });
    let write_args = json!({
        "path": test_file.to_string_lossy(),
        "content": "modified",
        "mode": "overwrite",
    });

    let first = registry.execute_tool_ref(tools::READ_FILE, &read_args).await?;
    assert_eq!(first["content"], "original");

    let second = registry.execute_tool(tools::READ_FILE, read_args.clone()).await?;
    assert_eq!(second["content"], "original");

    let write_result = registry.execute_tool(tools::WRITE_FILE, write_args).await?;
    assert_eq!(write_result["success"], json!(true));

    let after_write = registry.execute_tool(tools::READ_FILE, read_args).await?;
    assert_eq!(after_write["content"], "modified");
    assert_ne!(after_write.get("reused_recent_result"), Some(&json!(true)));
    assert_ne!(after_write.get("loop_detected"), Some(&json!(true)));

    Ok(())
}

#[tokio::test]
async fn read_only_command_session_results_are_fast_reused() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;

    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "hello")?;

    let cat_args = json!({
        "cmd": format!("cat {}", test_file.to_string_lossy()),
    });

    let first = registry.execute_tool_ref(tools::EXEC_COMMAND, &cat_args).await?;
    assert!(first.get("reused_recent_result").is_none(), "first call should not be reused");

    let second = registry.execute_tool_ref(tools::EXEC_COMMAND, &cat_args).await?;
    assert_eq!(
        second.get("reused_recent_result"),
        Some(&json!(true)),
        "second identical read-only exec call should reuse the first result"
    );

    Ok(())
}

#[tokio::test]
async fn web_fetch_structured_errors_are_not_reused_as_successful_results() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;

    let args = json!({
        "url": "http://example.com",
    });

    let first = registry.execute_tool_ref(tools::WEB_FETCH, &args).await?;
    assert!(
        first
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|error| error.contains("Only HTTPS URLs are allowed")),
        "first web_fetch call should return the structured tool error"
    );

    let second = registry.execute_tool_ref(tools::WEB_FETCH, &args).await?;
    assert!(
        second.get("reused_recent_result").is_none(),
        "failed web_fetch output must not be cached as a successful read-only result"
    );
    assert!(
        second.get("loop_detected").is_none(),
        "failed web_fetch output must not count toward identical successful-call loops"
    );

    Ok(())
}

#[tokio::test]
async fn prevalidated_execution_enforces_planning_workflow_guards() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    registry.enable_planning();
    registry.planning_workflow_state().enable();

    let blocked_path = temp_dir.path().join("blocked.txt");
    let args = json!({
        "path": blocked_path.to_string_lossy().to_string(),
        "content": "should-not-write"
    });

    let err = registry
        .execute_tool_ref_prevalidated(tools::WRITE_FILE, &args)
        .await
        .expect_err("planning workflow should block prevalidated mutating tool call");
    assert!(err.to_string().contains("planning workflow"));
    assert!(!blocked_path.exists());

    Ok(())
}

#[tokio::test]
async fn prevalidated_execution_denies_direct_stdin_and_patch_in_planning_workflow() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    registry.enable_planning();

    let stdin_error = registry
        .execute_tool_ref_prevalidated(
            tools::WRITE_STDIN,
            &json!({"session_id": "planning-session", "chars": "echo blocked\n"}),
        )
        .await
        .expect_err("direct write_stdin must remain blocked during planning");
    assert!(stdin_error.to_string().contains("planning workflow"));

    let patch_error = registry
        .execute_tool_ref_prevalidated(
            tools::APPLY_PATCH,
            &json!({
                "patch": "*** Begin Patch\n*** Add File: blocked-by-planning.txt\n+blocked\n*** End Patch"
            }),
        )
        .await
        .expect_err("direct apply_patch must remain blocked during planning");
    assert!(patch_error.to_string().contains("planning workflow"));
    assert!(!temp_dir.path().join("blocked-by-planning.txt").exists());

    Ok(())
}

#[tokio::test]
async fn prevalidated_execution_allows_task_tracker_in_planning_workflow() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    registry.enable_planning();
    registry.planning_workflow_state().enable();

    let plans_dir = temp_dir.path().join(".vtcode").join("plans");
    fs::create_dir_all(&plans_dir)?;
    let plan_file = plans_dir.join("adaptive-test.md");
    fs::write(&plan_file, "# Adaptive test\n")?;
    registry.planning_workflow_state().set_plan_file(Some(plan_file)).await;

    let args = json!({"action": "create", "items": ["Track step"]});

    let response = registry
        .execute_tool_ref_prevalidated(tools::TASK_TRACKER, &args)
        .await
        .expect("task_tracker should be allowed in planning workflow");
    assert_eq!(response["status"], "created");

    Ok(())
}

#[tokio::test]
async fn preflight_task_tracker_schema_tracks_planning_state() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry.preflight_validate_call(
        tools::TASK_TRACKER,
        &json!({
            "action": "update",
            "index": 0,
            "status": "completed"
        }),
    )?;

    registry.enable_planning();
    registry.planning_workflow_state().enable();

    let plans_dir = temp_dir.path().join(".vtcode").join("plans");
    fs::create_dir_all(&plans_dir)?;
    let plan_file = plans_dir.join("preflight-schema.md");
    fs::write(&plan_file, "# Preflight schema\n")?;
    registry.planning_workflow_state().set_plan_file(Some(plan_file)).await;

    registry.preflight_validate_call(
        tools::TASK_TRACKER,
        &json!({
            "action": "update",
            "index_path": "1.1",
            "status": "completed"
        }),
    )?;

    let error = registry
        .preflight_validate_call(
            tools::TASK_TRACKER,
            &json!({
                "action": "update",
                "index": 0,
                "status": "completed"
            }),
        )
        .expect_err("planning schema should reject index 0");
    let error_text = error.to_string();
    assert!(error_text.contains("Invalid arguments for tool 'task_tracker'"));
    assert!(error_text.contains("index"));

    Ok(())
}

#[tokio::test]
async fn preflight_task_tracker_create_rejects_update_fields_actionably() -> Result<()> {
    // Regression for session-vtcode-20261005T085022Z: a `create` bundled with
    // `index`/`index_path`/`status` previously failed with a bare
    // `field '(root)' failed validation: {...}` dump. It must name the
    // forbidden fields and point at the action-aware correction.
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let error = registry
        .preflight_validate_call(
            tools::TASK_TRACKER,
            &json!({
                "action": "create",
                "title": "README improvement plan",
                "items": ["Review README structure"],
                "index_path": "1",
                "index": 1,
                "status": "completed",
                "description": ""
            }),
        )
        .expect_err("create must reject index/index_path");
    let error_text = error.to_string();
    assert!(error_text.contains("Invalid arguments for tool 'task_tracker'"), "{error_text}");
    assert!(error_text.contains("must not include"), "{error_text}");
    assert!(error_text.contains("\"index\""), "{error_text}");
    assert!(error_text.contains("\"index_path\""), "{error_text}");
    assert!(!error_text.contains("failed validation: {\"action\""), "{error_text}");
    assert!(error_text.contains("For create, index/index_path are forbidden"), "{error_text}");

    Ok(())
}

#[tokio::test]
async fn preflight_task_tracker_value_error_omits_shape_correction() -> Result<()> {
    // A single-field value error (bad enum) must not carry the shape
    // correction: telling the model to remove index fields would point at
    // the wrong axis. The enum message alone must identify the field.
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let error = registry
        .preflight_validate_call(
            tools::TASK_TRACKER,
            &json!({
                "action": "update",
                "index": 1,
                "status": "done"
            }),
        )
        .expect_err("bad status enum must fail");
    let error_text = error.to_string();
    assert!(error_text.contains("is not one of the allowed enum"), "{error_text}");
    assert!(error_text.contains("/status"), "{error_text}");
    assert!(!error_text.contains("For create, index/index_path are forbidden"), "{error_text}");

    Ok(())
}

#[tokio::test]
async fn preflight_rejects_removed_exec_code_alias() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let err = registry
        .preflight_validate_call(
            "exec_code",
            &json!({
                "command": "echo vtcode"
            }),
        )
        .expect_err("exec_code alias should be rejected");
    assert!(err.to_string().contains("Unknown tool"));

    Ok(())
}

#[tokio::test]
async fn preflight_rejects_removed_humanized_exec_label_alias() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let err = registry
        .preflight_validate_call(
            "Exec code",
            &json!({
                "command": "echo vtcode"
            }),
        )
        .expect_err("Exec code alias should be rejected");
    assert!(err.to_string().contains("Unknown tool"));

    Ok(())
}

#[tokio::test]
async fn preflight_rejects_removed_execute_code_alias() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let err = registry
        .preflight_validate_call(
            tools::EXECUTE_CODE,
            &json!({
                "code": "print('vtcode')"
            }),
        )
        .expect_err("execute_code alias should be rejected");
    assert!(err.to_string().contains("Unknown tool"));

    Ok(())
}

#[tokio::test]
async fn preflight_normalizes_raw_apply_patch_payload_to_input_object() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let patch = "*** Begin Patch\n*** End Patch\n";

    let outcome = registry.preflight_validate_call(tools::APPLY_PATCH, &json!(patch))?;

    assert_eq!(outcome.normalized_tool_name, tools::APPLY_PATCH);
    assert_eq!(outcome.effective_args, json!({ "input": patch }));

    Ok(())
}

#[tokio::test]
async fn preflight_rejects_repo_browser_file_aliases() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let read_err = registry
        .preflight_validate_call("repo_browser.read_file", &json!({"path": "crates/codegen/vtcode-core/src/lib.rs"}))
        .expect_err("repo_browser.read_file alias should be rejected");
    assert!(read_err.to_string().contains("Unknown tool"));

    let list_err = registry
        .preflight_validate_call("repo_browser.list_files", &json!({"path": "crates/codegen/vtcode-core/src"}))
        .expect_err("repo_browser.list_files alias should be rejected");
    assert!(list_err.to_string().contains("Unknown tool"));

    Ok(())
}

#[tokio::test]
async fn preflight_rejects_removed_harness_browse_tool_routes() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let read_err = registry
        .preflight_validate_call(tools::READ_FILE, &json!({"path": "crates/codegen/vtcode-core/src/lib.rs"}))
        .expect_err("read_file should be rejected");
    assert!(read_err.to_string().contains("Unknown tool"));

    let list_err = registry
        .preflight_validate_call(
            tools::LIST_FILES,
            &json!({"path": "crates/codegen/vtcode-core/src", "page": 1, "per_page": 20}),
        )
        .expect_err("list_files should be rejected");
    assert!(list_err.to_string().contains("Unknown tool"));

    Ok(())
}

#[tokio::test]
async fn harness_preflight_admits_hidden_file_helpers() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let read = registry
        .preflight_validate_harness_call(tools::READ_FILE, &json!({"path": "crates/codegen/vtcode-core/src/lib.rs"}))
        .expect("harness path should admit read_file");
    assert_eq!(read.normalized_tool_name, tools::READ_FILE);

    let list = registry
        .preflight_validate_harness_call(tools::LIST_FILES, &json!({"path": "crates/codegen/vtcode-core/src"}))
        .expect("harness path should admit list_files");
    assert_eq!(list.normalized_tool_name, tools::LIST_FILES);

    Ok(())
}

#[tokio::test]
async fn harness_preflight_rejects_non_allowlisted_hidden_tool() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    // `get_errors` is a registered, model-hidden builtin that is NOT in the
    // harness-dispatchable allowlist. Even the harness path must reject it,
    // so registering a new `with_llm_visibility(false)` tool cannot silently
    // widen the dispatchable surface.
    let err = registry
        .preflight_validate_harness_call(tools::GET_ERRORS, &json!({}))
        .expect_err("non-allowlisted hidden tool must not be harness-dispatchable");
    assert!(err.to_string().contains("Unknown tool"));

    Ok(())
}

#[tokio::test]
async fn public_execution_rejects_hidden_file_helper_even_when_prevalidated() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;

    // The prevalidated flag is a performance hint (skip re-preflight) and is
    // independent of dispatch authority. The direct model-public entry must
    // still refuse model-hidden file helpers even with prevalidated=true, so
    // a stray prevalidated flag can never widen the public surface.
    let err = registry
        .execute_public_tool_ref_prevalidated(
            tools::READ_FILE,
            &json!({"path": "crates/codegen/vtcode-core/src/lib.rs"}),
        )
        .await
        .expect_err("model-public entry must reject read_file even when prevalidated");
    assert!(err.to_string().contains("Unknown tool"));

    Ok(())
}

#[tokio::test]
async fn preflight_rejects_removed_planning_start_aliases() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let outcome = registry.preflight_validate_call(tools::START_PLANNING, &json!({}))?;
    assert_eq!(outcome.normalized_tool_name, tools::START_PLANNING);

    let tracker_outcome = registry.preflight_validate_call(tools::TASK_TRACKER, &json!({"action": "list"}))?;
    assert_eq!(tracker_outcome.normalized_tool_name, tools::TASK_TRACKER);

    let old_start_name = ["enter", "plan", "mode"].join("_");
    let old_name = registry
        .preflight_validate_call(&old_start_name, &json!({}))
        .expect_err("old planning tool name should be rejected");
    assert!(old_name.to_string().contains("Unknown tool"));

    let old_tracker_name = ["plan", "task", "tracker"].join("_");
    let old_alias = registry
        .preflight_validate_call(&old_tracker_name, &json!({"action": "list"}))
        .expect_err("removed planning alias should be rejected");
    assert!(old_alias.to_string().contains("Unknown tool"));

    Ok(())
}

#[tokio::test]
async fn preflight_rejects_removed_planning_finish_aliases() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let outcome = registry
        .preflight_validate_call("finish_planning", &json!({}))
        .expect_err("removed finish_planning tool should be rejected");
    assert!(outcome.to_string().contains("Unknown tool"));

    let old_finish_name = ["exit", "plan", "mode"].join("_");
    let old_name = registry
        .preflight_validate_call(&old_finish_name, &json!({}))
        .expect_err("old planning tool name should be rejected");
    assert!(old_name.to_string().contains("Unknown tool"));

    let old_alias = registry
        .preflight_validate_call("mode_edit", &json!({}))
        .expect_err("removed planning alias should be rejected");
    assert!(old_alias.to_string().contains("Unknown tool"));

    Ok(())
}

#[tokio::test]
async fn suggest_fallback_prefers_exec_command_for_exec_code_alias() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let fallback = registry.suggest_fallback_tool("exec_code").await;
    assert_eq!(fallback.as_deref(), Some(tools::EXEC_COMMAND));

    Ok(())
}

#[tokio::test]
async fn suggest_fallback_returns_none_for_humanized_exec_label() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let fallback = registry.suggest_fallback_tool("Exec code").await;
    assert_eq!(fallback, None);

    Ok(())
}

#[tokio::test]
async fn suggest_fallback_returns_none_for_task_tracker() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let fallback = registry.suggest_fallback_tool(tools::TASK_TRACKER).await;
    assert!(fallback.is_none());

    Ok(())
}

#[tokio::test]
async fn suggest_fallback_returns_none_for_unknown_tool() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let fallback = registry.suggest_fallback_tool("not_a_real_tool").await;
    assert!(fallback.is_none());

    Ok(())
}

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

#[tokio::test]
async fn apply_patch_alias_executes_without_recursive_reentry() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;

    let patch = "*** Begin Patch\n*** Add File: patched_via_alias.txt\n+patched\n*** End Patch\n";
    let response = registry.execute_tool(tools::APPLY_PATCH, json!({ "patch": patch })).await?;

    assert_eq!(response.get("success").and_then(Value::as_bool), Some(true));
    let expected_path = canonicalize(temp_dir.path().join("patched_via_alias.txt"))?
        .to_string_lossy()
        .into_owned();
    let modified_files = response
        .get("modified_files")
        .and_then(Value::as_array)
        .map_or(&[] as &[Value], |arr| arr)
        .iter()
        .filter_map(|v| v.as_str().map(std::path::PathBuf::from))
        .filter_map(|p| canonicalize(p).ok())
        .filter_map(|p| p.to_str().map(String::from))
        .collect::<Vec<_>>();
    assert_eq!(modified_files, vec![expected_path]);

    let file_contents = fs::read_to_string(temp_dir.path().join("patched_via_alias.txt"))?;
    assert_eq!(file_contents, "patched\n");

    Ok(())
}

#[tokio::test]
async fn apply_patch_response_includes_a_bounded_diff_preview() -> Result<()> {
    let temp_dir = TempDir::new()?;
    fs::write(temp_dir.path().join("README.md"), "before\n")?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let patch = "*** Begin Patch\n*** Update File: README.md\n@@\n-before\n+after\n*** End Patch\n";
    let response = registry.execute_tool(tools::APPLY_PATCH, json!({ "input": patch })).await?;

    let diff = response
        .get("diff")
        .and_then(Value::as_array)
        .and_then(|diffs| diffs.first())
        .and_then(|entry| entry.get("content"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("apply_patch response did not include a diff preview"))?;
    assert!(diff.contains("-before"));
    assert!(diff.contains("+after"));

    Ok(())
}

#[tokio::test]
async fn write_file_response_includes_a_canonical_diff_preview() -> Result<()> {
    let temp_dir = TempDir::new()?;
    fs::write(temp_dir.path().join("README.md"), "before\n")?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let response = registry
        .execute_tool(
            tools::WRITE_FILE,
            json!({
                "path": "README.md",
                "content": "after\n",
                "mode": "overwrite"
            }),
        )
        .await?;

    let diff_entry = response
        .get("diff")
        .and_then(Value::as_array)
        .and_then(|diffs| diffs.first())
        .ok_or_else(|| anyhow::anyhow!("write_file response did not include a canonical diff entry"))?;
    assert_eq!(diff_entry.get("path").and_then(Value::as_str), Some("README.md"));
    assert_eq!(diff_entry.get("operation").and_then(Value::as_str), Some("updated"));
    assert_eq!(diff_entry.get("additions").and_then(Value::as_u64), Some(1));
    assert_eq!(diff_entry.get("deletions").and_then(Value::as_u64), Some(1));
    let diff_content = diff_entry
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("canonical write_file diff did not include content"))?;
    assert!(diff_content.contains("-before"));
    assert!(diff_content.contains("+after"));

    Ok(())
}

#[tokio::test]
async fn write_file_noop_response_exposes_an_empty_canonical_diff_entry() -> Result<()> {
    let temp_dir = TempDir::new()?;
    fs::write(temp_dir.path().join("README.md"), "same\n")?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let response = registry
        .execute_tool(
            tools::WRITE_FILE,
            json!({
                "path": "README.md",
                "content": "same\n",
                "mode": "overwrite"
            }),
        )
        .await?;

    let diff_entry = response
        .get("diff")
        .and_then(Value::as_array)
        .and_then(|diffs| diffs.first())
        .ok_or_else(|| anyhow::anyhow!("no-op write did not include a canonical diff entry"))?;
    assert_eq!(diff_entry.get("is_empty").and_then(Value::as_bool), Some(true));
    assert_eq!(diff_entry.get("operation").and_then(Value::as_str), Some("updated"));

    Ok(())
}

#[tokio::test]
async fn apply_patch_reports_deduplicated_paths_for_every_successful_operation() -> Result<()> {
    let temp_dir = TempDir::new()?;
    fs::create_dir(temp_dir.path().join("src"))?;
    fs::write(temp_dir.path().join("src/delete.rs"), "delete me\n")?;
    fs::write(temp_dir.path().join("src/old.rs"), "old\n")?;
    let canonical_base = canonicalize(temp_dir.path())?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let mut expected = ["add.rs", "delete.rs", "new.rs", "old.rs"]
        .into_iter()
        .map(|name| canonical_base.join("src").join(name).to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    expected.sort();

    let patch = "*** Begin Patch\n*** Add File: src/add.rs\n+new\n*** Delete File: src/delete.rs\n*** Update File: src/old.rs\n*** Move to: src/new.rs\n@@\n-old\n+new\n*** End Patch\n";
    let response = registry.execute_tool(tools::APPLY_PATCH, json!({ "input": patch })).await?;

    let modified_files = response
        .get("modified_files")
        .and_then(Value::as_array)
        .map_or(&[] as &[Value], |arr| arr)
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect::<Vec<_>>();
    assert_eq!(modified_files, expected);
    assert!(temp_dir.path().join("src/add.rs").exists());
    assert!(!temp_dir.path().join("src/delete.rs").exists());
    assert!(!temp_dir.path().join("src/old.rs").exists());
    assert_eq!(fs::read_to_string(temp_dir.path().join("src/new.rs"))?, "new\n");

    Ok(())
}

#[tokio::test]
async fn failed_apply_patch_does_not_report_modified_files() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;

    let outcome = registry
        .execute_public_tool_request(ToolExecutionRequest::new(
            tools::APPLY_PATCH,
            json!({ "input": "*** Begin Patch\n*** Not An Operation\n*** End Patch\n" }),
        ))
        .await;

    assert!(!outcome.is_success());
    assert!(outcome.output.is_none());
    assert!(
        outcome
            .error
            .expect("failed patch should expose an error")
            .to_json_value()
            .get("modified_files")
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn apply_patch_accepts_input_payload() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;

    let patch = "*** Begin Patch\n*** Add File: patched_via_input.txt\n+patched\n*** End Patch\n";
    let response = registry.execute_tool(tools::APPLY_PATCH, json!({ "input": patch })).await?;

    assert_eq!(response.get("success").and_then(Value::as_bool), Some(true));

    let file_contents = fs::read_to_string(temp_dir.path().join("patched_via_input.txt"))?;
    assert_eq!(file_contents, "patched\n");

    Ok(())
}

#[tokio::test]
async fn public_apply_patch_accepts_raw_string_payload() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;

    let patch = "*** Begin Patch\n*** Add File: patched_via_raw_string.txt\n+patched\n*** End Patch\n";
    let response = registry.execute_public_tool_ref(tools::APPLY_PATCH, &json!(patch)).await?;

    assert_eq!(response.get("success").and_then(Value::as_bool), Some(true));

    let file_contents = fs::read_to_string(temp_dir.path().join("patched_via_raw_string.txt"))?;
    assert_eq!(file_contents, "patched\n");

    Ok(())
}

#[tokio::test]
async fn execution_history_records_harness_context() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry.set_harness_session("session-history");
    registry.set_harness_task(Some("task-history".to_owned()));

    registry
        .register_tool(ToolRegistration::from_tool_instance(
            CUSTOM_TOOL_NAME,
            CapabilityLevel::CodeSearch,
            CustomEchoTool,
        ))
        .await?;
    registry.allow_all_tools().await?;

    let args = json!({"input": "value"});
    let response = registry.execute_tool(CUSTOM_TOOL_NAME, args.clone()).await?;
    assert!(response["success"].as_bool().unwrap_or(false));

    let records = registry.get_recent_tool_records(1);
    let record = records.first().expect("execution record captured");
    assert_eq!(record.tool_name, CUSTOM_TOOL_NAME);
    assert_eq!(record.context.session_id, "session-history");
    assert_eq!(record.context.task_id.as_deref(), Some("task-history"));
    assert_eq!(record.args, args);
    assert!(record.success);

    Ok(())
}

#[tokio::test]
async fn reentrancy_guard_blocks_recursive_tool_loops() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry
        .register_tool(ToolRegistration::new(
            REENTRANT_TOOL_NAME,
            CapabilityLevel::CodeSearch,
            false,
            reentrant_tool_executor,
        ))
        .await?;
    registry.allow_all_tools().await?;

    let err = registry
        .execute_tool(REENTRANT_TOOL_NAME, json!({"input": "loop"}))
        .await
        .expect_err("reentrancy guard must reject recursive tool loops");

    let err_msg = err.to_string();
    assert!(err_msg.contains("tool reentrancy blocked"));
    assert!(err.chain().any(|cause| cause.to_string().contains(REENTRANT_TOOL_NAME)));

    Ok(())
}

#[tokio::test]
async fn reentrancy_guard_blocks_cross_tool_cycles() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry
        .register_tool(ToolRegistration::new(
            MUTUAL_REENTRANT_TOOL_A,
            CapabilityLevel::CodeSearch,
            false,
            mutual_reentrant_tool_a_executor,
        ))
        .await?;
    registry
        .register_tool(ToolRegistration::new(
            MUTUAL_REENTRANT_TOOL_B,
            CapabilityLevel::CodeSearch,
            false,
            mutual_reentrant_tool_b_executor,
        ))
        .await?;
    registry.allow_all_tools().await?;

    let err = registry
        .execute_tool(MUTUAL_REENTRANT_TOOL_A, json!({"input": "cycle"}))
        .await
        .expect_err("reentrancy guard must reject cross-tool cycles");

    let err_msg = err.to_string();
    assert!(err_msg.contains("tool reentrancy blocked"));
    assert!(err.chain().any(|cause| cause.to_string().contains(MUTUAL_REENTRANT_TOOL_A)));
    assert!(err.chain().any(|cause| cause.to_string().contains(MUTUAL_REENTRANT_TOOL_B)));

    Ok(())
}

#[tokio::test]
async fn full_auto_allowlist_enforced() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry.enable_full_auto_permission(&[tools::EXEC_COMMAND.to_string()]).await;

    assert!(registry.preflight_tool_permission(tools::EXEC_COMMAND).await?);
    assert!(!registry.preflight_tool_permission(tools::READ_FILE).await?);
    assert!(!registry.preflight_tool_permission(tools::RUN_PTY_CMD).await?);

    Ok(())
}

#[tokio::test]
async fn wildcard_initialisation_retains_registration_from_snapshot_window() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let hooks = policy_catalogue_test_hooks(&registry).await;
    let snapshot_pause = policy::PolicyCatalogueTestPause::default();
    hooks.install_after_enable_snapshot(snapshot_pause.clone());

    let enabling_registry = registry.clone();
    let enable_task = tokio::spawn(async move {
        enabling_registry
            .enable_full_auto_permission_for_session(
                &[tools::WILDCARD_ALL.to_string()],
                advanced_session_tools_config(),
            )
            .await;
    });
    wait_for_catalogue_pause(&snapshot_pause).await;

    let tool_name = "wildcard_snapshot_window_dynamic_tool";
    let registering_registry = registry.clone();
    let registration_task = tokio::spawn(async move {
        registering_registry
            .register_tool(
                ToolRegistration::new(tool_name, CapabilityLevel::Basic, false, catalogue_race_executor)
                    .with_description("tool registered after the wildcard snapshot"),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !registry.has_tool(tool_name).await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("registration entered the wildcard snapshot window");
    assert!(!registration_task.is_finished());

    snapshot_pause.resume();
    enable_task.await.expect("wildcard enable task");
    registration_task.await.expect("registration task")?;

    assert!(registry.is_allowed_in_full_auto(tool_name).await);
    Ok(())
}

#[tokio::test]
async fn in_flight_catalogue_refresh_cannot_restore_wildcard_after_disable() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let config = advanced_session_tools_config();
    registry
        .enable_full_auto_permission_for_session(&[tools::WILDCARD_ALL.to_string()], config)
        .await;

    let hooks = policy_catalogue_test_hooks(&registry).await;
    let refresh_pause = policy::PolicyCatalogueTestPause::default();
    hooks.install_after_refresh_snapshot(refresh_pause.clone());
    let registering_registry = registry.clone();
    let registration_task = tokio::spawn(async move {
        registering_registry
            .register_tool(
                ToolRegistration::new(
                    "disable_during_refresh_dynamic_tool",
                    CapabilityLevel::Basic,
                    false,
                    catalogue_race_executor,
                )
                .with_description("tool whose registration pauses catalogue refresh"),
            )
            .await
    });
    wait_for_catalogue_pause(&refresh_pause).await;

    let disable_pause = policy::PolicyCatalogueTestPause::default();
    hooks.install_before_disable_lifecycle(disable_pause.clone());
    let disabling_registry = registry.clone();
    let mut disable_task = tokio::spawn(async move {
        disabling_registry.disable_full_auto_permission().await;
    });
    wait_for_catalogue_pause(&disable_pause).await;
    disable_pause.resume();
    assert!(!registration_task.is_finished());
    assert_catalogue_task_remains_pending(&mut disable_task, "disable task").await;

    refresh_pause.resume();
    registration_task.await.expect("registration task")?;
    disable_task.await.expect("disable task");

    assert_eq!(registry.current_full_auto_allowlist().await, None);
    Ok(())
}

#[tokio::test]
async fn in_flight_refresh_cannot_overwrite_same_config_explicit_replacement() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let config = advanced_session_tools_config();
    registry
        .enable_full_auto_permission_for_session(&[tools::WILDCARD_ALL.to_string()], config.clone())
        .await;

    let hooks = policy_catalogue_test_hooks(&registry).await;
    let refresh_pause = policy::PolicyCatalogueTestPause::default();
    hooks.install_after_refresh_snapshot(refresh_pause.clone());
    let registering_registry = registry.clone();
    let registration_task = tokio::spawn(async move {
        registering_registry
            .register_tool(
                ToolRegistration::new(
                    "replacement_during_refresh_dynamic_tool",
                    CapabilityLevel::Basic,
                    false,
                    catalogue_race_executor,
                )
                .with_description("tool whose registration pauses catalogue refresh"),
            )
            .await
    });
    wait_for_catalogue_pause(&refresh_pause).await;

    let replacement_pause = policy::PolicyCatalogueTestPause::default();
    hooks.install_before_enable_lifecycle(replacement_pause.clone());
    let replacing_registry = registry.clone();
    let mut replacement_task = tokio::spawn(async move {
        replacing_registry
            .enable_full_auto_permission_for_session(&[tools::EXEC_COMMAND.to_string()], config)
            .await;
    });
    wait_for_catalogue_pause(&replacement_pause).await;
    replacement_pause.resume();
    assert!(!registration_task.is_finished());
    assert_catalogue_task_remains_pending(&mut replacement_task, "replacement task").await;

    refresh_pause.resume();
    registration_task.await.expect("registration task")?;
    replacement_task.await.expect("replacement task");

    assert_eq!(registry.current_full_auto_allowlist().await, Some(vec![tools::EXEC_COMMAND.to_string()]));
    assert!(
        !registry
            .is_allowed_in_full_auto("replacement_during_refresh_dynamic_tool")
            .await
    );
    Ok(())
}

#[test]
fn normalizes_mcp_tool_identifiers() {
    assert_eq!(normalize_mcp_tool_identifier("sequential-thinking"), "sequentialthinking");
    assert_eq!(normalize_mcp_tool_identifier("Context7.Lookup"), "context7lookup");
    assert_eq!(normalize_mcp_tool_identifier("alpha_beta"), "alphabeta");
}

#[test]
fn timeout_policy_derives_from_config() {
    let config = TimeoutsConfig {
        default_ceiling_seconds: 0,
        pty_ceiling_seconds: 600,
        mcp_ceiling_seconds: 90,
        warning_threshold_percent: 75,
        ..Default::default()
    };

    let policy = ToolTimeoutPolicy::from_config(&config);
    assert_eq!(policy.ceiling_for(ToolTimeoutCategory::Default), None);
    assert_eq!(policy.ceiling_for(ToolTimeoutCategory::Pty), Some(Duration::from_secs(600)));
    assert_eq!(policy.ceiling_for(ToolTimeoutCategory::Mcp), Some(Duration::from_secs(90)));
    assert!((policy.warning_fraction() - 0.75).abs() < f32::EPSILON);
}

#[tokio::test]
async fn timeout_errors_are_structured_and_track_failures() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry
        .register_tool(ToolRegistration::from_tool_instance(
            SLOW_TIMEOUT_TOOL_NAME,
            CapabilityLevel::CodeSearch,
            SlowTimeoutTool,
        ))
        .await?;
    registry.allow_all_tools().await?;

    registry.apply_timeout_policy(&TimeoutsConfig {
        default_ceiling_seconds: 1,
        pty_ceiling_seconds: 1,
        mcp_ceiling_seconds: 1,
        ..Default::default()
    });

    let mut policy = ExecutionPolicySnapshot::default().with_max_retries(4);
    policy.retry_base_delay = Duration::from_millis(1);
    policy.retry_max_delay = Duration::from_millis(1);
    policy.retry_multiplier = 1.0;

    let request = ToolExecutionRequest::new(SLOW_TIMEOUT_TOOL_NAME, json!({})).with_policy(policy);
    let outcome = registry.execute_public_tool_request(request).await;

    assert!(!outcome.is_success());
    assert_eq!(outcome.attempts, 5);

    let error = outcome.error.expect("timeout outcome should include error");
    assert_eq!(error.tool_name, SLOW_TIMEOUT_TOOL_NAME);
    assert!(matches!(error.error_type, ToolErrorType::Timeout));
    assert_eq!(error.category, vtcode_commons::ErrorCategory::Timeout);
    assert!(error.is_recoverable);
    assert!(error.retry_after_ms.is_some());
    assert!(error.message.contains("exceeded the standard timeout ceiling"));
    assert_eq!(error.debug_context.as_ref().and_then(|ctx| ctx.surface.as_deref()), Some("tool_registry"));

    let failures = registry.execution_history.get_recent_failures(1);
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].timeout_category.as_deref(), Some("standard"));
    assert_eq!(failures[0].effective_timeout_ms, Some(1_000));

    let consecutive_failures = registry
        .resiliency
        .lock()
        .failure_trackers
        .get(&ToolTimeoutCategory::Default)
        .map(|tracker| tracker.consecutive_failures)
        .unwrap_or(0);
    assert_eq!(consecutive_failures, 5);

    Ok(())
}

#[tokio::test]
async fn code_search_executes_with_policy_capped_max_results() -> Result<()> {
    let temp_dir = TempDir::new()?;
    fs::write(temp_dir.path().join("Widget.rs"), "struct Widget;\n")?;
    let policy_path = temp_dir.path().join("tool-policy.json");
    let mut config = ToolPolicyConfig::default();
    config.policies.insert(tools::CODE_SEARCH.to_string(), ToolPolicy::Allow);
    config.constraints.insert(
        tools::CODE_SEARCH.to_string(),
        ToolConstraints {
            max_results_per_call: Some(1),
            ..ToolConstraints::default()
        },
    );
    fs::write(&policy_path, serde_json::to_vec_pretty(&config)?)?;
    let policy_manager = crate::tool_policy::ToolPolicyManager::new_with_config_path(policy_path).await?;
    let registry = ToolRegistry::new_with_custom_policy(temp_dir.path().to_path_buf(), policy_manager).await;

    let response = registry
        .execute_tool(
            tools::CODE_SEARCH,
            json!({
                "query": "Widget",
                "path": ".",
                "result_types": ["path"],
                "max_results": 50
            }),
        )
        .await?;

    assert_eq!(response["filters"]["max_results"], json!(1));
    assert_eq!(response["returned"], json!(1));
    assert_eq!(response["results"].as_array().map(Vec::len), Some(1));
    Ok(())
}

#[tokio::test]
async fn apply_patch_payload_correction_is_concrete_and_preserves_aliases() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    for args in [
        json!({}),
        json!({"input": 42}),
        json!({"patch": []}),
        json!(null),
        json!({"cmd":"*** Begin Patch\n*** Add File: created.txt\n+wrong field\n*** End Patch\n"}),
    ] {
        let error = registry
            .preflight_validate_call(tools::APPLY_PATCH, &args)
            .expect_err("invalid payload");
        let message = error.to_string();
        assert!(message.contains(r#"{"input":"*** Begin Patch\n"#), "{message}");
        assert!(message.contains("retry once"), "{message}");
        assert!(!message.contains("Missing required argument: patch"), "{message}");
        assert!(!temp_dir.path().join("created.txt").exists());
    }
    for (index, field) in ["input", "patch"].into_iter().enumerate() {
        let patch = format!("*** Begin Patch\n*** Add File: created{index}.txt\n+payload{index}\n*** End Patch\n");
        let outcome = registry
            .execute_public_tool_request(ToolExecutionRequest::new(tools::APPLY_PATCH, json!({field: patch})))
            .await;
        assert!(outcome.is_success(), "{outcome:?}");
        let output = outcome.output.expect("patch result");
        assert!(output["diff"].as_array().is_some_and(|diff| !diff.is_empty()));
        assert!(output["modified_files"].as_array().is_some_and(|paths| paths.len() == 1));
        assert_eq!(
            fs::read_to_string(temp_dir.path().join(format!("created{index}.txt")))?,
            format!("payload{index}\n")
        );
    }
    registry.enable_planning();
    let denied = registry
        .execute_public_tool_request(ToolExecutionRequest::new(
            tools::APPLY_PATCH,
            json!({"input":"*** Begin Patch\n*** Add File: denied.txt\n+no\n*** End Patch\n"}),
        ))
        .await;
    assert!(!denied.is_success());
    assert!(!temp_dir.path().join("denied.txt").exists());
    Ok(())
}

#[tokio::test]
async fn apply_patch_shell_invocations_are_rejected_before_process_creation() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    let executable = temp_dir.path().join("apply_patch");
    let marker = temp_dir.path().join("launched");
    fs::write(&executable, format!("#!/bin/sh\ntouch '{}'\n", marker.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
    }
    for name in [
        "apply_patch".to_string(),
        "'apply_patch'".to_string(),
        "applypatch".to_string(),
        "./applypatch".to_string(),
        format!("'{}'", executable.display()),
    ] {
        let args = json!({"cmd":format!("{name} <<'PATCH'\n*** Begin Patch\n*** End Patch\nPATCH"), "workdir":temp_dir.path(), "yield_time_ms":1000});
        let error = registry
            .preflight_validate_call(tools::EXEC_COMMAND, &args)
            .expect_err("shell patch rejected");
        assert!(error.to_string().contains("apply_patch is a tool"));
        assert!(error.to_string().contains(r#"{"input":"*** Begin Patch\n"#));
        let outcome = registry
            .execute_public_tool_request(ToolExecutionRequest::new(tools::EXEC_COMMAND, args))
            .await;
        assert!(!outcome.is_success(), "shell rejection must happen before launch");
        assert!(!marker.exists(), "no process may launch");
    }
    registry.preflight_validate_call(tools::EXEC_COMMAND, &json!({"cmd":"printf '%s' apply_patch"}))?;
    registry.preflight_validate_call(tools::EXEC_COMMAND, &json!({"cmd":"rg apply_patch src"}))?;
    assert!(registry.exec_sessions.list_sessions().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn apply_patch_context_diagnostics_ignore_quoted_configuration_and_bound_evidence() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    fs::write(temp_dir.path().join("guide.md"), "actual content\n")?;
    for quoted in [
        "vtcode.toml",
        "provider openai authentication API key credentials",
        "permission denied sandbox",
        &"vtcode.toml ".repeat(100),
    ] {
        let patch = format!("*** Begin Patch\n*** Update File: guide.md\n@@\n-{quoted}\n+fixed\n*** End Patch\n");
        let outcome = registry
            .execute_public_tool_request(ToolExecutionRequest::new(tools::APPLY_PATCH, json!({"input":patch})))
            .await;
        let error = outcome.error.expect("mismatch must fail");
        assert_eq!(error.patch_context_mismatch_path(), Some("guide.md"));
        assert_eq!(error.category, vtcode_commons::ErrorCategory::ExecutionError);
        assert!(error.is_recoverable);
        assert!(!error.retryable);
        assert!(error.recovery_suggestions[0].contains("file read limit of 1-200"));
        assert!(!error.recovery_suggestions[0].contains("configuration"));
        let wire = error.to_json_value();
        let evidence = wire["error"]["patch_failure"]["ContextMismatch"]["evidence"]
            .as_str()
            .expect("bounded mismatch evidence");
        assert!(evidence.chars().count() <= 320);
        assert_eq!(
            ToolExecutionError::from_tool_output(&wire)
                .expect("round trip")
                .patch_context_mismatch_path(),
            Some("guide.md")
        );
        assert_eq!(fs::read_to_string(temp_dir.path().join("guide.md"))?, "actual content\n");
        assert!(registry.has_patch_recovery_read(tools::READ_FILE, &json!({"path":"guide.md", "limit":10})));
    }
    Ok(())
}
