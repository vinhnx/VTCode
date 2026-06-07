//! Utility functions for the VT Code agent
//!
//! This module contains common utility functions that are used across different parts
//! of the VT Code agent, helping to reduce code duplication and improve maintainability.

use crate::utils::colors::style;
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::Path;

pub use vtcode_commons::project::{ProjectOverview, build_project_overview};
pub use vtcode_commons::utils::{
    current_timestamp, extract_readme_excerpt, extract_toml_str, safe_replace_text,
};

/// Merge a base list of patterns with patterns loaded from an environment variable.
/// The environment variable, if set, is expected to be a comma-separated list of values.
pub fn merge_env_patterns(base: &[String], env_var: &str) -> Vec<String> {
    let extra_val = std::env::var(env_var).ok();
    let extra_count = extra_val
        .as_ref()
        .map(|s| s.split(',').count())
        .unwrap_or(0);

    let mut combined = Vec::with_capacity(base.len() + extra_count);

    for entry in base {
        let trimmed = entry.trim();
        if !trimmed.is_empty() {
            combined.push(trimmed.to_owned());
        }
    }

    if let Some(extra) = extra_val {
        for item in extra.split(',') {
            let trimmed = item.trim();
            if !trimmed.is_empty() {
                combined.push(trimmed.to_owned());
            }
        }
    }

    combined
}

const WORKSPACE_LANGUAGE_SCAN_LIMIT: usize = 5_000;

/// Render PTY output in a terminal-like interface
pub fn render_pty_output_fn(output: &str, title: &str, command: Option<&str>) -> Result<()> {
    use std::io::Write;

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();

    writeln!(handle, "{}", style("=".repeat(80)).dim())?;
    writeln!(handle, "{} {}", style("==").bold(), style(title).bold())?;

    if let Some(cmd) = command {
        writeln!(handle, "{}", style(format!("> {}", cmd)).dim())?;
    }

    writeln!(handle, "{}", style("-".repeat(80)).dim())?;
    write!(handle, "{}", output)?;
    writeln!(handle, "{}", style("-".repeat(80)).dim())?;
    writeln!(handle, "{}", style("==").bold())?;
    writeln!(handle, "{}", style("=".repeat(80)).dim())?;
    handle.flush()?;

    Ok(())
}

/// Summarize workspace languages using file extension heuristics
pub fn summarize_workspace_languages(root: &Path) -> Option<String> {
    let counts = collect_workspace_language_counts(root);
    if counts.is_empty() {
        return None;
    }

    Some(
        counts
            .into_iter()
            .map(|(language, count)| format!("{language}:{count}"))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// Detect the dominant workspace languages using file extension heuristics.
pub fn detect_workspace_languages(root: &Path) -> Vec<String> {
    let mut counts = collect_workspace_language_counts(root)
        .into_iter()
        .collect::<Vec<_>>();
    counts.sort_by(|(left_lang, left_count), (right_lang, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_lang.cmp(right_lang))
    });
    counts
        .into_iter()
        .map(|(language, _)| language)
        .take(5)
        .collect()
}

pub fn display_language_from_path(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?;
    display_language_from_extension(extension)
}

pub fn display_language_from_editor_language_id(language_id: &str) -> Option<&'static str> {
    match language_id.trim().to_ascii_lowercase().as_str() {
        "rust" => Some("Rust"),
        "python" => Some("Python"),
        "javascript" | "javascriptreact" => Some("JavaScript"),
        "typescript" | "typescriptreact" => Some("TypeScript"),
        "go" => Some("Go"),
        "java" => Some("Java"),
        "shellscript" | "bash" | "shell" | "zsh" | "sh" => Some("Bash"),
        "swift" => Some("Swift"),
        "c" => Some("C"),
        "cpp" | "c++" => Some("C++"),
        "ruby" => Some("Ruby"),
        "php" => Some("PHP"),
        _ => None,
    }
}

fn collect_workspace_language_counts(root: &Path) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    let mut total = 0usize;

    for entry in vtcode_commons::walk::build_walker_single_threaded(root)
        .max_depth(Some(4))
        .build()
        .filter_map(|entry| entry.ok())
    {
        let path = entry.path();
        if path.is_file()
            && let Some(language) = display_language_from_path(path)
        {
            *counts.entry(language.to_string()).or_insert(0) += 1;
            total += 1;
        }

        if total > WORKSPACE_LANGUAGE_SCAN_LIMIT {
            break;
        }
    }

    counts
}

fn display_language_from_extension(extension: &str) -> Option<&'static str> {
    match extension {
        "rs" => Some("Rust"),
        "py" => Some("Python"),
        "js" | "jsx" => Some("JavaScript"),
        "ts" | "tsx" => Some("TypeScript"),
        "go" => Some("Go"),
        "java" => Some("Java"),
        "sh" | "bash" => Some("Bash"),
        "swift" => Some("Swift"),
        "c" | "h" => Some("C"),
        "cpp" | "cc" | "cxx" | "hpp" => Some("C++"),
        "rb" => Some("Ruby"),
        "php" => Some("PHP"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        detect_workspace_languages, display_language_from_editor_language_id,
        display_language_from_path, summarize_workspace_languages,
    };
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn detect_workspace_languages_returns_top_languages() {
        let workspace = TempDir::new().expect("workspace tempdir");
        fs::create_dir_all(workspace.path().join("src")).expect("create src");
        fs::create_dir_all(workspace.path().join("web")).expect("create web");
        fs::write(workspace.path().join("src/lib.rs"), "fn alpha() {}\n").expect("write rust");
        fs::write(workspace.path().join("src/main.rs"), "fn main() {}\n").expect("write rust");
        fs::write(workspace.path().join("web/app.ts"), "const app = 1;\n").expect("write ts");

        let languages = detect_workspace_languages(workspace.path());
        assert_eq!(
            languages,
            vec!["Rust".to_string(), "TypeScript".to_string()]
        );
    }

    #[test]
    fn summarize_workspace_languages_reports_counts() {
        let workspace = TempDir::new().expect("workspace tempdir");
        fs::create_dir_all(workspace.path().join("src")).expect("create src");
        fs::write(workspace.path().join("src/lib.rs"), "fn alpha() {}\n").expect("write rust");
        fs::write(workspace.path().join("src/main.rs"), "fn main() {}\n").expect("write rust");

        let summary = summarize_workspace_languages(workspace.path()).expect("summary");
        assert_eq!(summary, "Rust:2");
    }

    #[test]
    fn display_language_helpers_cover_paths_and_editor_language_ids() {
        assert_eq!(
            display_language_from_path(Path::new("src/lib.rs")),
            Some("Rust")
        );
        assert_eq!(
            display_language_from_editor_language_id("typescriptreact"),
            Some("TypeScript")
        );
        assert_eq!(
            display_language_from_editor_language_id("shellscript"),
            Some("Bash")
        );
        assert_eq!(display_language_from_editor_language_id("unknown"), None);
    }
}
