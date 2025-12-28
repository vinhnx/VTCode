use vtcode_core::prompts::CustomSlashCommandRegistry;
use tempfile::tempdir;
use std::fs;

#[tokio::test]
async fn test_custom_slash_command_loading() {
    let temp = tempdir().unwrap();
    let commands_dir = temp.path().join("commands");
    fs::create_dir_all(&commands_dir).await.unwrap();
    
    // Create a test command file
    fs::write(
        commands_dir.join("review.md"),
        r#"---
description: Review code
argument-hint: [file]
---
Please review the file: $1
"#,
    ).await.unwrap();

    let mut cfg = vtcode_core::prompts::CustomSlashCommandConfig::default();
    cfg.directory = commands_dir.to_string_lossy().into_owned();
    let registry = CustomSlashCommandRegistry::load(Some(&cfg), temp.path())
        .await
        .expect("load registry");
    
    assert!(registry.enabled());
    assert!(!registry.is_empty());
    
    let command = registry.get("review").unwrap();
    assert_eq!(command.name, "review");
    assert_eq!(command.description.as_deref(), Some("Review code"));
    assert_eq!(command.argument_hint.as_deref(), Some("[file]"));
    
    // Test command expansion
    let expanded = command.expand_content("main.rs");
    assert!(expanded.contains("main.rs"));
}

#[tokio::test]
async fn test_custom_slash_command_with_bash_execution() {
    let temp = tempdir().unwrap();
    let commands_dir = temp.path().join("commands");
    fs::create_dir_all(&commands_dir).await.unwrap();
    
    // Create a test command file with bash execution
    fs::write(
        commands_dir.join("status.md"),
        r#"---
description: Show git status
---
Current status: !`echo "clean"`
"#,
    ).await.unwrap();

    let mut cfg = vtcode_core::prompts::CustomSlashCommandConfig::default();
    cfg.directory = commands_dir.to_string_lossy().into_owned();
    let registry = CustomSlashCommandRegistry::load(Some(&cfg), temp.path())
        .await
        .expect("load registry");
    
    let command = registry.get("status").unwrap();
    assert!(command.has_bash_execution);
    
    // Test that bash execution is detected
    let expanded = command.expand_content("");
    assert!(expanded.contains("clean"));
}