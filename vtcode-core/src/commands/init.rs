//! Guided `/init` support for repository analysis and `AGENTS.md` generation.

use crate::tools::ToolRegistry;
use crate::utils::colors::style;
use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::{DirEntry, WalkDir};

const AGENTS_FILENAME: &str = "AGENTS.md";
const MAX_SCAN_DEPTH: usize = 4;
const MAX_FILE_BYTES: usize = 64 * 1024;
const CLEAR_SCORE_GAP: u32 = 3;
const CONTROL_GENERIC: &str = "__generic__";
const CONTROL_NONE: &str = "__none__";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
}

impl PackageManager {
    fn command(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
        }
    }
}

#[derive(Debug, Clone)]
struct SignalCandidate {
    value: String,
    label: String,
    description: String,
    score: u32,
}

#[derive(Debug, Default)]
struct CandidateAccumulator {
    values: BTreeMap<String, SignalCandidate>,
}

impl CandidateAccumulator {
    fn add(
        &mut self,
        value: impl Into<String>,
        label: impl Into<String>,
        description: impl Into<String>,
        score: u32,
    ) {
        let value = value.into();
        let normalized = value.trim();
        if normalized.is_empty() {
            return;
        }

        let label = label.into();
        let description = description.into();
        let entry = self
            .values
            .entry(normalized.to_owned())
            .or_insert_with(|| SignalCandidate {
                value: normalized.to_owned(),
                label,
                description,
                score: 0,
            });

        entry.score += score;
    }

    fn into_sorted(self) -> Vec<SignalCandidate> {
        let mut values: Vec<_> = self.values.into_values().collect();
        values.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.value.cmp(&right.value))
        });
        values
    }
}

#[derive(Debug, Clone)]
struct ProjectAnalysis {
    project_name: String,
    grounded_project_summary: Option<String>,
    languages: Vec<String>,
    build_systems: Vec<String>,
    scripts: Vec<String>,
    dependencies: IndexMap<String, Vec<String>>,
    source_dirs: Vec<String>,
    config_files: Vec<String>,
    documentation_files: Vec<String>,
    commit_patterns: Vec<String>,
    has_git_history: bool,
    is_library: bool,
    is_application: bool,
    has_ci_cd: bool,
    has_docker: bool,
    package_manager: Option<PackageManager>,
    verification_candidates: Vec<SignalCandidate>,
    orientation_candidates: Vec<SignalCandidate>,
    critical_instruction_candidates: Vec<SignalCandidate>,
    selected_verification_command: Option<String>,
    selected_orientation_doc: Option<String>,
    selected_critical_instruction: Option<String>,
    grounded_verification_command: Option<String>,
    grounded_orientation_doc: Option<String>,
    grounded_critical_instruction: Option<String>,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuidedInitGrounding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation_doc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub critical_instruction: Option<String>,
}

impl GuidedInitGrounding {
    #[must_use]
    pub fn has_any(&self) -> bool {
        self.project_summary.is_some()
            || self.verification_command.is_some()
            || self.orientation_doc.is_some()
            || self.critical_instruction.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuidedInitQuestionKey {
    VerificationCommand,
    OrientationDoc,
    CriticalInstruction,
}

impl GuidedInitQuestionKey {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VerificationCommand => "verification_command",
            Self::OrientationDoc => "orientation_doc",
            Self::CriticalInstruction => "critical_instruction",
        }
    }

    pub fn header(self) -> &'static str {
        match self {
            Self::VerificationCommand => "Verify",
            Self::OrientationDoc => "Orient",
            Self::CriticalInstruction => "Rule",
        }
    }

    pub fn custom_label(self) -> &'static str {
        match self {
            Self::VerificationCommand => "Custom command",
            Self::OrientationDoc => "Custom path",
            Self::CriticalInstruction => "Custom instruction",
        }
    }

    pub fn custom_placeholder(self) -> &'static str {
        match self {
            Self::VerificationCommand => "cargo nextest run",
            Self::OrientationDoc => "docs/ARCHITECTURE.md",
            Self::CriticalInstruction => "State the one rule agents should always follow",
        }
    }

    fn blank_custom_value(self) -> &'static str {
        match self {
            Self::CriticalInstruction => CONTROL_NONE,
            Self::VerificationCommand | Self::OrientationDoc => CONTROL_GENERIC,
        }
    }
}

