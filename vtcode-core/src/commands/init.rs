//! Init command implementation - project analysis and AGENTS.md generation
//!
//! Generates AGENTS.md files following the open specification published at <https://agents.md/>
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
use std::collections::HashSet;
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerateAgentsFileStatus {
    Created,
    Overwritten,
    SkippedExisting,
}

#[derive(Debug, Clone)]
pub struct GenerateAgentsFileReport {
    pub path: PathBuf,
    pub status: GenerateAgentsFileStatus,
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
    let report = write_agents_file(registry, workspace, &agents_md_content, true).await?;

    println!(
        "{} {}",
        style("✓").green().bold(),
        style("AGENTS.md generated successfully!").green()
    );
    println!("{} {}", style(" Location:").blue(), report.path.display());

    Ok(())
}

/// Analyze the workspace and write an AGENTS.md file, optionally overwriting an existing file.
pub async fn generate_agents_file(
    registry: &mut ToolRegistry,
    workspace: &Path,
    overwrite: bool,
) -> Result<GenerateAgentsFileReport> {
    let workspace_path = workspace.to_path_buf();
    let agents_md_path = workspace_path.join("AGENTS.md");

    if agents_md_path.exists() && !overwrite {
        return Ok(GenerateAgentsFileReport {
            path: agents_md_path,
            status: GenerateAgentsFileStatus::SkippedExisting,
        });
    }

    let analysis = analyze_project(registry, &workspace_path).await?;
    let agents_md_content = generate_agents_md(&analysis)?;

    write_agents_file(registry, &workspace_path, &agents_md_content, overwrite).await
}

async fn write_agents_file(
    registry: &mut ToolRegistry,
    workspace: &Path,
    content: &str,
    overwrite: bool,
) -> Result<GenerateAgentsFileReport> {
    let agents_md_path = workspace.join("AGENTS.md");
    let existed_before = agents_md_path.exists();

    if existed_before && !overwrite {
        return Ok(GenerateAgentsFileReport {
            path: agents_md_path,
            status: GenerateAgentsFileStatus::SkippedExisting,
        });
    }

    let mode = if overwrite {
        "overwrite"
    } else {
        "skip_if_exists"
    };
    let path_string = agents_md_path.to_string_lossy().to_string();

    let response = registry
        .execute_tool(
            tools::WRITE_FILE,
            json!({
                "path": path_string,
                "content": content,
                "mode": mode,
            }),
        )
        .await?;

    if !overwrite {
        if response
            .get("skipped")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            return Ok(GenerateAgentsFileReport {
                path: agents_md_path,
                status: GenerateAgentsFileStatus::SkippedExisting,
            });
        }
    }

    let status = if existed_before {
        GenerateAgentsFileStatus::Overwritten
    } else {
        GenerateAgentsFileStatus::Created
    };

    Ok(GenerateAgentsFileReport {
        path: agents_md_path,
        status,
    })
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
    let mut sections = Vec::new();
    sections.push("# AGENTS.md\n\n".to_string());

    if let Some(section) = build_overview_section(analysis) {
        sections.push(section);
    }
    if let Some(section) = build_setup_section(analysis) {
        sections.push(section);
    }
    if let Some(section) = build_code_style_section(analysis) {
        sections.push(section);
    }
    if let Some(section) = build_testing_section(analysis) {
        sections.push(section);
    }
    if let Some(section) = build_pr_section(analysis) {
        sections.push(section);
    }
    if let Some(section) = build_additional_notes_section(analysis) {
        sections.push(section);
    }

    Ok(sections.join(""))
}

fn build_overview_section(analysis: &ProjectAnalysis) -> Option<String> {
    let mut lines = Vec::new();

    if !analysis.project_name.trim().is_empty() {
        lines.push(format!("- Repository: {}", analysis.project_name));
    }

    let languages = unique_preserving_order(&analysis.languages);
    if !languages.is_empty() {
        lines.push(format!("- Primary languages: {}", languages.join(", ")));
    }

    let dirs: Vec<String> = unique_preserving_order(&analysis.source_dirs)
        .into_iter()
        .map(|dir| format!("`{dir}/`"))
        .collect();
    if !dirs.is_empty() {
        lines.push(format!("- Key directories: {}", dirs.join(", ")));
    }

    if analysis.is_library && !analysis.is_application {
        lines.push("- This repository behaves like a reusable library crate/package.".to_string());
    } else if analysis.is_application {
        lines
            .push("- Application entrypoints live under the source directories above.".to_string());
    }

    if analysis.has_ci_cd {
        lines.push(
            "- Continuous integration workflows detected; review `.github/workflows/` for required checks.".to_string(),
        );
    }

    if analysis.has_docker {
        lines.push(
            "- Docker artifacts detected; container workflows may be required for local testing."
                .to_string(),
        );
    }

    format_section("Project overview", lines)
}

