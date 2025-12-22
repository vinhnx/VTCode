//! Integration test for Phase 4: Split Tool Results
//!
//! Demonstrates dual-output execution with real tools showing token savings.

use serde_json::json;
use std::path::PathBuf;
use vtcode_core::tools::registry::ToolRegistry;

#[tokio::test]
async fn test_grep_dual_output_integration() {
    // Setup: Create a registry with workspace
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut registry = ToolRegistry::new(workspace).await;

    // Test: Execute grep_file with dual output
    let args = json!({
        "pattern": "pub fn",
        "path": "src/tools",
        "max_results": 10
    });

    let result = registry
        .execute_tool_dual("grep_file", args)
        .await
        .expect("grep_file execution should succeed");

    // Verify: Dual output structure
    assert!(
        !result.llm_content.is_empty(),
        "LLM content should not be empty"
    );
    assert!(
        !result.ui_content.is_empty(),
        "UI content should not be empty"
    );
    assert_eq!(result.tool_name, "grep_file");
    assert!(result.success, "Tool should succeed");

    // Verify: Token counting
    let counts = &result.metadata.token_counts;
    assert!(counts.llm_tokens > 0, "Should count LLM tokens");
    assert!(counts.ui_tokens > 0, "Should count UI tokens");

    // Verify: If UI has significant content, LLM should be summarized
    if counts.ui_tokens > 100 {
        assert!(
            counts.llm_tokens < counts.ui_tokens,
            "LLM content should be more concise than UI content for large outputs"
        );
        assert!(
            counts.savings_percent > 0.0,
            "Should show token savings for large outputs"
        );

        println!("✅ Token Savings Achieved:");
        println!("   UI tokens: {}", counts.ui_tokens);
        println!("   LLM tokens: {}", counts.llm_tokens);
        println!(
            "   Saved: {} tokens ({:.1}%)",
            counts.savings_tokens, counts.savings_percent
        );
    }

    // Verify: Summary structure for grep results
    // Should contain match count and file information
    assert!(
        result.llm_content.to_lowercase().contains("match")
            || result.llm_content.to_lowercase().contains("found"),
        "LLM summary should mention matches: {}",
        result.llm_content
    );
}

#[tokio::test]
async fn test_list_dual_output_integration() {
    // Setup: Create a registry with workspace
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut registry = ToolRegistry::new(workspace).await;

    // Test: Execute list_files with dual output
    let args = json!({
        "path": "src/tools",
        "max_depth": 2
    });

    let result = registry
        .execute_tool_dual("list_files", args)
        .await
        .expect("list_files execution should succeed");

    // Verify: Basic structure
    assert!(!result.llm_content.is_empty());
    assert!(!result.ui_content.is_empty());
    assert_eq!(result.tool_name, "list_files");

    // Verify: Token counting
    let counts = &result.metadata.token_counts;
    assert!(counts.llm_tokens > 0);
    assert!(counts.ui_tokens > 0);

    // Verify: Summary structure for list results
    // Should contain item count information
    assert!(
        result.llm_content.to_lowercase().contains("item")
            || result.llm_content.to_lowercase().contains("file")
            || result.llm_content.to_lowercase().contains("listed"),
        "LLM summary should mention items/files: {}",
        result.llm_content
    );

    println!("✅ List Dual Output:");
    println!("   LLM: {}", result.llm_content);
    println!("   Savings: {}", result.savings_summary());
}

#[tokio::test]
async fn test_read_file_dual_output() {
    // Setup: Create a registry
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut registry = ToolRegistry::new(workspace).await;

    // Test: Execute read_file with ReadSummarizer
    let args = json!({
        "file_path": "README.md"
    });

    let result = registry
        .execute_tool_dual("read_file", args)
        .await
        .expect("read_file execution should succeed");

    // Verify: Dual output with summarization
    assert!(!result.llm_content.is_empty());
    assert!(!result.ui_content.is_empty());
    assert_eq!(result.tool_name, "read_file");

    // Verify: LLM summary should mention line count
    assert!(
        result.llm_content.to_lowercase().contains("read")
            || result.llm_content.to_lowercase().contains("line"),
        "LLM summary should mention file stats: {}",
        result.llm_content
    );

    // Verify: Should have token savings on larger files
    let counts = &result.metadata.token_counts;
    if counts.ui_tokens > 200 {
        assert!(
            counts.savings_percent > 50.0,
            "Should have significant savings on large files: {:.1}%",
            counts.savings_percent
        );

        println!("✅ Read File Dual Output:");
        println!("   UI tokens: {}", counts.ui_tokens);
        println!("   LLM tokens: {}", counts.llm_tokens);
        println!("   Savings: {:.1}%", counts.savings_percent);
    } else {
        println!("✅ Read File Dual Output (small file):");
        println!("   File too small for significant savings");
    }
}

