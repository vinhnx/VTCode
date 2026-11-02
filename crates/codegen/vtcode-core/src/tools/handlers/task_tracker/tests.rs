use super::*;
use tempfile::TempDir;

fn setup_tool(temp: &TempDir) -> (PlanningWorkflowState, TaskTrackerTool) {
    let state = PlanningWorkflowState::new(temp.path().to_path_buf());
    let tool = TaskTrackerTool::new(temp.path().to_path_buf(), state.clone());
    (state, tool)
}

#[tokio::test]
async fn test_create_checklist() {
    let temp = TempDir::new().unwrap();
    let (_state, tool) = setup_tool(&temp);

    let result = tool
        .execute(json!({
            "action": "create",
            "title": "Refactor Auth",
            "items": ["Extract middleware", "Add tests", "Update docs"]
        }))
        .await
        .unwrap();

    assert_eq!(result["status"], "created");
    assert_eq!(result["checklist"]["total"], 3);
    assert_eq!(result["checklist"]["completed"], 0);
    assert_eq!(result["view"]["title"], "Refactor Auth");
    assert_eq!(result["task_file"], ".vtcode/tasks/current_task.md");
}

#[tokio::test]
async fn test_create_accepts_metadata_and_verify_string_forms() {
    let temp = TempDir::new().unwrap();
    let (_state, tool) = setup_tool(&temp);

    let result = tool
        .execute(json!({
            "action": "create",
            "title": "Harness tracker",
            "items": [
                {
                    "description": "Analyze current harness",
                    "files": ["docs/ARCHITECTURE.md"],
                    "outcome": "Document the harness map",
                    "verify": "cargo check"
                },
                {
                    "description": "Wire continuation",
                    "verify": ["cargo test -p vtcode-core continuation", "cargo check -p vtcode"]
                }
            ]
        }))
        .await
        .unwrap();

    assert_eq!(result["checklist"]["items"][0]["files"], json!(["docs/ARCHITECTURE.md"]));
    assert_eq!(result["checklist"]["items"][0]["outcome"], "Document the harness map");
    assert_eq!(result["checklist"]["items"][0]["verify"], json!(["cargo check"]));
    assert_eq!(
        result["checklist"]["items"][1]["verify"],
        json!(["cargo test -p vtcode-core continuation", "cargo check -p vtcode"])
    );

    let persisted = std::fs::read_to_string(temp.path().join(".vtcode/tasks/current_task.md")).unwrap();
    assert!(persisted.contains("files: docs/ARCHITECTURE.md"));
    assert!(persisted.contains("outcome: Document the harness map"));
    assert!(persisted.contains("verify: cargo check"));
}

#[tokio::test]
async fn test_scope_review_metadata_survives_public_create_and_status_update() {
    let temp = TempDir::new().unwrap();
    let (_state, tool) = setup_tool(&temp);
    let created = tool
        .execute(json!({
            "action": "create", "title": "Scope review", "items": [{
                "description": "Review final scope", "files": ["README.md"],
                "scope_review": ["git status --porcelain", "git diff --stat HEAD -- README.md"]
            }]
        }))
        .await
        .unwrap();
    assert_eq!(
        created["checklist"]["items"][0]["scope_review"],
        json!(["git status --porcelain", "git diff --stat HEAD -- README.md"])
    );
    assert_eq!(created["checklist"]["items"][0]["verify"], json!([]));
    tool.execute(json!({"action": "update", "index": 1, "status": "completed"}))
        .await
        .unwrap();
    let persisted = std::fs::read_to_string(temp.path().join(".vtcode/tasks/current_task.md")).unwrap();
    assert!(persisted.contains("[x] Review final scope"));
    assert!(persisted.contains("  scope-review: git status --porcelain"));
    assert!(persisted.contains("  scope-review: git diff --stat HEAD -- README.md"));
    assert!(!persisted.contains("  verify:"));
}

#[tokio::test]
async fn test_update_item() {
    let temp = TempDir::new().unwrap();
    let (_state, tool) = setup_tool(&temp);

    tool.execute(json!({
        "action": "create",
        "title": "Test",
        "items": ["Step 1", "Step 2"]
    }))
    .await
    .unwrap();

    let result = tool
        .execute(json!({
            "action": "update",
            "index": 1,
            "status": "completed"
        }))
        .await
        .unwrap();

    assert_eq!(result["status"], "updated");
    assert_eq!(result["checklist"]["completed"], 1);
    assert_eq!(result["checklist"]["progress_percent"], 50);
}

