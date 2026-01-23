//! Plan mode tools for entering, exiting, and managing planning workflow
//!
//! These tools allow the agent to programmatically enter and exit plan mode,
//! similar to Claude Code's plan mode implementation. The agent can:
//! - Enter plan mode to switch to read-only exploration
//! - Exit plan mode (triggering plan review) to start implementation
//! - Write plans to `.vtcode/plans/` directory
//!
//! Based on insights from Claude Code's plan mode implementation:
//! - Plan files are written to a dedicated directory
//! - The agent edits its own plan file during planning
//! - Exiting plan mode reads the plan file and starts execution
//! - User confirmation is required before transitioning to execution (HITL)

use crate::config::constants::tools;
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::tools::traits::Tool;
use crate::ui::tui::PlanContent;

/// Shared state for plan mode across tools
#[derive(Debug, Clone)]
pub struct PlanModeState {
    /// Whether plan mode is currently active
    is_active: Arc<AtomicBool>,
    /// Path to the current plan file (if any)
    current_plan_file: Arc<tokio::sync::RwLock<Option<PathBuf>>>,
    /// Workspace root for plan directory
    workspace_root: PathBuf,
}

impl PlanModeState {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            is_active: Arc::new(AtomicBool::new(false)),
            current_plan_file: Arc::new(tokio::sync::RwLock::new(None)),
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

    /// Get the plans directory path
    pub fn plans_dir(&self) -> PathBuf {
        self.workspace_root.join(".vtcode").join("plans")
    }

    /// Set the current plan file
    pub async fn set_plan_file(&self, path: Option<PathBuf>) {
        let mut guard = self.current_plan_file.write().await;
        *guard = path;
    }

    /// Get the current plan file path
    pub async fn get_plan_file(&self) -> Option<PathBuf> {
        self.current_plan_file.read().await.clone()
    }

    /// Ensure plans directory exists
    pub async fn ensure_plans_dir(&self) -> Result<PathBuf> {
        let dir = self.plans_dir();
        tokio::fs::create_dir_all(&dir)
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
                // Generate timestamp-based name
                let now = chrono::Utc::now();
                format!("plan-{}", now.format("%Y%m%d-%H%M%S"))
            }
        }
    }
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let args: EnterPlanModeArgs = serde_json::from_value(args).unwrap_or(EnterPlanModeArgs {
            plan_name: None,
            description: None,
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

        // Create plans directory and plan file
        let plans_dir = self.state.ensure_plans_dir().await?;
        let plan_name = self.generate_plan_name(args.plan_name.as_deref());
        let plan_file = plans_dir.join(format!("{}.md", plan_name));

        // Create initial plan file with template
        let initial_content = format!(
            r#"# {}

## Summary
{}

## Context
- Key files: (to be identified)
- Dependencies: (to be identified)

## Phase 1: Initial Understanding
[ ] Understand the user's request
[ ] Identify relevant code paths
[ ] Ask clarifying questions if needed

## Phase 2: Design
[ ] Provide background context
[ ] Describe requirements and constraints
[ ] Propose implementation approach

## Phase 3: Review
[ ] Verify plan aligns with request
[ ] Check for edge cases
[ ] Clarify remaining questions

## Phase 4: Implementation Steps
1. **Step 1**
   - Files:
   - Details:

## Open Questions
- (Add questions here)

---
*Plan created: {}*
"#,
            plan_name.replace('-', " ").to_uppercase(),
            args.description
                .as_deref()
                .unwrap_or("(Describe the goal here)"),
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        );

        tokio::fs::write(&plan_file, &initial_content)
            .await
            .with_context(|| format!("Failed to create plan file: {}", plan_file.display()))?;

        // Track the current plan file
        self.state.set_plan_file(Some(plan_file.clone())).await;

        Ok(json!({
            "status": "success",
            "message": "Entered Plan Mode. You are now in read-only mode for exploration and planning.",
            "plan_file": plan_file.display().to_string(),
            "active_agent": "planner",
            "instructions": [
                "1. Read files and search code to understand the codebase",
                "2. Ask clarifying questions if requirements are ambiguous",
                "3. Update the plan file with your implementation approach",
                "4. Use exit_plan_mode when ready for the user to review and approve"
            ],
            "workflow_phases": [
                "Phase 1: Initial Understanding - Explore code and ask questions",
                "Phase 2: Design - Propose implementation approach",
                "Phase 3: Review - Verify alignment with user intent",
                "Phase 4: Final Plan - Write detailed implementation steps"
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

        // Read the plan content if file exists
        let (plan_content, plan_title) = if let Some(ref path) = plan_file {
            let title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Implementation Plan")
                .to_string();
            match tokio::fs::read_to_string(path).await {
                Ok(content) => (Some(content), title),
                Err(_) => (None, title),
            }
        } else {
            (None, "Implementation Plan".to_string())
        };

        // Parse structured plan content for the confirmation dialog
        let structured_plan = plan_content.as_ref().map(|content| {
            PlanContent::from_markdown(
                plan_title.clone(),
                content,
                plan_file.as_ref().map(|p| p.display().to_string()),
            )
        });

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
            "plan_content": plan_content,
            "plan_summary": plan_summary,
            "pending_active_agent": "coder",
            "next_steps": [
                "User will see the Implementation Blueprint panel",
                "User can choose: Execute, Edit Plan, or Cancel",
                "If approved, active agent switches to 'coder' and mutating tools will be enabled",
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
    async fn test_already_in_plan_mode() {
        let temp_dir = TempDir::new().unwrap();
        let state = PlanModeState::new(temp_dir.path().to_path_buf());
        state.enable();

        let tool = EnterPlanModeTool::new(state);
        let result = tool.execute(json!({})).await.unwrap();

        assert_eq!(result["status"], "already_active");
    }
}
