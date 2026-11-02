use super::*;

pub(super) fn analyze_project(workspace: &Path) -> Result<ProjectAnalysis> {
    let root_files = collect_workspace_files(workspace)?;
    let package_manager = detect_package_manager(workspace);
    let project_name = workspace
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project")
        .to_owned();

    let mut analysis = ProjectAnalysis {
        project_name,
        grounded_project_summary: None,
        languages: Vec::new(),
        build_systems: Vec::new(),
        scripts: Vec::new(),
        dependencies: IndexMap::new(),
        source_dirs: Vec::new(),
        config_files: Vec::new(),
        documentation_files: Vec::new(),
        commit_patterns: Vec::new(),
        has_git_history: false,
        is_library: false,
        is_application: false,
        has_ci_cd: false,
        has_docker: false,
        package_manager,
        verification_candidates: Vec::new(),
        orientation_candidates: Vec::new(),
        critical_instruction_candidates: Vec::new(),
        selected_verification_command: None,
        selected_orientation_doc: None,
        selected_critical_instruction: None,
        grounded_verification_command: None,
        grounded_orientation_doc: None,
        grounded_critical_instruction: None,
    };

    for path in &root_files {
        analyze_file(&mut analysis, path);
    }

    load_dependency_signals(workspace, &mut analysis)?;
    analyze_git_history(workspace, &mut analysis);
    analyze_project_characteristics(&mut analysis);

    let text_samples = load_text_samples(workspace, &root_files)?;
    analysis.verification_candidates = build_verification_candidates(workspace, &analysis, &text_samples);
    analysis.selected_verification_command =
        choose_clear_candidate(&analysis.verification_candidates, 4).map(|candidate| candidate.value.clone());

    analysis.orientation_candidates = build_orientation_candidates(&analysis, &text_samples);
    analysis.selected_orientation_doc =
        choose_clear_candidate(&analysis.orientation_candidates, 4).map(|candidate| candidate.value.clone());

    analysis.critical_instruction_candidates = build_critical_instruction_candidates(&text_samples);
    analysis.selected_critical_instruction =
        choose_clear_candidate(&analysis.critical_instruction_candidates, 7).map(|candidate| candidate.value.clone());

    Ok(analysis)
}

fn collect_workspace_files(workspace: &Path) -> Result<Vec<String>> {
    let mut files = BTreeSet::new();
    for entry in build_default_walker(workspace)
        .max_depth(Some(MAX_SCAN_DEPTH))
        .filter_entry(|entry| !is_excluded_dir(entry))
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_some_and(|ft| ft.is_file()))
    {
        let relative = entry
            .path()
            .strip_prefix(workspace)
            .with_context(|| format!("failed to relativize {}", entry.path().display()))?;
        files.insert(relative.to_string_lossy().replace('\\', "/"));
    }
    Ok(files.into_iter().collect())
}

