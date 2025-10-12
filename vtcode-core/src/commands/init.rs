//! Init command implementation - project analysis and Repository Guidelines generation
//!
//! Generates AGENTS.md files following the standardized contributor guide format
//! as specified in <https://github.com/openai/codex/blob/main/codex-rs/tui/prompt_for_init_command.md>
//!
//! This tool analyzes any repository and generates a concise (200-400 words) AGENTS.md file
//! that serves as a contributor guide, adapting content based on the specific project structure,
//! commit history, and detected technologies.

use crate::config::constants::tools;
use crate::tools::ToolRegistry;
use anyhow::Result;
use console::style;
use indexmap::IndexMap;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Project analysis result
#[derive(Debug, Clone)]
struct ProjectAnalysis {
    // Core project info
    project_name: String,
    languages: Vec<String>,
    build_systems: Vec<String>,
    dependencies: IndexMap<String, Vec<String>>,

    // Structure analysis
    source_dirs: Vec<String>,
    test_patterns: Vec<String>,
    config_files: Vec<String>,
    documentation_files: Vec<String>,

    // Git analysis
    commit_patterns: Vec<String>,
    has_git_history: bool,

    // Project characteristics
    is_library: bool,
    is_application: bool,
    has_ci_cd: bool,
    has_docker: bool,

    // Content optimization
    estimated_word_count: usize,
}

/// Handle the init command - analyze project and generate AGENTS.md
pub async fn handle_init_command(registry: &mut ToolRegistry, workspace: &PathBuf) -> Result<()> {
    println!(
        "{}",
        style("Initializing project with AGENTS.md...")
            .cyan()
            .bold()
    );

    // Step 1: Analyze the project structure
    println!("{}", style("1. Analyzing project structure...").dim());
    let analysis = analyze_project(registry, workspace).await?;

    // Step 2: Generate AGENTS.md content
    println!("{}", style("2. Generating AGENTS.md content...").dim());
    let agents_md_content = generate_agents_md(&analysis)?;

    // Step 3: Write AGENTS.md file
    println!("{}", style("3. Writing AGENTS.md file...").dim());
    let agents_md_path = workspace.join("AGENTS.md");

    registry
        .execute_tool(
            tools::WRITE_FILE,
            json!({
                "path": agents_md_path.to_string_lossy(),
                "content": agents_md_content,
                "overwrite": true
            }),
        )
        .await?;

    println!(
        "{} {}",
        style("✓").green().bold(),
        style("AGENTS.md generated successfully!").green()
    );
    println!(
        "{} {}",
        style(" Location:").blue(),
        agents_md_path.display()
    );

    Ok(())
}

/// Analyze the current project structure
async fn analyze_project(
    registry: &mut ToolRegistry,
    workspace: &PathBuf,
) -> Result<ProjectAnalysis> {
    let project_name = workspace
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string();

    let mut analysis = ProjectAnalysis {
        project_name,
        languages: Vec::new(),
        build_systems: Vec::new(),
        dependencies: IndexMap::new(),
        source_dirs: Vec::new(),
        test_patterns: Vec::new(),
        config_files: Vec::new(),
        documentation_files: Vec::new(),
        commit_patterns: Vec::new(),
        has_git_history: false,
        is_library: false,
        is_application: false,
        has_ci_cd: false,
        has_docker: false,
        estimated_word_count: 0,
    };

    // Analyze root directory structure
    let root_files = registry
        .execute_tool(tools::LIST_FILES, json!({"path": ".", "max_items": 100}))
        .await?;

    if let Some(files) = root_files.get("files") {
        if let Some(files_array) = files.as_array() {
            for file_obj in files_array {
                if let Some(path) = file_obj.get("path").and_then(|p| p.as_str()) {
                    analyze_file(&mut analysis, path, registry).await?;
                }
            }
        }
    }

    // Detect common source directories
    let common_src_dirs = vec!["src", "lib", "pkg", "internal", "cmd", "app", "core"];
    for dir in common_src_dirs {
        if workspace.join(dir).exists() {
            analysis.source_dirs.push(dir.to_string());
        }
    }

    // Detect test patterns
    let test_patterns = vec!["test_", "_test", ".test.", ".spec.", "__tests__"];
    for pattern in test_patterns {
        if files_contain_pattern(&analysis, pattern) {
            analysis.test_patterns.push(pattern.to_string());
        }
    }

    // Analyze git history for commit patterns
    analyze_git_history(&mut analysis, registry).await?;

    // Analyze project characteristics
    analyze_project_characteristics(&mut analysis);

    Ok(analysis)
}

