//! Exit-planning tool: `finish_planning`.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::constants::tools;
use crate::tools::handlers::planning_workflow::artifacts::{
    merge_plan_content, tracker_file_for_plan_file, validate_plan_content,
};
use crate::tools::handlers::planning_workflow::state::PlanningWorkflowState;
use crate::tools::traits::Tool;
use crate::ui::tui::PlanContent;
use crate::utils::file_utils::read_file_with_context;

/// Arguments for exiting planning workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct FinishPlanningArgs {
    /// Optional: Reason for exiting (e.g., "planning complete", "need more info")
    #[serde(default)]
    pub(super) reason: Option<String>,
}

/// Tool for exiting planning workflow
pub struct FinishPlanningTool {
    state: PlanningWorkflowState,
}

impl FinishPlanningTool {
    pub fn new(state: PlanningWorkflowState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Tool for FinishPlanningTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let args: FinishPlanningArgs = serde_json::from_value(args).unwrap_or(FinishPlanningArgs { reason: None });
        let auto_trigger = args
            .reason
            .as_deref()
            .is_some_and(|reason| reason == "auto_trigger_on_plan_ready");

        // Check if not in planning workflow
        if !self.state.is_active() {
            return Ok(json!({
                "status": "not_active",
                "message": "Planning workflow is not currently active."
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
        let tracker_file = plan_file.as_ref().and_then(|path| tracker_file_for_plan_file(path));
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
            PlanContent::from_markdown(plan_title.clone(), content, plan_file.as_ref().map(|p| p.display().to_string()))
        });

        let plan_validation = plan_content.as_deref().map(validate_plan_content).unwrap_or_default();
        let plan_ready = plan_validation.is_ready();
        let plan_recently_updated = if let (Some(path), Some(baseline)) = (plan_file.as_ref(), plan_baseline) {
            match tokio::fs::metadata(path).await.and_then(|meta| meta.modified()) {
                Ok(modified) => modified > baseline,
                Err(_) => false,
            }
        } else {
            true
        };

        if !plan_ready || !plan_recently_updated {
            let mut blockers = Vec::new();
            if !plan_validation.missing_sections.is_empty() {
                blockers
                    .push(format!("Missing or incomplete sections: {}", plan_validation.missing_sections.join(", ")));
            }
            if !plan_validation.placeholder_tokens.is_empty() {
                blockers.push(format!(
                    "Template placeholders still present: {}",
                    plan_validation.placeholder_tokens.join(", ")
                ));
            }
            if !plan_validation.open_decisions.is_empty() {
                blockers.push(format!("Open decisions remain: {}", plan_validation.open_decisions.join(" | ")));
            }
            if !plan_recently_updated {
                blockers.push("Plan file has not been updated since entering Planning workflow.".to_string());
            }
            if auto_trigger {
                return Ok(json!({
                    "status": "pending_confirmation",
                    "message": "Plan draft is incomplete. Waiting for user confirmation before execution.",
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
                    "requires_confirmation": true,
                    "draft_incomplete": true
                }));
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

        // NOTE: The actual planning workflow state transition is now handled by the caller
        // after the user confirms via the plan confirmation dialog.
        // We keep planning workflow active until confirmation is received.
        // The caller should:
        // 1. Display the shared plan confirmation overlay
        // 2. Wait for user approval (PlanApproved action)
        // 3. Only then disable planning workflow and enable edit tools

        Ok(json!({
            "status": "pending_confirmation",
            "message": "Plan ready for review. Waiting for user confirmation before execution.",
            "reason": args.reason,
            "plan_file": plan_file.map(|p| p.display().to_string()),
            "plan_tracker_file": tracker_file.map(|p| p.display().to_string()),
            "plan_content": plan_content,
            "plan_summary": plan_summary,
            "next_steps": [
                "Planning workflow is STILL ACTIVE until the user explicitly confirms the plan.",
                "Mutating tools (apply_patch and shell commands that touch files) remain DISABLED.",
                "Do not attempt to write or edit files now; wait for the user to approve the plan in the UI.",
                "User can choose: Execute (exit planning and enable mutating tools) or Stay in Planning workflow (revise the plan).",
                "If the user says 'implement', 'yes', 'go', or 'start', the plan will be presented for confirmation automatically."
            ],
            "requires_confirmation": true,
            "draft_incomplete": false
        }))
    }

    fn name(&self) -> &str {
        tools::FINISH_PLANNING
    }

    fn description(&self) -> &str {
        "Exit the Planning workflow and present the captured plan to the user for approval. Use finish_planning when the plan is complete enough to commit to, or when the user explicitly asks to leave planning mode. Do NOT call finish_planning with an empty plan — the harness requires at least one goal entry."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "properties": {
                "reason": {
                    "type": "string",
                    "description": "Optional reason for exiting planning workflow (e.g., 'planning complete', 'need clarification from user')"
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