fn analyze_file(analysis: &mut ProjectAnalysis, path: &str) {
    match path {
        "Cargo.toml" => {
            analysis.languages.push("Rust".to_owned());
            analysis.build_systems.push("Cargo".to_owned());
        }
        "Cargo.lock" => {
            analysis.config_files.push("Cargo.lock".to_owned());
        }
        "package.json" => {
            analysis.languages.push("JavaScript/TypeScript".to_owned());
            analysis.build_systems.push("npm/yarn/pnpm".to_owned());
        }
        "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" => {
            analysis.config_files.push(path.to_owned());
        }
        "requirements.txt" | "pyproject.toml" | "setup.py" | "Pipfile" => {
            analysis.languages.push("Python".to_owned());
            analysis.build_systems.push("pip/poetry".to_owned());
            analysis.config_files.push(path.to_owned());
        }
        "go.mod" | "go.sum" => {
            analysis.languages.push("Go".to_owned());
            analysis.build_systems.push("Go Modules".to_owned());
            analysis.config_files.push(path.to_owned());
        }
        "pom.xml" | "build.gradle" | "build.gradle.kts" => {
            analysis.languages.push("Java/Kotlin".to_owned());
            analysis.build_systems.push("Maven/Gradle".to_owned());
            analysis.config_files.push(path.to_owned());
        }
        "README.md"
        | "CHANGELOG.md"
        | "CONTRIBUTING.md"
        | "LICENSE"
        | "LICENSE.md"
        | "AGENTS.md"
        | "AGENT.md"
        | "docs/README.md"
        | "docs/ARCHITECTURE.md"
        | "docs/modules/vtcode_docs_map.md" => {
            analysis.documentation_files.push(path.to_owned());
        }
        ".gitignore" | ".editorconfig" | ".prettierrc" | ".eslintrc" | ".eslintrc.js" | ".eslintrc.json"
        | "vtcode.toml" | "sgconfig.yml" => {
            analysis.config_files.push(path.to_owned());
        }
        "Dockerfile" | "docker-compose.yml" | "docker-compose.yaml" | ".dockerignore" => {
            analysis.config_files.push(path.to_owned());
        }
        path if path.starts_with(".github/workflows/") => {
            analysis.config_files.push(path.to_owned());
        }
        path if path.starts_with("scripts/") && path.ends_with(".sh") => {
            analysis.scripts.push(path.to_owned());
        }
        "run.sh" | "run-debug.sh" | "run-dev.sh" | "run-prod.sh" => {
            analysis.scripts.push(path.to_owned());
        }
        path if path.starts_with("src/")
            || path.starts_with("tests/")
            || path.starts_with("lib/")
            || path.starts_with("app/")
            || path.starts_with("cmd/")
            || path.starts_with("core/") =>
        {
            if let Some(root) = path.split('/').next() {
                analysis.source_dirs.push(root.to_owned());
            }
        }
        _ => {}
    }
}

fn load_dependency_signals(workspace: &Path, analysis: &mut ProjectAnalysis) -> Result<()> {
    let cargo_path = workspace.join("Cargo.toml");
    if cargo_path.exists()
        && let Some(content) = read_text_file(&cargo_path)?
    {
        extract_cargo_dependencies(analysis, &content);
    }

    let package_path = workspace.join("package.json");
    if package_path.exists()
        && let Some(content) = read_text_file(&package_path)?
    {
        extract_package_dependencies(analysis, &content);
    }

    Ok(())
}

fn read_text_file(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let truncated = bytes.into_iter().take(MAX_FILE_BYTES).collect::<Vec<_>>();
    Ok(Some(String::from_utf8_lossy(&truncated).into_owned()))
}

fn extract_cargo_dependencies(analysis: &mut ProjectAnalysis, content: &str) {
    let Ok(value) = content.parse::<toml::Value>() else {
        return;
    };

    let mut deps = Vec::new();
    for key in ["dependencies", "dev-dependencies", "workspace.dependencies"] {
        let Some(table) = lookup_toml_table(&value, key) else {
            continue;
        };
        deps.extend(table.keys().cloned());
    }

    if !deps.is_empty() {
        deps.sort();
        deps.dedup();
        analysis
            .dependencies
            .insert("Rust (Cargo)".to_owned(), deps.into_iter().take(8).collect());
    }
}

fn lookup_toml_table<'a>(value: &'a toml::Value, dotted_key: &str) -> Option<&'a toml::map::Map<String, toml::Value>> {
    let mut current = value;
    for part in dotted_key.split('.') {
        current = current.get(part)?;
    }
    current.as_table()
}

