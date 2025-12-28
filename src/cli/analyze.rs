use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::utils::colors::style;
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisType {
    Full,
    Structure,
    Security,
    Performance,
    Dependencies,
    Complexity,
}

impl AnalysisType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "full" => Some(Self::Full),
            "structure" => Some(Self::Structure),
            "security" => Some(Self::Security),
            "performance" => Some(Self::Performance),
            "dependencies" => Some(Self::Dependencies),
            "complexity" => Some(Self::Complexity),
            _ => None,
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Full => "Comprehensive analysis including structure, dependencies, and metrics",
            Self::Structure => "File structure and language distribution",
            Self::Security => "Security vulnerability patterns and best practices",
            Self::Performance => "Performance bottlenecks and optimization opportunities",
            Self::Dependencies => "Dependency analysis and version management",
            Self::Complexity => "Code complexity metrics and maintainability",
        }
    }
}

/// Handle the analyze command
pub async fn handle_analyze_command(
    config: &CoreAgentConfig,
    analysis_type: AnalysisType,
) -> Result<()> {
    println!("{}", style("[ANALYZE]").blue().bold());
    println!("  {:16} {}", "workspace", config.workspace.display());
    println!("  {:16} {}\n", "type", analysis_type.description());

    // Workspace analysis implementation
    analyze_workspace(&config.workspace, analysis_type).await?;

    Ok(())
}

/// Analyze the workspace and provide insights
async fn analyze_workspace(workspace_path: &Path, analysis_type: AnalysisType) -> Result<()> {
    match analysis_type {
        AnalysisType::Structure => analyze_structure(workspace_path).await?,
        AnalysisType::Security => analyze_security(workspace_path).await?,
        AnalysisType::Performance => analyze_performance(workspace_path).await?,
        AnalysisType::Dependencies => analyze_dependencies(workspace_path).await?,
        AnalysisType::Complexity => analyze_complexity(workspace_path).await?,
        AnalysisType::Full => {
            analyze_structure(workspace_path).await?;
            analyze_dependencies(workspace_path).await?;
            analyze_complexity(workspace_path).await?;
            analyze_security(workspace_path).await?;
            analyze_performance(workspace_path).await?;
        }
    }

    Ok(())
}

/// Analyze workspace structure
async fn analyze_structure(workspace_path: &Path) -> Result<()> {
    println!("{}", style("Structure Analysis").bold());

    let mut total_files = 0;
    let mut total_dirs = 0;
    let mut language_files: HashMap<String, usize> = HashMap::new();
    let mut total_size = 0u64;
    let mut max_depth = 0;

    for entry in WalkDir::new(workspace_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let depth = entry.depth();
        if depth > max_depth {
            max_depth = depth;
        }

        if entry.file_type().is_dir() {
            total_dirs += 1;
        } else if entry.file_type().is_file() {
            total_files += 1;

            // Count files by extension
            let entry_path = entry.path().to_path_buf();
            if let Some(ext) = entry_path.extension().and_then(|e| e.to_str()) {
                *language_files.entry(ext.to_string()).or_insert(0) += 1;
            }

            // Calculate total size
            if let Ok(metadata) = entry.metadata() {
                total_size += metadata.len();
            }
        }
    }

    println!(
        "  {:<20} {}",
        "Directories:",
        style(&total_dirs.to_string()).cyan()
    );
    println!(
        "  {:<20} {}",
        "Files:",
        style(&total_files.to_string()).cyan()
    );
    println!(
        "  {:<20} {}",
        "Max depth:",
        style(&max_depth.to_string()).cyan()
    );
    println!(
        "  {:<20} {}",
        "Total size:",
        style(&format_size(total_size)).cyan()
    );
    println!();

    // Show language distribution
    if !language_files.is_empty() {
        println!("  {}", style("Language distribution:").bold());
        let mut langs: Vec<_> = language_files.iter().collect();
        langs.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

        let total = total_files as f64;
        for (i, (lang, count)) in langs.iter().take(15).enumerate() {
            let percentage = (**count as f64 / total) * 100.0;
            let bar = generate_bar(percentage, 20);
            println!(
                "  {:>2}. {:<12} {:>4} files [{:>5.1}%] {}",
                i + 1,
                lang,
                count,
                percentage,
                bar
            );
        }
        println!();
    }

    // Show key directories
    let key_dirs = find_key_directories(workspace_path);
    if !key_dirs.is_empty() {
        println!("  {}", style("Key directories:").bold());
        for (name, path) in key_dirs.iter().take(10) {
            println!("  • {} → {}", name, path.display());
        }
        println!();
    }

    Ok(())
}

