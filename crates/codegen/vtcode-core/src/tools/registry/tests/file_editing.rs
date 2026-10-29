use super::*;

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
