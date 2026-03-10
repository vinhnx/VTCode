use anyhow::Result;
use serde_json::json;
use tempfile::TempDir;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::edited_file_monitor::FILE_CONFLICT_OVERRIDE_ARG;
use vtcode_core::tools::registry::ToolRegistry;

async fn create_registry(workspace: &TempDir) -> ToolRegistry {
    ToolRegistry::new(workspace.path().to_path_buf()).await
}

#[tokio::test]
async fn write_file_returns_conflict_when_disk_changed_since_read() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = create_registry(&workspace).await;
    let path = workspace.path().join("sample.txt");
    std::fs::write(&path, "before\n")?;

    registry.read_file(json!({ "path": "sample.txt" })).await?;
    std::fs::write(&path, "external\n")?;

    let result = registry
        .write_file(json!({
            "path": "sample.txt",
            "content": "agent\n",
            "mode": "overwrite"
        }))
        .await?;

    assert_eq!(result["conflict_detected"], json!(true));
    assert_eq!(result["conflict_path"], json!("sample.txt"));
    assert_eq!(result["disk_content"], json!("external\n"));
    assert_eq!(result["intended_content"], json!("agent\n"));
    assert_eq!(std::fs::read_to_string(&path)?, "external\n");
    Ok(())
}

#[tokio::test]
async fn edit_file_returns_conflict_with_intended_content() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = create_registry(&workspace).await;
    let path = workspace.path().join("sample.txt");
    std::fs::write(&path, "before\n")?;

    registry.read_file(json!({ "path": "sample.txt" })).await?;
    std::fs::write(&path, "external\n")?;

    let result = registry
        .edit_file(json!({
            "path": "sample.txt",
            "old_str": "before",
            "new_str": "agent"
        }))
        .await?;

    assert_eq!(result["conflict_detected"], json!(true));
    assert_eq!(result["conflict_path"], json!("sample.txt"));
    assert_eq!(result["disk_content"], json!("external\n"));
    assert_eq!(result["intended_content"], json!("agent\n"));
    assert_eq!(std::fs::read_to_string(&path)?, "external\n");
    Ok(())
}

#[tokio::test]
async fn apply_patch_returns_conflict_without_partial_write() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = create_registry(&workspace).await;
    let path = workspace.path().join("sample.txt");
    std::fs::write(&path, "before\n")?;

    registry.read_file(json!({ "path": "sample.txt" })).await?;
    std::fs::write(&path, "external\n")?;

    let patch = "\
*** Begin Patch
*** Update File: sample.txt
@@
-before
+agent
*** End Patch
";

    let result = registry
        .execute_tool(
            tools::UNIFIED_FILE,
            json!({
                "action": "patch",
                "patch": patch,
            }),
        )
        .await?;

    assert_eq!(result["conflict_detected"], json!(true));
    assert_eq!(result["conflict_path"], json!("sample.txt"));
    assert_eq!(result["disk_content"], json!("external\n"));
    assert_eq!(result["intended_content"], json!("agent\n"));
    assert_eq!(std::fs::read_to_string(&path)?, "external\n");
    Ok(())
}

#[tokio::test]
async fn override_snapshot_requires_latest_disk_state() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = create_registry(&workspace).await;
    let path = workspace.path().join("sample.txt");
    std::fs::write(&path, "before\n")?;

    registry.read_file(json!({ "path": "sample.txt" })).await?;
    std::fs::write(&path, "external one\n")?;

    let initial_conflict = registry
        .write_file(json!({
            "path": "sample.txt",
            "content": "agent\n",
            "mode": "overwrite"
        }))
        .await?;

    std::fs::write(&path, "external two\n")?;

    let mut override_args = json!({
        "path": "sample.txt",
        "content": "agent\n",
        "mode": "overwrite"
    });
    override_args[FILE_CONFLICT_OVERRIDE_ARG] = initial_conflict["disk_snapshot"].clone();

    let retried = registry.write_file(override_args).await?;

    assert_eq!(retried["conflict_detected"], json!(true));
    assert_eq!(retried["disk_content"], json!("external two\n"));
    assert_eq!(std::fs::read_to_string(&path)?, "external two\n");
    Ok(())
}