/// Analyze security patterns
async fn analyze_security(workspace_path: &Path) -> Result<()> {
    println!("{}", style("Security Analysis").bold());

    let mut findings = Vec::new();

    // Check for common security-related files
    let security_files = vec![
        (".env", "Environment variables"),
        (".gitignore", "Git ignore rules"),
        ("Dockerfile", "Docker configuration"),
        ("SECURITY.md", "Security policy"),
        (".vtcodegitignore", "VT Code ignore rules"),
    ];

    println!("  {}", style("Security-related files:").bold());
    for (filename, description) in security_files {
        let path = workspace_path.join(filename);
        if path.exists() {
            let status = if filename == ".env" {
                style("[Warning] EXISTS (check for secrets)").yellow()
            } else {
                style("✓ EXISTS").green()
            };
            println!("  • {}: {}", description, status);
        } else {
            println!("  • {}: {}", description, style("✗ MISSING").red());
        }
    }
    println!();

    // Scan for potential secret patterns
    findings.extend(scan_for_secrets(workspace_path).await?);

    if !findings.is_empty() {
        println!("  {}", style("Potential security issues:").bold().yellow());
        for finding in findings.iter().take(10) {
            println!("  [Warning] {}", finding);
        }
        if findings.len() > 10 {
            println!("  ... and {} more issues", findings.len() - 10);
        }
        println!();
    } else {
        println!(
            "  {}",
            style("✓ No obvious secrets found in codebase").green()
        );
        println!();
    }

    Ok(())
}

/// Analyze performance patterns
async fn analyze_performance(workspace_path: &Path) -> Result<()> {
    println!("{}", style("Performance Analysis").bold());

    let mut suggestions = Vec::new();

    // Check for build configuration
    let build_files = vec![
        ("Cargo.toml", "Rust"),
        ("package.json", "Node.js"),
        ("pyproject.toml", "Python"),
        ("go.mod", "Go"),
        ("Makefile", "Make"),
    ];

    println!("  {}", style("Build configuration:").bold());
    for (filename, language) in build_files {
        let path = workspace_path.join(filename);
        if path.exists() {
            println!("  • {}: {}", language, style("✓ Configured").green());

            // Add performance-specific suggestions
            if filename == "Cargo.toml" {
                suggestions.push("Consider using cargo workspaces for faster builds");
            } else if filename == "package.json" {
                suggestions.push("Check for unused dependencies to reduce bundle size");
            }
        }
    }
    println!();

    // Check for CI/CD configuration
    let ci_configs = vec![
        (".github/workflows", "GitHub Actions"),
        (".gitlab-ci.yml", "GitLab CI"),
        ("Jenkinsfile", "Jenkins"),
        (".circleci", "CircleCI"),
    ];

    println!("  {}", style("CI/CD configuration:").bold());
    let mut has_ci = false;
    for (path_str, name) in ci_configs {
        let path = workspace_path.join(path_str);
        if path.exists() {
            println!("  • {}", name);
            has_ci = true;
        }
    }
    if !has_ci {
        println!("  {}", style("  No CI/CD configuration found").yellow());
    }
    println!();

    // Check for large files
    let large_files = find_large_files(workspace_path, 10 * 1024 * 1024).await?; // 10MB threshold
    if !large_files.is_empty() {
        println!("  {}", style("Large files (>10MB):").bold().yellow());
        for (path, size) in large_files.iter().take(5) {
            println!("  • {} ({})", path.display(), format_size(*size));
        }
        println!();
    }

    // Display suggestions
    if !suggestions.is_empty() {
        println!("  {}", style("Performance suggestions:").bold());
        for suggestion in suggestions {
            println!("  • {}", suggestion);
        }
        println!();
    }

    Ok(())
}

