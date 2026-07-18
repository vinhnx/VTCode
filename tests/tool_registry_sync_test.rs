#![allow(missing_docs)]
use assert_fs::TempDir;
use futures::future::BoxFuture;
use serde_json::{Value, json};
use vtcode_core::config::ToolDocumentationMode;
use vtcode_core::config::constants::tools;
use vtcode_core::config::types::CapabilityLevel;
use vtcode_core::tools::handlers::{SessionSurface, SessionToolsConfig, ToolModelCapabilities, ToolProfile};
use vtcode_core::tools::{ToolRegistration, ToolRegistry};

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

fn dynamic_tool_executor<'a>(_registry: &'a ToolRegistry, _args: Value) -> BoxFuture<'a, anyhow::Result<Value>> {
    Box::pin(async { Ok(json!({"status": "ok"})) })
}

#[tokio::test]
async fn public_catalog_uses_canonical_names_only() {
    let temp = TempDir::new().expect("tempdir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

    let public_tools = registry.available_tools().await;

    assert_same_tool_set(&public_tools, &default_public_tool_names());
    assert!(!public_tools.contains(&tools::CODE_SEARCH.to_string()));
    assert!(!public_tools.contains(&tools::UNIFIED_SEARCH.to_string()));
    assert!(!public_tools.contains(&tools::UNIFIED_FILE.to_string()));
    assert!(!public_tools.contains(&tools::UNIFIED_EXEC.to_string()));
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

    assert_same_tool_set(&acp_tools, &default_public_tool_names());
    assert!(!acp_tools.contains(&tools::CODE_SEARCH.to_string()));
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

    assert_same_tool_set(&names, &default_public_tool_names());
    assert!(!names.contains(&tools::CODE_SEARCH.to_string()));
    assert!(!names.contains(&tools::UNIFIED_SEARCH.to_string()));
    assert!(!names.contains(&tools::UNIFIED_FILE.to_string()));
    assert!(!names.contains(&tools::UNIFIED_EXEC.to_string()));
    assert!(!names.contains(&tools::READ_FILE.to_string()));
    assert!(!names.contains(&tools::WRITE_FILE.to_string()));
    assert!(!names.contains(&tools::RUN_PTY_CMD.to_string()));
}

#[tokio::test]
async fn advanced_profile_exposes_code_search_outside_default_catalog() {
    let temp = TempDir::new().expect("tempdir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

    let schema_entries = registry
        .schema_entries(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode),
        )
        .await;
    let names: Vec<_> = schema_entries.into_iter().map(|entry| entry.name).collect();

    assert!(names.contains(&tools::CODE_SEARCH.to_string()));
}

#[tokio::test]
async fn explicit_full_auto_retains_tool_exposed_by_planning_transition() {
    let temp = TempDir::new().expect("tempdir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

    assert!(!registry.available_tools().await.contains(&tools::CODE_SEARCH.to_string()));

    registry.enable_full_auto_permission(&[tools::CODE_SEARCH.to_string()]).await;

    assert_eq!(registry.current_full_auto_allowlist().await, Some(vec![tools::CODE_SEARCH.to_string()]));
    assert!(registry.is_allowed_in_full_auto(tools::CODE_SEARCH).await);

    registry.enable_planning();

    assert!(registry.available_tools().await.contains(&tools::CODE_SEARCH.to_string()));
    assert!(
        registry
            .preflight_tool_permission(tools::CODE_SEARCH)
            .await
            .expect("code_search permission after planning transition")
    );
}

#[tokio::test]
async fn wildcard_full_auto_tracks_late_tools_visible_to_the_effective_profile() {
    let temp = TempDir::new().expect("tempdir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    let default_config = SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    );
    let advanced_config = default_config.clone().with_tool_profile(ToolProfile::AdvancedVtCode);
    let profile_filtered_tool = "profile_filtered_dynamic_tool";
    let late_advanced_tool = "late_advanced_dynamic_tool";
    let post_explicit_tool = "post_explicit_dynamic_tool";

    registry
        .enable_full_auto_permission_for_session(&[tools::WILDCARD_ALL.to_string()], default_config)
        .await;
    registry
        .register_tool(
            ToolRegistration::new(profile_filtered_tool, CapabilityLevel::Basic, false, dynamic_tool_executor)
                .with_description("profile-filtered dynamic tool"),
        )
        .await
        .expect("register profile-filtered dynamic tool");
    assert!(!registry.is_allowed_in_full_auto(profile_filtered_tool).await);

    registry
        .enable_full_auto_permission_for_session(&[tools::WILDCARD_ALL.to_string()], advanced_config)
        .await;
    assert!(registry.is_allowed_in_full_auto(profile_filtered_tool).await);

    registry
        .register_tool(
            ToolRegistration::new(late_advanced_tool, CapabilityLevel::Basic, false, dynamic_tool_executor)
                .with_description("late advanced dynamic tool"),
        )
        .await
        .expect("register late advanced dynamic tool");
    assert!(registry.is_allowed_in_full_auto(late_advanced_tool).await);

    registry.enable_full_auto_permission(&[tools::EXEC_COMMAND.to_string()]).await;
    registry
        .register_tool(
            ToolRegistration::new(post_explicit_tool, CapabilityLevel::Basic, false, dynamic_tool_executor)
                .with_description("dynamic tool registered after an explicit allow-list"),
        )
        .await
        .expect("register post-explicit dynamic tool");
    assert!(!registry.is_allowed_in_full_auto(late_advanced_tool).await);
    assert!(!registry.is_allowed_in_full_auto(post_explicit_tool).await);
}
