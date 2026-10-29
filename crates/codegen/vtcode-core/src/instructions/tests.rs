use super::*;
use tempfile::tempdir;

fn write_doc(dir: &Path, content: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("AGENTS.md");
    std::fs::write(&path, content)?;
    Ok(path)
}

fn write_claude_doc(dir: &Path, content: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("CLAUDE.md");
    std::fs::write(&path, content)?;
    Ok(path)
}

fn write_rule(dir: &Path, name: &str, content: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    std::fs::write(&path, content)?;
    Ok(path)
}

fn default_options<'a>(
    current_dir: &'a Path,
    project_root: &'a Path,
    home_dir: Option<&'a Path>,
    match_paths: &'a [PathBuf],
) -> InstructionDiscoveryOptions<'a> {
    InstructionDiscoveryOptions {
        current_dir,
        project_root,
        home_dir,
        extra_patterns: &[],
        fallback_filenames: &[],
        exclude_patterns: &[],
        match_paths,
        import_max_depth: 5,
    }
}

#[tokio::test]
async fn discovers_user_workspace_and_rule_sources_in_priority_order() -> Result<()> {
    let workspace = tempdir()?;
    let project_root = workspace.path();
    let nested = project_root.join("src/app");
    std::fs::create_dir_all(&nested)?;

    let home = tempdir()?;
    write_doc(&home.path().join(".vtcode"), "user agents")?;
    write_rule(&home.path().join(".vtcode/rules"), "shared.md", "# Shared\n- user shared rule\n")?;
    write_rule(
        &home.path().join(".vtcode/rules"),
        "matched.md",
        "---\npaths:\n  - \"src/**/*.rs\"\n---\n# Matched\n- user matched rule\n",
    )?;

    write_doc(project_root, "root agents")?;
    write_doc(&project_root.join("src"), "nested agents")?;
    write_rule(&project_root.join(".vtcode/rules"), "workspace.md", "# Workspace\n- workspace rule\n")?;
    write_rule(
        &project_root.join(".vtcode/rules"),
        "workspace-matched.md",
        "---\npaths:\n  - \"src/**/*.rs\"\n---\n# Workspace Matched\n- workspace matched rule\n",
    )?;

    let match_paths = vec![project_root.join("src/main.rs")];
    let sources =
        discover_instruction_sources(&default_options(&nested, project_root, Some(home.path()), &match_paths)).await?;

    let labels = sources.iter().map(instruction_source_label).collect::<Vec<_>>();

    assert_eq!(
        labels,
        vec![
            "user AGENTS",
            "user rule",
            "user matched rule",
            "workspace AGENTS",
            "workspace AGENTS",
            "workspace rule",
            "workspace matched rule",
        ]
    );

    Ok(())
}

#[tokio::test]
async fn discovers_vtcode_home_agents_without_extra_instruction_files() -> Result<()> {
    let workspace = tempdir()?;
    let project_root = workspace.path();

    let home = tempdir()?;
    let user_agents_path = write_doc(&home.path().join(".vtcode"), "user agents")?;

    let sources =
        discover_instruction_sources(&default_options(project_root, project_root, Some(home.path()), &[])).await?;

    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].path, canonicalize(user_agents_path)?);
    assert_eq!(sources[0].scope, InstructionScope::User);
    assert_eq!(sources[0].kind, InstructionSourceKind::Agents);

    Ok(())
}

#[tokio::test]
async fn discovers_claude_files_alongside_workspace_agents() -> Result<()> {
    let workspace = tempdir()?;
    let project_root = canonicalize(workspace.path())?;
    let nested = project_root.join("src");
    std::fs::create_dir_all(&nested)?;

    write_doc(&project_root, "root agents")?;
    write_claude_doc(&project_root, "root claude")?;
    write_doc(&nested, "nested agents")?;
    write_claude_doc(&nested, "nested claude")?;

    let sources = discover_instruction_sources(&default_options(&nested, &project_root, None, &[])).await?;
    let workspace_paths = sources
        .iter()
        .filter(|source| matches!(source.scope, InstructionScope::Workspace))
        .map(|source| format_instruction_path(&source.path, &project_root, None))
        .collect::<Vec<_>>();

    assert_eq!(workspace_paths, vec!["AGENTS.md", "CLAUDE.md", "src/AGENTS.md", "src/CLAUDE.md"]);

    Ok(())
}