#[tokio::test]
async fn test_update_index_zero_allows_checklist_completion_note() {
    let temp = TempDir::new().unwrap();
    let (_state, tool) = setup_tool(&temp);

    tool.execute(json!({
        "action": "create",
        "title": "Test",
        "items": ["Step 1", "Step 2"]
    }))
    .await
    .unwrap();

    let result = tool
        .execute(json!({
            "action": "update",
            "index": 0,
            "status": "completed",
            "outcome": "Reported summary to user"
        }))
        .await
        .unwrap();

    assert_eq!(result["status"], "updated");
    assert_eq!(result["checklist"]["completed"], 0);
    assert_eq!(result["checklist"]["notes"], "Checklist outcome: Reported summary to user");
}

#[test]
fn task_tracker_schema_describes_action_aware_indices() {
    let (_state, tool) = setup_tool(&TempDir::new().unwrap());
    let schema = tool.parameter_schema().expect("task tracker schema");

    assert_eq!(schema["properties"]["index"]["minimum"], 0);
    let items_description = schema["properties"]["items"]["description"].as_str().unwrap();
    assert!(items_description.contains("Full checklist replacement"));
    assert!(items_description.contains("never JSON-encoded strings"));
    assert_eq!(schema["properties"]["index_path"]["pattern"], "^[1-9][0-9]*$");
    let description = schema["properties"]["index"]["description"]
        .as_str()
        .expect("index description");
    assert!(description.contains("index: 0"));
    assert!(description.contains("action: update"));
    assert!(description.contains("completed"));
    assert_eq!(schema["allOf"][2]["then"]["properties"]["status"]["const"], "completed");
}

#[test]
fn standard_task_tracker_schema_rejects_zero_except_legacy_completion() {
    let schema = standard_task_tracker_parameter_schema();
    let valid_cases = [
        json!({"action": "update", "index": 0, "status": "completed"}),
        json!({"action": "update", "index": 1, "status": "completed"}),
        json!({"action": "update", "items": ["[x] Done"]}),
    ];
    let invalid_cases = [
        json!({"action": "list", "index": 0}),
        json!({"action": "list", "index": 1}),
        json!({"action": "list", "index_path": "1"}),
        json!({"action": "add", "index": 0, "description": "New item"}),
        json!({"action": "add", "index": 1, "description": "New item"}),
        json!({"action": "create", "index": 1, "items": ["New item"]}),
        json!({"action": "update", "index": 0, "status": "pending"}),
    ];

    for args in valid_cases {
        assert!(jsonschema::validate(&schema, &args).is_ok(), "expected valid args: {args}");
    }
    for args in invalid_cases {
        assert!(jsonschema::validate(&schema, &args).is_err(), "expected invalid args: {args}");
    }
}

#[test]
fn standard_task_tracker_schema_rejects_mixed_bulk_and_single_updates() {
    let schema = standard_task_tracker_parameter_schema();
    let invalid_cases = [
        json!({"action": "update", "items": ["Done"], "index": 1, "status": "completed"}),
        json!({"action": "update", "items": ["Done"], "index_path": "1", "status": "completed"}),
        json!({"action": "update", "index": 1, "index_path": "1", "status": "completed"}),
        json!({"action": "update", "items": ["Done"], "status": "completed"}),
        json!({"action": "update", "items": ["Done"], "index": 0, "status": "completed"}),
    ];

    for args in invalid_cases {
        assert!(jsonschema::validate(&schema, &args).is_err(), "expected invalid args: {args}");
    }
}

