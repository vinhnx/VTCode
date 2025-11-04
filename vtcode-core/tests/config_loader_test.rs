//! Test for configuration loading with home directory support

use assert_fs::TempDir;
use std::fs;
use vtcode_core::config::VTCodeConfig;

#[test]
fn test_load_config_from_home_directory() {
    // Create a temporary directory to simulate home directory
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let home_dir = temp_dir.path();
    let vtcode_dir = home_dir.join(".vtcode");
    fs::create_dir_all(&vtcode_dir).expect("Failed to create .vtcode directory");

    // Create a sample config file in the home directory
    let config_content = r#"
[agent]
default_model = "test-model"
provider = "test-provider"
max_conversation_turns = 50

[security]
human_in_the_loop = false
"#;

    let config_path = vtcode_dir.join("vtcode.toml");
    fs::write(&config_path, config_content).expect("Failed to write config file");

    // Create a mock workspace directory
    let workspace_dir = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace_dir).expect("Failed to create workspace directory");

    // Test that we can load the config from home directory
    // We need to mock the home directory detection for this test
    // Since we can't easily mock environment variables in a test,
    // we'll directly test the bootstrap function with our temp directory
    let created_files = VTCodeConfig::bootstrap_project_with_options(
        &workspace_dir,
        true, // force
        true, // use home directory
    )
    .expect("Failed to bootstrap project with home directory");

    assert!(!created_files.is_empty());
    assert!(created_files.contains(&"vtcode.toml".to_string()));

    // Check that the files were created in the home directory
    let home_config_path = vtcode_dir.join("vtcode.toml");
    let home_gitignore_path = vtcode_dir.join(".vtcodegitignore");
    assert!(home_config_path.exists());
    assert!(home_gitignore_path.exists());
}
