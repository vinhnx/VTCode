//! Plan mode tools for entering, exiting, and managing planning workflow
//!
//! These tools allow the agent to programmatically enter and exit plan mode.
//! The agent can:
//! - Enter plan mode to switch to read-only exploration
//! - Exit plan mode (triggering plan review) to start implementation
//! - Persist canonical plans under `.vtcode/plans/` by default (with optional custom path)
//!
//! Based on insights from Claude Code's plan mode implementation:
//! - Plan files are written to a dedicated directory
//! - The agent edits its own plan file during planning
//! - Exiting plan mode reads the plan file and starts execution
//! - User confirmation is required before transitioning to execution (HITL)

use crate::config::constants::tools;
use crate::utils::file_utils::{
    ensure_dir_exists, read_file_with_context, write_file_with_context,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use crate::tools::traits::Tool;
use crate::ui::tui::PlanContent;

const PLAN_TRACKER_START: &str = "<!-- vtcode:plan-tracker:start -->";
const PLAN_TRACKER_END: &str = "<!-- vtcode:plan-tracker:end -->";

const REQUIRED_PLAN_SECTIONS: [&str; 4] = [
    "Summary",
    "Implementation Steps",
    "Test Cases and Validation",
    "Assumptions and Defaults",
];

const PLACEHOLDER_TOKENS: [&str; 11] = [
    "[step]",
    "[paths]",
    "[check]",
    "[explicit assumption]",
    "[default chosen when user did not specify]",
    "[out-of-scope items intentionally not changed]",
    "[project build and lint command",
    "[project test command",
    "[2-4 lines: goal, user impact, what will change, what will not]",
    "[explicit commands/manual checks]",
    "[what must not break]",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum PlanLifecyclePhase {
    #[default]
    Off = 0,
    EnterPendingApproval = 1,
    ActiveDrafting = 2,
    InterviewPending = 3,
    DraftReady = 4,
    ReviewPending = 5,
}

impl PlanLifecyclePhase {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::EnterPendingApproval,
            2 => Self::ActiveDrafting,
            3 => Self::InterviewPending,
            4 => Self::DraftReady,
            5 => Self::ReviewPending,
            _ => Self::Off,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanValidationReport {
    pub missing_sections: Vec<String>,
    pub placeholder_tokens: Vec<String>,
    pub open_decisions: Vec<String>,
    pub implementation_step_count: usize,
    pub validation_item_count: usize,
    pub assumption_count: usize,
    pub summary_present: bool,
}

impl PlanValidationReport {
    pub fn is_ready(&self) -> bool {
        self.missing_sections.is_empty()
            && self.placeholder_tokens.is_empty()
            && self.open_decisions.is_empty()
            && self.summary_present
            && self.implementation_step_count > 0
            && self.validation_item_count > 0
            && self.assumption_count > 0
    }
}

#[derive(Debug, Clone)]
pub struct PersistedPlanDraft {
    pub plan_file: PathBuf,
    pub tracker_file: Option<PathBuf>,
    pub validation: PlanValidationReport,
}

/// Shared state for plan mode across tools
#[derive(Debug, Clone)]
pub struct PlanModeState {
    /// Whether plan mode is currently active
    is_active: Arc<AtomicBool>,
    /// Path to the current plan file (if any)
    current_plan_file: Arc<tokio::sync::RwLock<Option<PathBuf>>>,
    /// Baseline time to require plan updates after initial creation
    plan_baseline: Arc<tokio::sync::RwLock<Option<SystemTime>>>,
    /// Workspace root for plan directory
    workspace_root: PathBuf,
    /// Shared plan lifecycle phase for the current session.
    lifecycle_phase: Arc<std::sync::atomic::AtomicU8>,
}

impl PlanModeState {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            is_active: Arc::new(AtomicBool::new(false)),
            current_plan_file: Arc::new(tokio::sync::RwLock::new(None)),
            plan_baseline: Arc::new(tokio::sync::RwLock::new(None)),
            workspace_root,
            lifecycle_phase: Arc::new(std::sync::atomic::AtomicU8::new(
                PlanLifecyclePhase::Off as u8,
            )),
        }
    }

    /// Check if plan mode is active
    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::Relaxed)
    }

    /// Enable plan mode
    pub fn enable(&self) {
        self.is_active.store(true, Ordering::Relaxed);
    }

    /// Disable plan mode
    pub fn disable(&self) {
        self.is_active.store(false, Ordering::Relaxed);
        self.set_phase(PlanLifecyclePhase::Off);
    }

    pub fn phase(&self) -> PlanLifecyclePhase {
        PlanLifecyclePhase::from_u8(self.lifecycle_phase.load(Ordering::Relaxed))
    }

    pub fn set_phase(&self, phase: PlanLifecyclePhase) {
        self.lifecycle_phase.store(phase as u8, Ordering::Relaxed);
    }

    /// Get the workspace root path
    pub fn workspace_root(&self) -> Option<PathBuf> {
        if self.workspace_root.as_os_str().is_empty() {
            None
        } else {
            Some(self.workspace_root.clone())
        }
    }

    /// Get the default plans directory path.
    pub fn plans_dir(&self) -> PathBuf {
        if self.workspace_root.as_os_str().is_empty() {
            std::env::temp_dir()
                .join("vtcode-plans")
                .join(workspace_slug_for_tmp(&self.workspace_root))
        } else {
            self.workspace_root.join(".vtcode").join("plans")
        }
    }

    /// Set the current plan file
    pub async fn set_plan_file(&self, path: Option<PathBuf>) {
        let mut guard = self.current_plan_file.write().await;
        *guard = path;
    }

    /// Set the baseline time for plan readiness checks
    pub async fn set_plan_baseline(&self, baseline: Option<SystemTime>) {
        let mut guard = self.plan_baseline.write().await;
        *guard = baseline;
    }

    /// Get the baseline time for plan readiness checks
    pub async fn plan_baseline(&self) -> Option<SystemTime> {
        *self.plan_baseline.read().await
    }

    /// Get the current plan file path
    pub async fn get_plan_file(&self) -> Option<PathBuf> {
        self.current_plan_file.read().await.clone()
    }
}

// ============================================================================
// Enter Plan Mode Tool
// ============================================================================

