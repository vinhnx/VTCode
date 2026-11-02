use super::*;
use tempfile::TempDir;

async fn setup_planning_workflow() -> (TempDir, PlanningWorkflowState, PlanningTaskTrackerTool) {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
    let plans_dir = state.plans_dir();
    std::fs::create_dir_all(&plans_dir).expect("create plans dir");
    let plan_file = plans_dir.join("test-plan.md");
    std::fs::write(&plan_file, "# Test Plan\n").expect("write plan");
    state.set_plan_file(Some(plan_file)).await;
    state.enable();

    let tool = PlanningTaskTrackerTool::new(state.clone());
    (temp_dir, state, tool)
}

#[tokio::test]
async fn create_and_list_tracker_with_hierarchy() {
    let (_temp_dir, _state, tool) = setup_planning_workflow().await;

    let created = tool
        .execute(json!({
            "action": "create",
            "title": "Updated Plan",
            "items": [
                "Add config cap",
                "  Use cap in guard logic",
                "[~] Expose setting in template"
            ]
        }))
        .await
        .expect("create tracker");

    assert_eq!(created["status"], "created");
    assert_eq!(created["checklist"]["total"], 3);
    assert_eq!(created["checklist"]["in_progress"], 1);
    assert_eq!(created["view"]["title"], "Updated Plan");
    assert_eq!(created["tracker_file"], ".vtcode/plans/test-plan.tasks.md");

    let lines = created["view"]["lines"].as_array().expect("view lines array");
    assert!(!lines.is_empty());
    let first = lines[0]["display"].as_str().unwrap_or_default();
    assert!(first.contains('└') || first.contains('├'));
}

#[tokio::test]
async fn create_accepts_metadata_and_verify_string_forms() {
    let (_temp_dir, _state, tool) = setup_planning_workflow().await;

    let created = tool
        .execute(json!({
            "action": "create",
            "title": "Harness plan",
            "items": [
                {
                    "description": "Analyze",
                    "files": ["docs/ARCHITECTURE.md"],
                    "outcome": "Map the harness",
                    "verify": "cargo check"
                },
                {
                    "description": "Implement",
                    "verify": ["cargo test -p vtcode-core task_tracker", "cargo check -p vtcode"]
                }
            ]
        }))
        .await
        .expect("create tracker");

    assert_eq!(created["checklist"]["items"][0]["files"], json!(["docs/ARCHITECTURE.md"]));
    assert_eq!(created["checklist"]["items"][0]["outcome"], "Map the harness");
    assert_eq!(created["checklist"]["items"][0]["verify"], json!(["cargo check"]));
    assert_eq!(
        created["checklist"]["items"][1]["verify"],
        json!(["cargo test -p vtcode-core task_tracker", "cargo check -p vtcode"])
    );
}

#[tokio::test]
async fn scope_review_survives_resume_bulk_sync_and_global_mirror() {
    let (temp, state, tool) = setup_planning_workflow().await;
    tool.execute(json!({
        "action": "create", "items": [{
            "description": "Review final scope", "files": ["README.md"],
            "scope_review": ["git status --porcelain", "git diff --stat HEAD -- README.md"]
        }]
    }))
    .await
    .expect("create scope tracker");
    let resumed = PlanningTaskTrackerTool::new(state);
    let updated = resumed
        .execute(json!({
            "action": "update", "items": ["[x] Review final scope"]
        }))
        .await
        .expect("bulk status update");
    assert_eq!(
        updated["checklist"]["items"][0]["scope_review"],
        json!(["git status --porcelain", "git diff --stat HEAD -- README.md"])
    );
    assert_eq!(updated["checklist"]["items"][0]["verify"], json!([]));
    for path in [".vtcode/plans/test-plan.tasks.md", ".vtcode/tasks/current_task.md"] {
        let persisted = std::fs::read_to_string(temp.path().join(path)).expect("persisted tracker");
        assert!(persisted.contains("[x] Review final scope"));
        assert_eq!(persisted.matches("  scope-review:").count(), 2);
        assert!(!persisted.contains("  verify:"));
    }
}

#[tokio::test]
async fn add_and_update_nested_item() {
    let (_temp_dir, _state, tool) = setup_planning_workflow().await;

    tool.execute(json!({
        "action": "create",
        "items": ["Parent task"]
    }))
    .await
    .expect("create tracker");

    tool.execute(json!({
        "action": "add",
        "parent_index_path": "1",
        "description": "Child task"
    }))
    .await
    .expect("add nested task");

    let updated = tool
        .execute(json!({
            "action": "update",
            "index_path": "1.1",
            "status": "completed"
        }))
        .await
        .expect("update nested task");

    assert_eq!(updated["status"], "updated");
    assert_eq!(updated["checklist"]["completed"], 1);
}

#[tokio::test]
async fn persistence_across_instances() {
    let (_temp_dir, state, tool) = setup_planning_workflow().await;

    tool.execute(json!({
        "action": "create",
        "items": ["Persisted step"]
    }))
    .await
    .expect("create tracker");

    tool.execute(json!({
        "action": "update",
        "index_path": "1",
        "status": "completed"
    }))
    .await
    .expect("update tracker");

    let tool2 = PlanningTaskTrackerTool::new(state);
    let listed = tool2.execute(json!({"action": "list"})).await.expect("list tracker");

    assert_eq!(listed["status"], "ok");
    assert_eq!(listed["checklist"]["completed"], 1);
}

