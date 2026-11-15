//! Analyze command implementation - workspace analysis

use crate::config::constants::tools;
use crate::config::types::{AgentConfig, AnalysisDepth, OutputFormat};
use crate::tools::ToolRegistry;
use crate::utils::colors::style;
use anyhow::Result;
use serde_json::json;

/// Handle the analyze command - comprehensive workspace analysis
pub async fn handle_analyze_command(config: AgentConfig, depth: String, format: String) -> Result<()> {
    let (depth, depth_ok) = parse_analysis_depth(&depth);
    let (output_format, format_ok) = parse_output_format(&format);
    let json_mode = matches!(output_format, OutputFormat::Json);

    if !json_mode {
        println!("{}", style("Analyzing workspace...").cyan().bold());
    }
    if !depth_ok {
        notice(json_mode, "Invalid depth. Using 'standard'.");
    }
    if !format_ok {
        notice(json_mode, "Invalid format. Using 'text'.");
    }
    if matches!(output_format, OutputFormat::Html) {
        notice(json_mode, "HTML output is not supported for analyze. Using 'text'.");
    }

    let registry = ToolRegistry::new(config.workspace.clone()).await;
    registry.mark_tool_preapproved(tools::LIST_FILES).await;

    // Step 1: Get high-level directory structure
    let root_files = registry
        .execute_tool(
            tools::LIST_FILES,
            json!({"mode": "list", "path": ".", "per_page": 50, "max_items": 200, "include_hidden": false}),
        )
        .await;

    let (entry_names, listed_root) = match root_files {
        Ok(result) => (extract_entry_names(&result), true),
        Err(e) => {
            vtcode_commons::diagnostics::stderr(format_args!("Failed to list root directory: {e}"));
            (Vec::new(), false)
        }
    };

    // Step 2: Look for important project files
    let important_files = [
        "README.md",
        "Cargo.toml",
        "package.json",
        "go.mod",
        "requirements.txt",
        "Makefile",
    ];
    let detected_project_files: Vec<String> = important_files
        .into_iter()
        .filter(|file| entry_names.iter().any(|name| name == file))
        .map(str::to_owned)
        .collect();

    // Step 3: Check key configuration files for presence and size
    let mut checked_config_files = Vec::new();
    for config_file in ["AGENTS.md", "README.md", "Cargo.toml", "package.json"] {
        let config_path = config.workspace.join(config_file);
        let Ok(metadata) = tokio::fs::metadata(&config_path).await else {
            continue;
        };
        checked_config_files.push((config_file.to_owned(), metadata.len()));
    }

    // Step 4: Analyze source code structure
    let src_dirs: Vec<String> = ["src", "lib", "pkg", "internal", "cmd"]
        .into_iter()
        .filter(|dir| entry_names.iter().any(|name| name == dir))
        .map(str::to_owned)
        .collect();

    let report = AnalysisReport {
        depth,
        listed_root,
        root_entries: entry_names.len(),
        detected_project_files,
        config_files: checked_config_files,
        source_dirs: src_dirs,
    };

    if json_mode {
        println!("{}", render_json_report(&report));
    } else {
        print!("{}", render_text_report(&report));
    }

    Ok(())
}

/// Print a notice without corrupting machine-readable output: stdout in text
/// mode, stderr in JSON mode.
fn notice(json_mode: bool, text: &str) {
    if json_mode {
        vtcode_commons::diagnostics::stderr(format_args!("{text}"));
    } else {
        println!("{}", style(text).red());
    }
}

fn parse_analysis_depth(raw: &str) -> (AnalysisDepth, bool) {
    match raw.to_lowercase().as_str() {
        "basic" => (AnalysisDepth::Basic, true),
        "standard" => (AnalysisDepth::Standard, true),
        "deep" => (AnalysisDepth::Deep, true),
        _ => (AnalysisDepth::Standard, false),
    }
}

fn parse_output_format(raw: &str) -> (OutputFormat, bool) {
    match raw.to_lowercase().as_str() {
        "text" => (OutputFormat::Text, true),
        "json" => (OutputFormat::Json, true),
        "html" => (OutputFormat::Html, true),
        _ => (OutputFormat::Text, false),
    }
}

fn analysis_depth_label(depth: &AnalysisDepth) -> &'static str {
    match depth {
        AnalysisDepth::Basic => "basic",
        AnalysisDepth::Standard => "standard",
        AnalysisDepth::Deep => "deep",
    }
}

struct AnalysisReport {
    depth: AnalysisDepth,
    listed_root: bool,
    root_entries: usize,
    detected_project_files: Vec<String>,
    /// (file name, file size in bytes) for present config files.
    config_files: Vec<(String, u64)>,
    source_dirs: Vec<String>,
}

