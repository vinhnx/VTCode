#![allow(missing_docs)]
//! Integration test for Phase 4: Split Tool Results
//!
//! Demonstrates dual-output execution with real tools showing token savings.

use serde_json::json;
use std::path::PathBuf;
use vtcode_config::constants::tools;
use vtcode_core::tools::registry::ToolRegistry;

#[tokio::test]
async fn test_code_search_dual_output_integration() {
    // Setup: Create a registry with workspace
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let registry = ToolRegistry::new(workspace).await;

    // Test: Execute code_search with dual output
    let args = json!({
        "action": "structural",
        "pattern": "pub fn $NAME($$$ARGS) $$$BODY",
        "path": "src/tools",
        "lang": "rust",
        "max_results": 10
    });

    let result = registry
        .execute_tool_dual(tools::CODE_SEARCH, args)
        .await
        .expect("code_search execution should succeed");

    // Verify: Dual output structure
    assert!(
        !result.llm_content.is_empty(),
        "LLM content should not be empty"
    );
    assert!(
        !result.ui_content.is_empty(),
        "UI content should not be empty"
    );
    // Tool name may vary based on implementation
    assert!(!result.tool_name.is_empty());
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

        println!("v Token Savings Achieved:");
        println!("   UI tokens: {}", counts.ui_tokens);
        println!("   LLM tokens: {}", counts.llm_tokens);
        println!(
            "   Saved: {} tokens ({:.1}%)",
            counts.savings_tokens, counts.savings_percent
        );
    }

    // Verify: Summary structure for search results
    // Should contain match or search information
    assert!(
        result.llm_content.to_lowercase().contains("match")
            || result.llm_content.to_lowercase().contains("found")
            || result.llm_content.to_lowercase().contains("search"),
        "LLM summary should mention matches: {}",
        result.llm_content
    );
}

#[tokio::test]
async fn test_shell_listing_dual_output_integration() {
    // Setup: Create a registry with workspace
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let registry = ToolRegistry::new(workspace).await;

    // Test: Execute a shell listing with dual output
    let args = json!({
        "cmd": "find src/tools -maxdepth 2 -type f | sort | head -n 20",
        "yield_time_ms": 1000
    });

    let result = registry
        .execute_tool_dual(tools::EXEC_COMMAND, args)
        .await
        .expect("exec_command listing should succeed");

    // Verify: Basic structure
    assert!(!result.llm_content.is_empty());
    assert!(!result.ui_content.is_empty());
    // Tool name may vary based on implementation
    assert!(!result.tool_name.is_empty());

    // Verify: Token counting
    let counts = &result.metadata.token_counts;
    assert!(counts.llm_tokens > 0);
    assert!(counts.ui_tokens > 0);

    // Verify: Summary structure for list results
    // Should contain item count information
    assert!(
        result.llm_content.to_lowercase().contains("item")
            || result.llm_content.to_lowercase().contains("file")
            || result.llm_content.to_lowercase().contains("listed")
            || result.llm_content.to_lowercase().contains("found")
            || result.llm_content.to_lowercase().contains("lines"),
        "LLM summary should mention items/files: {}",
        result.llm_content
    );

    println!("v List Dual Output:");
    println!("   LLM: {}", result.llm_content);
    println!("   Savings: {}", result.savings_summary());
}

#[tokio::test]
async fn test_file_inspection_dual_output() {
    // Setup: Create a registry
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let registry = ToolRegistry::new(workspace).await;

    // Test: Inspect a file through exec_command.
    let args = json!({
        "cmd": "sed -n '1,120p' README.md",
        "yield_time_ms": 1000
    });

    let result = registry
        .execute_tool_dual(tools::EXEC_COMMAND, args)
        .await
        .expect("exec_command file inspection should succeed");

    // Verify: Dual output with summarization
    assert!(!result.llm_content.is_empty());
    assert!(!result.ui_content.is_empty());
    // Tool name may vary based on implementation
    assert!(!result.tool_name.is_empty());

    // Verify: LLM summary should mention line count
    assert!(
        result.llm_content.to_lowercase().contains("read")
            || result.llm_content.to_lowercase().contains("line")
            || result.llm_content.to_lowercase().contains("file"),
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

        println!("v Read File Dual Output:");
        println!("   UI tokens: {}", counts.ui_tokens);
        println!("   LLM tokens: {}", counts.llm_tokens);
        println!("   Savings: {:.1}%", counts.savings_percent);
    } else {
        println!("v Read File Dual Output (small file):");
        println!("   File too small for significant savings");
    }
}

