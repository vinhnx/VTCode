//! Plan mode tools for entering, exiting, and managing planning workflow
//!
//! These tools allow the agent to programmatically enter and exit plan mode,
//! similar to Claude Code's plan mode implementation. The agent can:
//! - Enter plan mode to switch to read-only exploration
//! - Exit plan mode (triggering plan review) to start implementation
//! - Write plans to `/tmp/vtcode-plans/` by default (with optional custom path)
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
}

impl PlanModeState {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            is_active: Arc::new(AtomicBool::new(false)),
            current_plan_file: Arc::new(tokio::sync::RwLock::new(None)),
            plan_baseline: Arc::new(tokio::sync::RwLock::new(None)),
            workspace_root,
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
    }

    /// Get the workspace root path
    pub fn workspace_root(&self) -> Option<PathBuf> {
        if self.workspace_root.as_os_str().is_empty() {
            None
        } else {
            Some(self.workspace_root.clone())
        }
    }

    /// Get the plans directory path (ephemeral by default)
    pub fn plans_dir(&self) -> PathBuf {
        std::env::temp_dir()
            .join("vtcode-plans")
            .join(workspace_slug_for_tmp(&self.workspace_root))
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

    /// Ensure plans directory exists
    pub async fn ensure_plans_dir(&self) -> Result<PathBuf> {
        let dir = self.plans_dir();
        ensure_dir_exists(&dir)
            .await
            .with_context(|| format!("Failed to create plans directory: {}", dir.display()))?;
        Ok(dir)
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
        });

        // Check if already in plan mode
        if self.state.is_active() {
            return Ok(json!({
                "status": "already_active",
                "message": "Plan Mode is already active. Continue with your planning workflow.",
                "plan_file": self.state.get_plan_file().await.map(|p| p.display().to_string())
            }));
        }

        // Enable plan mode
        self.state.enable();

        // Resolve target plan path. Defaults to /tmp, but allows explicit custom location.
        let plan_name = self.generate_plan_name(args.plan_name.as_deref());
        let plan_file = if let Some(raw_path) = args.plan_path.as_deref() {
            let trimmed = raw_path.trim();
            let resolved = if Path::new(trimmed).is_absolute() {
                PathBuf::from(trimmed)
            } else {
                self.state
                    .workspace_root()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(trimmed)
            };
            if let Some(parent) = resolved.parent() {
                ensure_dir_exists(parent).await.with_context(|| {
                    format!("Failed to create plan directory: {}", parent.display())
                })?;
            }
            resolved
        } else {
            let plans_dir = self.state.ensure_plans_dir().await?;
            plans_dir.join(format!("{}.md", plan_name))
        };
        let workspace_root = self
            .state
            .workspace_root()
            .unwrap_or_else(|| PathBuf::from("."));
        let validation_hints = detect_validation_command_hints(&workspace_root);

        // Create initial plan file using the canonical blueprint structure.
        let initial_content = format!(
            r#"• Scope checkpoint: [what is locked] / [what remains open].
• Decision needed: [single high-impact choice] and why it affects
implementation.

• Questions 1/1 answered
• [exact question text]
answer: [selected option label]

• Locked decision: [choice], so implementation will [concrete consequence].
• Next open decision: [if any], otherwise: "No remaining scope decisions;
drafting final plan."

<proposed_plan>
• Proposed Plan


# {}

## Summary

{}

## Scope Locked

1. [Decision A]
2. [Decision B]
3. [Decision C]

## Public API / Interface Changes

1. [Removed/added/changed API, command, config, schema]
2. [Tooling/runtime behavior changes]
3. [Compatibility or break behavior]

## Implementation Plan

1. [Step] → files: [paths] → verify: [check]
2. [Step] → files: [paths] → verify: [check]
3. [Step] → files: [paths] → verify: [check]

## Test Cases and Validation

1. Build and lint: {}
2. Tests: {}
3. Targeted behavior checks: [explicit commands/manual checks]
4. Regression checks: [what must not break]

## Assumptions and Defaults

1. [Explicit assumption]
2. [Default chosen when user did not specify]
3. [Out-of-scope items intentionally not changed]

</proposed_plan>

> Plan confirmation is shown as an interactive list in the TUI when you exit Plan Mode.

> Plan file: `{}`
> Plan created: {}
"#,
            plan_name.replace('-', " ").to_uppercase(),
            args.description
                .as_deref()
                .unwrap_or("[2-4 lines: goal, user impact, what will change, what will not]"),
            validation_hints.build_and_lint,
            validation_hints.tests,
            plan_file.display(),
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
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
                "1. Read files and search code to understand the codebase",
                "2. Lock decisions with concise decision-log entries",
                "3. Fill the reasoning+decision log and final proposed plan blueprint",
                "4. Use exit_plan_mode when ready for the user to review and approve"
            ],
            "workflow_phases": [
                "Phase A: Decision Log - lock high-impact choices",
                "Phase B: Final Plan - produce one canonical blueprint",
                "Phase C: HITL Review - execute or revise"
            ]
        }))
    }

    fn name(&self) -> &'static str {
        tools::ENTER_PLAN_MODE
    }

    fn description(&self) -> &'static str {
        "Enter Plan Mode to switch to read-only exploration. In Plan Mode, you can only read files, search code, and write to the plan file. Use this when you need to understand requirements before making changes."
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
                    "description": "Optional explicit plan file path. Use this to persist plans in a custom workspace path instead of the default /tmp location."
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

