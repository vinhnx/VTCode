use super::*;
use crate::config::types::CapabilityLevel;
use crate::tool_policy::ToolPolicy;
use crate::tools::registry::{ToolExecutionRecord, ToolRegistration};
use anyhow::Result;
use futures::future::BoxFuture;
use tempfile::TempDir;

struct OutputCase {
    raw: Value,
    error: Option<&'static str>,
}

#[tokio::test]
async fn tracker_adoption_latches_only_successful_current_request_results() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    for (action, output, is_mcp) in [
        ("list", json!({"status":"updated"}), false),
        ("update", json!({"status":"created"}), false),
        ("update", json!({"status":"updated", "success":false}), false),
        ("update", json!({"status":"updated", "blocked":true}), false),
        ("update", json!({"status":"updated", "not_executed":true}), false),
        ("update", json!({"status":"updated", "error":"rejected"}), false),
        ("update", json!({"status":"updated"}), true),
    ] {
        registry
            .prepare_execution_output(tools::TASK_TRACKER, &json!({"action":action}), output, is_mcp, 2000)
            .await;
        assert!(!registry.tracker_adopted_for_request());
    }
    registry
        .prepare_execution_output(
            tools::TASK_TRACKER,
            &json!({"action":"update"}),
            json!({"status":"updated"}),
            false,
            2000,
        )
        .await;
    assert!(registry.tracker_adopted_for_request());
    registry
        .prepare_execution_output(
            tools::TASK_TRACKER,
            &json!({"action":"list"}),
            json!({"status":"empty"}),
            false,
            2000,
        )
        .await;
    assert!(registry.tracker_adopted_for_request(), "later reads must retain adoption");
    registry.begin_tracker_request(false);
    assert!(!registry.tracker_adopted_for_request());
    registry.begin_tracker_request(true);
    assert!(registry.tracker_adopted_for_request(), "explicit continuation restores adoption");
    Ok(())
}

#[tokio::test]
async fn tracker_adoption_reexecutes_identical_calls_after_a_request_reset() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let args = json!({"action":"create", "title":"Current task", "items":["Finish implementation"]});
    let first = registry.execute_tool(tools::TASK_TRACKER, args.clone()).await?;
    assert_eq!(first["status"], "created");
    assert!(registry.tracker_adopted_for_request());
    registry.begin_tracker_request(false);
    let current = registry.execute_tool(tools::TASK_TRACKER, args).await?;
    assert!(registry.tracker_adopted_for_request(), "a current-request tracker call must execute and adopt");
    assert_eq!(current["status"], "unchanged", "replay must not masquerade as a fresh create");
    assert!(current.get("reused_recent_result").is_none());
    Ok(())
}

#[tokio::test]
async fn prepared_output_distinguishes_payload_errors_from_success() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let cases = [
        OutputCase { raw: json!({"answer": 17}), error: None },
        OutputCase {
            raw: json!({"success": false}),
            error: Some("tool reported success=false"),
        },
        OutputCase {
            raw: json!({"error": "string failure"}),
            error: Some("string failure"),
        },
        OutputCase {
            raw: json!({"success": true, "error": {"message": "object failure", "code": 41}}),
            error: Some("object failure"),
        },
        OutputCase {
            raw: json!({"error": [3, 1]}),
            error: Some("[3,1]"),
        },
        OutputCase { raw: json!({"error": null}), error: Some("null") },
        OutputCase { raw: json!([2, 9]), error: None },
        OutputCase { raw: json!("scalar answer"), error: None },
    ];
    for case in cases {
        let prepared = registry
            .prepare_execution_output("result_probe", &json!({}), case.raw.clone(), false, 2000)
            .await;
        assert_eq!(prepared.structured_error.as_deref(), case.error);
        if case.raw.is_object() {
            assert_eq!(prepared.normalized_value["success"], case.raw.get("success").cloned().unwrap_or(json!(true)));
        } else {
            assert_eq!(prepared.normalized_value, json!({"success": true, "result": case.raw}));
        }
    }
    assert!(registry.execution_history.get_recent_records(10).is_empty());
    Ok(())
}

#[tokio::test]
async fn code_search_omits_success_field_but_preserves_error_evidence() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let raw = json!({"success": false, "error": {"message": "search failed"}, "matches": []});
    let prepared = registry
        .prepare_execution_output(tools::CODE_SEARCH, &json!({}), raw, false, 2000)
        .await;
    assert_eq!(prepared.normalized_value, json!({"error": {"message": "search failed"}, "matches": []}));
    assert_eq!(prepared.structured_error.as_deref(), Some("search failed"));
    let success = registry
        .prepare_execution_output(tools::CODE_SEARCH, &json!({}), json!({"matches": ["a.rs"]}), false, 2000)
        .await;
    assert_eq!(success.normalized_value, json!({"matches": ["a.rs"]}));
    assert!(success.structured_error.is_none());
    Ok(())
}

