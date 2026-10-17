use super::*;
use std::time::{Duration, SystemTime};
use tempfile::TempDir;

async fn setup_tracker(planning: bool) -> (TempDir, PlanningWorkflowState, TaskTrackerTool) {
    let temp = TempDir::new().unwrap();
    let state = PlanningWorkflowState::new(temp.path().to_path_buf());
    let plan = temp.path().join(".vtcode/plans/metadata.md");
    std::fs::create_dir_all(plan.parent().unwrap()).unwrap();
    std::fs::write(&plan, "# Metadata regression\n").unwrap();
    state.set_plan_file(Some(plan)).await;
    if planning {
        state.enable();
    }
    let tool = TaskTrackerTool::new(temp.path().to_path_buf(), state.clone());
    (temp, state, tool)
}

fn initial_items() -> Value {
    json!([
        {"description":"Edit README", "files":["README.md"], "outcome":"Clearer guidance", "verify":"cargo check"},
        {"description":"Check links", "files":["docs/index.md"], "outcome":"Working links", "verify":["cargo clippy"]},
        {"description":"Retired step", "verify":["retired verifier"]}
    ])
}

#[tokio::test]
async fn bulk_status_updates_preserve_metadata_across_reorder_and_reload() {
    for planning in [false, true] {
        let (temp, state, tool) = setup_tracker(planning).await;
        tool.execute(json!({"action":"create", "title":"Metadata", "items":initial_items()}))
            .await
            .unwrap();
        let updated = tool
            .execute(json!({"action":"update", "items":[
                "[x] Check links", "[~] Edit README", "[ ] New step"
            ]}))
            .await
            .unwrap();
        assert_eq!(updated["status"], "updated");
        let items = updated["checklist"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["description"], "Check links");
        assert_eq!(items[0]["status"], "completed");
        assert_eq!(items[0]["files"], json!(["docs/index.md"]));
        assert_eq!(items[0]["outcome"], "Working links");
        assert_eq!(items[0]["verify"], json!(["cargo clippy"]));
        assert_eq!(items[1]["status"], "in_progress");
        assert_eq!(items[1]["files"], json!(["README.md"]));
        assert_eq!(items[1]["outcome"], "Clearer guidance");
        assert_eq!(items[1]["verify"], json!(["cargo check"]));
        assert_eq!(items[2]["description"], "New step");
        assert_eq!(items[2]["verify"], json!([]));
        let reloaded = TaskTrackerTool::new(temp.path().to_path_buf(), state)
            .execute(json!({"action":"list"}))
            .await
            .unwrap();
        assert_eq!(reloaded["checklist"], updated["checklist"]);
    }
}

#[tokio::test]
async fn bulk_updates_honor_explicit_metadata_replacements_and_clears() {
    for planning in [false, true] {
        let (_temp, _state, tool) = setup_tracker(planning).await;
        tool.execute(json!({"action":"create", "items":initial_items()})).await.unwrap();
        let updated = tool
            .execute(json!({"action":"update", "items":[
                {"description":"Edit README", "status":"completed", "files":[], "outcome":"", "verify":[]},
                {"description":"Check links", "status":"in_progress", "verify":["replacement verifier"]}
            ]}))
            .await
            .unwrap();
        let items = updated["checklist"]["items"].as_array().unwrap();
        assert_eq!(items[0]["files"], json!([]));
        assert!(items[0]["outcome"].is_null());
        assert_eq!(items[0]["verify"], json!([]));
        assert_eq!(items[1]["files"], json!(["docs/index.md"]));
        assert_eq!(items[1]["outcome"], "Working links");
        assert_eq!(items[1]["verify"], json!(["replacement verifier"]));
    }
}