fn build_setup_section(analysis: &ProjectAnalysis) -> Option<String> {
    let mut lines = Vec::new();
    let systems = unique_preserving_order(&analysis.build_systems);

    for system in systems {
        match system.as_str() {
            "Cargo" => {
                lines.push(
                    "- Install the Rust toolchain via `rustup` and warm the cache with `cargo fetch`.".to_string(),
                );
            }
            "npm/yarn/pnpm" => {
                lines.push(
                    "- Install Node dependencies with `pnpm install` (or the matching package manager).".to_string(),
                );
            }
            "pip/poetry" => {
                lines.push(
                    "- Create a virtual environment and install requirements via `pip install -r requirements.txt` or `poetry install`.".to_string(),
                );
            }
            "Go Modules" => {
                lines.push("- Synchronize Go modules with `go mod download`.".to_string());
            }
            "Maven/Gradle" => {
                lines.push(
                    "- Prime the Java build by running `./gradlew assemble` or `mvn compile`."
                        .to_string(),
                );
            }
            other => {
                lines.push(format!(
                    "- Review setup steps for `{other}` tooling and document them here."
                ));
            }
        }
    }

    if analysis.has_docker {
        lines.push(
            "- Container workflows available; use `docker compose up --build` when services are required.".to_string(),
        );
    }

    if lines.is_empty() {
        lines.push(
            "- Document project-specific setup commands (dependency installation, environment variables, services)."
                .to_string(),
        );
    }

    format_section("Setup commands", lines)
}

fn build_code_style_section(analysis: &ProjectAnalysis) -> Option<String> {
    let mut lines = Vec::new();

    for language in unique_preserving_order(&analysis.languages) {
        match language.as_str() {
            "Rust" => lines.push(
                "- Rust: 4-space indentation, snake_case functions, PascalCase types, run `cargo fmt` and `cargo clippy`."
                    .to_string(),
            ),
            "JavaScript/TypeScript" => lines.push(
                "- JavaScript/TypeScript: prefer functional patterns, run your formatter (Prettier) and ESLint checks."
                    .to_string(),
            ),
            "Python" => lines.push(
                "- Python: follow PEP 8, use Black for formatting, and enforce type hints where practical.".to_string(),
            ),
            "Go" => lines.push(
                "- Go: rely on `gofmt` and `go vet`, keep packages focused, and avoid unused exports.".to_string(),
            ),
            "Java/Kotlin" => lines.push(
                "- Java/Kotlin: follow existing code conventions, run `./gradlew spotlessApply` or equivalent formatters.".to_string(),
            ),
            other => lines.push(format!(
                "- {}: match existing style guides and run the project formatter/linter before committing.",
                other
            )),
        }
    }

    if lines.is_empty() {
        lines.push(
            "- Align new code with the surrounding style and add formatting details here."
                .to_string(),
        );
    }

    format_section("Code style", lines)
}

fn build_testing_section(analysis: &ProjectAnalysis) -> Option<String> {
    let mut lines = Vec::new();
    let systems = unique_preserving_order(&analysis.build_systems);

    for system in systems {
        match system.as_str() {
            "Cargo" => lines.push("- Run Rust tests with `cargo test` and address clippy warnings.".to_string()),
            "npm/yarn/pnpm" => lines.push(
                "- Execute JavaScript/TypeScript suites with `pnpm test -- --run` or the configured test script.".to_string(),
            ),
            "pip/poetry" => lines.push(
                "- Run Python checks via `pytest` and lint with `ruff` or `flake8` if configured.".to_string(),
            ),
            "Go Modules" => lines.push("- Execute Go tests using `go test ./...` and review race detector output.".to_string()),
            "Maven/Gradle" => lines.push(
                "- Run JVM suites with `./gradlew test` or `mvn test` before submitting changes.".to_string(),
            ),
            other => lines.push(format!(
                "- Ensure test workflows for `{other}` run locally before opening a PR."
            )),
        }
    }

    if !analysis.test_patterns.is_empty() {
        lines.push(format!(
            "- Test file patterns observed: {}.",
            unique_preserving_order(&analysis.test_patterns).join(", ")
        ));
    }

    if analysis.has_ci_cd {
        lines.push(
            "- Match CI expectations; replicate workflows from `.github/workflows` when possible."
                .to_string(),
        );
    }

    if lines.is_empty() {
        lines.push("- Describe how to run tests and quality checks for this project.".to_string());
    }

    format_section("Testing instructions", lines)
}