#[tokio::test]
async fn expands_imports_strips_comments_and_matches_rule_paths() -> Result<()> {
    let workspace = tempdir()?;
    let project_root = workspace.path();
    let docs_dir = project_root.join("docs");
    std::fs::create_dir_all(&docs_dir)?;
    std::fs::write(docs_dir.join("shared.md"), "# Shared\n- imported detail\n")?;

    write_doc(project_root, "# Root\n<!-- hidden -->\n- visible\n\nSee @docs/shared.md\n- trailing detail\n")?;
    write_rule(
        &project_root.join(".vtcode/rules"),
        "rust.md",
        "---\npaths:\n  - \"src/**/*.rs\"\n---\n# Rust\n- rust rule\n",
    )?;

    let match_paths = vec![project_root.join("src/lib.rs")];
    let bundle = read_instruction_bundle(&default_options(project_root, project_root, None, &match_paths), 16 * 1024)
        .await?
        .expect("instruction bundle");

    assert_eq!(bundle.segments.len(), 2);
    let combined = bundle.combined_text();
    assert!(combined.contains("- visible"));
    assert!(combined.contains("imported detail"));
    assert!(combined.contains("- rust rule"));
    assert!(!combined.contains("hidden"));
    assert!(combined.find("See @docs/shared.md") < combined.find("imported detail"));
    assert!(combined.find("imported detail") < combined.find("- trailing detail"));

    Ok(())
}

#[tokio::test]
async fn excludes_instruction_paths_with_globs() -> Result<()> {
    let workspace = tempdir()?;
    let project_root = workspace.path();
    let nested = project_root.join("src");
    std::fs::create_dir_all(&nested)?;
    write_doc(project_root, "root agents")?;
    write_doc(&nested, "nested agents")?;

    let exclude = vec![project_root.join("src/AGENTS.md").display().to_string()];
    let options = InstructionDiscoveryOptions {
        exclude_patterns: &exclude,
        ..default_options(&nested, project_root, None, &[])
    };
    let sources = discover_instruction_sources(&options).await?;

    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].path.file_name().and_then(|value| value.to_str()).unwrap_or_default(), "AGENTS.md");

    Ok(())
}

#[tokio::test]
async fn ignores_rules_readme_files() -> Result<()> {
    let workspace = tempdir()?;
    let project_root = workspace.path();
    std::fs::create_dir_all(project_root.join(".vtcode/rules"))?;
    write_rule(&project_root.join(".vtcode/rules"), "README.md", "# Rules\n- should stay out of prompt memory\n")?;
    write_rule(&project_root.join(".vtcode/rules"), "rust.md", "# Rust\n- keep changes surgical\n")?;

    let sources = discover_instruction_sources(&default_options(project_root, project_root, None, &[])).await?;

    assert_eq!(sources.len(), 1);
    assert!(
        sources[0].path.ends_with("rust.md"),
        "sources: {:?}",
        sources
            .iter()
            .map(|source| source.path.display().to_string())
            .collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn highlight_fallback_uses_non_bullet_content() {
    let segments = vec![InstructionSegment {
        source: InstructionSource {
            path: PathBuf::from("AGENTS.md"),
            scope: InstructionScope::Workspace,
            kind: InstructionSourceKind::Agents,
            matched: false,
        },
        contents: "Root summary\n".to_owned(),
    }];

    let highlights = extract_instruction_highlights(&segments, 1);
    assert_eq!(highlights, vec!["Root summary".to_owned()]);
}
