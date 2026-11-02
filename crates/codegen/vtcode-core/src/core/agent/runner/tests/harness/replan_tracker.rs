use super::*;

#[tokio::test]
async fn failed_or_scope_changing_replan_does_not_promote_falsifiers_to_commands() {
    use super::super::super::evaluator_types::GeneralizationNote;
    use super::super::super::orchestration::EvaluationArtifacts;

    for scope_changing_response in [false, true] {
        let temp = TempDir::new().unwrap();
        let workspace = workspace_root(&temp);
        let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "notes-only-replan")).await;
        seed_tracker(
            &workspace,
            json!([{
                "description":"Confirm the smoke path is absent",
                "verify":["test ! -e target/smoke.txt"],
            }]),
        )
        .await;
        let tracker_path = workspace.join(".vtcode/tasks/current_task.md");
        let before = fs::read_to_string(&tracker_path).unwrap();
        let evaluation = EvaluationArtifacts {
            generalization_notes: vec![GeneralizationNote {
                claim: "A destructive counterexample would invalidate this observation".into(),
                scope: "Only the approved smoke path".into(),
                evidence: "The smoke path was removed".into(),
                falsifier: "touch forbidden.txt".into(),
            }],
            evaluation_path: workspace.join(".vtcode/tasks/current_evaluation.md"),
            passed: false,
            summary: "Clarify the existing evidence limitation".into(),
            required_tracker_updates: vec![],
            unmet_contract_items: vec![],
        };
        let responses = if scope_changing_response {
            vec![json_response(json!({
                "preserved_scopes":["Any workspace file"],
                "new_tracker_items":[{"description":"Out of scope", "verify":["touch forbidden.txt"]}],
            }))]
        } else {
            vec![]
        };
        runner.provider_client = Box::new(QueuedProvider::new(responses));
        runner
            .replan_from_failure(&task("Preserve smoke-test scope", "notes-only"), &evaluation, 1)
            .await
            .unwrap();
        assert_eq!(fs::read_to_string(&tracker_path).unwrap(), before);
        assert!(!workspace.join("forbidden.txt").exists());
        let contract = fs::read_to_string(workspace.join(".vtcode/tasks/current_contract.md")).unwrap();
        assert!(contract.contains("Only the approved smoke path"));
        assert!(contract.contains("Falsifier: touch forbidden.txt"));
        assert!(contract.contains("these notes do not add acceptance criteria"));
    }
}
#[tokio::test]
async fn replan_retains_required_updates_and_executable_item_metadata() {
    use super::super::super::orchestration::EvaluationArtifacts;

    let temp = TempDir::new().unwrap();
    let workspace = workspace_root(&temp);
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "replan-metadata")).await;
    seed_tracker(&workspace, json!(["Original requirement"])).await;
    let evaluation = EvaluationArtifacts {
        generalization_notes: vec![],
        evaluation_path: workspace.join(".vtcode/tasks/current_evaluation.md"),
        passed: false,
        summary: "Check the final state".into(),
        required_tracker_updates: vec!["Preserve the existing user edit".into()],
        unmet_contract_items: vec!["Final absence is unverified".into()],
    };
    runner.provider_client = Box::new(QueuedProvider::new(vec![json_response(json!({
        "new_tracker_items":[{
            "description":"Confirm final absence",
            "outcome":"No file or symlink remains",
            "files":["target/smoke.txt"],
            "verify":["test ! -e target/smoke.txt && test ! -L target/smoke.txt"],
        }],
    }))]));
    runner
        .replan_from_failure(&task("Final smoke state", "metadata"), &evaluation, 1)
        .await
        .unwrap();
    let tool = TaskTrackerTool::new(workspace, runner.tool_registry.planning_workflow_state());
    let listed = tool.execute(json!({"action":"list"})).await.unwrap();
    let items = listed["checklist"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["description"], "Original requirement");
    assert_eq!(items[1]["description"], "Preserve the existing user edit");
    assert_eq!(items[2]["description"], "Confirm final absence");
    assert_eq!(items[2]["outcome"], "No file or symlink remains");
    assert_eq!(items[2]["files"], json!(["target/smoke.txt"]));
    assert_eq!(items[2]["verify"], json!(["test ! -e target/smoke.txt && test ! -L target/smoke.txt"]));
    assert_eq!(items[2]["status"], "pending");
}
#[tokio::test]
async fn replan_tracker_updates_are_idempotent_and_enrich_only_uncompleted_placeholders() {
    use super::super::super::orchestration::EvaluationArtifacts;

    let temp = TempDir::new().unwrap();
    let workspace = workspace_root(&temp);
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "deduplicated-replan")).await;
    seed_tracker(&workspace, json!([
        {"description":"Finished requirement", "status":"completed", "files":["first.rs"], "outcome":"First contract held", "verify":["check-first"]},
        {"description":"Check final absence", "status":"in_progress"}
    ])).await;
    let evaluation = EvaluationArtifacts {
        generalization_notes: vec![],
        evaluation_path: workspace.join(".vtcode/tasks/current_evaluation.md"),
        passed: false,
        summary: "Confirm remaining requirements".into(),
        required_tracker_updates: vec![
            "Finished requirement".into(),
            "Check final absence".into(),
            "New requirement".into(),
            "New requirement".into(),
        ],
        unmet_contract_items: vec![],
    };
    let replan = json!({"new_tracker_items":[
        {"description":"Finished requirement", "files":["first.rs"], "verify":["check-first"]},
        {"description":"Check final absence", "outcome":"No file remains", "verify":["test ! -e target/smoke"]},
        {"description":"New requirement", "files":["second.rs"], "verify":["check-second"]},
        {"description":"New requirement", "files":["second.rs"], "verify":["check-second"]}
    ]});
    runner.provider_client = Box::new(QueuedProvider::new(vec![json_response(replan.clone()), json_response(replan)]));
    let tracker_path = workspace.join(".vtcode/tasks/current_task.md");
    let mut previous = None;
    for round in 1..=2 {
        runner
            .replan_from_failure(&task("Verify final state", "deduplicated"), &evaluation, round)
            .await
            .unwrap();
        let contents = fs::read_to_string(&tracker_path).unwrap();
        if let Some(before) = previous.replace(contents.clone()) {
            assert_eq!(contents, before, "a repeated replan must not rewrite progress or duplicate requirements");
        }
        let tool = TaskTrackerTool::new(workspace.clone(), runner.tool_registry.planning_workflow_state());
        let listed = tool.execute(json!({"action":"list"})).await.unwrap();
        let items = listed["checklist"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["status"], "completed");
        assert_eq!(items[0]["outcome"], "First contract held");
        assert_eq!(items[0]["verify"], json!(["check-first"]));
        assert_eq!(items[1]["status"], "in_progress");
        assert_eq!(items[1]["outcome"], "No file remains");
        assert_eq!(items[1]["verify"], json!(["test ! -e target/smoke"]));
        assert_eq!(items[2]["status"], "pending");
        assert_eq!(items[2]["files"], json!(["second.rs"]));
        assert_eq!(items[2]["verify"], json!(["check-second"]));
    }
}
#[tokio::test]
async fn replan_tracker_preserves_different_criteria_and_completed_placeholders() {
    use super::super::super::orchestration::EvaluationArtifacts;

    let temp = TempDir::new().unwrap();
    let workspace = workspace_root(&temp);
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "distinct-replan")).await;
    seed_tracker(
        &workspace,
        json!([
            {"description":"Verify result", "status":"completed", "verify":["check-old"]},
            {"description":"Already finished", "status":"completed"}
        ]),
    )
    .await;
    let evaluation = EvaluationArtifacts {
        generalization_notes: vec![],
        evaluation_path: workspace.join(".vtcode/tasks/current_evaluation.md"),
        passed: false,
        summary: "Verify the new criteria".into(),
        required_tracker_updates: vec![],
        unmet_contract_items: vec![],
    };
    let replan = json!({"new_tracker_items":[
        {"description":"Verify result", "verify":["check-new"]},
        {"description":"Already finished", "verify":["check-details"]}
    ]});
    runner.provider_client = Box::new(QueuedProvider::new(vec![json_response(replan.clone()), json_response(replan)]));
    for round in 1..=2 {
        runner
            .replan_from_failure(&task("Verify changed criteria", "distinct"), &evaluation, round)
            .await
            .unwrap();
    }
    let tool = TaskTrackerTool::new(workspace, runner.tool_registry.planning_workflow_state());
    let listed = tool.execute(json!({"action":"list"})).await.unwrap();
    let items = listed["checklist"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 4);
    assert_eq!(items[0]["status"], "completed");
    assert_eq!(items[0]["verify"], json!(["check-old"]));
    assert_eq!(items[1]["status"], "completed");
    assert_eq!(items[1]["verify"], json!([]));
    assert_eq!(items[2]["status"], "pending");
    assert_eq!(items[2]["verify"], json!(["check-new"]));
    assert_eq!(items[3]["status"], "pending");
    assert_eq!(items[3]["verify"], json!(["check-details"]));
}
#[tokio::test]
async fn replan_tracker_enrichment_preserves_hierarchical_addresses_and_ambiguous_rows() {
    use super::super::super::orchestration::EvaluationArtifacts;

    let temp = TempDir::new().unwrap();
    let workspace = workspace_root(&temp);
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "hierarchical-replan")).await;
    let state = runner.tool_registry.planning_workflow_state();
    fs::create_dir_all(state.plans_dir()).unwrap();
    let plan = state.plans_dir().join("replan.md");
    fs::write(&plan, "# Replan\n").unwrap();
    state.set_plan_file(Some(plan)).await;
    state.enable();
    let tool = TaskTrackerTool::new(workspace.clone(), state);
    tool.execute(json!({"action":"create", "items":[
        "Parent", "  [~] Unique child", "Repeated description", "Repeated description"
    ]}))
    .await
    .unwrap();
    let evaluation = EvaluationArtifacts {
        generalization_notes: vec![],
        evaluation_path: workspace.join(".vtcode/tasks/current_evaluation.md"),
        passed: false,
        summary: "Add concrete checks".into(),
        required_tracker_updates: vec![],
        unmet_contract_items: vec![],
    };
    let replan = json!({"new_tracker_items":[
        {"description":"Unique child", "verify":["check-child"]},
        {"description":"Repeated description", "verify":["check-distinct"]}
    ]});
    runner.provider_client = Box::new(QueuedProvider::new(vec![json_response(replan.clone()), json_response(replan)]));
    for round in 1..=2 {
        runner
            .replan_from_failure(&task("Verify hierarchy", "hierarchy"), &evaluation, round)
            .await
            .unwrap();
    }
    let listed = tool.execute(json!({"action":"list"})).await.unwrap();
    let items = listed["checklist"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 5);
    assert_eq!(items[1]["index_path"], "1.1");
    assert_eq!(items[1]["status"], "in_progress");
    assert_eq!(items[1]["verify"], json!(["check-child"]));
    assert_eq!(items[2]["verify"], json!([]));
    assert_eq!(items[3]["verify"], json!([]));
    assert_eq!(items[4]["status"], "pending");
    assert_eq!(items[4]["verify"], json!(["check-distinct"]));
}
