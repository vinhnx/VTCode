use anyhow::Result;
use std::fs;
use std::io::Write;
use std::path::Path;
use vtcode_core::config::{ConfigManager, VTCodeConfig};
use vtcode_core::utils::colors::style;

/// Handle the config command
pub async fn handle_config_command(output: Option<&Path>, use_home_dir: bool) -> Result<()> {
    println!("{}", style("Generate configuration").blue().bold());

    if use_home_dir {
        // Create config in user's home directory
        let created_files = VTCodeConfig::bootstrap_project_with_options(
            std::env::current_dir()?,
            true, // force overwrite
            true, // use home directory
        )?;

        if !created_files.is_empty() {
            println!("Configuration files created in user home directory:");
            for file in created_files {
                println!("  - {}", file);
            }
        } else {
            println!("Configuration files already exist in user home directory");
        }
    } else if let Some(output_path) = output {
        println!("Output path: {}", output_path.display());

        // Write to specified file
        let mut file = fs::File::create(output_path)?;
        file.write_all(generate_default_config().as_bytes())?;
        println!("Configuration written to {}", output_path.display());
    } else {
        // Print to stdout
        println!("\nGenerated configuration:\n");
        println!("{}", generate_default_config());
    }

    Ok(())
}

/// Generate default configuration content
/// This function creates a complete configuration by:
/// 1. Loading existing vtcode.toml if it exists (preserving user customizations)
/// 2. Using default values if no config exists
/// 3. Generating a complete TOML structure with all sections
fn generate_default_config() -> String {
    // Try to load existing configuration to preserve user settings
    let config = if Path::new("vtcode.toml").exists() {
        // Load existing config to preserve user customizations
        match ConfigManager::load_from_file("vtcode.toml") {
            Ok(config_manager) => config_manager.config().clone(),
            Err(_) => VTCodeConfig::default(), // Fall back to defaults if loading fails
        }
    } else {
        // Use system defaults if no config file exists
        VTCodeConfig::default()
    };

    // Generate TOML content from the loaded/created config
    toml::to_string_pretty(&config).unwrap_or_else(|_| {
        // Fallback to hardcoded template if serialization fails
        r#"# VTCode Configuration File
# This file contains the configuration for VTCode

[agent]
# Default model to use
default_model = "qwen/qwen3-4b-2507"
# AI provider (gemini, openai, anthropic)
provider = "gemini"
# Maximum conversation turns
max_conversation_turns = 150
# Reasoning effort level for models that support it (low, medium, high)
reasoning_effort = "medium"

[security]
# Enable human-in-the-loop mode
human_in_the_loop = true

[tools]
# Default tool execution policy
default_policy = "prompt"

[commands]
# Allowed shell commands (whitelist)
allow_list = ["ls", "pwd", "cat", "grep", "git status", "git diff"]

[pty]
# Enable PTY support
enabled = true
# Default terminal dimensions
default_rows = 24
default_cols = 80
# Maximum concurrent PTY sessions
max_sessions = 10
# Command execution timeout in seconds
command_timeout_seconds = 300
"#
        .to_string()
    })
}
