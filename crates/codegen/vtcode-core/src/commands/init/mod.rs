//! Guided `/init` support for repository analysis and `AGENTS.md` generation.

use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use vtcode_commons::walk::{build_default_walker, is_excluded_dir};

mod analysis;
use analysis::analyze_project;

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
    fn add(&mut self, value: impl Into<String>, label: impl Into<String>, description: impl Into<String>, score: u32) {
        let value = value.into();
        let normalized = value.trim();
        if normalized.is_empty() {
            return;
        }

        let label = label.into();
        let description = description.into();
        let entry = self.values.entry(normalized.to_owned()).or_insert_with(|| SignalCandidate {
            value: normalized.to_owned(),
            label,
            description,
            score: 0,
        });

        entry.score += score;
    }

    fn into_sorted(self) -> Vec<SignalCandidate> {
        let mut values: Vec<_> = self.values.into_values().collect();
        values.sort_by(|left, right| right.score.cmp(&left.score).then_with(|| left.value.cmp(&right.value)));
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
            Self::OrientationDoc => "AGENTS.md",
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
    pub fn from_input(key: GuidedInitQuestionKey, selected: Option<&str>, custom: Option<&str>) -> Option<Self> {
        let custom_selected = custom.is_some();
        if let Some(custom) = custom.map(str::trim).filter(|value| !value.is_empty()) {
            return Some(Self {
                key,
                selected: String::new(),
                custom: Some(custom.to_owned()),
            });
        }

        if let Some(selected) = selected.map(str::trim).filter(|value| !value.is_empty()) {
            return Some(Self { key, selected: selected.to_owned(), custom: None });
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
    content.push_str(&build_quick_start_section(analysis, verification_command.as_deref()));
    content.push_str(&build_architecture_section(analysis, orientation_doc.as_deref()));
    if let Some(section) = build_important_instructions_section(critical_instruction.as_deref()) {
        content.push_str(&section);
    }
    content.push_str(&build_code_style_section(analysis));
    content.push_str(&build_testing_section(analysis, verification_command.as_deref()));
    content.push_str(&build_performance_section());
    if let Some(section) = build_pr_guidelines_section(analysis) {
        content.push_str(&section);
    }
    if let Some(section) = build_additional_guidance_section(analysis, orientation_doc.as_deref()) {
        content.push_str(&section);
    }

    Ok(content)
}

pub fn write_agents_file(workspace: &Path, content: &str, overwrite: bool) -> Result<GenerateAgentsFileReport> {
    let path = workspace.join(AGENTS_FILENAME);
    let existed_before = path.exists();

    if existed_before && !overwrite {
        return Ok(GenerateAgentsFileReport {
            path,
            status: GenerateAgentsFileStatus::SkippedExisting,
        });
    }

    fs::write(&path, content).with_context(|| format!("failed to write AGENTS.md to {}", path.display()))?;

    let status = if existed_before {
        GenerateAgentsFileStatus::Overwritten
    } else {
        GenerateAgentsFileStatus::Created
    };

    Ok(GenerateAgentsFileReport { path, status })
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
            header: GuidedInitQuestionKey::VerificationCommand.header().to_owned(),
            prompt: "Which command should agents run by default before claiming the work is done?".to_owned(),
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
                recommended: analysis.orientation_candidates.is_empty() && analysis.grounded_orientation_doc.is_none(),
            }),
            3,
        );
        questions.push(GuidedInitQuestion {
            key: GuidedInitQuestionKey::OrientationDoc,
            header: GuidedInitQuestionKey::OrientationDoc.header().to_owned(),
            prompt: "Which file should agents read first when they need repo orientation?".to_owned(),
            options,
            allow_custom: true,
        });
    }

    let critical_needs_question = (analysis.selected_critical_instruction.is_none()
        && (!analysis.critical_instruction_candidates.is_empty() || analysis.grounded_critical_instruction.is_some()))
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
            header: GuidedInitQuestionKey::CriticalInstruction.header().to_owned(),
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

    if let Some(value) = grounded_value.map(str::trim).filter(|value| !value.is_empty()) {
        options.push(GuidedInitQuestionOption {
            value: value.to_owned(),
            label: value.to_owned(),
            description: grounded_description.to_owned(),
            recommended: true,
        });
    }

    for (index, candidate) in candidates.iter().take(max_candidates).enumerate() {
        if options.iter().any(|existing| existing.value == candidate.value) {
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
        && options.iter().all(|existing| existing.value != option.value)
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

fn resolve_verification_command(analysis: &ProjectAnalysis, answers: &GuidedInitAnswers) -> Option<String> {
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

fn resolve_orientation_doc(analysis: &ProjectAnalysis, answers: &GuidedInitAnswers) -> Option<String> {
    resolve_guided_answer(
        answers.answer(GuidedInitQuestionKey::OrientationDoc),
        analysis.grounded_orientation_doc.clone(),
        analysis.selected_orientation_doc.clone(),
        analysis.orientation_candidates.first().map(|candidate| candidate.value.clone()),
    )
}

fn resolve_critical_instruction(analysis: &ProjectAnalysis, answers: &GuidedInitAnswers) -> Option<String> {
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

fn build_quick_start_section(analysis: &ProjectAnalysis, verification_command: Option<&str>) -> String {
    let mut lines = Vec::new();

    if let Some(command) = verification_command {
        lines.push(format!("Default verification command: `{command}` before calling work complete."));
    }

    if analysis.build_systems.iter().any(|system| system == "Cargo") {
        lines.push("Build with `cargo check` (preferred) or `cargo build --release`.".to_owned());
        lines.push("Format via `cargo fmt` and lint with `cargo clippy` before committing.".to_owned());
        lines.push("Run tests with `cargo nextest run` or `cargo test <name> -- --nocapture`.".to_owned());
    }

    if let Some(package_manager) = analysis.package_manager.map(PackageManager::command) {
        lines.push(format!("Install JavaScript dependencies with `{package_manager} install`."));
    }

    if analysis.scripts.iter().any(|script| script == "run.sh") {
        lines.push("Start interactive sessions with `./run.sh`.".to_owned());
    }

    if lines.is_empty() {
        lines.push("Install dependencies and run the standard build before starting new work.".to_owned());
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
        lines.push(format!("Start with `{path}` when you need repo orientation or architectural context."));
    }

    lines.push(format!("Repository: {}.", analysis.project_name));

    if !analysis.languages.is_empty() {
        lines.push(format!("Primary languages: {}.", analysis.languages.join(", ")));
    }

    if !analysis.source_dirs.is_empty() {
        let dirs = analysis.source_dirs.iter().map(|dir| format!("`{dir}/`")).collect::<Vec<_>>();
        lines.push(format!("Key source directories: {}.", dirs.join(", ")));
    }

    if analysis.is_application {
        lines.push("Application entrypoints live under the primary source directories.".to_owned());
    } else if analysis.is_library {
        lines.push("Library-style project; expect reusable crates and packages.".to_owned());
    }

    if analysis.has_ci_cd {
        lines.push("CI workflows detected under `.github/workflows/`; match those expectations locally.".to_owned());
    }

    if analysis.has_docker {
        lines.push("Docker assets are present; some integration flows may depend on container setup.".to_owned());
    }

    render_section("Architecture & layout", lines)
        .unwrap_or_else(|| "## Architecture & layout\n\n- Review the repository layout before editing.\n\n".to_owned())
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
                "Follow PEP 8, prefer Black-compatible formatting, and add type hints when practical.".to_owned(),
            ),
            "Go" => lines.push("Use `gofmt`/`go vet` and keep exported APIs intentional.".to_owned()),
            other => lines.push(format!(
                "Match the surrounding {other} conventions and run the project formatter before pushing."
            )),
        }
    }

    if lines.is_empty() {
        lines.push("Match the surrounding style and keep commits free of formatting noise.".to_owned());
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

    if analysis.build_systems.iter().any(|system| system == "Cargo") {
        lines.push("Rust suite: `cargo nextest run` for speed, or `cargo test` for targeted fallback.".to_owned());
        lines.push("Run `cargo clippy --workspace --all-targets -- -D warnings` for lint coverage.".to_owned());
    }

    if let Some(package_manager) = analysis.package_manager.map(PackageManager::command) {
        lines.push(format!(
            "Run JavaScript/TypeScript checks with `{package_manager} test` or the repo's `check` script when present."
        ));
    }

    if analysis.build_systems.iter().any(|system| system == "Go Modules") {
        lines.push("Run `go test ./...` for Go coverage.".to_owned());
    }

    if analysis.has_ci_cd {
        lines.push("Keep CI green by mirroring workflow steps locally before pushing.".to_owned());
    }

    if lines.is_empty() {
        lines.push("Run the project's automated checks before submitting changes.".to_owned());
    }

    render_section("Testing", lines).unwrap_or_else(|| {
        "## Testing\n\n- Run the project's automated checks before submitting changes.\n\n".to_owned()
    })
}

fn build_performance_section() -> String {
    render_section(
        "Performance & simplicity",
        vec![
            "Do not guess at bottlenecks; measure before optimizing.".to_owned(),
            "Prefer simple algorithms and data structures until workload data proves otherwise.".to_owned(),
            "Keep performance changes surgical and behavior-preserving.".to_owned(),
        ],
    )
    .unwrap_or_else(|| "## Performance & simplicity\n\n- Measure before optimizing.\n\n".to_owned())
}

fn build_pr_guidelines_section(analysis: &ProjectAnalysis) -> Option<String> {
    let mut lines = Vec::new();

    if analysis.commit_patterns.iter().any(|pattern| pattern == "Conventional Commits") {
        lines.push("Use Conventional Commits (`type(scope): subject`) with short, descriptive summaries.".to_owned());
    } else {
        lines.push("Write descriptive, imperative commit messages.".to_owned());
    }

    lines.push("Reference issues with `Fixes #123` or `Closes #123` when applicable.".to_owned());
    lines.push("Keep pull requests focused and include test evidence for non-trivial changes.".to_owned());

    render_section("PR guidelines", lines)
}

fn build_additional_guidance_section(analysis: &ProjectAnalysis, orientation_doc: Option<&str>) -> Option<String> {
    let mut lines = Vec::new();

    if let Some(path) = orientation_doc {
        lines.push(format!("Preferred orientation doc: `{path}`."));
    }

    if !analysis.documentation_files.is_empty() {
        lines.push(format!("Repository docs spotted: {}.", analysis.documentation_files.join(", ")));
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
        && analysis.scripts.iter().any(|script| script == "run-debug.sh")
    {
        lines.push("Use `./run.sh` for release runs and `./run-debug.sh` for debug sessions.".to_owned());
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
mod tests;