#[tokio::test]
async fn metadata_is_not_guessed_for_duplicate_or_renamed_steps() {
    for planning in [false, true] {
        let (_temp, _state, tool) = setup_tracker(planning).await;
        tool.execute(json!({"action":"create", "items":[
            {"description":"Duplicate", "verify":["first verifier"]},
            {"description":"Duplicate", "verify":["second verifier"]},
            {"description":"Unique", "verify":["unique verifier"]},
            {"description":"Old name", "verify":["old verifier"]}
        ]}))
        .await
        .unwrap();
        let before = tool.execute(json!({"action":"list"})).await.unwrap();
        let error = tool
            .execute(json!({"action":"update", "items":[
                "[x] Duplicate", "[x] Unique", "[ ] Unique", "[ ] New name"
            ]}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Ambiguous task description"));
        assert_eq!(tool.execute(json!({"action":"list"})).await.unwrap()["checklist"], before["checklist"]);
        let updated = tool.execute(json!({"action":"update", "items":[
            {"description":"Duplicate", "status":"completed", "files":[], "outcome":"", "verify":["chosen verifier"]},
            {"description":"Unique", "status":"completed", "files":[], "outcome":"", "verify":["unique verifier"]},
            {"description":"Unique", "status":"pending", "files":[], "outcome":"", "verify":[]},
            "[ ] New name"
        ]})).await.unwrap();
        assert_eq!(updated["checklist"]["items"][0]["verify"], json!(["chosen verifier"]));
        assert_eq!(updated["checklist"]["items"][1]["verify"], json!(["unique verifier"]));
        assert_eq!(updated["checklist"]["items"][2]["verify"], json!([]));
        assert_eq!(updated["checklist"]["items"][3]["verify"], json!([]));
    }
}

#[tokio::test]
async fn identical_updates_and_lists_do_not_rewrite_tracker_artifacts() {
    for planning in [false, true] {
        let (temp, state, tool) = setup_tracker(planning).await;
        tool.execute(json!({"action":"create", "items":[{
            "description":"Verify README", "verify":["cargo check"]
        }]}))
        .await
        .unwrap();
        let indexed_args = json!({"action":"update", "index":1, "status":"completed"});
        assert_eq!(tool.execute(indexed_args.clone()).await.unwrap()["status"], "updated");
        let plan_file = state.get_plan_file().await.unwrap();
        let paths = [
            temp.path().join(".vtcode/tasks/current_task.md"),
            tracker_file_for_plan_file(&plan_file).unwrap(),
            plan_file,
        ];
        let old_time = SystemTime::UNIX_EPOCH + Duration::from_secs(60);
        let before: Vec<_> = paths
            .iter()
            .map(|path| {
                let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
                file.set_times(std::fs::FileTimes::new().set_modified(old_time)).unwrap();
                std::fs::read(path).unwrap()
            })
            .collect();
        for args in [indexed_args, json!({"action":"update", "items":["[x] Verify README"]})] {
            let result = tool.execute(args).await.unwrap();
            assert_eq!(result["status"], "unchanged", "{result}");
            assert_eq!(result["checklist"]["completed"], 1);
            assert_eq!(result["checklist"]["items"][0]["verify"], json!(["cargo check"]));
        }
        tool.execute(json!({"action":"list"})).await.unwrap();
        for (path, original) in paths.iter().zip(before) {
            assert_eq!(std::fs::read(path).unwrap(), original);
            assert_eq!(std::fs::metadata(path).unwrap().modified().unwrap(), old_time, "{}", path.display());
        }
    }
}

#[tokio::test]
async fn invalid_bulk_status_does_not_publish_partial_updates() {
    for planning in [false, true] {
        let (_temp, _state, tool) = setup_tracker(planning).await;
        let created = tool.execute(json!({"action":"create", "items":initial_items()})).await.unwrap();
        assert!(
            tool.execute(json!({"action":"update", "items":[
                {"description":"Edit README", "status":"completed"},
                {"description":"Check links", "status":"invalid"}
            ]}))
            .await
            .is_err()
        );
        assert_eq!(tool.execute(json!({"action":"list"})).await.unwrap()["checklist"], created["checklist"]);
    }
}

#[tokio::test]
async fn hierarchical_bulk_updates_preserve_child_metadata() {
    let (temp, state, tool) = setup_tracker(true).await;
    tool.execute(json!({"action":"create", "items":[
        "Inspect", {"description":"  Read README", "files":["README.md"], "verify":["cargo check"]}, "Review"
    ]}))
    .await
    .unwrap();
    let updated = tool
        .execute(json!({"action":"update", "items":[
            "[x] Inspect", "  [x] Read README", "[~] Review"
        ]}))
        .await
        .unwrap();
    assert_eq!(updated["checklist"]["items"][1]["index_path"], "1.1");
    assert_eq!(updated["checklist"]["items"][1]["files"], json!(["README.md"]));
    assert_eq!(updated["checklist"]["items"][1]["verify"], json!(["cargo check"]));
    let reloaded = TaskTrackerTool::new(temp.path().to_path_buf(), state)
        .execute(json!({"action":"list"}))
        .await
        .unwrap();
    assert_eq!(reloaded["checklist"], updated["checklist"]);
}

#[tokio::test]
async fn bulk_sync_after_leaving_planning_preserves_indented_step_metadata() {
    let (_temp, state, tool) = setup_tracker(true).await;
    tool.execute(json!({"action":"create", "items":[
        "Inspect", {"description":"  Read README", "files":["README.md"],
        "outcome":"Checked documentation", "verify":["cargo check"]}, "Review"
    ]}))
    .await
    .unwrap();
    state.disable();
    let updated = tool
        .execute(json!({"action":"update", "items":[
            "[x] Inspect", "  [x] Read README", "[~] Review"
        ]}))
        .await
        .unwrap();
    let child = &updated["checklist"]["items"][1];
    assert_eq!(child["files"], json!(["README.md"]));
    assert_eq!(child["outcome"], "Checked documentation");
    assert_eq!(child["verify"], json!(["cargo check"]));
}

#[tokio::test]
async fn indented_duplicate_descriptions_require_explicit_metadata_after_planning() {
    let (_temp, state, tool) = setup_tracker(true).await;
    tool.execute(json!({"action":"create", "items":[
        {"description":"Review", "verify":["parent verifier"]},
        {"description":"  Review", "verify":["child verifier"]}
    ]}))
    .await
    .unwrap();
    state.disable();
    let error = tool
        .execute(json!({"action":"update", "items":["[x] Review"]}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Ambiguous task description"));
    let unchanged = tool.execute(json!({"action":"list"})).await.unwrap();
    assert_eq!(unchanged["checklist"]["items"][0]["verify"], json!(["parent verifier"]));
    assert_eq!(unchanged["checklist"]["items"][1]["verify"], json!(["child verifier"]));
}

#[tokio::test]
async fn unchanged_planning_update_repairs_missing_and_stale_mirrors() {
    let (temp, state, tool) = setup_tracker(true).await;
    tool.execute(json!({"action":"create", "items":[{
        "description":"Verify README", "verify":["cargo check"]
    }]}))
    .await
    .unwrap();
    let global = temp.path().join(".vtcode/tasks/current_task.md");
    let expected_global = std::fs::read(&global).unwrap();
    let plan = state.get_plan_file().await.unwrap();
    let sidecar = tracker_file_for_plan_file(&plan).unwrap();
    let original_sidecar = std::fs::read(&sidecar).unwrap();
    std::fs::remove_file(&global).unwrap();
    std::fs::write(&plan, "# Preserve this plan prose\n").unwrap();
    let result = tool
        .execute(json!({"action":"update", "index":1, "status":"pending"}))
        .await
        .unwrap();
    assert_eq!(result["status"], "unchanged");
    assert_eq!(std::fs::read(&global).unwrap(), expected_global);
    assert_eq!(std::fs::read(&sidecar).unwrap(), original_sidecar);
    let repaired_plan = std::fs::read_to_string(&plan).unwrap();
    assert!(repaired_plan.starts_with("# Preserve this plan prose\n"));
    assert!(repaired_plan.contains("Verify README"));
    assert!(repaired_plan.contains("cargo check"));
}

#[cfg(unix)]
#[tokio::test]
async fn tracker_updates_keep_private_artifacts_private() {
    use std::os::unix::fs::PermissionsExt;

    for planning in [false, true] {
        let (temp, state, tool) = setup_tracker(planning).await;
        tool.execute(json!({"action":"create", "items":["Private task details"]}))
            .await
            .unwrap();
        let plan = state.get_plan_file().await.unwrap();
        let paths = [
            temp.path().join(".vtcode/tasks/current_task.md"),
            tracker_file_for_plan_file(&plan).unwrap(),
            plan,
        ];
        for path in &paths {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        tool.execute(json!({"action":"update", "index":1, "status":"completed"}))
            .await
            .unwrap();
        for path in paths {
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600, "{}", path.display());
        }
    }
}

#[tokio::test]
async fn failed_standard_create_does_not_publish_a_partial_global_tracker() {
    let temp = TempDir::new().unwrap();
    let state = PlanningWorkflowState::new(temp.path().to_path_buf());
    std::fs::create_dir(temp.path().join(".vtcode")).unwrap();
    let blocker = temp.path().join(".vtcode/plans");
    std::fs::write(&blocker, "preserve this blocker").unwrap();
    state.set_plan_file(Some(blocker.join("broken.md"))).await;
    let tool = TaskTrackerTool::new(temp.path().to_path_buf(), state);
    assert!(
        tool.execute(json!({"action":"create", "items":["Never published"]}))
            .await
            .is_err()
    );
    assert!(!temp.path().join(".vtcode/tasks/current_task.md").exists());
    assert_eq!(std::fs::read_to_string(blocker).unwrap(), "preserve this blocker");
}

#[tokio::test]
async fn failed_planning_mirror_write_keeps_sidecar_and_plan_unchanged() {
    let (temp, state, tool) = setup_tracker(true).await;
    tool.execute(json!({"action":"create", "items":[{
        "description":"Check README", "verify":["cargo check"]
    }]}))
    .await
    .unwrap();
    let plan = state.get_plan_file().await.unwrap();
    let sidecar = tracker_file_for_plan_file(&plan).unwrap();
    let original_plan = std::fs::read(&plan).unwrap();
    let original_sidecar = std::fs::read(&sidecar).unwrap();
    let tasks_dir = temp.path().join(".vtcode/tasks");
    std::fs::remove_file(tasks_dir.join("current_task.md")).unwrap();
    std::fs::remove_dir(&tasks_dir).unwrap();
    std::fs::write(&tasks_dir, "preserve mirror blocker").unwrap();
    let args = json!({"action":"update", "index":1, "status":"completed"});
    assert!(tool.execute(args.clone()).await.is_err());
    assert_eq!(std::fs::read(&sidecar).unwrap(), original_sidecar);
    assert_eq!(std::fs::read(&plan).unwrap(), original_plan);
    assert_eq!(std::fs::read_to_string(&tasks_dir).unwrap(), "preserve mirror blocker");
    std::fs::remove_file(&tasks_dir).unwrap();
    std::fs::create_dir(&tasks_dir).unwrap();
    let recovered = tool.execute(args).await.unwrap();
    assert_eq!(recovered["checklist"]["completed"], 1);
    assert_eq!(recovered["checklist"]["items"][0]["verify"], json!(["cargo check"]));
}