#[tokio::test]
async fn update_supports_bulk_item_sync_and_global_mirror() {
    let (temp_dir, _state, tool) = setup_planning_workflow().await;

    tool.execute(json!({
        "action": "create",
        "items": ["Step 1", "Step 2"]
    }))
    .await
    .expect("create tracker");

    let updated = tool
        .execute(json!({
            "action": "update",
            "items": ["[x] Step 1", "[~] Step 2", "[ ] Step 3"]
        }))
        .await
        .expect("bulk update");

    assert_eq!(updated["status"], "updated");
    assert_eq!(updated["checklist"]["completed"], 1);
    assert_eq!(updated["checklist"]["in_progress"], 1);
    assert_eq!(updated["checklist"]["pending"], 1);

    let mirrored = temp_dir.path().join(".vtcode").join("tasks").join("current_task.md");
    let mirrored_content = std::fs::read_to_string(mirrored).expect("read mirrored checklist");
    assert!(mirrored_content.contains("Step 3"));
}

#[tokio::test]
async fn update_accepts_flat_index_fallback() {
    let (_temp_dir, _state, tool) = setup_planning_workflow().await;

    tool.execute(json!({
        "action": "create",
        "items": ["Parent task"]
    }))
    .await
    .expect("create tracker");

    let updated = tool
        .execute(json!({
            "action": "update",
            "index": 1,
            "status": "completed"
        }))
        .await
        .expect("flat-index update");

    assert_eq!(updated["status"], "updated");
    assert_eq!(updated["checklist"]["completed"], 1);
}

#[tokio::test]
async fn update_rejects_zero_flat_index_even_for_checklist_completion() {
    let (_temp_dir, _state, tool) = setup_planning_workflow().await;

    tool.execute(json!({
        "action": "create",
        "items": ["Parent task"]
    }))
    .await
    .expect("create tracker");

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
async fn planning_task_tracker_schema_requires_positive_indices() {
    let (_temp_dir, _state, tool) = setup_planning_workflow().await;
    let schema = tool.parameter_schema().expect("planning task tracker schema");

    assert_eq!(schema["properties"]["index"]["minimum"], 1);
    let items_description = schema["properties"]["items"]["description"].as_str().unwrap();
    assert!(items_description.contains("Full checklist replacement"));
    assert!(items_description.contains("never JSON-encoded strings"));
    assert!(items_description.contains("2-space indentation"));
    assert_eq!(schema["properties"]["index_path"]["pattern"], "^[1-9][0-9]*(\\.[1-9][0-9]*)*$");
    assert!(
        schema["properties"]["index_path"]["description"]
            .as_str()
            .expect("index path description")
            .contains("positive")
    );
}

#[tokio::test]
async fn planning_task_tracker_schema_rejects_mixed_bulk_and_single_updates() {
    let (_temp_dir, _state, tool) = setup_planning_workflow().await;
    let schema = tool.parameter_schema().expect("planning task tracker schema");
    let invalid_cases = [
        json!({"action": "list", "index": 1}),
        json!({"action": "list", "index_path": "1"}),
        json!({"action": "create", "index": 1, "items": ["New item"]}),
        json!({"action": "add", "index_path": "1", "description": "New item"}),
        json!({"action": "update", "items": ["Done"], "index": 1, "status": "completed"}),
        json!({"action": "update", "items": ["Done"], "index_path": "1.1", "status": "completed"}),
        json!({"action": "update", "index": 1, "index_path": "1.1", "status": "completed"}),
        json!({"action": "update", "items": ["Done"], "status": "completed"}),
    ];

    for args in invalid_cases {
        assert!(jsonschema::validate(&schema, &args).is_err(), "expected invalid args: {args}");
    }
}

#[tokio::test]
async fn bulk_update_rejects_encoded_commands_without_resetting_completed_steps() {
    let (_temp_dir, _state, tool) = setup_planning_workflow().await;
    tool.execute(json!({
        "action": "create",
        "title": "Preserve progress",
        "items": [
            {"description": "Update introduction", "status": "completed", "files": ["README.md"], "outcome": "Clear overview", "verify": ["review headings"]},
            {"description": "Review links", "status": "blocked", "outcome": "Await network"}
        ]
    })).await.unwrap();
    let tracker_path = _temp_dir.path().join(".vtcode/tasks/current_task.md");
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
async fn planning_update_rejects_mixed_bulk_and_single_fields_before_mutation() {
    let (_temp_dir, _state, tool) = setup_planning_workflow().await;

    tool.execute(json!({
        "action": "create",
        "items": ["Original"]
    }))
    .await
    .expect("create tracker");

    let error = tool
        .execute(json!({
            "action": "update",
            "items": ["Replacement"],
            "index_path": "1",
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
async fn planning_update_rejects_both_index_forms_before_mutation() {
    let (_temp_dir, _state, tool) = setup_planning_workflow().await;

    tool.execute(json!({
        "action": "create",
        "items": ["Original"]
    }))
    .await
    .expect("create tracker");

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
    let (_temp_dir, _state, tool) = setup_planning_workflow().await;

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

#[tokio::test]
async fn rejects_when_planning_workflow_is_inactive() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
    let tool = PlanningTaskTrackerTool::new(state);

    let err = tool
        .execute(json!({"action": "list"}))
        .await
        .expect_err("should fail outside planning workflow");

    assert!(err.to_string().contains("only available while planning"));
}