/// Arguments for entering plan mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnterPlanModeArgs {
    /// Optional: Name for the plan file (defaults to timestamp-based name)
    #[serde(default)]
    pub plan_name: Option<String>,

    /// Optional: Explicit output path for the plan file (absolute or workspace-relative)
    #[serde(default)]
    pub plan_path: Option<String>,

    /// Optional: Initial description of what you're planning
    #[serde(default)]
    pub description: Option<String>,

    /// Internal: when true, request confirmation instead of entering immediately.
    #[serde(default)]
    pub require_confirmation: bool,

    /// Internal: confirmation has already been granted.
    #[serde(default)]
    pub approved: bool,
}

/// Tool for entering plan mode
pub struct EnterPlanModeTool {
    state: PlanModeState,
}

impl EnterPlanModeTool {
    pub fn new(state: PlanModeState) -> Self {
        Self { state }
    }

    fn generate_plan_name(&self, provided: Option<&str>) -> String {
        match provided {
            Some(name) => {
                // Sanitize the name for filesystem
                name.to_lowercase()
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '-' {
                            c
                        } else {
                            '-'
                        }
                    })
                    .collect()
            }
            None => {
                // Generate human-readable slug with timestamp prefix
                // Format: {timestamp_millis}-{adjective}-{noun} (e.g., "1768330644696-gentle-harbor")
                // This follows the OpenCode pattern for memorable plan file names
                vtcode_commons::slug::create_timestamped()
            }
        }
    }
}

fn workspace_slug_for_tmp(workspace_root: &Path) -> String {
    let fallback = "workspace".to_string();
    let candidate = workspace_root
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
        .unwrap_or(fallback);
    let sanitized = candidate
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    if sanitized.trim_matches('-').is_empty() {
        "workspace".to_string()
    } else {
        sanitized
    }
}

fn title_from_plan_name(plan_name: &str) -> String {
    plan_name
        .split('-')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => {
                    format!(
                        "{}{}",
                        first.to_ascii_uppercase(),
                        chars.as_str().to_ascii_lowercase()
                    )
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn tracker_file_for_plan_file(plan_file: &Path) -> Option<PathBuf> {
    let stem = plan_file.file_stem()?.to_str()?;
    Some(plan_file.with_file_name(format!("{stem}.tasks.md")))
}

pub fn plan_file_for_tracker_file(tracker_file: &Path) -> Option<PathBuf> {
    let file_name = tracker_file.file_name()?.to_str()?;
    let stem = file_name.strip_suffix(".tasks.md")?;
    Some(tracker_file.with_file_name(format!("{stem}.md")))
}

fn strip_embedded_tracker(plan_content: &str) -> String {
    let Some(start) = plan_content.find(PLAN_TRACKER_START) else {
        return plan_content.trim().to_string();
    };
    let end = plan_content[start..]
        .find(PLAN_TRACKER_END)
        .map(|offset| start + offset + PLAN_TRACKER_END.len())
        .unwrap_or(plan_content.len());
    let mut merged = String::new();
    merged.push_str(plan_content[..start].trim_end());
    if !merged.is_empty() && !plan_content[end..].trim().is_empty() {
        merged.push_str("\n\n");
    }
    merged.push_str(plan_content[end..].trim_start());
    merged.trim().to_string()
}

fn extract_embedded_tracker(plan_content: &str) -> Option<String> {
    let start = plan_content.find(PLAN_TRACKER_START)?;
    let end = plan_content.find(PLAN_TRACKER_END)?;
    if end <= start {
        return None;
    }
    let content = plan_content[start + PLAN_TRACKER_START.len()..end].trim();
    if content.is_empty() {
        None
    } else {
        Some(content.to_string())
    }
}

fn render_plan_with_tracker(plan_markdown: &str, tracker_markdown: Option<&str>) -> String {
    let base_plan = strip_embedded_tracker(plan_markdown);
    let Some(tracker_markdown) = tracker_markdown
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return format!("{}\n", base_plan.trim_end());
    };
    format!(
        "{}\n\n{}\n{}\n{}\n",
        base_plan.trim_end(),
        PLAN_TRACKER_START,
        tracker_markdown,
        PLAN_TRACKER_END
    )
}

pub fn merge_plan_content(
    plan_content: Option<String>,
    tracker_content: Option<String>,
) -> Option<String> {
    match (plan_content, tracker_content) {
        (Some(plan), tracker) => {
            let plan_trimmed = strip_embedded_tracker(&plan);
            if plan_trimmed.is_empty() {
                return tracker
                    .map(|content| content.trim().to_string())
                    .filter(|content| !content.is_empty());
            }
            let embedded_tracker = extract_embedded_tracker(&plan);
            let tracker_trimmed = tracker
                .as_deref()
                .map(str::trim)
                .filter(|content| !content.is_empty())
                .map(ToOwned::to_owned)
                .or(embedded_tracker);
            if let Some(tracker_trimmed) = tracker_trimmed {
                Some(format!(
                    "{}\n\n{}\n",
                    plan_trimmed.trim_end(),
                    tracker_trimmed.trim()
                ))
            } else {
                Some(plan_trimmed.to_string())
            }
        }
        (None, Some(tracker)) => {
            let trimmed = tracker.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        (None, None) => None,
    }
}

fn section_body(content: &str, header: &str) -> Option<String> {
    let mut capture = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(found) = trimmed.strip_prefix("## ") {
            if capture {
                break;
            }
            capture = found.trim().eq_ignore_ascii_case(header);
            continue;
        }
        if capture {
            lines.push(line.to_string());
        }
    }
    let body = lines.join("\n").trim().to_string();
    if body.is_empty() { None } else { Some(body) }
}

fn meaningful_section_lines(body: &str) -> Vec<&str> {
    body.lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with('>')
                && !line.starts_with("<!--")
                && *line != PLAN_TRACKER_START
                && *line != PLAN_TRACKER_END
        })
        .collect()
}

fn is_numbered_line(line: &str) -> bool {
    let mut seen_digit = false;
    for ch in line.chars() {
        if ch.is_ascii_digit() {
            seen_digit = true;
            continue;
        }
        return seen_digit && (ch == '.' || ch == ')');
    }
    false
}

