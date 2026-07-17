#![allow(missing_docs)]
use assert_fs::TempDir;
use serde_json::json;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::tool_policy::ToolPolicy as RuntimeToolPolicy;
use vtcode_core::tools::ToolRegistry;

#[cfg(test)]
mod integration_tests {

    use super::*;

    #[tokio::test]
    async fn test_tool_registry_creation() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let _registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    }

    #[tokio::test]
    async fn test_list_files_tool_is_not_public() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        // Create some test files
        std::fs::write(temp_dir.path().join("test1.txt"), "content1").unwrap();
        std::fs::write(temp_dir.path().join("test2.txt"), "content2").unwrap();
        std::fs::create_dir(temp_dir.path().join("subdir")).unwrap();

        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        let args = json!({
            "path": "."
        });

        let err = registry
            .execute_public_tool_ref(tools::LIST_FILES, &args)
            .await
            .expect_err("list_files should not be exposed as a public tool");
        assert!(err.to_string().contains("Unknown tool"));
    }

    #[tokio::test]
    async fn test_unified_file_tool_is_not_public() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let test_content = "This is test content";
        std::fs::write(temp_dir.path().join("read_test.txt"), test_content).unwrap();

        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();

        let args = json!({
            "action": "read",
            "path": "read_test.txt"
        });

        let err = registry
            .execute_public_tool_ref(tools::UNIFIED_FILE, &args)
            .await
            .expect_err("file_operation_internal should not be exposed as a public tool");
        assert!(err.to_string().contains("Unknown tool"));
    }

    #[tokio::test]
    async fn test_tools_config_overrides_policies() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path();
        std::env::set_current_dir(workspace).unwrap();

        let config_contents = r#"
[tools]
default_policy = "deny"

[tools.policies]
exec_command = "allow"
apply_patch = "allow"
"#;

        std::fs::write(workspace.join("vtcode.toml"), config_contents).unwrap();

        let registry = ToolRegistry::new(workspace.to_path_buf()).await;
        registry.initialize_async().await.unwrap();

        let cfg_manager = ConfigManager::load_from_workspace(workspace).unwrap();
        registry
            .apply_tool_runtime_config(&cfg_manager.config().commands, &cfg_manager.config().tools)
            .await
            .unwrap();

        assert_eq!(registry.get_tool_policy(tools::EXEC_COMMAND).await, RuntimeToolPolicy::Allow);
        assert_eq!(registry.get_tool_policy(tools::APPLY_PATCH).await, RuntimeToolPolicy::Allow);
    }

    #[tokio::test]
    async fn test_write_file_tool_is_not_public() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();

        let args = json!({
            "path": "write_test.txt",
            "content": "Hello, World!",
            "overwrite": false,
            "create_dirs": false
        });

        let err = registry
            .execute_public_tool_ref("write_file", &args)
            .await
            .expect_err("write_file should not be exposed as a public tool");
        assert!(err.to_string().contains("Unknown tool"));

        // Verify the rejected public call did not create a file.
        let file_path = temp_dir.path().join("write_test.txt");
        assert!(!file_path.exists());
    }

    #[tokio::test]
    async fn test_grep_file_tool_is_not_public() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let rust_content = r#"fn main() {
    println!("Hello, world!");
    let x = 42;
}

fn calculate_sum(a: i32, b: i32) -> i32 {
    a + b
}"#;
        std::fs::write(temp_dir.path().join("search_test.rs"), rust_content).unwrap();

        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        let args = json!({
            "pattern": "fn main",
            "path": ".",
            "type": "regex"
        });

        let err = registry
            .execute_public_tool_ref(tools::GREP_FILE, &args)
            .await
            .expect_err("grep_file should not be exposed as a public tool");
        assert!(err.to_string().contains("Unknown tool"));
    }
}
