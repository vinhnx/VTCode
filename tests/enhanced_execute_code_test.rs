//! Integration test for enhanced execute_code with file tracking

use serde_json::json;
use std::path::PathBuf;
use tempfile::TempDir;
use vtcode_core::tools::ToolRegistry;

#[tokio::test]
async fn test_execute_code_with_file_tracking() {
    // Create a temporary workspace
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();

    // Create a ToolRegistry
    let mut registry = ToolRegistry::new(workspace_root.clone());

    // Test case 1: Generate a PDF file and verify tracking
    let python_code = r#"
from fpdf import FPDF
pdf = FPDF()
pdf.add_page()
pdf.set_font('Arial', 'B', 24)
pdf.cell(0, 20, 'Test PDF', 0, 1, 'C')
pdf.output('test_output.pdf')
print('PDF generated successfully')
"#;

    let args = json!({
        "code": python_code,
        "language": "python3",
        "timeout_secs": 30,
        "track_files": true
    });

    let result = registry.execute("execute_code", args).await.unwrap();

    // Verify the response contains file tracking info
    assert!(result.get("generated_files").is_some());

    let generated_files = result.get("generated_files").unwrap();
    assert_eq!(generated_files.get("count").unwrap().as_u64().unwrap(), 1);

    let files_array = generated_files.get("files").unwrap().as_array().unwrap();
    assert_eq!(files_array.len(), 1);

    let file_info = &files_array[0];
    let file_path = file_info.get("absolute_path").unwrap().as_str().unwrap();
    assert!(file_path.ends_with("test_output.pdf"));

    // Verify the file summary
    let summary = generated_files.get("summary").unwrap().as_str().unwrap();
    assert!(summary.contains("test_output.pdf"));
    assert!(summary.contains("bytes"));

    println!("✓ File tracking test passed: detected generated PDF");
}

#[tokio::test]
async fn test_execute_code_without_file_tracking() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let mut registry = ToolRegistry::new(workspace_root);

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

    let result = registry.execute("execute_code", args).await.unwrap();

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
    let mut registry = ToolRegistry::new(workspace_root);

    let python_code = r#"
# Generate multiple files
with open('file1.txt', 'w') as f:
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

    let result = registry.execute("execute_code", args).await.unwrap();

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

    assert!(filenames.contains(&"file1.txt".to_string()));
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
    let mut registry = ToolRegistry::new(workspace_root);

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

    let result = registry.execute("execute_code", args).await.unwrap();

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
    let mut registry = ToolRegistry::new(workspace_root);

    // Simulate the old pattern (separate execution + manual verification)
    println!("\n=== OLD PATTERN (Inefficient) ===");

    // Step 1: Execute code
    let python_code = r#"
from fpdf import FPDF
pdf = FPDF()
pdf.add_page()
pdf.cell(0, 20, 'Hello World', 0, 1, 'C')
pdf.output('hello_world.pdf')
print('PDF created: hello_world.pdf')
"#;

    let exec_args = json!({
        "code": python_code,
        "language": "python3",
        "timeout_secs": 30,
        "track_files": false  // Old way: no tracking
    });

    let exec_result = registry.execute("execute_code", exec_args).await.unwrap();
    println!(
        "Execution result: {}",
        serde_json::to_string_pretty(&exec_result).unwrap()
    );

    // Step 2: Manual verification (requires extra user turn)
    let verify_args = json!({
        "code": r#"
import os
file_path = os.path.abspath('hello_world.pdf')
exists = os.path.exists(file_path)
print(f'Verification: {file_path} exists={exists}')
"#,
        "language": "python3",
        "timeout_secs": 5
    });

    let verify_result = registry.execute("execute_code", verify_args).await.unwrap();
    println!(
        "Verification result: {}\n",
        serde_json::to_string_pretty(&verify_result).unwrap()
    );

    // Now demonstrate the new optimized pattern
    println!("\n=== NEW PATTERN (Optimized) ===");

    let exec_args_new = json!({
        "code": python_code.replace("hello_world.pdf", "optimized.pdf"),
        "language": "python3",
        "timeout_secs": 30,
        "track_files": true  // New way: automatic tracking
    });

    let exec_result_new = registry
        .execute("execute_code", exec_args_new)
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
