use assert_fs::TempDir;
use vtcode_core::config::ToolDocumentationMode;
use vtcode_core::config::constants::tools;
use vtcode_core::config::types::CapabilityLevel;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::handlers::{SessionSurface, SessionToolsConfig, ToolModelCapabilities};

#[tokio::test]
async fn public_catalog_uses_canonical_names_only() {
    let temp = TempDir::new().expect("tempdir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

    let public_tools = registry.available_tools().await;

    assert!(public_tools.contains(&tools::UNIFIED_SEARCH.to_string()));
    assert!(public_tools.contains(&tools::UNIFIED_FILE.to_string()));
    assert!(public_tools.contains(&tools::UNIFIED_EXEC.to_string()));
    assert!(!public_tools.contains(&tools::READ_FILE.to_string()));
    assert!(!public_tools.contains(&tools::WRITE_FILE.to_string()));
    assert!(!public_tools.contains(&tools::RUN_PTY_CMD.to_string()));
    assert!(!public_tools.contains(&tools::SEND_PTY_INPUT.to_string()));
}

#[tokio::test]
async fn acp_surface_matches_canonical_local_subset() {
    let temp = TempDir::new().expect("tempdir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

    let acp_tools = registry
        .public_tool_names(SessionSurface::Acp, CapabilityLevel::CodeSearch)
        .await;

    assert_eq!(
        acp_tools,
        vec![
            tools::UNIFIED_EXEC.to_string(),
            tools::UNIFIED_FILE.to_string(),
            tools::UNIFIED_SEARCH.to_string(),
        ]
    );
}

#[tokio::test]
async fn schema_entries_follow_public_catalog() {
    let temp = TempDir::new().expect("tempdir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

    let schema_entries = registry
        .schema_entries(SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        ))
        .await;
    let names: Vec<_> = schema_entries.into_iter().map(|entry| entry.name).collect();

    assert!(names.contains(&tools::UNIFIED_SEARCH.to_string()));
    assert!(names.contains(&tools::UNIFIED_FILE.to_string()));
    assert!(names.contains(&tools::UNIFIED_EXEC.to_string()));
    assert!(names.contains(&tools::APPLY_PATCH.to_string()));
    assert!(!names.contains(&tools::READ_FILE.to_string()));
    assert!(!names.contains(&tools::WRITE_FILE.to_string()));
    assert!(!names.contains(&tools::RUN_PTY_CMD.to_string()));
}