fn find_placeholder_tokens(content: &str) -> Vec<String> {
    let lower = content.to_ascii_lowercase();
    PLACEHOLDER_TOKENS
        .iter()
        .filter(|token| lower.contains(**token))
        .map(|token| token.to_string())
        .collect()
}

fn find_open_decisions(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("next open decision")
                && ![
                    "none",
                    "no remaining",
                    "no further",
                    "resolved",
                    "closed",
                    "n/a",
                    "not applicable",
                ]
                .iter()
                .any(|needle| lower.contains(needle))
        })
        .map(ToString::to_string)
        .collect()
}

pub fn validate_plan_content(content: &str) -> PlanValidationReport {
    let stripped = strip_embedded_tracker(content);
    let mut report = PlanValidationReport {
        placeholder_tokens: find_placeholder_tokens(&stripped),
        open_decisions: find_open_decisions(&stripped),
        ..PlanValidationReport::default()
    };

    let summary_body = section_body(&stripped, "Summary");
    let implementation_body = section_body(&stripped, "Implementation Steps");
    let validation_body = section_body(&stripped, "Test Cases and Validation");
    let assumptions_body = section_body(&stripped, "Assumptions and Defaults");

    for section in REQUIRED_PLAN_SECTIONS {
        if section_body(&stripped, section).is_none() {
            report.missing_sections.push(section.to_string());
        }
    }

    if let Some(body) = summary_body.as_deref() {
        report.summary_present = !meaningful_section_lines(body).is_empty();
    }
    if !report.summary_present && !report.missing_sections.iter().any(|s| s == "Summary") {
        report.missing_sections.push("Summary".to_string());
    }

    if let Some(body) = implementation_body.as_deref() {
        report.implementation_step_count = meaningful_section_lines(body)
            .into_iter()
            .filter(|line| is_numbered_line(line))
            .count();
    }
    if report.implementation_step_count == 0
        && !report
            .missing_sections
            .iter()
            .any(|s| s == "Implementation Steps")
    {
        report
            .missing_sections
            .push("Implementation Steps".to_string());
    }

    if let Some(body) = validation_body.as_deref() {
        report.validation_item_count = meaningful_section_lines(body)
            .into_iter()
            .filter(|line| is_numbered_line(line) || line.starts_with("- "))
            .count();
    }
    if report.validation_item_count == 0
        && !report
            .missing_sections
            .iter()
            .any(|s| s == "Test Cases and Validation")
    {
        report
            .missing_sections
            .push("Test Cases and Validation".to_string());
    }

    if let Some(body) = assumptions_body.as_deref() {
        report.assumption_count = meaningful_section_lines(body)
            .into_iter()
            .filter(|line| is_numbered_line(line) || line.starts_with("- "))
            .count();
    }
    if report.assumption_count == 0
        && !report
            .missing_sections
            .iter()
            .any(|s| s == "Assumptions and Defaults")
    {
        report
            .missing_sections
            .push("Assumptions and Defaults".to_string());
    }

    report
}

fn parse_bracket_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim().trim_start_matches('[').trim_end_matches(']');
    trimmed
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn tracker_has_progress_or_notes(tracker: &str) -> bool {
    let lower = tracker.to_ascii_lowercase();
    if lower.contains("## notes") {
        return true;
    }
    ["[x]", "[~]", "[!]", "[/]"]
        .iter()
        .any(|marker| lower.contains(marker))
}

pub fn generate_tracker_markdown_from_plan(plan_markdown: &str) -> Option<String> {
    let implementation = section_body(plan_markdown, "Implementation Steps")?;
    let title = plan_markdown
        .lines()
        .find_map(|line| line.trim().strip_prefix("# ").map(str::trim))
        .filter(|line| !line.is_empty())
        .unwrap_or("Implementation Plan");

    let mut items = Vec::new();
    for line in implementation
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if !is_numbered_line(line) {
            continue;
        }
        let description = line
            .split_once(['.', ')'])
            .map(|(_, rest)| rest.trim())
            .unwrap_or(line);
        let segments = description.split("->").map(str::trim).collect::<Vec<_>>();
        let main = segments.first().copied().unwrap_or_default();
        if main.is_empty() {
            continue;
        }

        let mut entry = format!("- [ ] {}\n", main);
        for segment in segments.iter().skip(1) {
            if let Some(files) = segment.strip_prefix("files:") {
                let values = parse_bracket_list(files);
                if !values.is_empty() {
                    entry.push_str(&format!("  files: {}\n", values.join(", ")));
                }
                continue;
            }
            if let Some(outcome) = segment.strip_prefix("outcome:") {
                let outcome = outcome.trim().trim_start_matches('[').trim_end_matches(']');
                if !outcome.is_empty() {
                    entry.push_str(&format!("  outcome: {}\n", outcome));
                }
                continue;
            }
            if let Some(verify) = segment.strip_prefix("verify:") {
                let values = parse_bracket_list(verify);
                if values.is_empty() {
                    let trimmed = verify.trim();
                    if !trimmed.is_empty() {
                        entry.push_str(&format!("  verify: {}\n", trimmed));
                    }
                } else {
                    for value in values {
                        entry.push_str(&format!("  verify: {}\n", value));
                    }
                }
            }
        }
        items.push(entry);
    }

    if items.is_empty() {
        return None;
    }

    Some(format!(
        "# {}\n\n## Plan of Work\n\n{}",
        title,
        items.concat().trim_end()
    ))
}

async fn persist_global_tracker_if_missing(
    workspace_root: &Path,
    tracker_markdown: &str,
) -> Result<()> {
    if workspace_root.as_os_str().is_empty() {
        return Ok(());
    }
    let task_file = workspace_root
        .join(".vtcode")
        .join("tasks")
        .join("current_task.md");
    if task_file.exists() {
        return Ok(());
    }
    if let Some(parent) = task_file.parent() {
        ensure_dir_exists(parent).await.with_context(|| {
            format!(
                "Failed to create task tracker directory: {}",
                parent.display()
            )
        })?;
    }
    write_file_with_context(&task_file, tracker_markdown, "task checklist")
        .await
        .with_context(|| format!("Failed to write task checklist: {}", task_file.display()))?;
    Ok(())
}

