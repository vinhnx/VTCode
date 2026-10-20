//! Analyze command implementation - workspace analysis

use crate::config::constants::tools;
use crate::config::types::{AgentConfig, AnalysisDepth, OutputFormat};
use crate::tools::ToolRegistry;
use crate::utils::colors::style;
use anyhow::Result;
use serde_json::json;

/// Handle the analyze command - comprehensive workspace analysis
pub async fn handle_analyze_command(config: AgentConfig, depth: String, format: String) -> Result<()> {
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
    registry.mark_tool_preapproved(tools::LIST_FILES).await;

    // Step 1: Get high-level directory structure
    println!("{}", style("1. Getting workspace structure...").dim());
    let root_files = registry
        .execute_tool(
            tools::LIST_FILES,
            json!({"mode": "list", "path": ".", "per_page": 50, "max_items": 200, "include_hidden": false}),
        )
        .await;

    let entry_names = match root_files {
        Ok(result) => {
            println!("{}", style("Root directory structure obtained").green());
            let names = extract_entry_names(&result);
            println!("   Found {} files/directories in root", names.len());
            names
        }
        Err(e) => {
            vtcode_commons::diagnostics::stderr(format_args!("Failed to list root directory: {e}"));
            Vec::new()
        }
    };

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
        if entry_names.iter().any(|name| name == file) {
            println!("   {} {}", style("Detected").green(), file);
        }
    }

    // Step 3: Read key configuration files
    println!("{}", style("3. Reading project configuration...").dim());
    let config_files = vec!["AGENTS.md", "README.md", "Cargo.toml", "package.json"];

    for config_file in config_files {
        let config_path = config.workspace.join(config_file);
        let Ok(metadata) = tokio::fs::metadata(&config_path).await else {
            continue;
        };
        println!("   {} {} ({} bytes)", style("Read").green(), config_file, metadata.len());
    }

    // Step 4: Analyze source code structure
    println!("{}", style("4. Analyzing source code structure...").dim());

    // Check for common source directories
    let src_dirs = vec!["src", "lib", "pkg", "internal", "cmd"];
    for dir in src_dirs {
        if entry_names.iter().any(|name| name == dir) {
            println!("   {} source directory: {}", style("Found").green(), dir);
        }
    }

    if matches!(depth, AnalysisDepth::Deep) {
        println!("{}", style("Deep analysis: use grep/search tools for detailed code inspection.").dim());
    }

    println!("{}", style("Workspace analysis complete!").green().bold());
    println!("{}", style("You can now ask me specific questions about the codebase.").dim());

    Ok(())
}

/// Extract entry names from a `list_files` tool result payload.
///
/// The list tool emits its entries under the `items` field; each entry carries
/// a `name` (and a `path`). Returns an empty vector for missing or malformed
/// payloads.
fn extract_entry_names(result: &serde_json::Value) -> Vec<String> {
    result
        .get("items")
        .and_then(|items| items.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("name").and_then(|name| name.as_str()))
                .map(|name| name.to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::extract_entry_names;
    use serde_json::json;

    #[test]
    fn extracts_names_from_items_payload() {
        let result = json!({
            "success": true,
            "items": [
                {"name": "src", "path": "src", "type": "directory"},
                {"name": "README.md", "path": "README.md", "type": "file"}
            ]
        });
        assert_eq!(extract_entry_names(&result), vec!["src", "README.md"]);
    }

    #[test]
    fn returns_empty_for_empty_items() {
        let result = json!({"success": true, "items": []});
        assert!(extract_entry_names(&result).is_empty());
    }

    #[test]
    fn returns_empty_when_items_field_missing() {
        let result = json!({"success": true, "count": 0});
        assert!(extract_entry_names(&result).is_empty());
    }
}
