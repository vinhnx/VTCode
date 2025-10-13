use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use tempfile::tempdir;
use vtcode_core::simple_indexer::{SimpleIndexer, SimpleIndexerOptions};

fn run_cli_search(
    workspace: &Path,
    pattern: &str,
    include_hidden: bool,
    path_filter: Option<&str>,
) -> Result<Vec<String>> {
    let mut options =
        SimpleIndexerOptions::new().with_index_directory(workspace.join("index-store"));

    if include_hidden {
        options = options.include_hidden_directories();
    }

    let mut indexer = SimpleIndexer::with_options(workspace.to_path_buf(), options);
    indexer
        .init()
        .with_context(|| format!("failed to initialize indexer at {}", workspace.display()))?;

    indexer
        .index_directory(workspace)
        .with_context(|| format!("failed to index workspace at {}", workspace.display()))?;

    let matches = indexer
        .grep(pattern, path_filter)
        .with_context(|| format!("failed to search for pattern '{pattern}'"))?;

    let formatted = matches
        .into_iter()
        .map(|hit| {
            format!(
                "{}:{}: {}",
                hit.file_path, hit.line_number, hit.line_content
            )
        })
        .collect();

    Ok(formatted)
}

#[test]
fn cli_snippet_demonstrates_hidden_toggle_and_filters() -> Result<()> {
    let temp = tempdir().context("failed to create temporary workspace")?;
    let workspace = temp.path();

    fs::create_dir_all(workspace.join("src"))
        .context("failed to create visible source directory")?;
    fs::create_dir_all(workspace.join(".config"))
        .context("failed to create hidden config directory")?;

    fs::write(workspace.join("src").join("lib.rs"), "// TODO: refactor\n")
        .context("failed to write lib.rs")?;
    fs::write(
        workspace.join(".config").join("settings.txt"),
        "TODO: rotate keys\n",
    )
    .context("failed to write settings.txt")?;

    let visible_hits = run_cli_search(workspace, "TODO", false, None)?;
    assert_eq!(visible_hits.len(), 1);
    assert!(visible_hits[0].contains("src/lib.rs:1"));

    let hidden_hits = run_cli_search(workspace, "TODO", true, Some(".config"))?;
    assert_eq!(hidden_hits.len(), 1);
    assert!(hidden_hits[0].contains(".config/settings.txt:1"));

    let filtered_visible = run_cli_search(workspace, "TODO", false, Some("src"))?;
    assert_eq!(filtered_visible, visible_hits);

    Ok(())
}
