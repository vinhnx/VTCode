use super::*;

#[tokio::test]
async fn cached_heading_inspection_adopted_as_required_check_executes_fresh() {
    for (required_contents, required_exit) in [("No heading here\n", 1), ("# Actual heading\n", 0)] {
        let mut backing = TestContextBacking::new(20).await;
        backing.select_build_primary_agent();
        backing.tool_registry.allow_all_tools().await.unwrap();
        let workspace = backing.sample_file.parent().unwrap().to_path_buf();
        let readme = workspace.join("README.md");
        std::fs::write(&readme, "# Cached heading\n").unwrap();
        let command = "grep -n '^#' README.md";
        let args = json!({"cmd":command});
        let mut tracker = LoopTracker::new();
        let mut modified = BTreeSet::new();
        let mut ctx = backing.turn_processing_context();
        for (id, required, contents, exit_code) in [
            ("prime", false, "# Cached heading\n", 0),
            ("required", true, required_contents, required_exit),
        ] {
            std::fs::write(&readme, contents).unwrap();
            if required {
                let adopted = ctx
                    .tool_registry
                    .execute_tool(
                        tool_names::TASK_TRACKER,
                        json!({"action":"create", "items":[{"description":"Check headings", "verify":[command]}]}),
                    )
                    .await
                    .unwrap();
                assert!(adopted.get("error").is_none(), "{adopted}");
                ctx.tool_registry.restore_completion_verification_debt();
            }
            let mut outcome_ctx = ToolOutcomeContext {
                ctx: &mut ctx,
                repeated_tool_attempts: &mut tracker,
                turn_modified_files: &mut modified,
            };
            handle_single_tool_call(&mut outcome_ctx, id, tool_names::EXEC_COMMAND, args.clone())
                .await
                .unwrap();
            let response = ctx
                .working_history
                .iter()
                .rev()
                .find(|m| m.tool_call_id.as_deref() == Some(id))
                .unwrap();
            let output: serde_json::Value = serde_json::from_str(&response.content.as_text()).unwrap();
            assert_eq!(output["exit_code"], exit_code, "{output}");
            assert!(output.get("reused_recent_result").is_none(), "required checks must execute: {output}");
            if required {
                assert_eq!(ctx.tool_registry.completion_verifier_passed(command), exit_code == 0);
                assert_eq!(ctx.tool_registry.completion_verification_pending(), exit_code != 0);
            }
        }
    }
}

#[tokio::test]
async fn repeated_read_only_guard_dedups_plan_file_in_planning_mode() {
    let mut backing = TestContextBacking::new(4).await;
    backing.select_build_primary_agent();

    // Create a runtime-owned plan file inside the temporary workspace so the
    // plan-artifact fast path (`.vtcode/plans/`) is exercised.
    let workspace = backing.sample_file.parent().unwrap().to_path_buf();
    let plans_dir = workspace.join(".vtcode").join("plans");
    std::fs::create_dir_all(&plans_dir).expect("create plans dir");
    let plan_path = plans_dir.join("modular-dreaming-pixel.md");
    let plan_content = "# Plan\n\n1. Fix planning-mode clarity\n2. Throttle memory envelopes\n";
    std::fs::write(&plan_path, plan_content).expect("write plan file");

    let mut ctx = backing.turn_processing_context();
    ctx.tool_registry.enable_planning();

    let args = json!({
        "path": plan_path.to_string_lossy()
    });

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let first = handle_single_tool_call(&mut outcome_ctx, "read_plan_1", tool_names::READ_FILE, args.clone())
        .await
        .expect("first plan-file read should succeed");
    assert!(first.is_none(), "first read should execute normally");

    let second = handle_single_tool_call(&mut outcome_ctx, "read_plan_2", tool_names::READ_FILE, args)
        .await
        .expect("second plan-file read should be deduplicated");
    assert!(second.is_none(), "duplicate read should be handled by guard");

    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::Tool && message.content.as_text().contains("\"reused_recent_result\":true")
    }));
    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::Tool
            && message
                .content
                .as_text()
                .contains("was already read. Stop re-reading and finalize the plan.")
    }));
}