/// Analyze dependencies
async fn analyze_dependencies(workspace_path: &Path) -> Result<()> {
    println!("{}", style("Dependency Analysis").bold());

    // Check for dependency files
    let dep_files = vec![
        ("Cargo.toml", "Rust (Cargo)"),
        ("package.json", "Node.js (npm/yarn/pnpm)"),
        ("requirements.txt", "Python (pip)"),
        ("pyproject.toml", "Python (Poetry)"),
        ("go.mod", "Go (modules)"),
        ("Gemfile", "Ruby (Bundler)"),
        ("composer.json", "PHP (Composer)"),
        ("pom.xml", "Java (Maven)"),
        ("build.gradle", "Java (Gradle)"),
    ];

    println!("  {}", style("Dependency management files:").bold());
    let mut has_deps = false;
    for (filename, description) in dep_files {
        let path = workspace_path.join(filename);
        if path.exists() {
            println!("  • {}", description);
            has_deps = true;

            // Count dependencies if possible
            if let Ok(count) = count_dependencies(&path).await {
                println!("    └─ {} dependencies", count);
            }
        }
    }

    if !has_deps {
        println!("  {}", style("  No dependency files found").yellow());
    }
    println!();

    // Check for lock files
    let lock_files = vec![
        ("Cargo.lock", "Cargo lock file"),
        ("package-lock.json", "npm lock file"),
        ("yarn.lock", "Yarn lock file"),
        ("poetry.lock", "Poetry lock file"),
        ("go.sum", "Go sum file"),
        ("Gemfile.lock", "Bundler lock file"),
        ("composer.lock", "Composer lock file"),
    ];

    println!("  {}", style("Lock files (reproducible builds):").bold());
    let mut has_lock = false;
    for (filename, description) in lock_files {
        let path = workspace_path.join(filename);
        if path.exists() {
            println!("  • {}", description);
            has_lock = true;
        }
    }

    if !has_lock {
        println!("  {}", style("  No lock files found").yellow());
    }
    println!();

    Ok(())
}

/// Analyze code complexity
async fn analyze_complexity(workspace_path: &Path) -> Result<()> {
    println!("{}", style("Code Complexity Analysis").bold());

    let mut complexity_stats = HashMap::new();

    // Analyze source code files
    let source_extensions = vec!["rs", "py", "js", "ts", "go", "java", "cpp", "c", "swift"];

    for entry in WalkDir::new(workspace_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
            let ext_string = ext.to_string(); // Own the string to avoid lifetime issues
            if source_extensions.contains(&ext) {
                if let Ok(content) = tokio::fs::read_to_string(entry.path()).await {
                    let lines = content.lines().count();
                    let functions = content.matches("fn ").count()
                        + content.matches("func ").count()
                        + content.matches("def ").count();
                    let complexity = estimate_complexity(&content);

                    let stats = complexity_stats
                        .entry(ext_string)
                        .or_insert_with(|| (0, 0, 0, 0));
                    stats.0 += 1; // file count
                    stats.1 += lines; // total lines
                    stats.2 += functions; // total functions
                    stats.3 += complexity; // total complexity
                }
            }
        }
    }

    if !complexity_stats.is_empty() {
        println!("  {}", style("Complexity by language:").bold());
        for (lang, (files, lines, functions, complexity)) in complexity_stats.iter() {
            let avg_complexity = if *functions > 0 {
                *complexity / *functions
            } else {
                0
            };
            println!(
                "  • {}: {} files, {} lines, {} functions, avg complexity: {}",
                lang, files, lines, functions, avg_complexity
            );
        }
        println!();

        // Provide suggestions
        println!("  {}", style("Maintainability suggestions:").bold());
        for (lang, (_, lines, functions, _)) in complexity_stats.iter() {
            let avg_lines_per_function = if *functions > 0 {
                *lines / *functions
            } else {
                0
            };
            if avg_lines_per_function > 50 {
                println!(
                    "  • {}: Consider refactoring large functions (avg {} lines)",
                    lang, avg_lines_per_function
                );
            }
        }
        println!();
    } else {
        println!(
            "  {}",
            style("No source code files found for complexity analysis").yellow()
        );
        println!();
    }

    if !complexity_stats.is_empty() {
        println!("  {}", style("Complexity by language:").bold());
        for (lang, (files, lines, functions, complexity)) in complexity_stats.iter() {
            let avg_complexity = if *functions > 0 {
                *complexity / *functions
            } else {
                0
            };
            println!(
                "  • {}: {} files, {} lines, {} functions, avg complexity: {}",
                lang, files, lines, functions, avg_complexity
            );
        }
        println!();

        // Provide suggestions
        println!("  {}", style("Maintainability suggestions:").bold());
        for (lang, (_, lines, functions, _)) in complexity_stats.iter() {
            let avg_lines_per_function = if *functions > 0 {
                *lines / *functions
            } else {
                0
            };
            if avg_lines_per_function > 50 {
                println!(
                    "  • {}: Consider refactoring large functions (avg {} lines)",
                    lang, avg_lines_per_function
                );
            }
        }
        println!();
    } else {
        println!(
            "  {}",
            style("No source code files found for complexity analysis").yellow()
        );
        println!();
    }

    Ok(())
}

// Helper functions

fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.1} {}", size, UNITS[unit_index])
}

