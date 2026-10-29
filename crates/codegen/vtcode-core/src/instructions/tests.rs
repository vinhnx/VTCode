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
async fn imports_are_limited_to_workspace_and_user_instruction_roots() -> Result<()> {
    let workspace = tempdir()?;
    let home = tempdir()?;
    let user_root = home.path().join(".vtcode");
    std::fs::create_dir_all(&user_root)?;
    std::fs::write(home.path().join("private.md"), "PRIVATE HOME CONTENT")?;
    std::fs::write(user_root.join("shared.md"), "ALLOWED USER CONTENT")?;
    std::fs::write(user_root.join("secret.md"), "PRIVATE USER ROOT CONTENT")?;
    std::fs::write(
        workspace.path().join("local.md"),
        format!("ALLOWED WORKSPACE CONTENT\n@{}\n", user_root.join("secret.md").display()),
    )?;
    write_doc(&user_root, &format!("@shared.md\n@{}\n", workspace.path().join("local.md").display()))?;
    write_doc(
        workspace.path(),
        &format!(
            "@{}\n@{}\n@local.md\n",
            home.path().join("private.md").display(),
            user_root.join("secret.md").display()
        ),
    )?;

    let bundle = read_instruction_bundle(
        &default_options(workspace.path(), workspace.path(), Some(home.path()), &[]),
        16 * 1024,
    )
    .await?
    .expect("instruction bundle");
    let combined = bundle.combined_text();
    assert!(!combined.contains("PRIVATE HOME CONTENT"));
    assert!(!combined.contains("PRIVATE USER ROOT CONTENT"));
    assert!(combined.contains("ALLOWED USER CONTENT"));
    assert!(combined.contains("ALLOWED WORKSPACE CONTENT"));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn workspace_instruction_discovery_rejects_symlink_escapes() -> Result<()> {
    use std::os::unix::fs::symlink;

    let workspace = tempdir()?;
    let outside = tempdir()?;
    std::fs::write(outside.path().join("private.md"), "PRIVATE EXTERNAL CONTENT")?;
    symlink(outside.path().join("private.md"), workspace.path().join("AGENTS.override.md"))?;
    write_doc(workspace.path(), "ALLOWED WORKSPACE INSTRUCTIONS")?;
    std::fs::create_dir_all(workspace.path().join(".vtcode/rules"))?;
    symlink(outside.path().join("private.md"), workspace.path().join(".vtcode/rules/escape.md"))?;
    symlink(outside.path(), workspace.path().join(".vtcode/rules/external"))?;
    write_rule(&workspace.path().join(".vtcode/rules"), "local.md", "ALLOWED WORKSPACE RULE")?;

    let bundle = read_instruction_bundle(&default_options(workspace.path(), workspace.path(), None, &[]), 16 * 1024)
        .await?
        .expect("instruction bundle");
    let combined = bundle.combined_text();
    assert!(combined.contains("ALLOWED WORKSPACE INSTRUCTIONS"));
    assert!(combined.contains("ALLOWED WORKSPACE RULE"));
    assert!(!combined.contains("PRIVATE EXTERNAL CONTENT"));
    Ok(())
}

#[tokio::test]
async fn recursive_rule_patterns_are_bounded_and_match_zero_directories() -> Result<()> {
    let workspace = tempdir()?;
    let rules = workspace.path().join(".vtcode/rules");
    let pattern = "**/a/**/b/**/c/**/d/**/e/**/f/**/g/**/h/file.rs";
    let rule = write_rule(&rules, "bounded.md", &format!("---\npaths:\n  - \"{pattern}\"\n---\nMATCHED\n"))?;
    let paths = vec![workspace.path().join("a/b/c/d/e/f/g/h/file.rs")];
    let options = default_options(workspace.path(), workspace.path(), None, &paths);
    let sources = discover_instruction_sources(&options).await?;
    assert!(sources.iter().any(|source| source.matched));

    std::fs::write(&rule, format!("---\npaths:\n  - \"{}\"\n---\nTOO LONG\n", "a".repeat(4097)))?;
    let error = discover_instruction_sources(&options)
        .await
        .expect_err("oversized pattern must fail");
    assert!(format!("{error:#}").contains("exceeds 4096 bytes"));
    Ok(())
}

#[test]
fn rule_matcher_preserves_glob_classes_and_handles_adversarial_patterns() -> Result<()> {
    for (pattern, value, expected) in [
        ("src/**/test?.[rR]s", "src/test1.rs", true),
        ("src/**/test?.[rR]s", "src/a/b/test2.Rs", true),
        ("src/**/test?.[!a-z]s", "src/test1.rs", false),
        ("foo[[]bar[]]", "foo[bar]", true),
        ("a*b*c", "a/b/c", true),
        ("a*b*c", "a/b/d", false),
    ] {
        let candidate = MatchCandidate { relative_path: value.to_string(), is_dir: false };
        assert_eq!(pattern_matches_candidate(&RulePattern::new(pattern)?, &candidate), expected, "{pattern} {value}");
    }
    let pattern = format!("{}z", "*a".repeat(100));
    let candidate = MatchCandidate { relative_path: "a".repeat(500), is_dir: false };
    assert!(!pattern_matches_candidate(&RulePattern::new(&pattern)?, &candidate));
    let recursive = format!("{}file.rs", "**/x/".repeat(40));
    let candidate = MatchCandidate {
        relative_path: format!("{}file.rs", "x/".repeat(40)),
        is_dir: false,
    };
    assert!(pattern_matches_candidate(&RulePattern::new(&recursive)?, &candidate));
    Ok(())
}

#[test]
fn bounded_rule_matcher_matches_legacy_glob_behavior() -> Result<()> {
    // `glob` requires `**` to form a whole path component; the bounded matcher
    // must reject the same malformed patterns instead of miscompiling them.
    for pattern in ["**a", "a**b", "a**"] {
        assert!(Pattern::new(pattern).is_err(), "{pattern} legacy");
        assert!(RulePattern::new(pattern).is_err(), "{pattern} bounded");
    }
    fn variants(pattern: &str) -> Vec<String> {
        let mut results = vec![pattern.to_string()];
        let mut cursor = 0;
        while cursor < results.len() {
            let current = results[cursor].clone();
            for (index, _) in current.match_indices("**/") {
                let variant = format!("{}{}", &current[..index], &current[index + 3..]);
                if !results.contains(&variant) {
                    results.push(variant);
                }
            }
            cursor += 1;
        }
        results
    }
    for pattern in [
        "**/a",
        "a/**/b",
        "**/a/**/b",
        "a*b?",
        "a/[!x-z]",
        "**",
        "a/**",
        "a/**/",
        "[[]x[]]",
    ] {
        let bounded = RulePattern::new(pattern)?;
        let legacy = variants(pattern)
            .into_iter()
            .filter_map(|variant| Pattern::new(&variant).ok())
            .collect::<Vec<_>>();
        for value in [
            "", "a", "a/", "a/b", "a/b/", "a/x/b", "x/a/b", "a/z", "abc", "xa", "axb", "[x]", "a/b/c",
        ] {
            let candidate = MatchCandidate { relative_path: value.to_string(), is_dir: false };
            assert_eq!(
                pattern_matches_candidate(&bounded, &candidate),
                legacy.iter().any(|p| p.matches(value)),
                "{pattern} {value}"
            );
        }
    }
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