#[tokio::test]
async fn test_bash_dual_output() {
    // Setup: Create a registry
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut registry = ToolRegistry::new(workspace).await;

    // Test: Execute run_pty_cmd with BashSummarizer
    let args = json!({
        "command": "ls -la src/tools",
        "timeout_ms": 5000
    });

    let result = registry
        .execute_tool_dual("run_pty_cmd", args)
        .await
        .expect("run_pty_cmd execution should succeed");

    // Verify: Dual output with summarization
    assert!(!result.llm_content.is_empty());
    assert!(!result.ui_content.is_empty());
    assert_eq!(result.tool_name, "run_pty_cmd");

    // Verify: LLM summary should mention command execution details
    let llm_lower = result.llm_content.to_lowercase();
    assert!(
        llm_lower.contains("command") || llm_lower.contains("exit") || llm_lower.contains("output"),
        "LLM summary should mention execution details: {}",
        result.llm_content
    );

    // Verify: Should have token savings on larger command outputs
    let counts = &result.metadata.token_counts;
    if counts.ui_tokens > 200 {
        assert!(
            counts.savings_percent > 50.0,
            "Should have significant savings on large command output: {:.1}%",
            counts.savings_percent
        );

        println!("✅ Bash Dual Output:");
        println!("   UI tokens: {}", counts.ui_tokens);
        println!("   LLM tokens: {}", counts.llm_tokens);
        println!("   Savings: {:.1}%", counts.savings_percent);
    } else {
        println!("✅ Bash Dual Output (small output):");
        println!("   Command output too small for significant savings");
    }
}

#[tokio::test]
async fn test_edit_dual_output() {
    // Setup: Create a registry
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let test_file = workspace.join("test_temp_write.txt");

    let mut registry = ToolRegistry::new(workspace).await;

    // Test with write_file which creates a temp file
    use std::fs;
    let args = json!({
        "file_path": test_file.to_str().unwrap(),
        "content": "Test content for write operation\nLine 2\nLine 3\n"
    });

    let result = registry
        .execute_tool_dual("write_file", args)
        .await
        .expect("write_file execution should succeed");

    // Cleanup
    let _ = fs::remove_file(&test_file);

    // Verify: Dual output structure
    assert!(!result.llm_content.is_empty());
    assert!(!result.ui_content.is_empty());
    assert_eq!(result.tool_name, "write_file");

    // Verify: LLM summary should mention file operation
    let llm_lower = result.llm_content.to_lowercase();
    assert!(
        llm_lower.contains("modified")
            || llm_lower.contains("file")
            || llm_lower.contains("success")
            || llm_lower.contains("wrote"),
        "LLM summary should mention file operation: {}",
        result.llm_content
    );

    println!("✅ Edit/Write Dual Output:");
    println!("   Tool: {}", result.tool_name);
    println!("   LLM summary: {}", result.llm_content);
    println!("   Savings: {}", result.savings_summary());
}

#[tokio::test]
async fn test_backward_compatibility() {
    // Verify that old execute_tool() still works alongside execute_tool_dual()
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut registry = ToolRegistry::new(workspace).await;

    let args = json!({
        "pattern": "pub struct",
        "path": "src/tools",
        "max_results": 5
    });

    // Old API should still work
    let old_result = registry
        .execute_tool("grep_file", args.clone())
        .await
        .expect("Old execute_tool should still work");

    assert!(old_result.is_object() || old_result.is_string());

    // New API should also work
    let new_result = registry
        .execute_tool_dual("grep_file", args)
        .await
        .expect("New execute_tool_dual should work");

    assert!(!new_result.llm_content.is_empty());
    assert!(!new_result.ui_content.is_empty());

    println!("✅ Backward Compatibility:");
    println!("   Old API: Working");
    println!(
        "   New API: Working with {} savings",
        new_result.savings_summary()
    );
}