fn render_text_report(report: &AnalysisReport) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(out, "{}", style("1. Getting workspace structure...").dim());
    if report.listed_root {
        let _ = writeln!(out, "{}", style("Root directory structure obtained").green());
        let _ = writeln!(out, "   Found {} files/directories in root", report.root_entries);
    }
    let _ = writeln!(out, "{}", style("2. Identifying project type...").dim());
    for file in &report.detected_project_files {
        let _ = writeln!(out, "   {} {}", style("Detected").green(), file);
    }
    let _ = writeln!(out, "{}", style("3. Checking project configuration...").dim());
    for (config_file, bytes) in &report.config_files {
        let _ = writeln!(out, "   {} {} ({} bytes)", style("Found").green(), config_file, bytes);
    }
    let _ = writeln!(out, "{}", style("4. Analyzing source code structure...").dim());
    for dir in &report.source_dirs {
        let _ = writeln!(out, "   {} source directory: {}", style("Found").green(), dir);
    }
    if matches!(report.depth, AnalysisDepth::Deep) {
        let _ = writeln!(out, "{}", style("Deep analysis: use grep/search tools for detailed code inspection.").dim());
    }
    let _ = writeln!(out, "{}", style("Workspace analysis complete!").green().bold());
    let _ = writeln!(out, "{}", style("You can now ask me specific questions about the codebase.").dim());
    out
}

fn render_json_report(report: &AnalysisReport) -> String {
    let value = json!({
        "depth": analysis_depth_label(&report.depth),
        "listed_root": report.listed_root,
        "root_entries": report.root_entries,
        "detected_project_files": report.detected_project_files,
        "config_files": report
            .config_files
            .iter()
            .map(|(name, bytes)| json!({"name": name, "bytes": bytes}))
            .collect::<Vec<_>>(),
        "source_dirs": report.source_dirs,
    });
    serde_json::to_string_pretty(&value)
        .unwrap_or_else(|error| format!(r#"{{"error":"failed to format analysis: {error}"}}"#))
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
    use super::*;
    use serde_json::json;

    fn fixture_report(depth: AnalysisDepth) -> AnalysisReport {
        AnalysisReport {
            depth,
            listed_root: true,
            root_entries: 3,
            detected_project_files: vec!["Cargo.toml".to_owned()],
            config_files: vec![("README.md".to_owned(), 42)],
            source_dirs: vec!["src".to_owned()],
        }
    }

    #[test]
    fn parse_analysis_depth_accepts_known_values() {
        assert!(matches!(parse_analysis_depth("basic"), (AnalysisDepth::Basic, true)));
        assert!(matches!(parse_analysis_depth("Deep"), (AnalysisDepth::Deep, true)));
    }

    #[test]
    fn parse_analysis_depth_falls_back_to_standard() {
        assert!(matches!(parse_analysis_depth("bogus"), (AnalysisDepth::Standard, false)));
    }

    #[test]
    fn parse_output_format_accepts_known_values() {
        assert!(matches!(parse_output_format("json"), (OutputFormat::Json, true)));
        assert!(matches!(parse_output_format("HTML"), (OutputFormat::Html, true)));
    }

    #[test]
    fn parse_output_format_falls_back_to_text() {
        assert!(matches!(parse_output_format("bogus"), (OutputFormat::Text, false)));
    }

    #[test]
    fn json_report_is_parseable_and_complete() {
        let rendered = render_json_report(&fixture_report(AnalysisDepth::Deep));
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid json");

        assert_eq!(value["depth"], "deep");
        assert_eq!(value["listed_root"], true);
        assert_eq!(value["root_entries"], 3);
        assert_eq!(value["detected_project_files"], json!(["Cargo.toml"]));
        assert_eq!(value["config_files"], json!([{"name": "README.md", "bytes": 42}]));
        assert_eq!(value["source_dirs"], json!(["src"]));
    }

    #[test]
    fn text_report_mentions_deep_analysis_only_for_deep_depth() {
        let deep = render_text_report(&fixture_report(AnalysisDepth::Deep));
        assert!(deep.contains("Found 3 files/directories in root"));
        assert!(deep.contains("Checking project configuration"));
        assert!(deep.contains("source directory: src"));
        assert!(deep.contains("Deep analysis"));

        let standard = render_text_report(&fixture_report(AnalysisDepth::Standard));
        assert!(!standard.contains("Deep analysis"));
    }

    #[test]
    fn text_report_omits_listing_lines_when_root_listing_failed() {
        let mut report = fixture_report(AnalysisDepth::Standard);
        report.listed_root = false;
        report.root_entries = 0;

        let rendered = render_text_report(&report);
        assert!(!rendered.contains("Root directory structure obtained"));
        assert!(!rendered.contains("files/directories in root"));
    }

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