pub async fn sync_tracker_into_plan_file(plan_file: &Path, tracker_markdown: &str) -> Result<()> {
    let plan_content = read_file_with_context(plan_file, "plan file")
        .await
        .with_context(|| format!("Failed to read plan file: {}", plan_file.display()))?;
    let updated = render_plan_with_tracker(&plan_content, Some(tracker_markdown));
    write_file_with_context(plan_file, &updated, "plan file")
        .await
        .with_context(|| format!("Failed to write plan file: {}", plan_file.display()))?;
    Ok(())
}

pub async fn persist_plan_draft(
    state: &PlanModeState,
    plan_markdown: &str,
) -> Result<PersistedPlanDraft> {
    let plan_file = state
        .get_plan_file()
        .await
        .context("No active plan file. Call enter_plan_mode first.")?;
    let existing_plan = read_file_with_context(&plan_file, "plan file").await.ok();
    let tracker_file = tracker_file_for_plan_file(&plan_file);
    let (existing_tracker, tracker_from_sidecar) = if let Some(path) = tracker_file.as_ref() {
        if path.exists() {
            (read_file_with_context(path, "plan tracker file").await.ok(), true)
        } else {
            (
                existing_plan
                    .as_deref()
                    .and_then(extract_embedded_tracker)
                    .filter(|content| !content.trim().is_empty()),
                false,
            )
        }
    } else {
        (
            existing_plan
                .as_deref()
                .and_then(extract_embedded_tracker)
                .filter(|content| !content.trim().is_empty()),
            false,
        )
    };

    let should_refresh_embedded = !tracker_from_sidecar
        && existing_tracker
            .as_deref()
            .is_some_and(|tracker| !tracker_has_progress_or_notes(tracker));
    let tracker_to_persist = if should_refresh_embedded {
        generate_tracker_markdown_from_plan(plan_markdown).or(existing_tracker.clone())
    } else {
        existing_tracker
            .clone()
            .or_else(|| generate_tracker_markdown_from_plan(plan_markdown))
    };
    let canonical_plan = render_plan_with_tracker(plan_markdown, tracker_to_persist.as_deref());
    write_file_with_context(&plan_file, &canonical_plan, "plan file")
        .await
        .with_context(|| format!("Failed to write plan file: {}", plan_file.display()))?;

    if let (Some(path), Some(tracker_markdown)) =
        (tracker_file.as_ref(), tracker_to_persist.as_deref())
    {
        if let Some(parent) = path.parent() {
            ensure_dir_exists(parent).await.with_context(|| {
                format!(
                    "Failed to create plan tracker directory: {}",
                    parent.display()
                )
            })?;
        }
        write_file_with_context(path, tracker_markdown, "plan tracker file")
            .await
            .with_context(|| format!("Failed to write plan tracker file: {}", path.display()))?;
        let workspace_root = state.workspace_root().unwrap_or_default();
        persist_global_tracker_if_missing(&workspace_root, tracker_markdown).await?;
    }

    Ok(PersistedPlanDraft {
        plan_file,
        tracker_file,
        validation: validate_plan_content(plan_markdown),
    })
}