/// Analyze individual files to detect languages, frameworks, etc.
async fn analyze_file(
    analysis: &mut ProjectAnalysis,
    path: &str,
    registry: &mut ToolRegistry,
) -> Result<()> {
    match path {
        // Rust project files
        "Cargo.toml" => {
            analysis.languages.push("Rust".to_string());
            analysis.build_systems.push("Cargo".to_string());

            // Read Cargo.toml to extract dependencies
            let cargo_content = registry
                .execute_tool(
                    tools::READ_FILE,
                    json!({"path": "Cargo.toml", "max_bytes": 5000}),
                )
                .await?;

            if let Some(content) = cargo_content.get("content").and_then(|c| c.as_str()) {
                extract_cargo_dependencies(analysis, content);
            }
        }
        "Cargo.lock" => {
            analysis.config_files.push("Cargo.lock".to_string());
        }

        // Node.js project files
        "package.json" => {
            analysis.languages.push("JavaScript/TypeScript".to_string());
            analysis.build_systems.push("npm/yarn/pnpm".to_string());

            // Read package.json to extract dependencies
            let package_content = registry
                .execute_tool(
                    tools::READ_FILE,
                    json!({"path": "package.json", "max_bytes": 5000}),
                )
                .await?;

            if let Some(content) = package_content.get("content").and_then(|c| c.as_str()) {
                extract_package_dependencies(analysis, content);
            }
        }
        "yarn.lock" | "package-lock.json" | "pnpm-lock.yaml" => {
            analysis.config_files.push(path.to_string());
        }

        // Python project files
        "requirements.txt" | "pyproject.toml" | "setup.py" | "Pipfile" => {
            if !analysis.languages.contains(&"Python".to_string()) {
                analysis.languages.push("Python".to_string());
            }
            analysis.build_systems.push("pip/poetry".to_string());
            analysis.config_files.push(path.to_string());
        }

        // Go project files
        "go.mod" | "go.sum" => {
            analysis.languages.push("Go".to_string());
            analysis.build_systems.push("Go Modules".to_string());
            analysis.config_files.push(path.to_string());
        }

        // Java project files
        "pom.xml" | "build.gradle" | "build.gradle.kts" => {
            analysis.languages.push("Java/Kotlin".to_string());
            analysis.build_systems.push("Maven/Gradle".to_string());
            analysis.config_files.push(path.to_string());
        }

        // Documentation files
        "README.md" | "CHANGELOG.md" | "CONTRIBUTING.md" | "LICENSE" | "LICENSE.md" => {
            analysis.documentation_files.push(path.to_string());
        }

        // Configuration files
        ".gitignore" | ".editorconfig" | ".prettierrc" | ".eslintrc" | ".eslintrc.js"
        | ".eslintrc.json" => {
            analysis.config_files.push(path.to_string());
        }

        // Docker files
        "Dockerfile" | "docker-compose.yml" | "docker-compose.yaml" | ".dockerignore" => {
            analysis.config_files.push(path.to_string());
        }

        // CI/CD files
        "Jenkinsfile" | ".travis.yml" | "azure-pipelines.yml" | ".circleci/config.yml" => {
            analysis.config_files.push(path.to_string());
        }

        // GitHub workflows (would be detected via directory listing)
        path if path.starts_with(".github/workflows/") => {
            analysis.config_files.push(path.to_string());
        }

        // Source directories
        "src" | "lib" | "pkg" | "internal" | "cmd" | "app" | "core" => {
            analysis.source_dirs.push(path.to_string());
        }

        _ => {}
    }

    Ok(())
}

/// Extract dependencies from Cargo.toml
fn extract_cargo_dependencies(analysis: &mut ProjectAnalysis, content: &str) {
    let mut deps = Vec::new();

    // Simple regex-like parsing for dependencies
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('"') && line.contains(" = ") {
            if let Some(dep_name) = line.split('"').nth(1) {
                deps.push(dep_name.to_string());
            }
        }
    }

    if !deps.is_empty() {
        analysis
            .dependencies
            .insert("Rust (Cargo)".to_string(), deps);
    }
}

/// Extract dependencies from package.json
fn extract_package_dependencies(analysis: &mut ProjectAnalysis, content: &str) {
    let mut deps = Vec::new();

    // Simple parsing for dependencies
    if content.contains("\"dependencies\":") {
        // Extract dependency names from JSON
        for line in content.lines() {
            if line.contains("\"")
                && line.contains(":")
                && !line.contains("{")
                && !line.contains("}")
            {
                if let Some(dep_name) = line.split('"').nth(1) {
                    if !dep_name.is_empty()
                        && dep_name != "dependencies"
                        && dep_name != "devDependencies"
                    {
                        deps.push(dep_name.to_string());
                    }
                }
            }
        }
    }

    if !deps.is_empty() {
        analysis
            .dependencies
            .insert("JavaScript/TypeScript (npm)".to_string(), deps);
    }
}