async fn execute_tracker_regression_call(
    outcome_ctx: &mut ToolOutcomeContext<'_, '_>,
    call_id: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    let history_len = outcome_ctx.ctx.tool_registry.execution_history_len();
    let outcome = handle_single_tool_call(outcome_ctx, call_id, tool_names::TASK_TRACKER, args)
        .await
        .expect("tracker call should execute");
    assert!(outcome.is_none());
    assert!(outcome_ctx.ctx.tool_registry.execution_history_len() > history_len, "tracker must execute live");
    let response = outcome_ctx
        .ctx
        .working_history
        .iter()
        .rev()
        .find(|message| message.tool_call_id.as_deref() == Some(call_id))
        .expect("tracker response");
    let payload: serde_json::Value = serde_json::from_str(&response.content.as_text()).expect("tracker JSON");
    assert!(payload.get("error").is_none(), "{payload}");
    assert!(
        payload.get("reused_recent_result").is_none(),
        "stateful tracker results must never replay: {payload}"
    );
    payload
}

#[tokio::test]
async fn tracker_repeated_lists_observe_current_progress_without_replay() {
    struct TrackerCallCase {
        call_id: &'static str,
        args: serde_json::Value,
        expected_completed: usize,
    }
    let mut backing = TestContextBacking::new(12).await;
    backing.select_build_primary_agent();
    let calls = [
        TrackerCallCase {
            call_id: "create",
            args: json!({"action":"create", "title":"README work", "items":["Revise copy", "Check links"]}),
            expected_completed: 0,
        },
        TrackerCallCase {
            call_id: "list_before",
            args: json!({"action":"list"}),
            expected_completed: 0,
        },
        TrackerCallCase {
            call_id: "list_before_again",
            args: json!({"action":"list"}),
            expected_completed: 0,
        },
        TrackerCallCase {
            call_id: "finish_links",
            args: json!({"action":"update", "index":2, "status":"completed"}),
            expected_completed: 1,
        },
        TrackerCallCase {
            call_id: "list_after",
            args: json!({"action":"list"}),
            expected_completed: 1,
        },
        TrackerCallCase {
            call_id: "list_after_again",
            args: json!({"action":"list"}),
            expected_completed: 1,
        },
    ];
    for case in &calls {
        cache_tool_permission(&mut backing, tool_names::TASK_TRACKER, &case.args, PermissionGrant::Permanent).await;
    }
    let mut ctx = backing.turn_processing_context();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    for case in calls {
        let payload = execute_tracker_regression_call(&mut outcome_ctx, case.call_id, case.args).await;
        assert_eq!(payload["checklist"]["completed"], case.expected_completed, "{}: {payload}", case.call_id);
        assert_eq!(payload["checklist"]["total"], 2);
    }
    handle_single_tool_call(&mut outcome_ctx, "list_over_limit", tool_names::TASK_TRACKER, json!({"action":"list"}))
        .await
        .expect("excessive tracker polling should return policy feedback");
    let response = outcome_ctx
        .ctx
        .working_history
        .iter()
        .rev()
        .find(|message| message.tool_call_id.as_deref() == Some("list_over_limit"))
        .expect("blocked tracker response");
    let payload: serde_json::Value = serde_json::from_str(&response.content.as_text()).expect("tracker JSON");
    assert_eq!(payload["error_class"], "policy_blocked", "repeat limits must remain active: {payload}");
    assert!(payload.get("reused_recent_result").is_none());
}

