use anyhow::Result;
use std::path::Path;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::utils::colors::style;
use walkdir::WalkDir;

/// Handle the analyze command
pub async fn handle_analyze_command(config: &CoreAgentConfig) -> Result<()> {
    println!("{}", style("Analyze workspace mode selected").blue().bold());
    println!("Workspace: {}", config.workspace.display());

    // Workspace analysis implementation
    analyze_workspace(&config.workspace).await?;

    Ok(())
}

/// Analyze the workspace and provide insights
async fn analyze_workspace(workspace_path: &Path) -> Result<()> {
    println!("Analyzing workspace structure...");

    // Count files and directories
    let mut total_files = 0;
    let mut total_dirs = 0;
    let mut language_files = std::collections::HashMap::new();

    for entry in WalkDir::new(workspace_path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_dir() {
            total_dirs += 1;
        } else if entry.file_type().is_file() {
            total_files += 1;

            // Count files by extension
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                *language_files.entry(ext.to_string()).or_insert(0) += 1;
            }
        }
    }

    println!("  Total directories: {}", total_dirs);
    println!("  Total files: {}", total_files);

    // Show language distribution
    println!("  Language distribution:");
    for (lang, count) in language_files.iter().take(10) {
        println!("    {}: {} files", lang, count);
    }

    // Placeholder for deeper analysis (tree-sitter integration lives in core)

    println!("Workspace analysis complete!");

    Ok(())
}