/// Check if files contain a specific pattern
fn files_contain_pattern(analysis: &ProjectAnalysis, pattern: &str) -> bool {
    // Scan source directories for the pattern
    for dir in &analysis.source_dirs {
        let path = Path::new(dir);
        if path.is_dir() {
            for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    if let Ok(content) = fs::read_to_string(entry.path()) {
                        if content.contains(pattern) {
                            return true;
                        }
                    }
                }
            }
        } else if path.is_file() {
            if let Ok(content) = fs::read_to_string(path) {
                if content.contains(pattern) {
                    return true;
                }
            }
        }
    }

    // Check config files
    for file in &analysis.config_files {
        let path = Path::new(file);
        if let Ok(content) = fs::read_to_string(path) {
            if content.contains(pattern) {
                return true;
            }
        }
    }

    // Fallback to metadata search
    analysis.languages.iter().any(|lang| lang.contains(pattern))
}

/// Analyze git history to detect commit message patterns
async fn analyze_git_history(
    analysis: &mut ProjectAnalysis,
    registry: &mut ToolRegistry,
) -> Result<()> {
    // Check if .git directory exists by trying to list it
    let git_check = registry
        .execute_tool("list_files", json!({"path": ".git", "max_items": 1}))
        .await;

    if git_check.is_ok() {
        analysis.has_git_history = true;

        // Try to get recent commit messages to analyze patterns
        let git_log_result = registry
            .execute_tool(
                tools::RUN_TERMINAL_CMD,
                json!({
                    "command": "git log --oneline -20 --pretty=format:'%s'",
                    "timeout": 5000
                }),
            )
            .await;

        if let Ok(output) = git_log_result {
            if let Some(stdout) = output.get("stdout").and_then(|s| s.as_str()) {
                let mut conventional_count = 0;
                let mut total_commits = 0;

                for line in stdout.lines() {
                    total_commits += 1;
                    let line = line.trim();

                    // Check for conventional commit patterns
                    if line.contains("feat:")
                        || line.contains("fix:")
                        || line.contains("docs:")
                        || line.contains("style:")
                        || line.contains("refactor:")
                        || line.contains("test:")
                        || line.contains("chore:")
                    {
                        conventional_count += 1;
                    }
                }

                // If more than 50% use conventional commits, note this pattern
                if total_commits > 0 && (conventional_count * 100 / total_commits) > 50 {
                    analysis
                        .commit_patterns
                        .push("Conventional Commits".to_string());
                } else {
                    analysis
                        .commit_patterns
                        .push("Standard commit messages".to_string());
                }
            }
        } else {
            // Fallback if git command fails - assume standard commits
            analysis
                .commit_patterns
                .push("Standard commit messages".to_string());
        }
    } else {
        // No git repository found
        analysis.has_git_history = false;
        analysis
            .commit_patterns
            .push("No version control detected".to_string());
    }

    Ok(())
}

/// Analyze project characteristics to determine what type of project this is
fn analyze_project_characteristics(analysis: &mut ProjectAnalysis) {
    // Determine if it's a library or application
    analysis.is_library = analysis.config_files.iter().any(|f| {
        f == "Cargo.toml" && analysis.languages.contains(&"Rust".to_string())
            || f == "package.json"
                && analysis
                    .languages
                    .contains(&"JavaScript/TypeScript".to_string())
            || f == "setup.py"
            || f == "pyproject.toml"
    });

    analysis.is_application = analysis.source_dirs.contains(&"src".to_string())
        || analysis.source_dirs.contains(&"cmd".to_string())
        || analysis.source_dirs.contains(&"app".to_string());

    // Check for CI/CD files
    analysis.has_ci_cd = analysis.config_files.iter().any(|f| {
        f.contains(".github/workflows")
            || f.contains(".gitlab-ci")
            || f.contains(".travis")
            || f == "Jenkinsfile"
            || f == ".circleci/config.yml"
            || f == "azure-pipelines.yml"
    });

    // Check for Docker files
    analysis.has_docker = analysis.config_files.iter().any(|f| {
        f == "Dockerfile"
            || f == "docker-compose.yml"
            || f == "docker-compose.yaml"
            || f == ".dockerignore"
    });
}