fn extract_package_dependencies(analysis: &mut ProjectAnalysis, content: &str) {
    let Ok(value) = serde_json::from_str::<JsonValue>(content) else {
        return;
    };

    let mut deps = Vec::new();
    for key in ["dependencies", "devDependencies"] {
        let Some(map) = value.get(key).and_then(|value| value.as_object()) else {
            continue;
        };
        deps.extend(map.keys().cloned());
    }

    if !deps.is_empty() {
        deps.sort();
        deps.dedup();
        analysis
            .dependencies
            .insert("JavaScript/TypeScript".to_owned(), deps.into_iter().take(8).collect());
    }
}

fn detect_package_manager(workspace: &Path) -> Option<PackageManager> {
    if workspace.join("pnpm-lock.yaml").exists() {
        Some(PackageManager::Pnpm)
    } else if workspace.join("yarn.lock").exists() {
        Some(PackageManager::Yarn)
    } else if workspace.join("package-lock.json").exists() || workspace.join("package.json").exists() {
        Some(PackageManager::Npm)
    } else {
        None
    }
}

fn analyze_git_history(workspace: &Path, analysis: &mut ProjectAnalysis) {
    if !workspace.join(".git").exists() {
        analysis.commit_patterns.push("No version control detected".to_owned());
        return;
    }

    analysis.has_git_history = true;
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["log", "--pretty=format:%s", "-20"])
        .output();

    let Ok(output) = output else {
        analysis.commit_patterns.push("Standard commit messages".to_owned());
        return;
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut conventional = 0usize;
    let mut total = 0usize;
    for line in stdout.lines() {
        total += 1;
        let line = line.trim();
        if ["feat:", "fix:", "docs:", "style:", "refactor:", "test:", "chore:"]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        {
            conventional += 1;
        }
    }

    if total > 0 && conventional * 100 / total > 50 {
        analysis.commit_patterns.push("Conventional Commits".to_owned());
    } else {
        analysis.commit_patterns.push("Standard commit messages".to_owned());
    }
}

fn analyze_project_characteristics(analysis: &mut ProjectAnalysis) {
    analysis.languages = unique_preserving_order(&analysis.languages);
    analysis.build_systems = unique_preserving_order(&analysis.build_systems);
    analysis.scripts = unique_preserving_order(&analysis.scripts);
    analysis.source_dirs = unique_preserving_order(&analysis.source_dirs);
    analysis.config_files = unique_preserving_order(&analysis.config_files);
    analysis.documentation_files = unique_preserving_order(&analysis.documentation_files);

    analysis.is_library = analysis
        .build_systems
        .iter()
        .any(|system| matches!(system.as_str(), "Cargo" | "npm/yarn/pnpm" | "pip/poetry" | "Go Modules"));
    analysis.is_application = analysis
        .source_dirs
        .iter()
        .any(|dir| matches!(dir.as_str(), "src" | "app" | "cmd"));
    analysis.has_ci_cd = analysis.config_files.iter().any(|path| path.starts_with(".github/workflows/"));
    analysis.has_docker = analysis.config_files.iter().any(|path| {
        matches!(path.as_str(), "Dockerfile" | "docker-compose.yml" | "docker-compose.yaml" | ".dockerignore")
    });
}

fn load_text_samples(workspace: &Path, files: &[String]) -> Result<BTreeMap<String, String>> {
    let mut samples = BTreeMap::new();
    for path in files {
        if !is_text_sample_candidate(path) {
            continue;
        }
        let absolute = workspace.join(path);
        if let Some(content) = read_text_file(&absolute)? {
            samples.insert(path.clone(), content);
        }
    }
    Ok(samples)
}

fn is_text_sample_candidate(path: &str) -> bool {
    matches!(
        path,
        "README.md"
            | "CONTRIBUTING.md"
            | "AGENTS.md"
            | "package.json"
            | "docs/README.md"
            | "docs/ARCHITECTURE.md"
            | "docs/modules/vtcode_docs_map.md"
            | "scripts/README.md"
    ) || path.starts_with(".github/workflows/")
}