#[test]
fn tracker_schemas_share_update_shapes_but_diverge_on_index_zero() {
    let standard = standard_task_tracker_parameter_schema();
    let planning = crate::tools::handlers::planning_task_tracker::planning_task_tracker_parameter_schema();

    // Shared shapes: both address modes validate on both schemas,
    // proving the normalized anyOf arm order is behavior-preserving.
    for args in [
        json!({"action": "update", "index": 1, "status": "completed"}),
        json!({"action": "update", "index_path": "1", "status": "completed"}),
        json!({"action": "update", "items": ["[x] Done"]}),
        json!({"action": "create", "title": "T", "items": ["New item"]}),
        json!({"action": "add", "description": "New item"}),
        json!({"action": "list"}),
    ] {
        assert!(jsonschema::validate(&standard, &args).is_ok(), "standard rejects shared shape: {args}");
        assert!(jsonschema::validate(&planning, &args).is_ok(), "planning rejects shared shape: {args}");
    }

    // Shared rejections on both schemas.
    for args in [
        json!({"action": "list", "index": 1}),
        json!({"action": "create", "index": 1, "items": ["New item"]}),
        json!({"action": "update", "index": 1, "index_path": "1", "status": "completed"}),
    ] {
        assert!(jsonschema::validate(&standard, &args).is_err(), "standard accepts rejected shape: {args}");
        assert!(jsonschema::validate(&planning, &args).is_err(), "planning accepts rejected shape: {args}");
    }

    // Divergent boundary: index 0 completes a standard checklist but is
    // invalid in planning (indices start at 1).
    let zero_completion = json!({"action": "update", "index": 0, "status": "completed"});
    assert!(jsonschema::validate(&standard, &zero_completion).is_ok());
    assert!(jsonschema::validate(&planning, &zero_completion).is_err());
}