impl std::str::FromStr for GuidedInitQuestionKey {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "verification_command" => Ok(Self::VerificationCommand),
            "orientation_doc" => Ok(Self::OrientationDoc),
            "critical_instruction" => Ok(Self::CriticalInstruction),
            other => anyhow::bail!("unknown guided init question key: {other}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuidedInitQuestionOption {
    pub value: String,
    pub label: String,
    pub description: String,
    pub recommended: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuidedInitQuestion {
    pub key: GuidedInitQuestionKey,
    pub header: String,
    pub prompt: String,
    pub options: Vec<GuidedInitQuestionOption>,
    pub allow_custom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuidedInitAnswer {
    pub key: GuidedInitQuestionKey,
    pub selected: String,
    pub custom: Option<String>,
}

impl GuidedInitAnswer {
    pub fn from_input(
        key: GuidedInitQuestionKey,
        selected: Option<&str>,
        custom: Option<&str>,
    ) -> Option<Self> {
        let custom_selected = custom.is_some();
        if let Some(custom) = custom.map(str::trim).filter(|value| !value.is_empty()) {
            return Some(Self {
                key,
                selected: String::new(),
                custom: Some(custom.to_owned()),
            });
        }

        if let Some(selected) = selected.map(str::trim).filter(|value| !value.is_empty()) {
            return Some(Self {
                key,
                selected: selected.to_owned(),
                custom: None,
            });
        }

        custom_selected.then(|| Self {
            key,
            selected: key.blank_custom_value().to_owned(),
            custom: None,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuidedInitAnswers {
    answers: BTreeMap<GuidedInitQuestionKey, GuidedInitAnswer>,
}

impl GuidedInitAnswers {
    pub fn insert(&mut self, answer: GuidedInitAnswer) {
        self.answers.insert(answer.key, answer);
    }

    pub fn answer(&self, key: GuidedInitQuestionKey) -> Option<&GuidedInitAnswer> {
        self.answers.get(&key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuidedInitOverwriteState {
    Skip,
    Confirm,
    Force,
}

impl GuidedInitOverwriteState {
    pub fn requires_confirmation(self) -> bool {
        matches!(self, Self::Confirm)
    }
}

#[derive(Debug, Clone)]
pub struct GuidedInitPlan {
    pub path: PathBuf,
    pub questions: Vec<GuidedInitQuestion>,
    pub overwrite_state: GuidedInitOverwriteState,
    analysis: ProjectAnalysis,
}

impl GuidedInitPlan {
    #[must_use]
    pub fn with_grounding(mut self, grounding: GuidedInitGrounding) -> Self {
        if !grounding.has_any() {
            return self;
        }

        self.analysis.apply_grounding(grounding);
        self.questions = build_guided_questions(&self.analysis);
        self
    }
}

/// Compatibility wrapper retained for older call sites.
pub async fn handle_init_command(_registry: &mut ToolRegistry, workspace: &Path) -> Result<()> {
    println!(
        "{}",
        style("Initializing project with AGENTS.md...")
            .cyan()
            .bold()
    );
    println!("{}", style("1. Analyzing project structure...").dim());
    let plan = prepare_guided_init(workspace, true)?;
    println!("{}", style("2. Rendering AGENTS.md content...").dim());
    let content = render_agents_md(&plan, &GuidedInitAnswers::default())?;
    println!("{}", style("3. Writing AGENTS.md file...").dim());
    let report = write_agents_file(workspace, &content, true)?;
    println!(
        "{} {}",
        style("[OK]").green().bold(),
        style("AGENTS.md generated successfully!").green()
    );
    println!("{} {}", style(" Location:").cyan(), report.path.display());
    Ok(())
}

/// Compatibility wrapper retained for older call sites.
pub async fn generate_agents_file(
    _registry: &mut ToolRegistry,
    workspace: &Path,
    overwrite: bool,
) -> Result<GenerateAgentsFileReport> {
    let plan = prepare_guided_init(workspace, overwrite)?;
    let content = render_agents_md(&plan, &GuidedInitAnswers::default())?;
    write_agents_file(workspace, &content, overwrite)
}

pub fn prepare_guided_init(workspace: &Path, force: bool) -> Result<GuidedInitPlan> {
    let analysis = analyze_project(workspace)?;
    let path = workspace.join(AGENTS_FILENAME);
    let overwrite_state = if force {
        GuidedInitOverwriteState::Force
    } else if path.exists() {
        GuidedInitOverwriteState::Confirm
    } else {
        GuidedInitOverwriteState::Skip
    };

    Ok(GuidedInitPlan {
        path,
        questions: build_guided_questions(&analysis),
        overwrite_state,
        analysis,
    })
}

pub fn render_agents_md(plan: &GuidedInitPlan, answers: &GuidedInitAnswers) -> Result<String> {
    let analysis = &plan.analysis;
    let verification_command = resolve_verification_command(analysis, answers);
    let orientation_doc = resolve_orientation_doc(analysis, answers);
    let critical_instruction = resolve_critical_instruction(analysis, answers);

    let mut content = String::new();
    content.push_str("# AGENTS.md\n\n");
    content.push_str(&build_quick_start_section(
        analysis,
        verification_command.as_deref(),
    ));
    content.push_str(&build_architecture_section(
        analysis,
        orientation_doc.as_deref(),
    ));
    if let Some(section) = build_important_instructions_section(critical_instruction.as_deref()) {
        content.push_str(&section);
    }
    content.push_str(&build_code_style_section(analysis));
    content.push_str(&build_testing_section(
        analysis,
        verification_command.as_deref(),
    ));
    content.push_str(&build_performance_section());
    if let Some(section) = build_pr_guidelines_section(analysis) {
        content.push_str(&section);
    }
    if let Some(section) = build_additional_guidance_section(analysis, orientation_doc.as_deref()) {
        content.push_str(&section);
    }

    Ok(content)
}

pub fn write_agents_file(
    workspace: &Path,
    content: &str,
    overwrite: bool,
) -> Result<GenerateAgentsFileReport> {
    let path = workspace.join(AGENTS_FILENAME);
    let existed_before = path.exists();

    if existed_before && !overwrite {
        return Ok(GenerateAgentsFileReport {
            path,
            status: GenerateAgentsFileStatus::SkippedExisting,
        });
    }

    fs::write(&path, content)
        .with_context(|| format!("failed to write AGENTS.md to {}", path.display()))?;

    let status = if existed_before {
        GenerateAgentsFileStatus::Overwritten
    } else {
        GenerateAgentsFileStatus::Created
    };

    Ok(GenerateAgentsFileReport { path, status })
}

fn analyze_project(workspace: &Path) -> Result<ProjectAnalysis> {
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
    analysis.verification_candidates =
        build_verification_candidates(workspace, &analysis, &text_samples);
    analysis.selected_verification_command =
        choose_clear_candidate(&analysis.verification_candidates, 4)
            .map(|candidate| candidate.value.clone());

    analysis.orientation_candidates = build_orientation_candidates(&analysis, &text_samples);
    analysis.selected_orientation_doc = choose_clear_candidate(&analysis.orientation_candidates, 4)
        .map(|candidate| candidate.value.clone());

    analysis.critical_instruction_candidates = build_critical_instruction_candidates(&text_samples);
    analysis.selected_critical_instruction =
        choose_clear_candidate(&analysis.critical_instruction_candidates, 7)
            .map(|candidate| candidate.value.clone());

    Ok(analysis)
}

fn collect_workspace_files(workspace: &Path) -> Result<Vec<String>> {
    let mut files = BTreeSet::new();
    for entry in WalkDir::new(workspace)
        .max_depth(MAX_SCAN_DEPTH)
        .into_iter()
        .filter_entry(|entry| !should_skip_entry(entry, workspace))
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
    {
        let relative = entry
            .path()
            .strip_prefix(workspace)
            .with_context(|| format!("failed to relativize {}", entry.path().display()))?;
        files.insert(relative.to_string_lossy().replace('\\', "/"));
    }
    Ok(files.into_iter().collect())
}

fn should_skip_entry(entry: &DirEntry, workspace: &Path) -> bool {
    if entry.path() == workspace {
        return false;
    }

    if !entry.file_type().is_dir() {
        return false;
    }

    entry
        .file_name()
        .to_str()
        .map(|name| {
            matches!(
                name,
                ".git" | "node_modules" | "target" | "dist" | ".next" | "vendor"
            )
        })
        .unwrap_or(false)
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
        ".gitignore" | ".editorconfig" | ".prettierrc" | ".eslintrc" | ".eslintrc.js"
        | ".eslintrc.json" | "vtcode.toml" | "sgconfig.yml" => {
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
        analysis.dependencies.insert(
            "Rust (Cargo)".to_owned(),
            deps.into_iter().take(8).collect(),
        );
    }
}

fn lookup_toml_table<'a>(
    value: &'a toml::Value,
    dotted_key: &str,
) -> Option<&'a toml::map::Map<String, toml::Value>> {
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
        analysis.dependencies.insert(
            "JavaScript/TypeScript".to_owned(),
            deps.into_iter().take(8).collect(),
        );
    }
}

fn detect_package_manager(workspace: &Path) -> Option<PackageManager> {
    if workspace.join("pnpm-lock.yaml").exists() {
        Some(PackageManager::Pnpm)
    } else if workspace.join("yarn.lock").exists() {
        Some(PackageManager::Yarn)
    } else if workspace.join("package-lock.json").exists()
        || workspace.join("package.json").exists()
    {
        Some(PackageManager::Npm)
    } else {
        None
    }
}

fn analyze_git_history(workspace: &Path, analysis: &mut ProjectAnalysis) {
    if !workspace.join(".git").exists() {
        analysis
            .commit_patterns
            .push("No version control detected".to_owned());
        return;
    }

    analysis.has_git_history = true;
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["log", "--pretty=format:%s", "-20"])
        .output();

    let Ok(output) = output else {
        analysis
            .commit_patterns
            .push("Standard commit messages".to_owned());
        return;
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut conventional = 0usize;
    let mut total = 0usize;
    for line in stdout.lines() {
        total += 1;
        let line = line.trim();
        if [
            "feat:",
            "fix:",
            "docs:",
            "style:",
            "refactor:",
            "test:",
            "chore:",
        ]
        .iter()
        .any(|prefix| line.starts_with(prefix))
        {
            conventional += 1;
        }
    }

    if total > 0 && conventional * 100 / total > 50 {
        analysis
            .commit_patterns
            .push("Conventional Commits".to_owned());
    } else {
        analysis
            .commit_patterns
            .push("Standard commit messages".to_owned());
    }
}

fn analyze_project_characteristics(analysis: &mut ProjectAnalysis) {
    analysis.languages = unique_preserving_order(&analysis.languages);
    analysis.build_systems = unique_preserving_order(&analysis.build_systems);
    analysis.scripts = unique_preserving_order(&analysis.scripts);
    analysis.source_dirs = unique_preserving_order(&analysis.source_dirs);
    analysis.config_files = unique_preserving_order(&analysis.config_files);
    analysis.documentation_files = unique_preserving_order(&analysis.documentation_files);

    analysis.is_library = analysis.build_systems.iter().any(|system| {
        matches!(
            system.as_str(),
            "Cargo" | "npm/yarn/pnpm" | "pip/poetry" | "Go Modules"
        )
    });
    analysis.is_application = analysis
        .source_dirs
        .iter()
        .any(|dir| matches!(dir.as_str(), "src" | "app" | "cmd"));
    analysis.has_ci_cd = analysis
        .config_files
        .iter()
        .any(|path| path.starts_with(".github/workflows/"));
    analysis.has_docker = analysis.config_files.iter().any(|path| {
        matches!(
            path.as_str(),
            "Dockerfile" | "docker-compose.yml" | "docker-compose.yaml" | ".dockerignore"
        )
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
        candidates.add(
            "./scripts/check.sh",
            "./scripts/check.sh",
            "Run the repository quality gate script.",
            7,
        );
    }

    if analysis
        .build_systems
        .iter()
        .any(|system| system == "Cargo")
    {
        candidates.add(
            "cargo check",
            "cargo check",
            "Run a fast Rust compile check.",
            2,
        );
        candidates.add(
            "cargo nextest run",
            "cargo nextest run",
            "Run the Rust test suite with nextest.",
            1,
        );
    }

    if analysis
        .build_systems
        .iter()
        .any(|system| system == "Go Modules")
    {
        candidates.add(
            "go test ./...",
            "go test ./...",
            "Run the Go test suite.",
            4,
        );
    }

    if let Some(command) = package_manager {
        let package_scripts = text_samples
            .get("package.json")
            .and_then(|content| parse_package_json_scripts(content.as_str()))
            .unwrap_or_default();
        if package_scripts.contains(&"test".to_owned()) {
            candidates.add(
                format!("{command} test"),
                format!("{command} test"),
                "Run the package test script.",
                5,
            );
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
            (
                "./scripts/check.sh",
                "./scripts/check.sh",
                "Run the repository quality gate script.",
                3,
            ),
            (
                "cargo nextest run",
                "cargo nextest run",
                "Run the Rust test suite with nextest.",
                4,
            ),
            (
                "cargo check",
                "cargo check",
                "Run a fast Rust compile check.",
                2,
            ),
            ("cargo test", "cargo test", "Run the Rust test suite.", 3),
            (
                "cargo clippy --workspace --all-targets -- -D warnings",
                "cargo clippy --workspace --all-targets -- -D warnings",
                "Run the strict Rust linter.",
                4,
            ),
            (
                "go test ./...",
                "go test ./...",
                "Run the Go test suite.",
                3,
            ),
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
        (
            "README.md",
            "Start here for repository overview and local setup.",
            4,
        ),
        (
            "docs/ARCHITECTURE.md",
            "Use this for system design and architecture context.",
            5,
        ),
        (
            "docs/modules/vtcode_docs_map.md",
            "Use this to map VT Code modules and docs quickly.",
            5,
        ),
        (
            "docs/README.md",
            "Use this to browse project documentation.",
            3,
        ),
        (
            "CONTRIBUTING.md",
            "Use this for contribution workflow and repo expectations.",
            2,
        ),
    ] {
        if analysis.documentation_files.iter().any(|file| file == path) {
            candidates.add(path, path, description, score);
        }
    }

    for content in text_samples.values() {
        for (path, description, score) in [
            (
                "docs/modules/vtcode_docs_map.md",
                "Use this to map VT Code modules and docs quickly.",
                4,
            ),
            (
                "docs/ARCHITECTURE.md",
                "Use this for system design and architecture context.",
                2,
            ),
            (
                "README.md",
                "Start here for repository overview and local setup.",
                1,
            ),
        ] {
            if content.contains(path) {
                candidates.add(path, path, description, score);
            }
        }
    }

    candidates.into_sorted()
}

fn build_critical_instruction_candidates(
    text_samples: &BTreeMap<String, String>,
) -> Vec<SignalCandidate> {
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
            let description = format!("Inferred from {}.", path);

            if lower.contains("conventional commit") {
                candidates.add(
                    "Use Conventional Commits (`type(scope): subject`).",
                    "Use Conventional Commits (`type(scope): subject`).",
                    description.clone(),
                    base_score + 4,
                );
            }

            if lower.contains("no unsafe")
                || lower.contains("do not use unsafe")
                || lower.contains("never use unsafe")
            {
                candidates.add(
                    "Do not use `unsafe` code.",
                    "Do not use `unsafe` code.",
                    description.clone(),
                    base_score + 5,
                );
            }

            if lower.contains("cargo check")
                && lower.contains("cargo nextest")
                && lower.contains("cargo clippy")
            {
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
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.starts_with("```")
        || trimmed.starts_with('|')
    {
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

fn choose_clear_candidate(
    candidates: &[SignalCandidate],
    minimum_score: u32,
) -> Option<&SignalCandidate> {
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
    fn apply_grounding(&mut self, grounding: GuidedInitGrounding) {
        self.grounded_project_summary = normalize_grounding_value(grounding.project_summary);
        self.grounded_verification_command =
            normalize_grounding_value(grounding.verification_command);
        self.grounded_orientation_doc = normalize_grounding_value(grounding.orientation_doc);
        self.grounded_critical_instruction =
            normalize_grounding_value(grounding.critical_instruction);
    }
}

fn normalize_grounding_value(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn build_guided_questions(analysis: &ProjectAnalysis) -> Vec<GuidedInitQuestion> {
    let mut questions = Vec::new();

    let verification_needs_question = analysis.selected_verification_command.is_none()
        || grounded_differs_from_selected(
            analysis.selected_verification_command.as_deref(),
            analysis.grounded_verification_command.as_deref(),
        );
    if verification_needs_question {
        let options = build_guided_options(
            analysis.grounded_verification_command.as_deref(),
            "Use the explorer-grounded verification command.",
            &analysis.verification_candidates,
            Some(GuidedInitQuestionOption {
                value: CONTROL_GENERIC.to_owned(),
                label: "Keep generic guidance".to_owned(),
                description: "Leave verification instructions generic for now.".to_owned(),
                recommended: analysis.verification_candidates.is_empty()
                    && analysis.grounded_verification_command.is_none(),
            }),
            3,
        );
        questions.push(GuidedInitQuestion {
            key: GuidedInitQuestionKey::VerificationCommand,
            header: GuidedInitQuestionKey::VerificationCommand
                .header()
                .to_owned(),
            prompt: "Which command should agents run by default before claiming the work is done?"
                .to_owned(),
            options,
            allow_custom: true,
        });
    }

    let orientation_needs_question = analysis.selected_orientation_doc.is_none()
        || grounded_differs_from_selected(
            analysis.selected_orientation_doc.as_deref(),
            analysis.grounded_orientation_doc.as_deref(),
        );
    if orientation_needs_question {
        let options = build_guided_options(
            analysis.grounded_orientation_doc.as_deref(),
            "Use the explorer-grounded orientation doc.",
            &analysis.orientation_candidates,
            Some(GuidedInitQuestionOption {
                value: CONTROL_GENERIC.to_owned(),
                label: "Keep generic orientation".to_owned(),
                description: "Avoid pinning one doc as the first read.".to_owned(),
                recommended: analysis.orientation_candidates.is_empty()
                    && analysis.grounded_orientation_doc.is_none(),
            }),
            3,
        );
        questions.push(GuidedInitQuestion {
            key: GuidedInitQuestionKey::OrientationDoc,
            header: GuidedInitQuestionKey::OrientationDoc.header().to_owned(),
            prompt: "Which file should agents read first when they need repo orientation?"
                .to_owned(),
            options,
            allow_custom: true,
        });
    }

    let critical_needs_question = (analysis.selected_critical_instruction.is_none()
        && (!analysis.critical_instruction_candidates.is_empty()
            || analysis.grounded_critical_instruction.is_some()))
        || grounded_differs_from_selected(
            analysis.selected_critical_instruction.as_deref(),
            analysis.grounded_critical_instruction.as_deref(),
        );
    if critical_needs_question {
        let options = build_guided_options(
            analysis.grounded_critical_instruction.as_deref(),
            "Use the explorer-grounded repo-wide instruction.",
            &analysis.critical_instruction_candidates,
            Some(GuidedInitQuestionOption {
                value: CONTROL_NONE.to_owned(),
                label: "No repo-wide rule".to_owned(),
                description: "Do not add a dedicated always-follow instruction.".to_owned(),
                recommended: false,
            }),
            3,
        );
        questions.push(GuidedInitQuestion {
            key: GuidedInitQuestionKey::CriticalInstruction,
            header: GuidedInitQuestionKey::CriticalInstruction
                .header()
                .to_owned(),
            prompt: "Is there one repo-wide instruction agents should always follow?".to_owned(),
            options,
            allow_custom: true,
        });
    }

    questions
}

fn grounded_differs_from_selected(selected: Option<&str>, grounded: Option<&str>) -> bool {
    let Some(grounded) = grounded.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    let Some(selected) = selected.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    grounded != selected
}

fn build_guided_options(
    grounded_value: Option<&str>,
    grounded_description: &str,
    candidates: &[SignalCandidate],
    trailing: Option<GuidedInitQuestionOption>,
    max_candidates: usize,
) -> Vec<GuidedInitQuestionOption> {
    let mut options = Vec::new();

    if let Some(value) = grounded_value
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        options.push(GuidedInitQuestionOption {
            value: value.to_owned(),
            label: value.to_owned(),
            description: grounded_description.to_owned(),
            recommended: true,
        });
    }

    for (index, candidate) in candidates.iter().take(max_candidates).enumerate() {
        if options
            .iter()
            .any(|existing| existing.value == candidate.value)
        {
            continue;
        }
        options.push(GuidedInitQuestionOption {
            value: candidate.value.clone(),
            label: candidate.label.clone(),
            description: candidate.description.clone(),
            recommended: grounded_value.is_none() && index == 0,
        });
    }

    if let Some(option) = trailing
        && options
            .iter()
            .all(|existing| existing.value != option.value)
    {
        options.push(option);
    }

    if options.is_empty() {
        options.push(GuidedInitQuestionOption {
            value: CONTROL_GENERIC.to_owned(),
            label: "Keep generic guidance".to_owned(),
            description: "Leave this section generic for now.".to_owned(),
            recommended: true,
        });
    }

    options
}

fn resolve_verification_command(
    analysis: &ProjectAnalysis,
    answers: &GuidedInitAnswers,
) -> Option<String> {
    resolve_guided_answer(
        answers.answer(GuidedInitQuestionKey::VerificationCommand),
        analysis.grounded_verification_command.clone(),
        analysis.selected_verification_command.clone(),
        analysis
            .verification_candidates
            .first()
            .map(|candidate| candidate.value.clone()),
    )
}

fn resolve_orientation_doc(
    analysis: &ProjectAnalysis,
    answers: &GuidedInitAnswers,
) -> Option<String> {
    resolve_guided_answer(
        answers.answer(GuidedInitQuestionKey::OrientationDoc),
        analysis.grounded_orientation_doc.clone(),
        analysis.selected_orientation_doc.clone(),
        analysis
            .orientation_candidates
            .first()
            .map(|candidate| candidate.value.clone()),
    )
}

fn resolve_critical_instruction(
    analysis: &ProjectAnalysis,
    answers: &GuidedInitAnswers,
) -> Option<String> {
    resolve_guided_answer(
        answers.answer(GuidedInitQuestionKey::CriticalInstruction),
        analysis.grounded_critical_instruction.clone(),
        analysis.selected_critical_instruction.clone(),
        None,
    )
}

fn resolve_guided_answer(
    answer: Option<&GuidedInitAnswer>,
    grounded: Option<String>,
    selected: Option<String>,
    fallback: Option<String>,
) -> Option<String> {
    match resolve_answered_value(answer) {
        AnsweredValue::Value(value) => Some(value),
        AnsweredValue::Control => None,
        AnsweredValue::Missing => grounded.or(selected).or(fallback),
    }
}

enum AnsweredValue {
    Missing,
    Control,
    Value(String),
}

fn resolve_answered_value(answer: Option<&GuidedInitAnswer>) -> AnsweredValue {
    let Some(answer) = answer else {
        return AnsweredValue::Missing;
    };

    if let Some(custom) = answer
        .custom
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return AnsweredValue::Value(custom.to_owned());
    }

    let selected = answer.selected.trim();
    if selected.is_empty() {
        return AnsweredValue::Missing;
    }

    if matches!(selected, CONTROL_GENERIC | CONTROL_NONE) {
        return AnsweredValue::Control;
    }

    AnsweredValue::Value(selected.to_owned())
}

fn build_quick_start_section(
    analysis: &ProjectAnalysis,
    verification_command: Option<&str>,
) -> String {
    let mut lines = Vec::new();

    if let Some(command) = verification_command {
        lines.push(format!(
            "Default verification command: `{command}` before calling work complete."
        ));
    }

    if analysis
        .build_systems
        .iter()
        .any(|system| system == "Cargo")
    {
        lines.push("Build with `cargo check` (preferred) or `cargo build --release`.".to_owned());
        lines.push(
            "Format via `cargo fmt` and lint with `cargo clippy` before committing.".to_owned(),
        );
        lines.push(
            "Run tests with `cargo nextest run` or `cargo test <name> -- --nocapture`.".to_owned(),
        );
    }

    if let Some(package_manager) = analysis.package_manager.map(PackageManager::command) {
        lines.push(format!(
            "Install JavaScript dependencies with `{package_manager} install`."
        ));
    }

    if analysis.scripts.iter().any(|script| script == "run.sh") {
        lines.push("Start interactive sessions with `./run.sh`.".to_owned());
    }

    if lines.is_empty() {
        lines.push(
            "Install dependencies and run the standard build before starting new work.".to_owned(),
        );
    }

    render_section("Quick start", lines).unwrap_or_else(|| {
        "## Quick start\n\n- Install dependencies and run the standard build before starting new work.\n\n".to_owned()
    })
}

fn build_architecture_section(analysis: &ProjectAnalysis, orientation_doc: Option<&str>) -> String {
    let mut lines = Vec::new();

    if let Some(summary) = analysis.grounded_project_summary.as_deref() {
        lines.push(summary.to_owned());
    }

    if let Some(path) = orientation_doc {
        lines.push(format!(
            "Start with `{path}` when you need repo orientation or architectural context."
        ));
    }

    lines.push(format!("Repository: {}.", analysis.project_name));

    if !analysis.languages.is_empty() {
        lines.push(format!(
            "Primary languages: {}.",
            analysis.languages.join(", ")
        ));
    }

    if !analysis.source_dirs.is_empty() {
        let dirs = analysis
            .source_dirs
            .iter()
            .map(|dir| format!("`{dir}/`"))
            .collect::<Vec<_>>();
        lines.push(format!("Key source directories: {}.", dirs.join(", ")));
    }

    if analysis.is_application {
        lines.push("Application entrypoints live under the primary source directories.".to_owned());
    } else if analysis.is_library {
        lines.push("Library-style project; expect reusable crates and packages.".to_owned());
    }

    if analysis.has_ci_cd {
        lines.push(
            "CI workflows detected under `.github/workflows/`; match those expectations locally."
                .to_owned(),
        );
    }

    if analysis.has_docker {
        lines.push(
            "Docker assets are present; some integration flows may depend on container setup."
                .to_owned(),
        );
    }

    render_section("Architecture & layout", lines).unwrap_or_else(|| {
        "## Architecture & layout\n\n- Review the repository layout before editing.\n\n".to_owned()
    })
}

fn build_important_instructions_section(instruction: Option<&str>) -> Option<String> {
    let instruction = instruction?.trim();
    if instruction.is_empty() {
        return None;
    }
    render_section("Important instructions", vec![instruction.to_owned()])
}

fn build_code_style_section(analysis: &ProjectAnalysis) -> String {
    let mut lines = Vec::new();

    for language in &analysis.languages {
        match language.as_str() {
            "Rust" => {
                lines.push("Rust code uses 4-space indentation, snake_case functions, PascalCase types, and `anyhow::Result<T>` with `.with_context()` for fallible paths.".to_owned());
                lines.push("Run `cargo fmt` before committing and avoid hardcoded configuration.".to_owned());
            }
            "JavaScript/TypeScript" => lines.push(
                "Use the repository formatter and linter settings; match existing component and module patterns."
                    .to_owned(),
            ),
            "Python" => lines.push(
                "Follow PEP 8, prefer Black-compatible formatting, and add type hints when practical."
                    .to_owned(),
            ),
            "Go" => lines.push(
                "Use `gofmt`/`go vet` and keep exported APIs intentional.".to_owned(),
            ),
            other => lines.push(format!(
                "Match the surrounding {other} conventions and run the project formatter before pushing."
            )),
        }
    }

    if lines.is_empty() {
        lines.push(
            "Match the surrounding style and keep commits free of formatting noise.".to_owned(),
        );
    }

    render_section("Code style", lines).unwrap_or_else(|| {
        "## Code style\n\n- Match the surrounding style and keep commits free of formatting noise.\n\n".to_owned()
    })
}

fn build_testing_section(analysis: &ProjectAnalysis, verification_command: Option<&str>) -> String {
    let mut lines = Vec::new();

    if let Some(command) = verification_command {
        lines.push(format!("Default verification command: `{command}`."));
    }

    if analysis
        .build_systems
        .iter()
        .any(|system| system == "Cargo")
    {
        lines.push(
            "Rust suite: `cargo nextest run` for speed, or `cargo test` for targeted fallback."
                .to_owned(),
        );
        lines.push(
            "Run `cargo clippy --workspace --all-targets -- -D warnings` for lint coverage."
                .to_owned(),
        );
    }

    if let Some(package_manager) = analysis.package_manager.map(PackageManager::command) {
        lines.push(format!(
            "Run JavaScript/TypeScript checks with `{package_manager} test` or the repo's `check` script when present."
        ));
    }

    if analysis
        .build_systems
        .iter()
        .any(|system| system == "Go Modules")
    {
        lines.push("Run `go test ./...` for Go coverage.".to_owned());
    }

    if analysis.has_ci_cd {
        lines.push("Keep CI green by mirroring workflow steps locally before pushing.".to_owned());
    }

    if lines.is_empty() {
        lines.push("Run the project's automated checks before submitting changes.".to_owned());
    }

    render_section("Testing", lines).unwrap_or_else(|| {
        "## Testing\n\n- Run the project's automated checks before submitting changes.\n\n"
            .to_owned()
    })
}

fn build_performance_section() -> String {
    render_section(
        "Performance & simplicity",
        vec![
            "Do not guess at bottlenecks; measure before optimizing.".to_owned(),
            "Prefer simple algorithms and data structures until workload data proves otherwise."
                .to_owned(),
            "Keep performance changes surgical and behavior-preserving.".to_owned(),
        ],
    )
    .unwrap_or_else(|| "## Performance & simplicity\n\n- Measure before optimizing.\n\n".to_owned())
}

fn build_pr_guidelines_section(analysis: &ProjectAnalysis) -> Option<String> {
    let mut lines = Vec::new();

    if analysis
        .commit_patterns
        .iter()
        .any(|pattern| pattern == "Conventional Commits")
    {
        lines.push(
            "Use Conventional Commits (`type(scope): subject`) with short, descriptive summaries."
                .to_owned(),
        );
    } else {
        lines.push("Write descriptive, imperative commit messages.".to_owned());
    }

    lines.push("Reference issues with `Fixes #123` or `Closes #123` when applicable.".to_owned());
    lines.push(
        "Keep pull requests focused and include test evidence for non-trivial changes.".to_owned(),
    );

    render_section("PR guidelines", lines)
}

fn build_additional_guidance_section(
    analysis: &ProjectAnalysis,
    orientation_doc: Option<&str>,
) -> Option<String> {
    let mut lines = Vec::new();

    if let Some(path) = orientation_doc {
        lines.push(format!("Preferred orientation doc: `{path}`."));
    }

    if !analysis.documentation_files.is_empty() {
        lines.push(format!(
            "Repository docs spotted: {}.",
            analysis.documentation_files.join(", ")
        ));
    }

    if !analysis.dependencies.is_empty() {
        let highlights = analysis
            .dependencies
            .iter()
            .map(|(ecosystem, deps)| format!("{ecosystem} ({})", deps.join(", ")))
            .collect::<Vec<_>>();
        lines.push(format!("Notable dependencies: {}.", highlights.join("; ")));
    }

    if analysis.scripts.iter().any(|script| script == "run.sh")
        && analysis
            .scripts
            .iter()
            .any(|script| script == "run-debug.sh")
    {
        lines.push(
            "Use `./run.sh` for release runs and `./run-debug.sh` for debug sessions.".to_owned(),
        );
    }

    render_section("Additional guidance", lines)
}

fn render_section(title: &str, lines: Vec<String>) -> Option<String> {
    if lines.is_empty() {
        return None;
    }

    let mut section = String::new();
    section.push_str("## ");
    section.push_str(title);
    section.push_str("\n\n");

    for line in lines {
        section.push_str("- ");
        section.push_str(&line);
        section.push('\n');
    }
    section.push('\n');
    Some(section)
}

fn unique_preserving_order(values: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            unique.push(value.clone());
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_file(dir: &TempDir, relative: &str, contents: &str) {
        let path = dir.path().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, contents).expect("write file");
    }

    #[test]
    fn no_questions_when_clear_signals_exist() {
        let workspace = TempDir::new().expect("workspace");
        write_file(
            &workspace,
            "Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        );
        write_file(&workspace, "README.md", "# Demo\n");
        write_file(&workspace, "scripts/check.sh", "#!/bin/sh\ncargo check\n");

        let plan = prepare_guided_init(workspace.path(), false).expect("plan");

        assert!(plan.questions.is_empty());
        assert_eq!(plan.overwrite_state, GuidedInitOverwriteState::Skip);
    }

    #[test]
    fn emits_verification_question_when_multiple_strong_candidates_exist() {
        let workspace = TempDir::new().expect("workspace");
        write_file(
            &workspace,
            "Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        );
        write_file(
            &workspace,
            "README.md",
            "Run ./scripts/check.sh for the full gate.\nUse cargo nextest run during local work.\n",
        );
        write_file(
            &workspace,
            "scripts/check.sh",
            "#!/bin/sh\ncargo nextest run\n",
        );
        write_file(
            &workspace,
            ".github/workflows/ci.yml",
            "jobs:\n  test:\n    steps:\n      - run: cargo nextest run\n",
        );

        let plan = prepare_guided_init(workspace.path(), false).expect("plan");

        assert!(
            plan.questions
                .iter()
                .any(|question| question.key == GuidedInitQuestionKey::VerificationCommand)
        );
    }

    #[test]
    fn emits_orientation_question_when_multiple_docs_are_plausible() {
        let workspace = TempDir::new().expect("workspace");
        write_file(
            &workspace,
            "README.md",
            "See docs/ARCHITECTURE.md for design.\n",
        );
        write_file(&workspace, "docs/ARCHITECTURE.md", "# Architecture\n");
        write_file(
            &workspace,
            "docs/modules/vtcode_docs_map.md",
            "# Docs Map\nUse this first.\n",
        );

        let plan = prepare_guided_init(workspace.path(), false).expect("plan");

        assert!(
            plan.questions
                .iter()
                .any(|question| question.key == GuidedInitQuestionKey::OrientationDoc)
        );
    }

    #[test]
    fn chosen_answers_override_heuristics() {
        let workspace = TempDir::new().expect("workspace");
        write_file(
            &workspace,
            "Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        );
        write_file(&workspace, "README.md", "# Demo\n");
        let plan = prepare_guided_init(workspace.path(), false).expect("plan");

        let mut answers = GuidedInitAnswers::default();
        answers.insert(GuidedInitAnswer {
            key: GuidedInitQuestionKey::VerificationCommand,
            selected: "cargo nextest run".to_owned(),
            custom: None,
        });

        let rendered = render_agents_md(&plan, &answers).expect("rendered");
        assert!(rendered.contains("Default verification command: `cargo nextest run`."));
    }

    #[test]
    fn critical_instruction_none_omits_section() {
        let workspace = TempDir::new().expect("workspace");
        write_file(&workspace, "README.md", "Use Conventional Commits.\n");
        let plan = prepare_guided_init(workspace.path(), false).expect("plan");

        let mut answers = GuidedInitAnswers::default();
        answers.insert(GuidedInitAnswer {
            key: GuidedInitQuestionKey::CriticalInstruction,
            selected: CONTROL_NONE.to_owned(),
            custom: None,
        });

        let rendered = render_agents_md(&plan, &answers).expect("rendered");
        assert!(!rendered.contains("## Important instructions"));
    }

    #[test]
    fn render_is_deterministic_for_same_analysis_and_answers() {
        let workspace = TempDir::new().expect("workspace");
        write_file(&workspace, "README.md", "# Demo\n");
        let plan = prepare_guided_init(workspace.path(), false).expect("plan");
        let answers = GuidedInitAnswers::default();

        let left = render_agents_md(&plan, &answers).expect("left");
        let right = render_agents_md(&plan, &answers).expect("right");

        assert_eq!(left, right);
    }

    #[test]
    fn existing_agents_requires_confirmation_without_force() {
        let workspace = TempDir::new().expect("workspace");
        write_file(&workspace, "AGENTS.md", "# Existing\n");

        let plan = prepare_guided_init(workspace.path(), false).expect("plan");

        assert_eq!(plan.overwrite_state, GuidedInitOverwriteState::Confirm);
    }

    #[test]
    fn force_skips_confirmation_for_existing_agents() {
        let workspace = TempDir::new().expect("workspace");
        write_file(&workspace, "AGENTS.md", "# Existing\n");

        let plan = prepare_guided_init(workspace.path(), true).expect("plan");

        assert_eq!(plan.overwrite_state, GuidedInitOverwriteState::Force);
    }

    #[test]
    fn write_agents_file_skips_existing_when_not_overwriting() {
        let workspace = TempDir::new().expect("workspace");
        write_file(&workspace, "AGENTS.md", "# Existing\n");

        let report = write_agents_file(workspace.path(), "# Fresh\n", false).expect("report");

        assert_eq!(report.status, GenerateAgentsFileStatus::SkippedExisting);
    }

    #[test]
    fn blank_custom_answers_normalize_to_control_values() {
        let verification = GuidedInitAnswer::from_input(
            GuidedInitQuestionKey::VerificationCommand,
            None,
            Some("   "),
        )
        .expect("verification");
        assert_eq!(verification.selected, CONTROL_GENERIC);
        assert_eq!(verification.custom, None);

        let critical = GuidedInitAnswer::from_input(
            GuidedInitQuestionKey::CriticalInstruction,
            None,
            Some(""),
        )
        .expect("critical");
        assert_eq!(critical.selected, CONTROL_NONE);
        assert_eq!(critical.custom, None);
    }

    #[test]
    fn grounding_conflict_reopens_verification_choice() {
        let workspace = TempDir::new().expect("workspace");
        write_file(
            &workspace,
            "Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        );
        write_file(&workspace, "README.md", "# Demo\n");
        write_file(&workspace, "scripts/check.sh", "#!/bin/sh\ncargo check\n");

        let plan = prepare_guided_init(workspace.path(), false)
            .expect("plan")
            .with_grounding(GuidedInitGrounding {
                verification_command: Some("cargo nextest run".to_owned()),
                ..GuidedInitGrounding::default()
            });

        assert!(
            plan.questions
                .iter()
                .any(|question| question.key == GuidedInitQuestionKey::VerificationCommand)
        );
    }

    #[test]
    fn grounding_summary_and_defaults_render_into_agents() {
        let workspace = TempDir::new().expect("workspace");
        write_file(&workspace, "README.md", "# Demo\n");

        let plan = prepare_guided_init(workspace.path(), false)
            .expect("plan")
            .with_grounding(GuidedInitGrounding {
                project_summary: Some(
                    "Terminal-first coding agent for repository work.".to_owned(),
                ),
                verification_command: Some("cargo nextest run".to_owned()),
                ..GuidedInitGrounding::default()
            });

        let rendered = render_agents_md(&plan, &GuidedInitAnswers::default()).expect("rendered");
        assert!(rendered.contains("Terminal-first coding agent for repository work."));
        assert!(rendered.contains("Default verification command: `cargo nextest run`."));
    }
}