#[tokio::test]
async fn tracker_repeated_update_targets_replaced_checklist_instead_of_old_history() {
    let mut backing = TestContextBacking::new(12).await;
    backing.select_build_primary_agent();
    let old_create = json!({"action":"create", "title":"Planning", "items":["Inspect README", "Draft plan"]});
    let finish_second = json!({"action":"update", "index":2, "status":"completed"});
    let new_create =
        json!({"action":"create", "title":"Implementation", "items":["[x] Revise copy", "Check new links"]});
    for args in [&old_create, &finish_second, &new_create] {
        cache_tool_permission(&mut backing, tool_names::TASK_TRACKER, args, PermissionGrant::Permanent).await;
    }
    let mut ctx = backing.turn_processing_context();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    execute_tracker_regression_call(&mut outcome_ctx, "old_create", old_create).await;
    let old_update = execute_tracker_regression_call(&mut outcome_ctx, "old_update", finish_second.clone()).await;
    assert_eq!(old_update["checklist"]["completed"], 1);
    execute_tracker_regression_call(&mut outcome_ctx, "new_create", new_create).await;
    outcome_ctx.ctx.tool_registry.begin_tracker_request(false);
    let new_update = execute_tracker_regression_call(&mut outcome_ctx, "new_update", finish_second).await;
    assert_eq!(new_update["checklist"]["completed"], 2);
    assert_eq!(new_update["changed_item"]["description"], "Check new links");
    assert!(
        outcome_ctx.ctx.tool_registry.tracker_adopted_for_request(),
        "a repeated update must adopt the current request"
    );
}

#[tokio::test]
async fn repeated_registry_results_keep_new_inspections_visible() {
    for planning in [false, true] {
        let mut backing = TestContextBacking::new(120).await;
        backing.select_build_primary_agent();
        if planning {
            backing.tool_registry.enable_planning();
        }
        let workspace = backing.sample_file.parent().unwrap().to_path_buf();
        let paths = (0..16)
            .map(|index| {
                let path = workspace.join(format!("evidence-{index}.txt"));
                std::fs::write(&path, format!("evidence-{index} {}\n", "x".repeat(8000))).unwrap();
                path
            })
            .collect::<Vec<_>>();
        let mut tracker = LoopTracker::new();
        let mut modified = BTreeSet::new();
        let mut ctx = backing.turn_processing_context();
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut tracker,
            turn_modified_files: &mut modified,
        };
        for (index, path) in paths.iter().enumerate() {
            let result = handle_single_tool_call(
                &mut outcome_ctx,
                &format!("read-{index}"),
                tool_names::READ_FILE,
                json!({"path": path, "limit": 2_000, "condense": false}),
            )
            .await
            .unwrap();
            assert!(result.is_none(), "a productive read must not trigger a recovery outcome");
            let stored = outcome_ctx
                .ctx
                .working_history
                .iter()
                .rev()
                .find(|message| message.role == uni::MessageRole::Tool)
                .unwrap()
                .content
                .as_text();
            assert!(stored.contains(&format!("evidence-{index}")), "fresh evidence must remain visible: {stored}");
            assert!(!stored.contains("preview_budget_exhausted"));
        }
        assert!(!outcome_ctx.ctx.is_recovery_active());
    }
}

#[tokio::test]
async fn spool_page_remains_visible_after_large_results() {
    use crate::agent::runloop::unified::turn::tool_outcomes::handlers::guards::spool_guard::enforce_spool_chunk_read_guard;
    let mut backing = TestContextBacking::new(8).await;
    let mut ctx = backing.turn_processing_context();
    ctx.push_tool_response("large", Some(tool_names::EXEC_COMMAND), "x".repeat(100_000));
    let passed = enforce_spool_chunk_read_guard(
        &mut ctx,
        "page",
        tool_names::READ_FILE,
        &json!({"path": ".vtcode/context/tool_outputs/write_stdin_run-abc123.txt"}),
    )
    .await;
    assert!(passed.is_none());
    let page = "visible page content ".repeat(256);
    ctx.push_tool_response("page", Some(tool_names::READ_FILE), page.clone());
    assert!(ctx.working_history.last().unwrap().content.as_text().contains(&page));
    assert!(!ctx.is_recovery_active());
}