fn build_verification_candidates(
    workspace: &Path,
    analysis: &ProjectAnalysis,
    text_samples: &BTreeMap<String, String>,
) -> Vec<SignalCandidate> {
    let mut candidates = CandidateAccumulator::default();
    let package_manager = analysis.package_manager.map(PackageManager::command);

    if workspace.join("scripts/check.sh").exists() {
        candidates.add("./scripts/check.sh", "./scripts/check.sh", "Run the repository quality gate script.", 7);
    }

    if analysis.build_systems.iter().any(|system| system == "Cargo") {
        candidates.add("cargo check", "cargo check", "Run a fast Rust compile check.", 2);
        candidates.add("cargo nextest run", "cargo nextest run", "Run the Rust test suite with nextest.", 1);
    }

    if analysis.build_systems.iter().any(|system| system == "Go Modules") {
        candidates.add("go test ./...", "go test ./...", "Run the Go test suite.", 4);
    }

    if let Some(command) = package_manager {
        let package_scripts = text_samples
            .get("package.json")
            .and_then(|content| parse_package_json_scripts(content.as_str()))
            .unwrap_or_default();
        if package_scripts.contains(&"test".to_owned()) {
            candidates.add(format!("{command} test"), format!("{command} test"), "Run the package test script.", 5);
        }
        if package_scripts.contains(&"check".to_owned()) {
            candidates.add(
                format!("{command} run check"),
                format!("{command} run check"),
                "Run the package verification script.",
                5,
            );
        }
    }

    for content in text_samples.values() {
        for (command, label, description, score) in [
            ("./scripts/check.sh", "./scripts/check.sh", "Run the repository quality gate script.", 3),
            ("cargo nextest run", "cargo nextest run", "Run the Rust test suite with nextest.", 4),
            ("cargo check", "cargo check", "Run a fast Rust compile check.", 2),
            ("cargo test", "cargo test", "Run the Rust test suite.", 3),
            (
                "cargo clippy --workspace --all-targets -- -D warnings",
                "cargo clippy --workspace --all-targets -- -D warnings",
                "Run the strict Rust linter.",
                4,
            ),
            ("go test ./...", "go test ./...", "Run the Go test suite.", 3),
        ] {
            if content.contains(command) {
                candidates.add(command, label, description, score);
            }
        }

        if let Some(command) = package_manager {
            for suffix in ["test", "run check", "run lint"] {
                let candidate = format!("{command} {suffix}");
                if content.contains(&candidate) {
                    candidates.add(
                        candidate.clone(),
                        candidate.clone(),
                        "Run the JavaScript/TypeScript verification script.",
                        3,
                    );
                }
            }
        }
    }

    candidates.into_sorted()
}

fn parse_package_json_scripts(content: &str) -> Option<Vec<String>> {
    let value = serde_json::from_str::<JsonValue>(content).ok()?;
    let scripts = value.get("scripts")?.as_object()?;
    Some(scripts.keys().cloned().collect())
}

fn build_orientation_candidates(
    analysis: &ProjectAnalysis,
    text_samples: &BTreeMap<String, String>,
) -> Vec<SignalCandidate> {
    let mut candidates = CandidateAccumulator::default();

    for (path, description, score) in [
        ("README.md", "Start here for repository overview and local setup.", 4),
        ("docs/ARCHITECTURE.md", "Use this for system design and architecture context.", 5),
        ("docs/modules/vtcode_docs_map.md", "Use this to map VT Code modules and docs quickly.", 5),
        ("docs/README.md", "Use this to browse project documentation.", 3),
        ("CONTRIBUTING.md", "Use this for contribution workflow and repo expectations.", 2),
    ] {
        if analysis.documentation_files.iter().any(|file| file == path) {
            candidates.add(path, path, description, score);
        }
    }

    for content in text_samples.values() {
        for (path, description, score) in [
            ("docs/modules/vtcode_docs_map.md", "Use this to map VT Code modules and docs quickly.", 4),
            ("docs/ARCHITECTURE.md", "Use this for system design and architecture context.", 2),
            ("README.md", "Start here for repository overview and local setup.", 1),
        ] {
            if content.contains(path) {
                candidates.add(path, path, description, score);
            }
        }
    }

    candidates.into_sorted()
}