#[tokio::test]
async fn spool_inspection_never_creates_nested_spools_for_object_scalar_or_array() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let body = "evidence ".repeat(5000);
    let args = json!({"command": "cat .vtcode/context/tool_outputs/existing.txt"});
    for raw in [json!({"output": body}), json!(body), json!([body, "tail evidence"])] {
        let prepared = registry
            .prepare_execution_output(tools::EXEC_COMMAND, &args, raw, false, 2000)
            .await;
        assert!(prepared.normalized_value.get("spool_path").is_none());
        assert!(prepared.structured_error.is_none());
        assert!(serde_json::to_string(&prepared.normalized_value)?.len() < body.len());
    }
    let ordinary = registry
        .prepare_execution_output(
            tools::EXEC_COMMAND,
            &json!({"command": "printf ordinary-output"}),
            json!({"output": body}),
            false,
            2000,
        )
        .await;
    let spool = ordinary.normalized_value["spool_path"]
        .as_str()
        .expect("ordinary large output should spool");
    assert!(workspace.path().join(spool).is_file());
    Ok(())
}

struct HistorySeed {
    tool_name: &'static str,
    args: Value,
}

fn seed_history(registry: &ToolRegistry) {
    for seed in [
        HistorySeed {
            tool_name: tools::READ_FILE,
            args: json!({"path": "src/source.rs"}),
        },
        HistorySeed {
            tool_name: tools::UNIFIED_FILE,
            args: json!({"action": "read", "path": "src/destination.rs"}),
        },
        HistorySeed {
            tool_name: tools::READ_FILE,
            args: json!({"path": "src/unrelated.rs"}),
        },
        HistorySeed {
            tool_name: tools::CODE_SEARCH,
            args: json!({"query": "unrelated", "path": "src/source.rs"}),
        },
    ] {
        registry.execution_history.add_record(ToolExecutionRecord::success(
            seed.tool_name.to_string(),
            seed.tool_name.to_string(),
            false,
            None,
            seed.args,
            json!({"success": true}),
            registry.harness_context_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));
    }
}

#[tokio::test]
async fn targeted_mutation_invalidates_source_and_destination_but_retains_other_evidence() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    seed_history(&registry);
    registry.invalidate_mutated_reads(
        "move_file",
        &json!({"path": "src/source.rs", "destination_path": "src/destination.rs"}),
    );
    let records = registry.execution_history.get_recent_records(10);
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .any(|record| record.tool_name == tools::READ_FILE && record.args["path"] == "src/unrelated.rs")
    );
    assert!(records.iter().any(|record| record.tool_name == tools::CODE_SEARCH));
    Ok(())
}

#[tokio::test]
async fn only_pathless_command_mutations_clear_all_read_records() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    for name in [
        tools::UNIFIED_EXEC,
        tools::EXEC_COMMAND,
        tools::EXEC_PTY_CMD,
        tools::WRITE_STDIN,
    ] {
        registry.execution_history.clear();
        seed_history(&registry);
        registry.invalidate_mutated_reads(name, &json!({"command": "cargo build"}));
        let records = registry.execution_history.get_recent_records(10);
        assert_eq!(records.len(), 1, "{name}");
        assert_eq!(records[0].tool_name, tools::CODE_SEARCH);
    }
    registry.execution_history.clear();
    seed_history(&registry);
    registry.invalidate_mutated_reads("result_probe", &json!({"marker": "no target"}));
    assert_eq!(registry.execution_history.len(), 4);
    Ok(())
}

fn failed_mutation<'a>(_registry: &'a ToolRegistry, _args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async { Ok(json!({"success": false, "error": {"message": "partial mutation failed"}})) })
}

#[tokio::test]
async fn public_structured_failure_invalidates_target_before_recording_failed_execution() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    registry
        .register_tool(ToolRegistration::new("result_probe", CapabilityLevel::CodeSearch, false, failed_mutation))
        .await?;
    registry.set_tool_policy("result_probe", ToolPolicy::Allow).await?;
    seed_history(&registry);
    let args = json!({"path": "src/source.rs"});
    let result = registry.execute_tool_ref("result_probe", &args).await?;
    assert_eq!(result, json!({"success": false, "error": {"message": "partial mutation failed"}}));
    let records = registry.execution_history.get_recent_records(10);
    assert_eq!(records.len(), 4);
    assert!(
        !records
            .iter()
            .any(|record| record.tool_name == tools::READ_FILE && record.args["path"] == "src/source.rs")
    );
    assert!(records.iter().any(|record| record.args["path"] == "src/unrelated.rs"));
    let failed = records.iter().find(|record| record.tool_name == "result_probe").unwrap();
    assert!(!failed.success);
    assert_eq!(failed.args, args);
    assert_eq!(failed.result.as_ref().unwrap_err(), "partial mutation failed");
    Ok(())
}
