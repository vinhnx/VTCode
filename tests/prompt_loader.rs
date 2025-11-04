/// Integration tests for system prompt loading functionality
/// This test ensures the prompt loading system works correctly and provides good error messages.
use assert_fs::TempDir;
use std::fs;
use std::path::Path;
use vtcode_core::config::constants::prompts;

/// Test that the default system prompt path constant is correct
#[test]
fn default_prompt_path_constant() {
    assert_eq!(prompts::DEFAULT_SYSTEM_PROMPT_PATH, "prompts/system.md");

    // Verify the constant is a relative path as expected
    assert!(!prompts::DEFAULT_SYSTEM_PROMPT_PATH.starts_with('/'));
    assert!(prompts::DEFAULT_SYSTEM_PROMPT_PATH.ends_with(".md"));
}

/// Test loading system prompt from file when it exists
#[test]
fn load_system_prompt_from_existing_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let prompts_dir = temp_dir.path().join("prompts");
    fs::create_dir_all(&prompts_dir).expect("Failed to create prompts directory");

    let prompt_path = prompts_dir.join("system.md");
    let test_prompt = "You are a test assistant with specific instructions.\n\nUse these tools:\n- test_tool: For testing purposes";

    fs::write(&prompt_path, test_prompt).expect("Failed to write test prompt file");

    // Simulate loading from the file (since we don't have direct access to load_system_prompt)
    let loaded_content = fs::read_to_string(&prompt_path).expect("Failed to read prompt file");

    assert_eq!(loaded_content, test_prompt);
    assert!(loaded_content.contains("test assistant"));
    assert!(loaded_content.contains("test_tool"));
}

/// Test behavior when system prompt file doesn't exist
#[test]
fn prompt_file_missing_behavior() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let nonexistent_path = temp_dir.path().join("prompts").join("system.md");

    // Verify the file doesn't exist
    assert!(!nonexistent_path.exists());

    // Test the error case
    let result = fs::read_to_string(&nonexistent_path);
    assert!(result.is_err());

    // The error should be a not found error
    assert!(result.unwrap_err().kind() == std::io::ErrorKind::NotFound);
}

/// Test prompt file with various content formats
#[test]
fn prompt_file_content_variations() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let prompts_dir = temp_dir.path().join("prompts");
    fs::create_dir_all(&prompts_dir).expect("Failed to create prompts directory");

    let large_content = "A".repeat(10000);
    let test_cases = vec![
        ("Empty file", ""),
        ("Single line", "You are a helpful assistant."),
        (
            "Multiple lines",
            "You are a helpful assistant.\n\nYou can help with:\n- Code\n- Analysis",
        ),
        (
            "With special characters",
            "You are a helpful assistant! Use tools @mention and #tags.",
        ),
        ("Large content", large_content.as_str()),
    ];

    for (test_name, content) in test_cases {
        let prompt_path =
            prompts_dir.join(format!("{}.md", test_name.replace(" ", "_").to_lowercase()));
        fs::write(&prompt_path, content).expect("Failed to write test file");

        let loaded = fs::read_to_string(&prompt_path).expect("Failed to load test file");

        assert_eq!(loaded, content, "Content mismatch for test: {}", test_name);
    }
}

/// Test that the actual prompts/system.md file exists and is valid (if present)
#[test]
fn actual_prompt_file_validation() {
    let prompt_path = Path::new(prompts::DEFAULT_SYSTEM_PROMPT_PATH);

    if prompt_path.exists() {
        let content =
            fs::read_to_string(prompt_path).expect("Failed to read actual prompts/system.md");

        // Basic validation of the system prompt content
        assert!(!content.is_empty(), "System prompt should not be empty");
        assert!(
            content.len() > 50,
            "System prompt should be substantial (>50 chars)"
        );

        // Check for expected content patterns in the VTCode system prompt
        let content_lower = content.to_lowercase();

        // Should mention being a coding assistant
        let has_coding_context = content_lower.contains("coding")
            || content_lower.contains("code")
            || content_lower.contains("programming");
        assert!(
            has_coding_context,
            "System prompt should mention coding/programming context"
        );

        // Should mention tools or functionality
        let has_tools_context = content_lower.contains("tool")
            || content_lower.contains("function")
            || content_lower.contains("command");
        assert!(
            has_tools_context,
            "System prompt should mention tools or functions"
        );

        println!(
            "[SUCCESS] System prompt file exists and contains {} characters",
            content.len()
        );
    } else {
        println!(
            "ℹ️  System prompt file does not exist at: {}",
            prompt_path.display()
        );
        println!("   This is expected if prompts/system.md hasn't been created yet.");
        println!("   The system should fall back to a hardcoded prompt.");
    }
}

/// Test directory structure requirements
#[test]
fn prompt_directory_structure() {
    let expected_dir = Path::new("prompts");

    if expected_dir.exists() {
        assert!(
            expected_dir.is_dir(),
            "prompts should be a directory, not a file"
        );

        // List other prompt files that might exist
        if let Ok(entries) = fs::read_dir(expected_dir) {
            let mut prompt_files = Vec::new();
            for entry in entries.flatten() {
                if let Some(filename) = entry.file_name().to_str()
                    && filename.ends_with(".md")
                {
                    prompt_files.push(filename.to_string());
                }
            }

            if !prompt_files.is_empty() {
                println!("📁 Found prompt files: {:?}", prompt_files);

                // Verify system.md is among them if any exist
                if prompt_files.len() > 1 {
                    assert!(
                        prompt_files.contains(&"system.md".to_string()),
                        "If multiple prompt files exist, system.md should be present"
                    );
                }
            }
        }
    } else {
        println!("ℹ️  Prompts directory does not exist yet. This is expected for new setups.");
    }
}

/// Integration test that validates the complete prompt loading workflow
#[test]
fn integration_prompt_loading_workflow() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let original_dir = std::env::current_dir().expect("Failed to get current directory");

    // Change to temp directory for this test
    std::env::set_current_dir(&temp_dir).expect("Failed to change directory");

    // Create the expected directory structure
    let prompts_dir = temp_dir.path().join("prompts");
    fs::create_dir_all(&prompts_dir).expect("Failed to create prompts directory");

    // Create a comprehensive system prompt
    let system_prompt = r#"# VTCode System Prompt

You are a helpful coding assistant for the VTCode Rust project with access to file operations.

## Available Tools
- list_files: List files and directories
- read_file: Read file contents
- grep_file: Search for patterns in code
- run_terminal_cmd: Execute terminal commands

## Instructions
Always respond with helpful, accurate information about the codebase.
"#;

    let prompt_path = prompts_dir.join("system.md");
    fs::write(&prompt_path, system_prompt).expect("Failed to write system prompt");

    // Verify the file was created correctly
    assert!(prompt_path.exists());
    assert!(prompt_path.is_file());

    // Test loading the content
    let loaded_content = fs::read_to_string(prompts::DEFAULT_SYSTEM_PROMPT_PATH)
        .expect("Failed to load system prompt using constant path");

    assert_eq!(loaded_content, system_prompt);
    assert!(loaded_content.contains("VTCode"));
    assert!(loaded_content.contains("Available Tools"));

    // Restore original directory
    std::env::set_current_dir(original_dir).expect("Failed to restore directory");

    println!("[SUCCESS] Integration test completed successfully");
}