fn render_initial_plan_file_content(
    plan_title: &str,
    description: Option<&str>,
    plan_file: &Path,
    validation_hints: &ValidationCommandHints,
) -> String {
    let mut content = format!("# {}\n\n", plan_title);
    content.push_str("Status: drafting\n");
    content.push_str(&format!(
        "Created: {}\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    ));
    content.push_str(&format!("Plan file: `{}`\n", plan_file.display()));
    if let Some(description) = description.map(str::trim).filter(|value| !value.is_empty()) {
        content.push_str(&format!("Description: {}\n", description));
    }
    content.push('\n');
    content.push_str("> Plan Mode is active. Research first, then materialize one concrete `<proposed_plan>` draft here.\n");
    content.push_str(&format!(
        "> Suggested validation defaults: build/lint {}; tests {}.\n",
        validation_hints.build_and_lint, validation_hints.tests
    ));
    content
}

#[derive(Debug, Clone)]
struct ValidationCommandHints {
    build_and_lint: String,
    tests: String,
}

fn package_manager_for_workspace(workspace_root: &Path) -> &'static str {
    if workspace_root.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if workspace_root.join("yarn.lock").exists() {
        "yarn"
    } else if workspace_root.join("bun.lockb").exists() || workspace_root.join("bun.lock").exists()
    {
        "bun"
    } else {
        "npm"
    }
}

fn node_script_command(pm: &str, script: &str) -> String {
    match pm {
        "yarn" => format!("yarn {script}"),
        "bun" => format!("bun run {script}"),
        _ => format!("{pm} run {script}"),
    }
}

fn package_json_has_script(workspace_root: &Path, script: &str) -> bool {
    let path = workspace_root.join("package.json");
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(json) = serde_json::from_str::<Value>(&content) else {
        return false;
    };
    json.get("scripts")
        .and_then(Value::as_object)
        .is_some_and(|scripts| scripts.contains_key(script))
}

fn detect_validation_command_hints(workspace_root: &Path) -> ValidationCommandHints {
    if workspace_root.join("Cargo.toml").exists() {
        return ValidationCommandHints {
            build_and_lint:
                "`cargo check`; `cargo clippy --workspace --all-targets -- -D warnings`".to_string(),
            tests: "`cargo test` (or `cargo nextest run` if nextest is configured)".to_string(),
        };
    }

    if workspace_root.join("package.json").exists() {
        let pm = package_manager_for_workspace(workspace_root);
        let has_build = package_json_has_script(workspace_root, "build");
        let has_lint = package_json_has_script(workspace_root, "lint");
        let has_test = package_json_has_script(workspace_root, "test");

        let build_and_lint = match (has_build, has_lint) {
            (true, true) => format!(
                "`{}`; `{}`",
                node_script_command(pm, "build"),
                node_script_command(pm, "lint")
            ),
            (true, false) => format!(
                "`{}`; plus configured lint command for the workspace",
                node_script_command(pm, "build")
            ),
            (false, true) => format!(
                "`{}`; plus configured build/typecheck command for the workspace",
                node_script_command(pm, "lint")
            ),
            (false, false) => {
                format!("Use configured {pm} build/lint (or typecheck) scripts for this workspace")
            }
        };
        let tests = if has_test {
            format!("`{}`", node_script_command(pm, "test"))
        } else {
            format!("Use configured {pm} test command for this workspace")
        };

        return ValidationCommandHints {
            build_and_lint,
            tests,
        };
    }

    if workspace_root.join("pyproject.toml").exists()
        || workspace_root.join("requirements.txt").exists()
        || workspace_root.join("setup.py").exists()
    {
        return ValidationCommandHints {
            build_and_lint:
                "`python -m compileall .`; run configured linter (for example `ruff check .`)"
                    .to_string(),
            tests: "`pytest`".to_string(),
        };
    }

    if workspace_root.join("go.mod").exists() {
        return ValidationCommandHints {
            build_and_lint: "`go build ./...`; `go vet ./...`".to_string(),
            tests: "`go test ./...`".to_string(),
        };
    }

    if workspace_root.join("Makefile").exists() {
        return ValidationCommandHints {
            build_and_lint: "`make lint` (or `make build` if no lint target exists)".to_string(),
            tests: "`make test`".to_string(),
        };
    }

    ValidationCommandHints {
        build_and_lint: "[project build and lint command(s)]".to_string(),
        tests: "[project test command(s)]".to_string(),
    }
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let args: EnterPlanModeArgs = serde_json::from_value(args).unwrap_or(EnterPlanModeArgs {
            plan_name: None,
            description: None,
            plan_path: None,
            require_confirmation: false,
            approved: false,
        });

        // Check if already in plan mode
        if self.state.is_active() {
            return Ok(json!({
                "status": "already_active",
                "message": "Plan Mode is already active. Continue with your planning workflow.",
                "plan_file": self.state.get_plan_file().await.map(|p| p.display().to_string())
            }));
        }

        // Resolve target plan path. Defaults to .vtcode/plans/, but allows explicit custom location.
        let plan_name = self.generate_plan_name(args.plan_name.as_deref());
        let plan_file = if let Some(raw_path) = args.plan_path.as_deref() {
            let trimmed = raw_path.trim();
            if Path::new(trimmed).is_absolute() {
                PathBuf::from(trimmed)
            } else {
                self.state
                    .workspace_root()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(trimmed)
            }
        } else {
            let plans_dir = self.state.plans_dir();
            plans_dir.join(format!("{}.md", plan_name))
        };
        let workspace_root = self
            .state
            .workspace_root()
            .unwrap_or_else(|| PathBuf::from("."));
        let validation_hints = detect_validation_command_hints(&workspace_root);

        if args.require_confirmation && !args.approved {
            self.state
                .set_phase(PlanLifecyclePhase::EnterPendingApproval);
            return Ok(json!({
                "status": "pending_confirmation",
                "requires_confirmation": true,
                "message": "Plan Mode entry requires user confirmation.",
                "plan_file": plan_file.display().to_string(),
                "plan_title": title_from_plan_name(&plan_name),
                "description": args.description,
            }));
        }

        // Enable plan mode only after explicit approval.
        self.state.enable();
        self.state.set_phase(PlanLifecyclePhase::ActiveDrafting);

        if let Some(parent) = plan_file.parent() {
            ensure_dir_exists(parent).await.with_context(|| {
                format!("Failed to create plan directory: {}", parent.display())
            })?;
        }

        let initial_content = render_initial_plan_file_content(
            &title_from_plan_name(&plan_name),
            args.description.as_deref(),
            &plan_file,
            &validation_hints,
        );

        write_file_with_context(&plan_file, &initial_content, "plan file")
            .await
            .with_context(|| format!("Failed to create plan file: {}", plan_file.display()))?;

        // Track the current plan file
        self.state.set_plan_file(Some(plan_file.clone())).await;
        let baseline = tokio::fs::metadata(&plan_file)
            .await
            .and_then(|meta| meta.modified())
            .unwrap_or_else(|_| SystemTime::now());
        self.state.set_plan_baseline(Some(baseline)).await;

        Ok(json!({
            "status": "success",
            "message": "Entered Plan Mode. You are now in read-only mode for exploration and planning.",
            "plan_file": plan_file.display().to_string(),
            "instructions": [
                "1. Explore files and capture repository facts before drafting the plan",
                "2. Ask or close only material blocking decisions",
                "3. Emit one concrete <proposed_plan> draft and persist it to the plan file",
                "4. Use exit_plan_mode when ready for the user to review and approve"
            ],
            "workflow_phases": [
                "Phase A: Explore facts",
                "Phase B: Close open decisions",
                "Phase C: Draft one proposed plan"
            ]
        }))
    }

    fn name(&self) -> &'static str {
        tools::ENTER_PLAN_MODE
    }

    fn description(&self) -> &'static str {
        "Enter Plan Mode to switch to read-only exploration. In Plan Mode, you can only read files, search code, and write canonical plan artifacts. Use this when you need to understand requirements before making changes."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "properties": {
                "plan_name": {
                    "type": "string",
                    "description": "Optional name for the plan file (e.g., 'add-auth-middleware'). Defaults to timestamp-based name."
                },
                "plan_path": {
                    "type": "string",
                    "description": "Optional explicit plan file path. Use this to persist plans in a custom workspace path instead of the default .vtcode/plans location."
                },
                "description": {
                    "type": "string",
                    "description": "Optional initial description of what you're planning to implement."
                }
            },
            "required": []
        }))
    }

    fn is_mutating(&self) -> bool {
        false // This is a mode switch, not a file mutation
    }

    fn is_parallel_safe(&self) -> bool {
        false // Mode switches should be sequential
    }
}

// ============================================================================
// Exit Plan Mode Tool
// ============================================================================

/// Arguments for exiting plan mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitPlanModeArgs {
    /// Optional: Reason for exiting (e.g., "planning complete", "need more info")
    #[serde(default)]
    pub reason: Option<String>,
}

/// Tool for exiting plan mode
pub struct ExitPlanModeTool {
    state: PlanModeState,
}

