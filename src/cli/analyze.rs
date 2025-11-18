use anyhow::Result;
use std::path::Path;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::utils::colors::style;
use walkdir::WalkDir;

/// Handle the analyze command
pub async fn handle_analyze_command(config: &CoreAgentConfig) -> Result<()> {
    println!("{}", style("[ANALYZE]").blue().bold());
    println!("  {:16} {}\n", "workspace", config.workspace.display());

    // Workspace analysis implementation
    analyze_workspace(&config.workspace).await?;

    Ok(())
}

/// Analyze the workspace and provide insights
async fn analyze_workspace(workspace_path: &Path) -> Result<()> {
    println!("{}", style("Structure").bold());

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

    println!("  {:16} {}", "directories", total_dirs);
    println!("  {:16} {}\n", "files", total_files);

    // Show language distribution
    if !language_files.is_empty() {
        println!("{}", style("Languages").bold());
        let mut langs: Vec<_> = language_files.iter().collect();
        langs.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
        for (i, (lang, count)) in langs.iter().take(10).enumerate() {
            println!("  {:>2}. {:<12} {} files", i + 1, lang, count);
        }
        println!();
    }

    Ok(())
}
