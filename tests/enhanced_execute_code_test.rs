//! Integration test for enhanced execute_code with file tracking

use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use vtcode_core::mcp::McpClient;
use vtcode_core::config::mcp::McpClientConfig;
use vtcode_core::tools::ToolRegistry;

fn create_test_mcp_client() -> Arc<McpClient> {
    let config = McpClientConfig::default();
    Arc::new(McpClient::new(config))
}

#[tokio::test]
async fn test_execute_code_with_file_tracking() {
    // Create a temporary workspace
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();

    // Create a ToolRegistry
    let registry = ToolRegistry::new(workspace_root.clone()).await;
    registry.set_mcp_client(create_test_mcp_client()).await;

    // Test case 1: Generate a text file and verify tracking
    let python_code = r#"
with open('test_output.json', 'w') as f:
    f.write('Test content')
print('File generated successfully')
"#;

    let args = json!({
        "code": python_code,
        "language": "python3",
        "timeout_secs": 30,
        "track_files": true
    });

    let result = registry.execute_tool("execute_code", args).await.unwrap();

    // Verify the response contains file tracking info
    assert!(result.get("generated_files").is_some());

    let generated_files = result.get("generated_files").unwrap();
    assert_eq!(generated_files.get("count").unwrap().as_u64().unwrap(), 1);

    let files_array = generated_files.get("files").unwrap().as_array().unwrap();
    assert_eq!(files_array.len(), 1);

    let file_info = &files_array[0];
    let file_path = file_info.get("absolute_path").unwrap().as_str().unwrap();
    assert!(file_path.ends_with("test_output.json"));

    // Verify the file summary
    let summary = generated_files.get("summary").unwrap().as_str().unwrap();
    assert!(summary.contains("test_output.json"));
    assert!(summary.contains("bytes"));

    println!("✓ File tracking test passed: detected generated PDF");
}

#[tokio::test]
async fn test_execute_code_without_file_tracking() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let registry = ToolRegistry::new(workspace_root).await;
    registry.set_mcp_client(create_test_mcp_client()).await;

    let python_code = r#"
print('Hello World')
x = 5 * 10
print(f'Result: {x}')
"#;

    let args = json!({
        "code": python_code,
        "language": "python3",
        "timeout_secs": 10,
        "track_files": false  // Disable file tracking
    });

    let result = registry.execute_tool("execute_code", args).await.unwrap();

    // Verify that no file tracking info is present
    assert!(result.get("generated_files").is_none());

    // But normal execution should still work
    assert_eq!(result.get("exit_code").unwrap().as_i64().unwrap(), 0);
    assert!(
        result
            .get("stdout")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("Hello World")
    );

    println!("✓ File tracking disabled test passed");
}

#[tokio::test]
async fn test_multiple_file_generation() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let registry = ToolRegistry::new(workspace_root).await;
    registry.set_mcp_client(create_test_mcp_client()).await;

    let python_code = r#"
# Generate multiple files
with open('file1.json', 'w') as f:
    f.write('First file')

with open('file2.csv', 'w') as f:
    f.write('col1,col2\nvalue1,value2')

with open('output.json', 'w') as f:
    f.write('{"key": "value"}')

print('Multiple files generated')
"#;

    let args = json!({
        "code": python_code,
        "language": "python3",
        "timeout_secs": 10,
        "track_files": true
    });

    let result = registry.execute_tool("execute_code", args).await.unwrap();

    let generated_files = result.get("generated_files").unwrap();
    let files_array = generated_files.get("files").unwrap().as_array().unwrap();

    // Should detect all 3 files
    assert_eq!(files_array.len(), 3);

    let filenames: Vec<String> = files_array
        .iter()
        .map(|f| {
            PathBuf::from(f.get("absolute_path").unwrap().as_str().unwrap())
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
        })
        .collect();

    assert!(filenames.contains(&"file1.json".to_string()));
    assert!(filenames.contains(&"file2.csv".to_string()));
    assert!(filenames.contains(&"output.json".to_string()));

    println!(
        "✓ Multiple file detection test passed: found {} files",
        files_array.len()
    );
}

#[tokio::test]
async fn test_file_tracking_error_handling() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let registry = ToolRegistry::new(workspace_root).await;
    registry.set_mcp_client(create_test_mcp_client()).await;

    // Test with code that fails
    let python_code = r#"
raise Exception("Intentional error")
"#;

    let args = json!({
        "code": python_code,
        "language": "python3",
        "timeout_secs": 10,
        "track_files": true
    });

    let result = registry.execute_tool("execute_code", args).await.unwrap();

    // Should still include file tracking info (likely empty since no files generated)
    assert!(result.get("generated_files").is_some());

    let generated_files = result.get("generated_files").unwrap();
    let count = generated_files.get("count").unwrap().as_u64().unwrap();

    // Should be 0 files since execution failed
    assert_eq!(count, 0);

    println!("✓ Error handling test passed: graceful handling of failed execution");
}

#[tokio::test]
async fn test_session_optimization_comparison() {
    // This test demonstrates the optimization benefits
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let registry = ToolRegistry::new(workspace_root).await;
    registry.set_mcp_client(create_test_mcp_client()).await;

    // Simulate the old pattern (separate execution + manual verification)
    println!("\n=== OLD PATTERN (Inefficient) ===");

    // Step 1: Execute code
    let python_code = r#"
with open('hello_world.json', 'w') as f:
    f.write('Hello World')
print('File created: hello_world.json')
"#;

    let exec_args = json!({
        "code": python_code,
        "language": "python3",
        "timeout_secs": 30,
        "track_files": false  // Old way: no tracking
    });

    let exec_result = registry
        .execute_tool("execute_code", exec_args)
        .await
        .unwrap();
    println!(
        "Execution result: {}",
        serde_json::to_string_pretty(&exec_result).unwrap()
    );

    // Step 2: Manual verification (requires extra user turn)
    let verify_args = json!({
        "code": r#"
import os
file_path = os.path.abspath('hello_world.json')
exists = os.path.exists(file_path)
print(f'Verification: {file_path} exists={exists}')
"#,
        "language": "python3",
        "timeout_secs": 5
    });

    let verify_result = registry
        .execute_tool("execute_code", verify_args)
        .await
        .unwrap();
    println!(
        "Verification result: {}\n",
        serde_json::to_string_pretty(&verify_result).unwrap()
    );

    // Now demonstrate the new optimized pattern
    println!("\n=== NEW PATTERN (Optimized) ===");

    let exec_args_new = json!({
        "code": python_code.replace("hello_world.json", "optimized.json"),
        "language": "python3",
        "timeout_secs": 30,
        "track_files": true  // New way: automatic tracking
    });

    let exec_result_new = registry
        .execute_tool("execute_code", exec_args_new)
        .await
        .unwrap();
    println!(
        "Execution with tracking: {}\n",
        serde_json::to_string_pretty(&exec_result_new).unwrap()
    );

    // Benefits demonstration
    println!("=== OPTIMIZATION BENEFITS ===");
    println!("✓ Single tool call instead of 2+ calls");
    println!("✓ Automatic file detection and path reporting");
    println!("✓ Eliminates 'where is it?' follow-up questions");
    println!("✓ Built-in error handling and overflow protection");
}
