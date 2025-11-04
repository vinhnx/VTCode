use anyhow::Result;
use assert_fs::TempDir;
use serde_json::json;
use vtcode_core::{
    tools::ToolRegistry,
    utils::ansi::{AnsiRenderer, MessageStyle},
};

#[tokio::test]
async fn test_write_and_edit_rendered() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut renderer = AnsiRenderer::stdout();
    if let Err(err) = registry.allow_all_tools().await {
        eprintln!("Skipping policy configuration in test: {}", err);
    }

    let write_args = json!({
        "path": "file.txt",
        "content": "hello",
        "mode": "overwrite"
    });
    let write_res = registry.write_file(write_args).await?;
    assert!(write_res["success"].as_bool().unwrap_or(false));
    renderer.line(MessageStyle::Info, "write ok")?;

    let edit_args = json!({
        "path": "file.txt",
        "old_str": "hello",
        "new_str": "world"
    });
    let edit_res = registry.edit_file(edit_args).await?;
    assert!(edit_res["success"].as_bool().unwrap_or(false));
    renderer.line(MessageStyle::Info, "edit ok")?;

    let read_args = json!({ "path": "file.txt" });
    let read_res = registry.read_file(read_args).await?;
    assert_eq!(read_res["content"], "world");

    Ok(())
}