impl ExitPlanModeTool {
    pub fn new(state: PlanModeState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Tool for ExitPlanModeTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let args: ExitPlanModeArgs =
            serde_json::from_value(args).unwrap_or(ExitPlanModeArgs { reason: None });

        // Check if not in plan mode
        if !self.state.is_active() {
            return Ok(json!({
                "status": "not_active",
                "message": "Plan Mode is not currently active."
            }));
        }

        // Get the current plan file
        let plan_file = self.state.get_plan_file().await;
        let plan_baseline = self.state.plan_baseline().await;

        // Read the plan content if file exists
        let (raw_plan_content, plan_title) = if let Some(ref path) = plan_file {
            let title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Implementation Plan")
                .to_string();
            match read_file_with_context(path, "plan file").await {
                Ok(content) => (Some(content), title),
                Err(_) => (None, title),
            }
        } else {
            (None, "Implementation Plan".to_string())
        };

        // Merge optional plan task tracker sidecar content (if present) so the
        // confirmation modal and readiness checks see the full plan state.
        let tracker_file = plan_file
            .as_ref()
            .and_then(|path| tracker_file_for_plan_file(path));
        let tracker_content = if let Some(ref path) = tracker_file {
            if path.exists() {
                read_file_with_context(path, "plan tracker file").await.ok()
            } else {
                None
            }
        } else {
            None
        };
        let plan_content = merge_plan_content(raw_plan_content, tracker_content);

        // Parse structured plan content for the confirmation dialog
        let structured_plan = plan_content.as_ref().map(|content| {
            PlanContent::from_markdown(
                plan_title.clone(),
                content,
                plan_file.as_ref().map(|p| p.display().to_string()),
            )
        });

        let plan_validation = plan_content
            .as_deref()
            .map(validate_plan_content)
            .unwrap_or_default();
        let plan_ready = plan_validation.is_ready();
        let plan_recently_updated =
            if let (Some(path), Some(baseline)) = (plan_file.as_ref(), plan_baseline) {
                match tokio::fs::metadata(path)
                    .await
                    .and_then(|meta| meta.modified())
                {
                    Ok(modified) => modified > baseline,
                    Err(_) => false,
                }
            } else {
                true
            };

        if !plan_ready || !plan_recently_updated {
            let mut blockers = Vec::new();
            if !plan_validation.missing_sections.is_empty() {
                blockers.push(format!(
                    "Missing or incomplete sections: {}",
                    plan_validation.missing_sections.join(", ")
                ));
            }
            if !plan_validation.placeholder_tokens.is_empty() {
                blockers.push(format!(
                    "Template placeholders still present: {}",
                    plan_validation.placeholder_tokens.join(", ")
                ));
            }
            if !plan_validation.open_decisions.is_empty() {
                blockers.push(format!(
                    "Open decisions remain: {}",
                    plan_validation.open_decisions.join(" | ")
                ));
            }
            if !plan_recently_updated {
                blockers
                    .push("Plan file has not been updated since entering Plan Mode.".to_string());
            }
            return Ok(json!({
                "status": "not_ready",
                "message": "Plan not ready for confirmation. Persist a concrete plan with complete sections, no template placeholders, and no open decisions, then retry.",
                "reason": args.reason,
                "plan_file": plan_file.map(|p| p.display().to_string()),
                "plan_tracker_file": tracker_file.map(|p| p.display().to_string()),
                "plan_content": plan_content,
                "validation": {
                    "missing_sections": plan_validation.missing_sections,
                    "placeholder_tokens": plan_validation.placeholder_tokens,
                    "open_decisions": plan_validation.open_decisions,
                    "implementation_step_count": plan_validation.implementation_step_count,
                    "validation_item_count": plan_validation.validation_item_count,
                    "assumption_count": plan_validation.assumption_count,
                },
                "blockers": blockers,
                "requires_confirmation": false
            }));
        }

        self.state.set_phase(PlanLifecyclePhase::ReviewPending);

        // Build plan summary for JSON response
        let plan_summary = structured_plan.as_ref().map(|p| {
            json!({
                "title": p.title,
                "summary": p.summary,
                "total_steps": p.total_steps,
                "completed_steps": p.completed_steps,
                "progress_percent": p.progress_percent(),
                "phases": p.phases.iter().map(|phase| {
                    json!({
                        "name": phase.name,
                        "completed": phase.completed,
                        "steps": phase.steps.iter().map(|step| {
                            json!({
                                "number": step.number,
                                "description": step.description,
                                "completed": step.completed
                            })
                        }).collect::<Vec<_>>()
                    })
                }).collect::<Vec<_>>(),
                "open_questions": p.open_questions
            })
        });

        // NOTE: The actual plan mode state transition is now handled by the caller
        // after the user confirms via the plan confirmation dialog.
        // We keep plan mode active until confirmation is received.
        // The caller should:
        // 1. Display the shared plan confirmation overlay
        // 2. Wait for user approval (PlanApproved action)
        // 3. Only then disable plan mode and enable edit tools

        Ok(json!({
            "status": "pending_confirmation",
            "message": "Plan ready for review. Waiting for user confirmation before execution.",
            "reason": args.reason,
            "plan_file": plan_file.map(|p| p.display().to_string()),
            "plan_tracker_file": tracker_file.map(|p| p.display().to_string()),
            "plan_content": plan_content,
            "plan_summary": plan_summary,
            "next_steps": [
                "User will see the Implementation Blueprint panel",
                "User can choose: Execute or Stay in Plan Mode",
                "If approved, Plan Mode will be disabled and mutating tools will be enabled",
                "Execute the plan step by step after approval"
            ],
            "requires_confirmation": true
        }))
    }