fn build_critical_instruction_candidates(text_samples: &BTreeMap<String, String>) -> Vec<SignalCandidate> {
    let mut candidates = CandidateAccumulator::default();
    for (path, content) in text_samples {
        let base_score = match path.as_str() {
            "AGENTS.md" => 5,
            "CONTRIBUTING.md" => 4,
            "README.md" => 3,
            _ => 2,
        };

        for raw_line in content.lines().take(400) {
            let Some(line) = normalized_instruction_line(raw_line) else {
                continue;
            };
            let lower = line.to_ascii_lowercase();
            let description = format!("Inferred from {path}.");

            if lower.contains("conventional commit") {
                candidates.add(
                    "Use Conventional Commits (`type(scope): subject`).",
                    "Use Conventional Commits (`type(scope): subject`).",
                    description.clone(),
                    base_score + 4,
                );
            }

            if lower.contains("no unsafe") || lower.contains("do not use unsafe") || lower.contains("never use unsafe")
            {
                candidates.add(
                    "Do not use `unsafe` code.",
                    "Do not use `unsafe` code.",
                    description.clone(),
                    base_score + 5,
                );
            }

            if lower.contains("cargo check") && lower.contains("cargo nextest") && lower.contains("cargo clippy") {
                candidates.add(
                    "Run `cargo check`, `cargo nextest`, and `cargo clippy` after changes.",
                    "Run `cargo check`, `cargo nextest`, and `cargo clippy` after changes.",
                    description.clone(),
                    base_score + 4,
                );
            }

            if lower.contains("keep changes surgical") {
                candidates.add(
                    "Keep changes surgical and avoid unrelated cleanup.",
                    "Keep changes surgical and avoid unrelated cleanup.",
                    description.clone(),
                    base_score + 3,
                );
            }

            if lower.contains("vt code") && lower.contains("capitalization") {
                candidates.add(
                    "Use the product name `VT Code` with proper capitalization and spacing.",
                    "Use the product name `VT Code` with proper capitalization and spacing.",
                    description,
                    base_score + 3,
                );
            }
        }
    }

    candidates.into_sorted()
}

fn normalized_instruction_line(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("```") || trimmed.starts_with('|') {
        return None;
    }

    let trimmed = trimmed
        .trim_start_matches(['-', '*', ' ', '\t'])
        .trim_start_matches(|ch: char| ch.is_ascii_digit() || matches!(ch, '.' | ')' | ' '));
    let trimmed = trimmed.trim();
    if trimmed.len() < 18 || trimmed.len() > 140 {
        return None;
    }
    Some(trimmed.to_owned())
}

fn choose_clear_candidate(candidates: &[SignalCandidate], minimum_score: u32) -> Option<&SignalCandidate> {
    let first = candidates.first()?;
    if first.score < minimum_score {
        return None;
    }
    if candidates.len() == 1 {
        return Some(first);
    }
    if first.score >= candidates[1].score + CLEAR_SCORE_GAP {
        return Some(first);
    }
    None
}

impl ProjectAnalysis {
    pub(super) fn apply_grounding(&mut self, grounding: GuidedInitGrounding) {
        self.grounded_project_summary = normalize_grounding_value(grounding.project_summary);
        self.grounded_verification_command = normalize_grounding_value(grounding.verification_command);
        self.grounded_orientation_doc = normalize_grounding_value(grounding.orientation_doc);
        self.grounded_critical_instruction = normalize_grounding_value(grounding.critical_instruction);
    }
}

fn normalize_grounding_value(value: Option<String>) -> Option<String> {
    value.map(|value| value.trim().to_owned()).filter(|value| !value.is_empty())
}