fn generate_bar(percentage: f64, width: usize) -> String {
    let filled = ((percentage / 100.0) * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

fn find_key_directories(workspace_path: &Path) -> Vec<(String, std::path::PathBuf)> {
    let mut key_dirs = Vec::new();
    let important_dirs = vec![
        ("src", "Source code"),
        ("lib", "Libraries"),
        ("tests", "Tests"),
        ("docs", "Documentation"),
        ("examples", "Examples"),
        ("benchmarks", "Benchmarks"),
        ("scripts", "Scripts"),
        (".github", "GitHub workflows"),
        (".vtcode", "VT Code"),
        ("target", "Build output"),
        ("node_modules", "Node modules"),
    ];

    for (dirname, description) in important_dirs {
        let path = workspace_path.join(dirname);
        if path.exists() && path.is_dir() {
            key_dirs.push((description.to_string(), path));
        }
    }

    key_dirs
}

async fn scan_for_secrets(workspace_path: &Path) -> Result<Vec<String>> {
    let mut findings = Vec::new();

    // Compile regex patterns once for efficiency
    let secret_patterns = vec![
        (
            regex::Regex::new(r#"(?i)(api_key|apikey|api-key).{0,20}["']?[A-Za-z0-9]{20,}["']?"#)
                .context("Failed to compile API key regex")?,
            "Potential API key",
        ),
        (
            regex::Regex::new(r#"(?i)(password|passwd|pwd).{0,20}["']?[^"'\s]{8,}["']?"#)
                .context("Failed to compile password regex")?,
            "Potential password",
        ),
        (
            regex::Regex::new(r#"(?i)sk-[A-Za-z0-9]{20,}"#)
                .context("Failed to compile secret key regex")?,
            "Potential secret key",
        ),
        (
            regex::Regex::new(r#"(?i)aws_.{0,20}["']?[A-Za-z0-9/+]{20,}["']?"#)
                .context("Failed to compile AWS credential regex")?,
            "Potential AWS credential",
        ),
    ];

    for entry in WalkDir::new(workspace_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        // Skip common binary and large files
        if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
            if matches!(
                ext,
                "bin"
                    | "png"
                    | "jpg"
                    | "jpeg"
                    | "gif"
                    | "ico"
                    | "pdf"
                    | "zip"
                    | "tar"
                    | "gz"
                    | "exe"
                    | "dll"
                    | "so"
                    | "dylib"
            ) {
                continue;
            }
        }

        // Skip large files to avoid performance issues
        if let Ok(metadata) = entry.metadata() {
            if metadata.len() > 10 * 1024 * 1024 {
                continue; // Skip files larger than 10MB
            }
        }

        if let Ok(content) = tokio::fs::read_to_string(entry.path()).await {
            for (pattern, description) in &secret_patterns {
                if pattern.is_match(&content) {
                    let relative_path = entry
                        .path()
                        .strip_prefix(workspace_path)
                        .unwrap_or(entry.path());
                    findings.push(format!("{} in {}", description, relative_path.display()));
                    break;
                }
            }
        }
    }

    // Deduplicate findings
    findings.sort();
    findings.dedup();

    Ok(findings)
}

async fn find_large_files(
    workspace_path: &Path,
    min_size: u64,
) -> Result<Vec<(std::path::PathBuf, u64)>> {
    let mut large_files = Vec::new();

    for entry in WalkDir::new(workspace_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        if let Ok(metadata) = entry.metadata() {
            let size = metadata.len();
            if size >= min_size {
                large_files.push((entry.path().to_path_buf(), size));
            }
        }
    }

    large_files.sort_by_key(|(_, size)| std::cmp::Reverse(*size));
    Ok(large_files)
}

async fn count_dependencies(path: &Path) -> Result<usize> {
    let content = tokio::fs::read_to_string(path).await?;

    if path.ends_with("Cargo.toml") {
        // Count [dependencies] section entries
        let deps_section = content.find("[dependencies]");
        let dev_deps_section = content.find("[dev-dependencies]");

        if let Some(start) = deps_section {
            let end = dev_deps_section.unwrap_or(content.len());
            let deps_text = &content[start..end.min(start + 1000)]; // Limit search
            Ok(deps_text
                .lines()
                .filter(|l| l.contains("=") && !l.trim().starts_with('#'))
                .count()
                .saturating_sub(1))
        } else {
            Ok(0)
        }
    } else if path.ends_with("package.json") {
        // Simple JSON parsing for dependencies
        Ok(content.matches("\"dependencies\"").count() * 15) // Rough estimate
    } else {
        Ok(0)
    }
}

fn estimate_complexity(content: &str) -> usize {
    // Simple complexity estimation based on control flow statements
    let mut complexity = 1;
    complexity += content.matches("if ").count();
    complexity += content.matches("else ").count();
    complexity += content.matches("match ").count();
    complexity += content.matches("for ").count();
    complexity += content.matches("while ").count();
    complexity += content.matches("loop ").count();
    complexity
}

// No extension traits needed