/// Generate AGENTS.md content based on project analysis
fn generate_agents_md(analysis: &ProjectAnalysis) -> Result<String> {
    let mut content = String::new();
    let mut word_count = 0;

    // Header - Title the document "Repository Guidelines"
    content.push_str("# Repository Guidelines\n\n");
    word_count += 2;

    // Brief introduction - keep concise
    let intro = format!(
        "This document serves as a contributor guide for the {} repository.\n\n",
        analysis.project_name
    );
    content.push_str(&intro);
    word_count += intro.split_whitespace().count();

    // Project Structure & Module Organization - prioritize based on detected structure
    if !analysis.source_dirs.is_empty() || !analysis.languages.is_empty() {
        content.push_str("## Project Structure & Module Organization\n\n");
        word_count += 5;

        // Only show relevant source directories
        if !analysis.source_dirs.is_empty() {
            for dir in &analysis.source_dirs {
                let line = format!("- `{}/` - Source code\n", dir);
                content.push_str(&line);
                word_count += line.split_whitespace().count();
            }
        }

        // Add language-specific structure info - only for detected languages
        for language in &analysis.languages {
            match language.as_str() {
                "Rust" => {
                    content.push_str(
                        "- `tests/` - Integration tests\n- `examples/` - Usage examples\n",
                    );
                    word_count += 8;
                }
                "JavaScript/TypeScript" => {
                    content.push_str(
                        "- `test/` or `__tests__/` - Test files\n- `dist/` - Built assets\n",
                    );
                    word_count += 10;
                }
                "Python" => {
                    content.push_str("- `tests/` - Test files\n- Package modules in root\n");
                    word_count += 9;
                }
                _ => {}
            }
        }
        content.push('\n');
    }

    // Build, Test, and Development Commands - only include relevant ones
    if !analysis.build_systems.is_empty() && word_count < 300 {
        content.push_str("## Build, Test, and Development Commands\n\n");
        word_count += 6;

        for system in &analysis.build_systems {
            match system.as_str() {
                "Cargo" => {
                    content.push_str("- `cargo build` - Build project\n- `cargo test` - Run tests\n- `cargo run` - Run application\n");
                    word_count += 15;
                }
                "npm/yarn/pnpm" => {
                    content.push_str("- `npm install` - Install dependencies\n- `npm test` - Run tests\n- `npm run build` - Build for production\n");
                    word_count += 18;
                }
                "pip/poetry" => {
                    content.push_str("- `python -m pytest` - Run tests\n- `pip install -r requirements.txt` - Install dependencies\n");
                    word_count += 15;
                }
                _ => {}
            }
        }
        content.push('\n');
    }

    // Coding Style & Naming Conventions - concise, language-specific
    if !analysis.languages.is_empty() && word_count < 350 {
        content.push_str("## Coding Style & Naming Conventions\n\n");
        word_count += 5;

        for language in &analysis.languages {
            match language.as_str() {
                "Rust" => {
                    content.push_str("- **Indentation:** 4 spaces\n- **Naming:** snake_case functions, PascalCase types\n- **Formatting:** `cargo fmt`\n\n");
                    word_count += 15;
                }
                "JavaScript/TypeScript" => {
                    content.push_str("- **Indentation:** 2 spaces\n- **Naming:** camelCase variables, PascalCase classes\n- **Formatting:** Prettier\n\n");
                    word_count += 14;
                }
                "Python" => {
                    content.push_str("- **Style:** PEP 8\n- **Indentation:** 4 spaces\n- **Formatting:** Black\n\n");
                    word_count += 10;
                }
                _ => {}
            }
        }
    }

    // Testing Guidelines - brief and relevant
    if !analysis.test_patterns.is_empty() && word_count < 370 {
        content.push_str("## Testing Guidelines\n\n");
        word_count += 3;

        let test_info = format!(
            "- Test files: {}\n- Run tests using build system commands above\n\n",
            analysis.test_patterns.join(", ")
        );
        content.push_str(&test_info);
        word_count += test_info.split_whitespace().count();
    }

    // Commit & Pull Request Guidelines - use detected patterns
    if word_count < 380 {
        content.push_str("## Commit & Pull Request Guidelines\n\n");
        word_count += 5;

        if analysis
            .commit_patterns
            .contains(&"Conventional Commits".to_string())
        {
            content.push_str("- Use conventional commit format: `type(scope): description`\n");
            content.push_str("- Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`\n");
            word_count += 14;
        } else {
            content.push_str("- Write clear, descriptive commit messages\n");
            content.push_str("- Use imperative mood: \"Add feature\" not \"Added feature\"\n");
            word_count += 13;
        }

        content.push_str("- Link issues with `Fixes #123` or `Closes #123`\n");
        content.push_str("- Ensure tests pass before submitting PRs\n\n");
        word_count += 12;
    }

    // Agent-Specific Instructions - always include if space allows
    if word_count < 390 {
        content.push_str("## Agent-Specific Instructions\n\n");
        content.push_str("- Follow established patterns above\n");
        content.push_str("- Include tests for new functionality\n");
        content.push_str("- Update documentation for API changes\n");
        word_count += 15;
    }

    // Store estimated word count for future optimization
    let mut updated_analysis = analysis.clone();
    updated_analysis.estimated_word_count = word_count;

    Ok(content)
}
