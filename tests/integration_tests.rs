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
    #[ignore]
    async fn test_list_files_tool() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        // Create some test files
        std::fs::write(temp_dir.path().join("test1.txt"), "content1").unwrap();
        std::fs::write(temp_dir.path().join("test2.txt"), "content2").unwrap();
        std::fs::create_dir(temp_dir.path().join("subdir")).unwrap();

        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        let args = json!({
            "path": "."
        });

        let result = registry.execute_tool("list_files", args).await;
        assert!(result.is_ok());

        let response: serde_json::Value = result.unwrap();
        assert!(response["files"].is_array());
        assert!(response["files"].as_array().unwrap().len() >= 2); // test1.txt, test2.txt, subdir
    }

    #[tokio::test]
    async fn test_read_file_tool() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let test_content = "This is test content";
        std::fs::write(temp_dir.path().join("read_test.txt"), test_content).unwrap();

        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();

        let args = json!({
            "path": "read_test.txt"
        });

        let result = registry.execute_tool("read_file", args).await;
        assert!(result.is_ok());

        let response: serde_json::Value = result.unwrap();
        assert_eq!(response["content"], test_content);
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
read_file = "allow"
"#;

        std::fs::write(workspace.join("vtcode.toml"), config_contents).unwrap();
        std::fs::write(workspace.join("sample.txt"), "hello world").unwrap();

        let mut registry = ToolRegistry::new(workspace.to_path_buf()).await;
        registry.initialize_async().await.unwrap();

        let cfg_manager = ConfigManager::load_from_workspace(workspace).unwrap();
        registry.apply_commands_config(&cfg_manager.config().commands);
        registry
            .apply_config_policies(&cfg_manager.config().tools)
            .await
            .unwrap();

        assert_eq!(
            registry.get_tool_policy(tools::READ_FILE),
            RuntimeToolPolicy::Allow
        );

        let result = registry
            .execute_tool(tools::READ_FILE, json!({ "path": "sample.txt" }))
            .await
            .unwrap();
        assert!(result["success"].as_bool().unwrap_or(false));
    }

    #[tokio::test]
    async fn test_write_file_tool() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();

        let args = json!({
            "path": "write_test.txt",
            "content": "Hello, World!",
            "overwrite": false,
            "create_dirs": false
        });

        let result = registry.execute_tool("write_file", args).await;
        assert!(result.is_ok());

        let _response: serde_json::Value = result.unwrap();

        // Verify file was created
        let file_path = temp_dir.path().join("write_test.txt");
        assert!(file_path.exists());
        let content = std::fs::read_to_string(file_path).unwrap();
        assert_eq!(content, "Hello, World!");
    }

    #[tokio::test]
    #[ignore]
    async fn test_grep_file_tool() {
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

        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        let args = json!({
            "pattern": "fn main",
            "path": ".",
            "type": "regex"
        });

        let result = registry.execute_tool("grep_file", args).await;
        assert!(result.is_ok());

        let response: serde_json::Value = result.unwrap();
        assert!(response["matches"].is_array());
        let matches = response["matches"].as_array().unwrap();
        assert!(!matches.is_empty());
    }
}
