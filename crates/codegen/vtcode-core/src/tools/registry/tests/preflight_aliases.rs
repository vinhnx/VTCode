use super::*;

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