    fn name(&self) -> &'static str {
        tools::EXIT_PLAN_MODE
    }

    fn description(&self) -> &'static str {
        "Exit Plan Mode after finishing your plan. This signals that you're done planning and ready for user review. The plan file content will be shown to the user for approval. Only use this when the task requires planning implementation steps, not for research tasks."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "properties": {
                "reason": {
                    "type": "string",
                    "description": "Optional reason for exiting plan mode (e.g., 'planning complete', 'need clarification from user')"
                }
            },
            "required": []
        }))
    }

    fn is_mutating(&self) -> bool {
        false
    }

    fn is_parallel_safe(&self) -> bool {
        false
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_enter_plan_mode() {
        let temp_dir = TempDir::new().unwrap();
        let state = PlanModeState::new(temp_dir.path().to_path_buf());
        let tool = EnterPlanModeTool::new(state.clone());

        // Initially not in plan mode
        assert!(!state.is_active());

        // Enter plan mode
        let result = tool
            .execute(json!({
                "plan_name": "test-plan",
                "description": "Test planning"
            }))
            .await
            .unwrap();

        // Should be in plan mode now
        assert!(state.is_active());
        assert_eq!(result["status"], "success");

        // Plan file should exist
        let plan_file = state.get_plan_file().await.unwrap();
        assert!(plan_file.exists());
        assert_eq!(
            plan_file,
            temp_dir
                .path()
                .join(".vtcode")
                .join("plans")
                .join("test-plan.md")
        );

        let content = std::fs::read_to_string(&plan_file).unwrap();
        assert!(content.contains("# Test Plan"));
        assert!(content.contains("Status: drafting"));
        assert!(content.contains(&format!("Plan file: `{}`", plan_file.display())));
        assert!(content.contains("Description: Test planning"));
        assert!(!content.contains("Repository facts checked"));
        assert!(!content.contains("[Step]"));
        assert!(!content.contains("## Implementation Steps"));
    }

    #[tokio::test]
    async fn test_enter_plan_mode_returns_pending_confirmation_when_requested() {
        let temp_dir = TempDir::new().unwrap();
        let state = PlanModeState::new(temp_dir.path().to_path_buf());
        let tool = EnterPlanModeTool::new(state.clone());

        let result = tool
            .execute(json!({
                "plan_name": "confirm-me",
                "require_confirmation": true
            }))
            .await
            .unwrap();

        assert_eq!(result["status"], "pending_confirmation");
        assert_eq!(result["requires_confirmation"], true);
        assert!(!state.is_active());
        assert_eq!(state.phase(), PlanLifecyclePhase::EnterPendingApproval);
        assert!(state.get_plan_file().await.is_none());
    }

    #[test]
    fn test_detect_validation_hints_for_rust_workspace() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(temp_dir.path().join("Cargo.toml"), "[package]\nname='x'\n").unwrap();

        let hints = detect_validation_command_hints(temp_dir.path());
        assert!(hints.build_and_lint.contains("cargo check"));
        assert!(hints.build_and_lint.contains("cargo clippy"));
        assert!(hints.tests.contains("cargo test"));
    }

    #[test]
    fn test_detect_validation_hints_for_node_workspace() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(
            temp_dir.path().join("package.json"),
            r#"{"name":"x","scripts":{"build":"tsc","lint":"eslint .","test":"vitest run"}}"#,
        )
        .unwrap();
        std::fs::write(temp_dir.path().join("pnpm-lock.yaml"), "lockfileVersion: 9").unwrap();

        let hints = detect_validation_command_hints(temp_dir.path());
        assert!(hints.build_and_lint.contains("pnpm run build"));
        assert!(hints.build_and_lint.contains("pnpm run lint"));
        assert_eq!(hints.tests, "`pnpm run test`");
    }

    #[tokio::test]
    async fn test_exit_plan_mode() {
        let temp_dir = TempDir::new().unwrap();
        let state = PlanModeState::new(temp_dir.path().to_path_buf());

        // Set up plan mode
        state.enable();
        let plans_dir = state.plans_dir();
        std::fs::create_dir_all(&plans_dir).unwrap();
        let plan_file = plans_dir.join("test.md");
        std::fs::write(
            &plan_file,
            "# Test Plan\n\n## Summary\nTest summary\n\n## Implementation Steps\n1. Prepare the change -> files: [src/main.rs] -> verify: [cargo test]\n2. Ship the update -> files: [src/lib.rs] -> verify: [cargo check]\n\n## Test Cases and Validation\n1. Run `cargo test`\n2. Run `cargo check`\n\n## Assumptions and Defaults\n1. The current task scope stays unchanged during review.\n",
        )
        .unwrap();
        state.set_plan_file(Some(plan_file)).await;

        let tool = ExitPlanModeTool::new(state.clone());

        // Exit plan mode
        let result = tool
            .execute(json!({
                "reason": "planning complete"
            }))
            .await
            .unwrap();

        // Plan mode should still be active - waiting for user confirmation (HITL)
        assert!(state.is_active());
        assert_eq!(result["status"], "pending_confirmation");
        assert!(result["requires_confirmation"].as_bool().unwrap());
        assert!(
            result["plan_content"]
                .as_str()
                .unwrap()
                .contains("Test Plan")
        );
        // Verify structured plan summary is included
        assert!(result["plan_summary"].is_object());
        let summary = &result["plan_summary"];
        assert!(summary["total_steps"].as_u64().unwrap_or_default() >= 2);
        assert_eq!(summary["completed_steps"], 0);
        assert_eq!(state.phase(), PlanLifecyclePhase::ReviewPending);
    }

    #[tokio::test]
    async fn test_exit_plan_mode_merges_plan_tracker_sidecar_content() {
        let temp_dir = TempDir::new().unwrap();
        let state = PlanModeState::new(temp_dir.path().to_path_buf());

        state.enable();
        let plans_dir = state.plans_dir();
        std::fs::create_dir_all(&plans_dir).unwrap();
        let plan_file = plans_dir.join("merge-test.md");
        std::fs::write(
            &plan_file,
            "# Test Plan\n\n## Summary\nMerge tracker sidecar into the canonical review artifact.\n\n## Implementation Steps\n1. Keep the base plan content -> files: [src/base.rs] -> verify: [cargo test]\n\n## Test Cases and Validation\n1. Run `cargo test`\n\n## Assumptions and Defaults\n1. Tracker sidecar content should remain visible during review.\n",
        )
        .unwrap();
        let tracker_file = plans_dir.join("merge-test.tasks.md");
        std::fs::write(
            &tracker_file,
            "# Updated Plan\n\n## Plan of Work\n- [~] Tracker step\n",
        )
        .unwrap();
        state.set_plan_file(Some(plan_file)).await;

        let tool = ExitPlanModeTool::new(state.clone());
        let result = tool
            .execute(json!({ "reason": "merge test" }))
            .await
            .unwrap();

        assert_eq!(result["status"], "pending_confirmation");
        assert_eq!(
            result["plan_tracker_file"],
            tracker_file.display().to_string()
        );
        let plan_content = result["plan_content"].as_str().unwrap_or_default();
        assert!(plan_content.contains("Keep the base plan content"));
        assert!(plan_content.contains("Tracker step"));
    }

    #[tokio::test]
    async fn test_exit_plan_mode_not_ready_without_actionable_steps() {
        let temp_dir = TempDir::new().unwrap();
        let state = PlanModeState::new(temp_dir.path().to_path_buf());

        state.enable();
        let plans_dir = state.plans_dir();
        std::fs::create_dir_all(&plans_dir).unwrap();
        let plan_file = plans_dir.join("test.md");
        std::fs::write(
            &plan_file,
            "# Test Plan\n\n## Plan of Work\n(Describe the sequence of edits and additions. For each edit, name the file and location.)\n",
        )
        .unwrap();
        state.set_plan_file(Some(plan_file)).await;

        let tool = ExitPlanModeTool::new(state.clone());
        let result = tool.execute(json!({})).await.unwrap();

        assert_eq!(result["status"], "not_ready");
        assert_eq!(result["requires_confirmation"], false);
        assert!(
            result["validation"]["missing_sections"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value.as_str() == Some("Summary"))
        );
    }

    #[tokio::test]
    async fn test_exit_plan_mode_not_ready_when_plan_not_updated_since_baseline() {
        let temp_dir = TempDir::new().unwrap();
        let state = PlanModeState::new(temp_dir.path().to_path_buf());
        let tool = EnterPlanModeTool::new(state.clone());

        let result = tool
            .execute(json!({ "plan_name": "baseline-test" }))
            .await
            .unwrap();
        assert_eq!(result["status"], "success");

        let plan_file = state.get_plan_file().await.unwrap();
        std::fs::write(&plan_file, "# Test Plan\n\n## Plan of Work\n- Step one\n").unwrap();

        // Reset baseline to simulate no updates after template creation.
        let baseline = std::fs::metadata(&plan_file)
            .and_then(|meta| meta.modified())
            .unwrap();
        state.set_plan_baseline(Some(baseline)).await;

        let exit_tool = ExitPlanModeTool::new(state.clone());
        let exit_result = exit_tool.execute(json!({})).await.unwrap();

        assert_eq!(exit_result["status"], "not_ready");
        assert_eq!(exit_result["requires_confirmation"], false);
    }

    #[tokio::test]
    async fn test_already_in_plan_mode() {
        let temp_dir = TempDir::new().unwrap();
        let state = PlanModeState::new(temp_dir.path().to_path_buf());
        state.enable();

        let tool = EnterPlanModeTool::new(state);
        let result = tool.execute(json!({})).await.unwrap();

        assert_eq!(result["status"], "already_active");
    }

    #[test]
    fn validate_plan_content_rejects_placeholder_template() {
        let report = validate_plan_content(
            r#"# Test Plan

## Summary
[2-4 lines: goal, user impact, what will change, what will not]

## Implementation Steps
1. [Step] -> files: [paths] -> verify: [check]

## Test Cases and Validation
1. Build and lint: [project build and lint command(s)]

## Assumptions and Defaults
1. [Explicit assumption]
"#,
        );

        assert!(!report.is_ready());
        assert!(!report.placeholder_tokens.is_empty());
    }

    #[test]
    fn validate_plan_content_accepts_concrete_plan() {
        let report = validate_plan_content(
            r#"# Fix Plan Mode

## Summary
Persist the reviewed plan draft and route execution through explicit approval.

## Implementation Steps
1. Add plan lifecycle state -> files: [vtcode-core/src/tools/handlers/plan_mode.rs] -> verify: [cargo test -p vtcode-core test_enter_plan_mode -- --nocapture]
2. Gate plan entry with overlay approval -> files: [src/agent/runloop/unified/tool_pipeline/execution_plan_mode.rs] -> verify: [cargo test -p vtcode test_run_tool_call_prevalidated_allows_task_tracker_in_plan_mode -- --nocapture]

## Test Cases and Validation
1. Build and lint: cargo check
2. Tests: cargo test -p vtcode-core test_enter_plan_mode -- --nocapture

## Assumptions and Defaults
1. Keep tracker sidecars for compatibility.
2. Reuse the existing overlay infrastructure.
"#,
        );

        assert!(report.is_ready());
    }

    #[tokio::test]
    async fn persist_plan_draft_generates_tracker_and_global_task_file() {
        let temp_dir = TempDir::new().unwrap();
        let state = PlanModeState::new(temp_dir.path().to_path_buf());
        let tool = EnterPlanModeTool::new(state.clone());
        tool.execute(json!({"plan_name":"draft-sync","approved":true}))
            .await
            .unwrap();

        let persisted = persist_plan_draft(
            &state,
            r#"# Draft Sync

## Summary
Persist a concrete draft and seed tracker state.

## Implementation Steps
1. Persist the plan -> files: [vtcode-core/src/tools/handlers/plan_mode.rs] -> verify: [cargo test]
2. Sync the tracker -> files: [vtcode-core/src/tools/handlers/task_tracker.rs] -> verify: [cargo test]

## Test Cases and Validation
1. Build and lint: cargo check
2. Tests: cargo test

## Assumptions and Defaults
1. Keep task tracker mirrors.
"#,
        )
        .await
        .unwrap();

        let tracker_file = persisted.tracker_file.expect("tracker file should exist");
        let plan_content = std::fs::read_to_string(&persisted.plan_file).unwrap();
        let tracker_content = std::fs::read_to_string(&tracker_file).unwrap();
        let global_task = std::fs::read_to_string(
            temp_dir
                .path()
                .join(".vtcode")
                .join("tasks")
                .join("current_task.md"),
        )
        .unwrap();

        assert!(persisted.validation.is_ready());
        assert!(plan_content.contains(PLAN_TRACKER_START));
        assert!(plan_content.contains("Persist the plan"));
        assert!(tracker_content.contains("- [ ] Persist the plan"));
        assert!(global_task.contains("- [ ] Persist the plan"));
    }
}