#[tokio::test]
async fn bulk_update_rejects_encoded_commands_without_resetting_completed_steps() {
    let temp = TempDir::new().unwrap();
    let (_state, tool) = setup_tool(&temp);
    tool.execute(json!({
        "action": "create",
        "title": "Preserve progress",
        "items": [
            {"description": "Update introduction", "status": "completed", "files": ["README.md"], "outcome": "Clear overview", "verify": ["review headings"]},
            {"description": "Review links", "status": "blocked", "outcome": "Await network"}
        ]
    })).await.unwrap();
    let tracker_path = temp.path().join(".vtcode/tasks/current_task.md");
    let persisted_before = std::fs::read_to_string(&tracker_path).unwrap();
    let before = tool.execute(json!({"action": "list"})).await.unwrap();

    for (action, encoded) in [
        ("update", r#"{"index_path":"1","status":"completed","outcome":"Done","verify":"review headings"}"#),
        ("update", r#"  [x] {"index_path":"1","status":"completed"}"#),
        ("create", r#"{"index":1,"status":"completed"}"#),
    ] {
        let error = tool
            .execute(json!({
                "action": action,
                "title": "Incorrect replacement",
                "items": ["Replacement must not leak", encoded]
            }))
            .await
            .expect_err("encoded updates must not replace the tracker");
        assert!(error.to_string().contains("JSON-encoded task updates"));
        assert!(
            error
                .to_string()
                .contains(r#"{"action":"update","index_path":"1","status":"completed"}"#)
        );
        assert_eq!(tool.execute(json!({"action": "list"})).await.unwrap(), before);
        assert_eq!(std::fs::read_to_string(&tracker_path).unwrap(), persisted_before);
    }

    tool.execute(json!({"action": "update", "index_path": "2", "status": "completed"}))
        .await
        .unwrap();
    let after = tool.execute(json!({"action": "list"})).await.unwrap();
    assert_eq!(after["checklist"]["completed"], 2);
    assert_eq!(after["checklist"]["items"][0]["description"], "Update introduction");
    assert_eq!(after["checklist"]["items"][0]["files"], json!(["README.md"]));
}

#[tokio::test]
async fn update_rejects_mixed_bulk_and_single_fields_before_mutation() {
    let temp = TempDir::new().unwrap();
    let (_state, tool) = setup_tool(&temp);

    tool.execute(json!({
        "action": "create",
        "title": "Test",
        "items": ["Original"]
    }))
    .await
    .unwrap();

    let error = tool
        .execute(json!({
            "action": "update",
            "items": ["Replacement"],
            "index": 1,
            "status": "completed"
        }))
        .await
        .expect_err("mixed update must fail closed");
    assert!(error.to_string().contains("cannot combine 'items'"));

    let result = tool.execute(json!({"action": "list"})).await.unwrap();
    assert_eq!(result["checklist"]["items"][0]["description"], "Original");
    assert_eq!(result["checklist"]["items"][0]["status"], "pending");
}

#[tokio::test]
async fn update_rejects_both_index_forms_before_mutation() {
    let temp = TempDir::new().unwrap();
    let (_state, tool) = setup_tool(&temp);

    tool.execute(json!({
        "action": "create",
        "title": "Test",
        "items": ["Original"]
    }))
    .await
    .unwrap();

    let error = tool
        .execute(json!({
            "action": "update",
            "index": 1,
            "index_path": "1",
            "status": "completed"
        }))
        .await
        .expect_err("ambiguous index forms must fail closed");
    assert!(error.to_string().contains("cannot combine 'index' and 'index_path'"));

    let result = tool.execute(json!({"action": "list"})).await.unwrap();
    assert_eq!(result["checklist"]["items"][0]["description"], "Original");
    assert_eq!(result["checklist"]["items"][0]["status"], "pending");
}

#[tokio::test]
async fn execute_rejects_indices_for_non_update_actions() {
    let temp = TempDir::new().unwrap();
    let (_state, tool) = setup_tool(&temp);

    let invalid_inputs = [
        json!({"action": "list", "index": 1}),
        json!({"action": "create", "index_path": "1", "items": ["New item"]}),
        json!({"action": "add", "index": 1, "description": "New item"}),
        json!({"action": "unexpected", "index_path": "1"}),
    ];

    for input in invalid_inputs {
        let error = tool.execute(input).await.expect_err("non-update index must fail closed");
        assert!(error.to_string().contains("cannot use 'index' or 'index_path'"), "unexpected error: {error}");
    }
}

#[test]
fn task_tracker_metadata_switches_with_planning_state() {
    let temp = TempDir::new().unwrap();
    let (state, tool) = setup_tool(&temp);

    assert_eq!(tool.description(), task_tracker_description_for_workflow(false));
    assert_eq!(tool.parameter_schema().expect("standard schema")["properties"]["index"]["minimum"], 0);

    state.enable();

    assert_eq!(tool.description(), task_tracker_description_for_workflow(true));
    assert_eq!(tool.parameter_schema().expect("planning schema")["properties"]["index"]["minimum"], 1);
}

#[tokio::test]
async fn test_add_item() {
    let temp = TempDir::new().unwrap();
    let (_state, tool) = setup_tool(&temp);

    tool.execute(json!({
        "action": "create",
        "title": "Test",
        "items": ["Step 1"]
    }))
    .await
    .unwrap();

    let result = tool
        .execute(json!({
            "action": "add",
            "description": "Step 2"
        }))
        .await
        .unwrap();

    assert_eq!(result["status"], "added");
    assert_eq!(result["checklist"]["total"], 2);
}

#[tokio::test]
async fn test_create_is_idempotent_for_same_structure() {
    let temp = TempDir::new().unwrap();
    let (_state, tool) = setup_tool(&temp);

    tool.execute(json!({
        "action": "create",
        "title": "Clippy Warnings",
        "items": ["Fix A", "Fix B"]
    }))
    .await
    .unwrap();

    tool.execute(json!({
        "action": "update",
        "index": 1,
        "status": "completed"
    }))
    .await
    .unwrap();

    let duplicate = tool
        .execute(json!({
            "action": "create",
            "title": "Clippy Warnings",
            "items": ["Fix A", "Fix B"]
        }))
        .await
        .unwrap();

    assert_eq!(duplicate["status"], "unchanged");
    assert_eq!(duplicate["checklist"]["completed"], 1);
}

#[tokio::test]
async fn test_create_with_new_structure_replaces_checklist() {
    let temp = TempDir::new().unwrap();
    let (_state, tool) = setup_tool(&temp);

    tool.execute(json!({
        "action": "create",
        "title": "First",
        "items": ["Fix A", "Fix B"]
    }))
    .await
    .unwrap();

    tool.execute(json!({
        "action": "update",
        "index": 1,
        "status": "completed"
    }))
    .await
    .unwrap();

    let replaced = tool
        .execute(json!({
            "action": "create",
            "title": "Second",
            "items": ["Fix C"]
        }))
        .await
        .unwrap();

    assert_eq!(replaced["status"], "replaced");
    assert_eq!(replaced["checklist"]["total"], 1);
    assert_eq!(replaced["checklist"]["completed"], 0);
}

#[test]
fn task_tracker_descriptions_state_that_repeat_create_replaces() {
    for planning_active in [false, true] {
        let description = task_tracker_description_for_workflow(planning_active);
        assert!(
            description.contains("Calling action=create again replaces"),
            "planning_active={planning_active}: {description}"
        );
        assert!(!description.contains("subsequent calls update"), "planning_active={planning_active}");
    }
    assert!(task_tracker_description_for_workflow(false).contains("no-op"));
}

#[tokio::test]
async fn test_update_supports_bulk_item_sync() {
    let temp = TempDir::new().unwrap();
    let (_state, tool) = setup_tool(&temp);

    tool.execute(json!({
        "action": "create",
        "title": "Sync Test",
        "items": ["Step 1", "Step 2", "Step 3"]
    }))
    .await
    .unwrap();

    let updated = tool
        .execute(json!({
            "action": "update",
            "items": ["[x] Step 1", "[~] Step 2", "[ ] Step 3"]
        }))
        .await
        .unwrap();

    assert_eq!(updated["status"], "updated");
    assert_eq!(updated["checklist"]["completed"], 1);
    assert_eq!(updated["checklist"]["in_progress"], 1);
    assert_eq!(updated["checklist"]["pending"], 1);
}

#[tokio::test]
async fn test_list_empty() {
    let temp = TempDir::new().unwrap();
    let (_state, tool) = setup_tool(&temp);

    let result = tool.execute(json!({"action": "list"})).await.unwrap();
    assert_eq!(result["status"], "empty");
}

#[tokio::test]
async fn test_persistence_across_loads() {
    let temp = TempDir::new().unwrap();

    {
        let (_state, tool) = setup_tool(&temp);
        tool.execute(json!({
            "action": "create",
            "title": "Persist Test",
            "items": ["Alpha", "Beta"]
        }))
        .await
        .unwrap();

        tool.execute(json!({
            "action": "update",
            "index": 1,
            "status": "completed"
        }))
        .await
        .unwrap();
    }

    let (_state, tool2) = setup_tool(&temp);
    let result = tool2.execute(json!({"action": "list"})).await.unwrap();

    assert_eq!(result["status"], "ok");
    assert_eq!(result["checklist"]["total"], 2);
    assert_eq!(result["checklist"]["completed"], 1);
}

#[tokio::test]
async fn test_planning_workflow_task_tracker_delegates_and_mirrors_global() {
    let temp = TempDir::new().unwrap();
    let (state, tool) = setup_tool(&temp);

    let plans_dir = state.plans_dir();
    std::fs::create_dir_all(&plans_dir).unwrap();
    let plan_file = plans_dir.join("adaptive.md");
    std::fs::write(&plan_file, "# Adaptive\n").unwrap();
    state.set_plan_file(Some(plan_file)).await;
    state.enable();

    let created = tool
        .execute(json!({
            "action": "create",
            "title": "Adaptive Plan",
            "items": ["Root task", "  Child task"]
        }))
        .await
        .unwrap();

    assert_eq!(created["status"], "created");
    assert_eq!(created["checklist"]["total"], 2);

    let task_file = temp.path().join(".vtcode/tasks/current_task.md");
    let persisted = std::fs::read_to_string(task_file).unwrap();
    assert!(persisted.contains("Root task"));
    assert!(persisted.contains("Child task"));
}

#[tokio::test]
async fn test_planning_workflow_adaptive_tool_accepts_hierarchical_index_path() {
    let temp = TempDir::new().unwrap();
    let (state, tool) = setup_tool(&temp);

    let plans_dir = state.plans_dir();
    std::fs::create_dir_all(&plans_dir).unwrap();
    let plan_file = plans_dir.join("hierarchical.md");
    std::fs::write(&plan_file, "# Hierarchical\n").unwrap();
    state.set_plan_file(Some(plan_file)).await;
    state.enable();

    tool.execute(json!({
        "action": "create",
        "items": ["Parent task", "  Child task"]
    }))
    .await
    .unwrap();

    let updated = tool
        .execute(json!({
            "action": "update",
            "index_path": "1.1",
            "status": "completed"
        }))
        .await
        .unwrap();

    assert_eq!(updated["status"], "updated");
    assert_eq!(updated["checklist"]["completed"], 1);
}

#[tokio::test]
async fn test_planning_workflow_adaptive_tool_rejects_index_zero() {
    let temp = TempDir::new().unwrap();
    let (state, tool) = setup_tool(&temp);

    let plans_dir = state.plans_dir();
    std::fs::create_dir_all(&plans_dir).unwrap();
    let plan_file = plans_dir.join("reject-zero.md");
    std::fs::write(&plan_file, "# Reject zero\n").unwrap();
    state.set_plan_file(Some(plan_file)).await;
    state.enable();

    tool.execute(json!({
        "action": "create",
        "items": ["Parent task"]
    }))
    .await
    .unwrap();

    let error = tool
        .execute(json!({
            "action": "update",
            "index": 0,
            "status": "completed"
        }))
        .await
        .expect_err("planning index zero must be rejected");

    assert!(error.to_string().contains("index_path components must be >= 1"));
}

#[tokio::test]
async fn test_planning_workflow_mirror_preserves_notes() {
    let temp = TempDir::new().unwrap();
    let (state, tool) = setup_tool(&temp);

    let plans_dir = state.plans_dir();
    std::fs::create_dir_all(&plans_dir).unwrap();
    let plan_file = plans_dir.join("notes.md");
    std::fs::write(&plan_file, "# Notes\n").unwrap();
    state.set_plan_file(Some(plan_file)).await;
    state.enable();

    tool.execute(json!({
        "action": "create",
        "items": ["Root task"],
        "notes": "Keep this note"
    }))
    .await
    .unwrap();

    let task_file = temp.path().join(".vtcode/tasks/current_task.md");
    let persisted = std::fs::read_to_string(task_file).unwrap();
    assert!(persisted.contains("## Notes"));
    assert!(persisted.contains("Keep this note"));
}

#[tokio::test]
async fn test_edit_mode_prefers_newer_plan_mirror_when_present() {
    let temp = TempDir::new().unwrap();
    let (state, tool) = setup_tool(&temp);

    let plans_dir = state.plans_dir();
    std::fs::create_dir_all(&plans_dir).unwrap();
    let plan_file = plans_dir.join("freshness.md");
    std::fs::write(&plan_file, "# Freshness\n").unwrap();
    state.set_plan_file(Some(plan_file.clone())).await;

    let global_file = temp.path().join(".vtcode/tasks/current_task.md");
    std::fs::create_dir_all(global_file.parent().unwrap()).unwrap();
    std::fs::write(&global_file, "# Freshness\n\n- [ ] stale global\n").unwrap();

    std::thread::sleep(std::time::Duration::from_millis(15));

    let sidecar = plans_dir.join("freshness.tasks.md");
    std::fs::write(&sidecar, "# Freshness\n\n## Plan of Work\n\n- [x] newer plan\n").unwrap();

    let listed = tool.execute(json!({"action": "list"})).await.unwrap();
    assert_eq!(listed["status"], "ok");
    assert_eq!(listed["checklist"]["completed"], 1);
    assert_eq!(listed["checklist"]["pending"], 0);

    let global_synced = std::fs::read_to_string(global_file).unwrap();
    assert!(global_synced.contains("newer plan"));
}

#[tokio::test]
async fn test_planning_workflow_prefers_plan_sidecar_even_if_global_is_newer() {
    let temp = TempDir::new().unwrap();
    let (state, tool) = setup_tool(&temp);

    let plans_dir = state.plans_dir();
    std::fs::create_dir_all(&plans_dir).unwrap();
    let plan_file = plans_dir.join("plan-primary.md");
    std::fs::write(&plan_file, "# Plan Primary\n").unwrap();
    state.set_plan_file(Some(plan_file.clone())).await;
    state.enable();

    let global_file = temp.path().join(".vtcode/tasks/current_task.md");
    std::fs::create_dir_all(global_file.parent().unwrap()).unwrap();
    std::fs::write(&global_file, "# Plan Primary\n\n- [x] global newer\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(15));

    let sidecar = plans_dir.join("plan-primary.tasks.md");
    std::fs::write(&sidecar, "# Plan Primary\n\n## Plan of Work\n\n- [ ] plan source\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(15));
    std::fs::write(&global_file, "# Plan Primary\n\n- [x] global newest\n").unwrap();

    let listed = tool.execute(json!({"action": "list"})).await.unwrap();
    assert_eq!(listed["status"], "ok");
    assert_eq!(listed["checklist"]["pending"], 1);
    assert_eq!(listed["checklist"]["completed"], 0);
}