#[tokio::test]
async fn test_exec_command_dual_output() {
    // Setup: Create a registry
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let registry = ToolRegistry::new(workspace).await;

    // Test: Execute exec_command with BashSummarizer
    let args = json!({
        "cmd": "ls -la src/tools",
        "yield_time_ms": 1000
    });

    let result = registry
        .execute_tool_dual(tools::EXEC_COMMAND, args)
        .await
        .expect("exec_command execution should succeed");

    // Verify: Dual output with summarization
    assert!(!result.llm_content.is_empty());
    assert!(!result.ui_content.is_empty());
    // Tool name may vary based on implementation
    assert!(!result.tool_name.is_empty());

    // Verify: LLM summary should mention command execution details
    let llm_lower = result.llm_content.to_lowercase();
    assert!(
        llm_lower.contains("command")
            || llm_lower.contains("exit")
            || llm_lower.contains("output")
            || llm_lower.contains("success"),
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

        println!("v Bash Dual Output:");
        println!("   UI tokens: {}", counts.ui_tokens);
        println!("   LLM tokens: {}", counts.llm_tokens);
        println!("   Savings: {:.1}%", counts.savings_percent);
    } else {
        println!("v Bash Dual Output (small output):");
        println!("   Command output too small for significant savings");
    }
}

#[tokio::test]
async fn test_apply_patch_dual_output() {
    // Setup: Create a registry
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let test_file = workspace.join("test_temp_write.txt");

    let registry = ToolRegistry::new(workspace).await;

    // Test with apply_patch which creates a temp file
    use std::fs;
    let patch = "*** Begin Patch\n*** Add File: test_temp_write.txt\n+Test content for write operation\n+Line 2\n+Line 3\n*** End Patch\n";
    let args = json!({
        "patch": patch
    });

    let result = registry
        .execute_tool_dual(tools::APPLY_PATCH, args)
        .await
        .expect("apply_patch execution should succeed");

    // Cleanup
    let _ = fs::remove_file(&test_file);

    // Verify: Dual output structure
    assert!(!result.llm_content.is_empty());
    assert!(!result.ui_content.is_empty());
    // Tool name may vary based on implementation
    assert!(!result.tool_name.is_empty());

    // Verify: LLM summary should mention file operation
    let llm_lower = result.llm_content.to_lowercase();
    assert!(
        llm_lower.contains("modified")
            || llm_lower.contains("file")
            || llm_lower.contains("success")
            || llm_lower.contains("wrote")
            || llm_lower.contains("write"),
        "LLM summary should mention file operation: {}",
        result.llm_content
    );

    println!("v Edit/Write Dual Output:");
    println!("   Tool: {}", result.tool_name);
    println!("   LLM summary: {}", result.llm_content);
    println!("   Savings: {}", result.savings_summary());
}

#[tokio::test]
async fn test_backward_compatibility() {
    // Verify that old execute_tool() still works alongside execute_tool_dual()
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let registry = ToolRegistry::new(workspace).await;

    let args = json!({
        "pattern": "pub struct",
        "path": "src/tools",
        "max_results": 5
    });

    // Old API should still work
    let old_result = registry
        .execute_tool(tools::GREP_FILE, args.clone())
        .await
        .expect("Old execute_tool should still work");

    assert!(old_result.is_object() || old_result.is_string());

    // New API should also work
    let new_result = registry
        .execute_tool_dual(tools::GREP_FILE, args)
        .await
        .expect("New execute_tool_dual should work");

    assert!(!new_result.llm_content.is_empty());
    assert!(!new_result.ui_content.is_empty());

    println!("v Backward Compatibility:");
    println!("   Old API: Working");
    println!(
        "   New API: Working with {} savings",
        new_result.savings_summary()
    );
}