fn plan_has_actionable_steps(content: &str) -> bool {
    let mut in_action_section = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(header) = trimmed.strip_prefix("## ") {
            let header_lower = header.trim().to_lowercase();
            in_action_section = header_lower == "plan of work"
                || header_lower == "concrete steps"
                || header_lower == "implementation plan"
                || header_lower == "implementation"
                || header_lower.starts_with("phase ");
            continue;
        }

        if !in_action_section {
            continue;
        }

        if trimmed.is_empty() || trimmed.starts_with('(') {
            continue;
        }

        let is_checkbox =
            trimmed.starts_with("[ ]") || trimmed.starts_with("[x]") || trimmed.starts_with("[X]");
        let is_bullet =
            trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ");
        let mut is_numbered = false;
        let mut seen_digit = false;
        for ch in trimmed.chars() {
            if ch.is_ascii_digit() {
                seen_digit = true;
                continue;
            }
            if seen_digit && (ch == '.' || ch == ')') {
                is_numbered = true;
            }
            break;
        }

        if is_checkbox || is_bullet || is_numbered {
            return true;
        }
    }

    false
}

fn tracker_file_for_plan_file(plan_file: &Path) -> Option<PathBuf> {
    let stem = plan_file.file_stem()?.to_str()?;
    Some(plan_file.with_file_name(format!("{stem}.tasks.md")))
}

fn merge_plan_content(
    plan_content: Option<String>,
    tracker_content: Option<String>,
) -> Option<String> {
    match (plan_content, tracker_content) {
        (Some(plan), Some(tracker)) => {
            let plan_trimmed = plan.trim();
            let tracker_trimmed = tracker.trim();
            if plan_trimmed.is_empty() {
                Some(tracker_trimmed.to_string())
            } else if tracker_trimmed.is_empty() {
                Some(plan_trimmed.to_string())
            } else {
                Some(format!("{plan_trimmed}\n\n{tracker_trimmed}\n"))
            }
        }
        (Some(plan), None) => {
            let trimmed = plan.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
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

        let plan_ready = plan_content
            .as_deref()
            .map(plan_has_actionable_steps)
            .unwrap_or(false);
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
            return Ok(json!({
                "status": "not_ready",
                "message": "Plan not ready for confirmation. Add actionable steps under an Implementation Plan/Implementation/Plan of Work/Concrete Steps section (or a Phase section) and update the plan file in this session, then retry.",
                "reason": args.reason,
                "plan_file": plan_file.map(|p| p.display().to_string()),
                "plan_tracker_file": tracker_file.map(|p| p.display().to_string()),
                "plan_content": plan_content,
                "requires_confirmation": false
            }));
        }

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
        // 1. Display the plan confirmation modal using show_plan_confirmation()
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

        let content = std::fs::read_to_string(&plan_file).unwrap();
        assert!(content.contains("## Test Cases and Validation"));
        assert!(content.contains("[project build and lint command(s)]"));
        assert!(content.contains("[project test command(s)]"));
        assert!(content.contains(&format!(
            "> Plan file: `{}`",
            plan_file.display()
        )));
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
        std::fs::write(&plan_file, "# Test Plan\n\n## Summary\nTest summary\n\n## Phase 1: Test\n[ ] Step one\n[x] Step two").unwrap();
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
        assert_eq!(summary["total_steps"], 2);
        assert_eq!(summary["completed_steps"], 1);
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
            "# Test Plan\n\n## Plan of Work\n- [ ] Base step\n",
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
        assert!(plan_content.contains("Base step"));
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
}
