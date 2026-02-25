//! Analyze command implementation - workspace analysis

use crate::config::constants::tools;
use crate::config::types::{AgentConfig, AnalysisDepth, OutputFormat};
use crate::tools::ToolRegistry;
use crate::utils::colors::style;
use anyhow::Result;
use serde_json::json;

/// Handle the analyze command - comprehensive workspace analysis
pub async fn handle_analyze_command(
    config: AgentConfig,
    depth: String,
    format: String,
) -> Result<()> {
    println!("{}", style("Analyzing workspace...").cyan().bold());

    let depth = match depth.to_lowercase().as_str() {
        "basic" => AnalysisDepth::Basic,
        "standard" => AnalysisDepth::Standard,
        "deep" => AnalysisDepth::Deep,
        _ => {
            println!("{}", style("Invalid depth. Using 'standard'.").red());
            AnalysisDepth::Standard
        }
    };

    let _output_format = match format.to_lowercase().as_str() {
        "text" => OutputFormat::Text,
        "json" => OutputFormat::Json,
        "html" => OutputFormat::Html,
        _ => {
            println!("{}", style("Invalid format. Using 'text'.").red());
            OutputFormat::Text
        }
    };

    let registry = ToolRegistry::new(config.workspace.clone()).await;

    // Step 1: Get high-level directory structure
    println!("{}", style("1. Getting workspace structure...").dim());
    let root_files = registry
        .execute_tool(tools::LIST_FILES, json!({"path": ".", "max_items": 50}))
        .await;

    match root_files {
        Ok(result) => {
            println!("{}", style("Root directory structure obtained").green());
            if let Some(files_array) = result.get("files") {
                println!(
                    "   Found {} files/directories in root",
                    files_array.as_array().unwrap_or(&vec![]).len()
                );
            }
        }
        Err(e) => println!("{} {}", style("Failed to list root directory:").red(), e),
    }

    // Step 2: Look for important project files
    println!("{}", style("2. Identifying project type...").dim());
    let important_files = vec![
        "README.md",
        "Cargo.toml",
        "package.json",
        "go.mod",
        "requirements.txt",
        "Makefile",
    ];

    for file in important_files {
        let check_file = registry
            .execute_tool(
                tools::LIST_FILES,
                json!({"path": ".", "include_hidden": false}),
            )
            .await;
        if let Ok(result) = check_file
            && let Some(files) = result.get("files")
            && let Some(files_array) = files.as_array()
        {
            for file_obj in files_array {
                if let Some(path) = file_obj.get("path")
                    && path.as_str().unwrap_or("") == file
                {
                    println!("   {} Detected: {}", style("Detected").green(), file);
                    break;
                }
            }
        }
    }

    // Step 3: Read key configuration files
    println!("{}", style("3. Reading project configuration...").dim());
    let config_files = vec!["AGENTS.md", "README.md", "Cargo.toml", "package.json"];

    for config_file in config_files {
        let read_result = registry
            .execute_tool(
                tools::READ_FILE,
                json!({"path": config_file, "max_bytes": 2000}),
            )
            .await;
        if let Ok(result) = read_result {
            println!(
                "   {} Read {} ({} bytes)",
                style("Read").green(),
                config_file,
                result
                    .get("metadata")
                    .and_then(|m| m.get("size"))
                    .unwrap_or(&serde_json::json!(null))
            );
        }
    }

    // Step 4: Analyze source code structure
    println!("{}", style("4. Analyzing source code structure...").dim());

    // Check for common source directories
    let src_dirs = vec!["src", "lib", "pkg", "internal", "cmd"];
    for dir in src_dirs {
        let check_dir = registry
            .execute_tool(
                tools::LIST_FILES,
                json!({"path": ".", "include_hidden": false}),
            )
            .await;
        if let Ok(result) = check_dir
            && let Some(files) = result.get("files")
            && let Some(files_array) = files.as_array()
        {
            for file_obj in files_array {
                if let Some(path) = file_obj.get("path")
                    && path.as_str().unwrap_or("") == dir
                {
                    println!(
                        "   {} Found source directory: {}",
                        style("Found").green(),
                        dir
                    );
                    break;
                }
            }
        }
    }

    if matches!(depth, AnalysisDepth::Deep) {
        println!(
            "{}",
            style("Deep analysis: use grep/search tools for detailed code inspection.").dim()
        );
    }

    println!("{}", style("Workspace analysis complete!").green().bold());
    println!(
        "{}",
        style("You can now ask me specific questions about the codebase.").dim()
    );

    Ok(())
}
