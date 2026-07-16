#![allow(missing_docs)]
use assert_fs::TempDir;
use vtcode_core::config::ToolDocumentationMode;
use vtcode_core::config::constants::tools;
use vtcode_core::config::types::CapabilityLevel;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::handlers::{
    SessionSurface, SessionToolsConfig, ToolModelCapabilities, ToolProfile,
};

fn default_public_tool_names() -> Vec<String> {
    vec![
        tools::EXEC_COMMAND.to_string(),
        tools::WRITE_STDIN.to_string(),
        tools::APPLY_PATCH.to_string(),
    ]
}

/// The public catalog order derives from rig's `ToolSet` (HashMap-backed), so
/// it is not a stable contract. Compare the tool set order-insensitively.
fn assert_same_tool_set(actual: &[String], expected: &[String]) {
    let mut actual_sorted = actual.to_vec();
    actual_sorted.sort();
    let mut expected_sorted = expected.to_vec();
    expected_sorted.sort();
    assert_eq!(actual_sorted, expected_sorted);
}

#[tokio::test]
async fn audit_public_catalog_is_runtime_driven() {
    let temp = TempDir::new().expect("tempdir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

    let public_names = registry.available_tools().await;
    let schema_names: Vec<_> = registry
        .schema_entries(SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        ))
        .await
        .into_iter()
        .map(|entry| entry.name)
        .collect();

    assert_eq!(public_names, schema_names);
    assert_same_tool_set(&public_names, &default_public_tool_names());
    assert!(!public_names.contains(&tools::CODE_SEARCH.to_string()));
    assert!(!public_names.contains(&tools::UNIFIED_SEARCH.to_string()));
    assert!(!public_names.contains(&tools::UNIFIED_FILE.to_string()));
    assert!(!public_names.contains(&tools::UNIFIED_EXEC.to_string()));
}

#[tokio::test]
async fn audit_code_search_is_advanced_only() {
    let temp = TempDir::new().expect("tempdir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

    let advanced_schema_names: Vec<_> = registry
        .schema_entries(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode),
        )
        .await
        .into_iter()
        .map(|entry| entry.name)
        .collect();

    assert!(advanced_schema_names.contains(&tools::CODE_SEARCH.to_string()));
}

#[tokio::test]
async fn audit_public_catalog_hides_legacy_aliases() {
    let temp = TempDir::new().expect("tempdir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

    let public_names = registry.available_tools().await;

    for legacy_name in [
        tools::READ_FILE,
        tools::WRITE_FILE,
        tools::EDIT_FILE,
        tools::DELETE_FILE,
        tools::RUN_PTY_CMD,
        tools::SEND_PTY_INPUT,
        tools::LIST_PTY_SESSIONS,
        tools::UNIFIED_SEARCH,
        tools::UNIFIED_FILE,
        tools::UNIFIED_EXEC,
    ] {
        assert!(
            !public_names.iter().any(|name| name == legacy_name),
            "legacy alias leaked into public catalog: {legacy_name}"
        );
    }
}

#[tokio::test]
async fn audit_acp_subset_comes_from_same_catalog() {
    let temp = TempDir::new().expect("tempdir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

    let acp_names = registry
        .public_tool_names(SessionSurface::Acp, CapabilityLevel::CodeSearch)
        .await;

    assert_same_tool_set(&acp_names, &default_public_tool_names());
    assert!(!acp_names.contains(&tools::CODE_SEARCH.to_string()));
}