fn build_pr_section(analysis: &ProjectAnalysis) -> Option<String> {
    let mut lines = Vec::new();

    if analysis
        .commit_patterns
        .iter()
        .any(|pattern| pattern == "Conventional Commits")
    {
        lines.push(
            "- Use Conventional Commits (`type(scope): subject`) and keep summaries under 72 characters.".to_string(),
        );
    } else {
        lines.push(
            "- Write descriptive, imperative commit messages and group related changes together."
                .to_string(),
        );
    }

    lines
        .push("- Reference issues with `Fixes #123` or `Closes #123` when applicable.".to_string());
    lines.push(
        "- Run linters and test suites before opening a pull request; attach logs for failures."
            .to_string(),
    );
    lines.push(
        "- Keep pull requests focused; split large features into reviewable chunks.".to_string(),
    );

    format_section("PR instructions", lines)
}

fn build_additional_notes_section(analysis: &ProjectAnalysis) -> Option<String> {
    let mut lines = Vec::new();

    if !analysis.documentation_files.is_empty() {
        let docs = unique_preserving_order(&analysis.documentation_files);
        lines.push(format!(
            "- Additional documentation available in: {}.",
            docs.join(", ")
        ));
    }

    if !analysis.dependencies.is_empty() {
        let mut highlighted = Vec::new();
        for (ecosystem, deps) in &analysis.dependencies {
            if !deps.is_empty() {
                let preview: Vec<&String> = deps.iter().take(3).collect();
                highlighted.push(format!(
                    "{} dependencies include {}{}",
                    ecosystem,
                    preview
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    if deps.len() > preview.len() {
                        " (see manifest for more)"
                    } else {
                        ""
                    }
                ));
            }
        }

        for line in highlighted {
            lines.push(format!("- {line}"));
        }
    }

    if lines.is_empty() {
        return None;
    }

    format_section("Additional context", lines)
}

fn unique_preserving_order(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for value in values {
        if seen.insert(value.clone()) {
            result.push(value.clone());
        }
    }

    result
}

fn format_section(title: &str, lines: Vec<String>) -> Option<String> {
    if lines.is_empty() {
        return None;
    }

    let mut section = String::new();
    section.push_str("## ");
    section.push_str(title);
    section.push_str("\n\n");

    for mut line in lines {
        if !line.starts_with('-') {
            line = format!("- {line}");
        }
        section.push_str(&line);
        if !line.ends_with('\n') {
            section.push('\n');
        }
    }

    section.push('\n');
    Some(section)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    fn base_analysis() -> ProjectAnalysis {
        ProjectAnalysis {
            project_name: "demo".to_string(),
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
        }
    }

    #[test]
    fn generates_agents_sections_from_analysis() {
        let mut analysis = base_analysis();
        analysis.languages = vec!["Rust".to_string()];
        analysis.build_systems = vec!["Cargo".to_string()];
        analysis.source_dirs = vec!["src".to_string(), "tests".to_string()];
        analysis.documentation_files = vec!["README.md".to_string()];
        analysis.commit_patterns = vec!["Conventional Commits".to_string()];
        analysis.has_ci_cd = true;
        analysis.dependencies.insert(
            "Rust (Cargo)".to_string(),
            vec!["anyhow".to_string(), "serde".to_string()],
        );

        let output = generate_agents_md(&analysis).expect("expected agents.md output");

        assert!(output.contains("# AGENTS.md"));
        assert!(output.contains("## Setup commands"));
        assert!(output.contains("Rust: 4-space indentation"));
        assert!(output.contains("Use Conventional Commits"));
        assert!(output.contains("Additional documentation available"));
    }

    #[test]
    fn fills_placeholders_when_data_missing() {
        let analysis = base_analysis();
        let output = generate_agents_md(&analysis).expect("expected placeholder output");

        assert!(output.contains("Document project-specific setup commands"));
        assert!(output.contains("Align new code with the surrounding style"));
        assert!(output.contains("Describe how to run tests"));
    }
}
