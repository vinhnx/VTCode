use super::AgentRunner;
use super::continuation::VerificationResult;
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::harness_artifacts;
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::task::Task;
use crate::exec::events::HarnessEventKind;
use crate::llm::provider::{LLMRequest, Message, ToolDefinition};
use crate::tools::handlers::TaskTrackerTool;
use crate::tools::traits::Tool;
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Clone)]
pub(super) struct PlannerArtifacts {
    pub spec_path: std::path::PathBuf,
    pub tracker_path: std::path::PathBuf,
}

#[derive(Debug, Clone)]
pub(super) struct EvaluationArtifacts {
    pub evaluation_path: std::path::PathBuf,
    pub passed: bool,
    pub summary: String,
    pub high_severity_findings: usize,
}

#[derive(Debug, Clone)]
pub(super) enum EvaluatorGateOutcome {
    Accept,
    Continue { prompt: String },
    Exhausted { reason: String },
}

#[derive(Debug, Deserialize)]
struct PlannerResponse {
    spec_markdown: String,
    #[serde(default)]
    task_title: Option<String>,
    #[serde(default)]
    items: Vec<PlannerItem>,
}

#[derive(Debug, Deserialize)]
struct PlannerItem {
    #[serde(default)]
    description: String,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    outcome: String,
    #[serde(default)]
    verify: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EvaluatorResponse {
    verdict: String,
    summary: String,
    #[serde(default)]
    high_severity_findings: usize,
    #[serde(default)]
    findings: Vec<EvaluatorFinding>,
    #[serde(default)]
    unmet_contract_items: Vec<String>,
    #[serde(default)]
    residual_risks: Vec<String>,
    #[serde(default)]
    required_tracker_updates: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EvaluatorFinding {
    severity: String,
    title: String,
    #[serde(default)]
    detail: Option<String>,
}

impl AgentRunner {
    pub(super) fn harness_plan_build_evaluate_enabled(
        &self,
        full_auto_active: bool,
        review_like: bool,
    ) -> bool {
        full_auto_active
            && !review_like
            && !self.tool_registry.is_plan_mode()
            && matches!(
                self.config().agent.harness.orchestration_mode,
                vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate
            )
    }

    pub(super) async fn run_planner_phase(
        &mut self,
        task: &Task,
        event_recorder: &mut ExecEventRecorder,
    ) -> Result<PlannerArtifacts> {
        event_recorder.harness_event(
            HarnessEventKind::PlanningStarted,
            Some("Generating execution spec and task tracker.".to_string()),
            None,
            None,
            None,
        );

        let planner_response = self.request_planner_response(task).await?;
        let spec_markdown = if planner_response.spec_markdown.trim().is_empty() {
            self.fallback_spec_markdown(task)
        } else {
            planner_response.spec_markdown
        };
        let spec_path = harness_artifacts::write_spec(&self._workspace, &spec_markdown).await?;

        let tracker_items = self.build_planner_tracker_items(task, planner_response.items);
        let tracker_title = planner_response
            .task_title
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| task.title.clone());
        let tracker_tool = TaskTrackerTool::new(
            self._workspace.clone(),
            self.tool_registry.plan_mode_state(),
        );
        tracker_tool
            .execute(json!({
                "action": "create",
                "title": tracker_title,
                "items": tracker_items,
            }))
            .await
            .context("seed planner task tracker")?;

        let tracker_path = harness_artifacts::current_task_path(&self._workspace);
        event_recorder.harness_event(
            HarnessEventKind::PlanningCompleted,
            Some(format!(
                "Planner wrote {} and seeded {}.",
                spec_path.display(),
                tracker_path.display()
            )),
            None,
            Some(spec_path.display().to_string()),
            None,
        );

        Ok(PlannerArtifacts {
            spec_path,
            tracker_path,
        })
    }

    pub(super) fn augment_generator_task(&self, task: &Task, artifacts: &PlannerArtifacts) -> Task {
        let mut effective_task = task.clone();
        let addendum = format!(
            "Generator contract:\n- Treat `{}` and `{}` as the source of truth.\n- Work one tracker step at a time.\n- Keep the tracker current.\n- Leave resumable state before yielding.",
            artifacts.spec_path.display(),
            artifacts.tracker_path.display()
        );
        effective_task.instructions = Some(match task.instructions.as_deref() {
            Some(existing) if !existing.trim().is_empty() => format!("{existing}\n\n{addendum}"),
            _ => addendum,
        });
        effective_task
    }

    pub(super) async fn run_evaluator_phase(
        &mut self,
        task: &Task,
        session_state: &AgentSessionState,
        event_recorder: &mut ExecEventRecorder,
        verification_results: &[VerificationResult],
    ) -> Result<EvaluationArtifacts> {
        event_recorder.harness_event(
            HarnessEventKind::EvaluationStarted,
            Some("Running skeptical evaluator pass.".to_string()),
            None,
            None,
            None,
        );

        let evaluator = self
            .request_evaluator_response(task, session_state, verification_results)
            .await?;
        let passed = evaluator.verdict.eq_ignore_ascii_case("pass");
        let evaluation_path = harness_artifacts::write_evaluation(
            &self._workspace,
            &self.render_evaluation(&evaluator),
        )
        .await?;

        Ok(EvaluationArtifacts {
            evaluation_path,
            passed,
            summary: evaluator.summary,
            high_severity_findings: evaluator.high_severity_findings,
        })
    }

    pub(super) fn evaluation_retry_prompt(
        &self,
        evaluation: &EvaluationArtifacts,
        revision_round: usize,
    ) -> String {
        let tracker_path = harness_artifacts::current_task_path(&self._workspace);
        format!(
            "Evaluator rejected the candidate implementation in round {}. Fix the reported issues, update `{}`, and try again.\n\nLatest evaluation summary:\n{}\n\nEvaluation artifact: {}",
            revision_round,
            tracker_path.display(),
            evaluation.summary,
            evaluation.evaluation_path.display()
        )
    }

    pub(super) async fn apply_evaluator_gate(
        &mut self,
        task: &Task,
        session_state: &AgentSessionState,
        event_recorder: &mut ExecEventRecorder,
        verification_results: &[VerificationResult],
        revision_rounds_used: &mut usize,
        max_revision_rounds: usize,
    ) -> Result<EvaluatorGateOutcome> {
        let evaluation = self
            .run_evaluator_phase(task, session_state, event_recorder, verification_results)
            .await?;
        if evaluation.passed && evaluation.high_severity_findings == 0 {
            event_recorder.harness_event(
                HarnessEventKind::EvaluationPassed,
                Some(evaluation.summary.clone()),
                None,
                Some(evaluation.evaluation_path.display().to_string()),
                Some(0),
            );
            return Ok(EvaluatorGateOutcome::Accept);
        }

        event_recorder.harness_event(
            HarnessEventKind::EvaluationFailed,
            Some(evaluation.summary.clone()),
            None,
            Some(evaluation.evaluation_path.display().to_string()),
            None,
        );

        if *revision_rounds_used >= max_revision_rounds {
            return Ok(EvaluatorGateOutcome::Exhausted {
                reason: format!(
                    "Evaluator rejected the run after {} revision rounds: {}",
                    max_revision_rounds, evaluation.summary
                ),
            });
        }

        *revision_rounds_used += 1;
        event_recorder.harness_event(
            HarnessEventKind::RevisionStarted,
            Some(format!(
                "Starting revision round {} after evaluator rejection.",
                *revision_rounds_used
            )),
            None,
            Some(evaluation.evaluation_path.display().to_string()),
            None,
        );

        Ok(EvaluatorGateOutcome::Continue {
            prompt: self.evaluation_retry_prompt(&evaluation, *revision_rounds_used),
        })
    }

    fn fallback_spec_markdown(&self, task: &Task) -> String {
        format!(
            "# Execution Spec\n\n## Goal\n{}\n\n## Acceptance Criteria\n- Complete the requested work.\n- Keep the tracker concrete and verifiable.\n\n## Assumptions\n- Scope remains limited to the user request.\n- Verification should use the lightest project-appropriate command available.\n",
            task.description.trim()
        )
    }

    fn fallback_planner_items(&self, task: &Task) -> Vec<serde_json::Value> {
        let verify = self.fallback_verify_commands();
        vec![json!({
            "description": task.description,
            "outcome": "Requested work is implemented and the tracker reflects the final state.",
            "verify": verify,
        })]
    }

    fn build_planner_tracker_items(
        &self,
        task: &Task,
        items: Vec<PlannerItem>,
    ) -> Vec<serde_json::Value> {
        let fallback_verify = self.fallback_verify_commands();
        let tracker_items = items
            .into_iter()
            .filter_map(|item| self.normalize_planner_item(task, item, &fallback_verify))
            .collect::<Vec<_>>();
        if tracker_items.is_empty() {
            self.fallback_planner_items(task)
        } else {
            tracker_items
        }
    }

    fn normalize_planner_item(
        &self,
        task: &Task,
        item: PlannerItem,
        fallback_verify: &[String],
    ) -> Option<serde_json::Value> {
        let description = item.description.trim();
        let description = if description.is_empty() {
            task.description.trim()
        } else {
            description
        };
        if description.is_empty() {
            return None;
        }

        let outcome = item.outcome.trim();
        let outcome = if outcome.is_empty() {
            "Requested work is implemented and the tracker reflects the final state."
        } else {
            outcome
        };
        let files = item
            .files
            .into_iter()
            .map(|file| file.trim().to_string())
            .filter(|file| !file.is_empty())
            .collect::<Vec<_>>();
        let verify = item
            .verify
            .into_iter()
            .map(|command| command.trim().to_string())
            .filter(|command| !command.is_empty())
            .collect::<Vec<_>>();
        let verify = if verify.is_empty() {
            fallback_verify.to_vec()
        } else {
            verify
        };

        Some(json!({
            "description": description,
            "files": files,
            "outcome": outcome,
            "verify": verify,
        }))
    }

    fn fallback_verify_commands(&self) -> Vec<String> {
        if self._workspace.join("Cargo.toml").exists() {
            return vec!["cargo check".to_string()];
        }
        if self._workspace.join("package.json").exists() {
            return vec!["npm test".to_string()];
        }
        if self._workspace.join("pyproject.toml").exists()
            || self._workspace.join("pytest.ini").exists()
        {
            return vec!["pytest".to_string()];
        }
        Vec::new()
    }

    async fn request_planner_response(&mut self, task: &Task) -> Result<PlannerResponse> {
        let model = self.get_selected_model();
        let system_prompt = "You are the VT Code exec harness planner. Expand the task into a concise markdown spec and a concrete tracker. Return strict JSON only with keys: spec_markdown, task_title, items. Each item must include description, outcome, and verify. Keep the spec high-level and implementation-agnostic.";
        let user_prompt = format!(
            "Plan this task.\n\nTitle: {}\nDescription: {}\nInstructions: {}\n\nReturn JSON only.",
            task.title,
            task.description,
            task.instructions.as_deref().unwrap_or("(none)")
        );
        let response = self
            .provider_client
            .generate(LLMRequest {
                messages: vec![Message::user(user_prompt)],
                system_prompt: Some(std::sync::Arc::new(system_prompt.to_string())),
                tools: Some(std::sync::Arc::new(Vec::<ToolDefinition>::new())),
                model,
                stream: false,
                temperature: Some(0.2),
                max_tokens: Some(1600),
                ..Default::default()
            })
            .await
            .context("planner request failed")?;
        parse_json_response::<PlannerResponse>(response.content.unwrap_or_default().as_str())
            .context("parse planner response")
    }

    async fn request_evaluator_response(
        &mut self,
        task: &Task,
        session_state: &AgentSessionState,
        verification_results: &[VerificationResult],
    ) -> Result<EvaluatorResponse> {
        let model = self.get_selected_model();
        let spec_content =
            tokio::fs::read_to_string(harness_artifacts::current_spec_path(&self._workspace))
                .await
                .unwrap_or_default();
        let tracker_content =
            tokio::fs::read_to_string(harness_artifacts::current_task_path(&self._workspace))
                .await
                .unwrap_or_default();
        let changed_files =
            load_changed_file_snapshots(&self._workspace, &session_state.modified_files).await;
        let verification_summary = format_verification_results(verification_results);
        let system_prompt = "You are the VT Code exec harness evaluator. Judge the candidate skeptically. Return strict JSON only with keys verdict, summary, high_severity_findings, findings, unmet_contract_items, residual_risks, required_tracker_updates. Use verdict=pass only when the tracker, verification evidence, and implemented state are consistent and there are no high-severity issues.";
        let user_prompt = format!(
            "Evaluate this run.\n\nTask title: {}\nTask description: {}\n\nCurrent spec:\n{}\n\nCurrent tracker:\n{}\n\nVerification results:\n{}\n\nModified files:\n{}\n\nWarnings:\n{}\n\nReturn JSON only.",
            task.title,
            task.description,
            spec_content,
            tracker_content,
            verification_summary,
            changed_files,
            format_string_list(&session_state.warnings)
        );
        let response = self
            .provider_client
            .generate(LLMRequest {
                messages: vec![Message::user(user_prompt)],
                system_prompt: Some(std::sync::Arc::new(system_prompt.to_string())),
                tools: Some(std::sync::Arc::new(Vec::<ToolDefinition>::new())),
                model,
                stream: false,
                temperature: Some(0.1),
                max_tokens: Some(1400),
                ..Default::default()
            })
            .await
            .context("evaluator request failed")?;
        parse_json_response::<EvaluatorResponse>(response.content.unwrap_or_default().as_str())
            .context("parse evaluator response")
    }

    fn render_evaluation(&self, evaluation: &EvaluatorResponse) -> String {
        let mut markdown = format!(
            "# Evaluation\n\n## Verdict\n{}\n\n## Summary\n{}\n",
            evaluation.verdict.trim(),
            evaluation.summary.trim()
        );

        if !evaluation.findings.is_empty() {
            markdown.push_str("\n## Findings\n");
            for finding in &evaluation.findings {
                markdown.push_str(&format!(
                    "- [{}] {}",
                    finding.severity.trim(),
                    finding.title.trim()
                ));
                if let Some(detail) = finding
                    .detail
                    .as_deref()
                    .filter(|text| !text.trim().is_empty())
                {
                    markdown.push_str(": ");
                    markdown.push_str(detail.trim());
                }
                markdown.push('\n');
            }
        }

        if !evaluation.unmet_contract_items.is_empty() {
            markdown.push_str("\n## Unmet Contract Items\n");
            for item in &evaluation.unmet_contract_items {
                markdown.push_str("- ");
                markdown.push_str(item.trim());
                markdown.push('\n');
            }
        }

        if !evaluation.residual_risks.is_empty() {
            markdown.push_str("\n## Residual Risks\n");
            for risk in &evaluation.residual_risks {
                markdown.push_str("- ");
                markdown.push_str(risk.trim());
                markdown.push('\n');
            }
        }

        if !evaluation.required_tracker_updates.is_empty() {
            markdown.push_str("\n## Required Tracker Updates\n");
            for update in &evaluation.required_tracker_updates {
                markdown.push_str("- ");
                markdown.push_str(update.trim());
                markdown.push('\n');
            }
        }

        markdown
    }
}

fn parse_json_response<T>(text: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let trimmed = text.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty model response")
    }

    if let Ok(parsed) = serde_json::from_str::<T>(trimmed) {
        return Ok(parsed);
    }

    let trimmed = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    let trimmed = trimmed
        .strip_suffix("```")
        .map(str::trim)
        .unwrap_or(trimmed);
    serde_json::from_str::<T>(trimmed).context("decode json payload")
}

async fn load_changed_file_snapshots(workspace_root: &std::path::Path, files: &[String]) -> String {
    const MAX_FILES: usize = 8;
    const MAX_TOTAL_CHARS: usize = 40_000;

    if files.is_empty() {
        return "(no modified files recorded)".to_string();
    }

    let mut output = String::new();
    let mut remaining = MAX_TOTAL_CHARS;
    for file in files.iter().take(MAX_FILES) {
        let path = workspace_root.join(file);
        if !path.exists() {
            continue;
        }
        let Ok(content) = tokio::fs::read_to_string(&path).await else {
            continue;
        };
        let slice = truncate_chars(&content, remaining.saturating_sub(file.len() + 32));
        if slice.is_empty() {
            break;
        }
        output.push_str("### ");
        output.push_str(file);
        output.push('\n');
        output.push_str(&slice);
        output.push_str("\n\n");
        remaining = remaining.saturating_sub(slice.len() + file.len() + 8);
        if remaining == 0 {
            break;
        }
    }

    if output.trim().is_empty() {
        "(no readable modified file snapshots)".to_string()
    } else {
        output
    }
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let truncated = text
        .chars()
        .take(limit.saturating_sub(3))
        .collect::<String>();
    format!("{truncated}...")
}

fn format_string_list(items: &[String]) -> String {
    if items.is_empty() {
        "(none)".to_string()
    } else {
        format!("- {}", items.join("\n- "))
    }
}

fn format_verification_results(results: &[VerificationResult]) -> String {
    if results.is_empty() {
        return "(no verification commands ran in the final acceptance pass)".to_string();
    }

    results
        .iter()
        .map(|result| {
            let status = if result.success { "PASS" } else { "FAIL" };
            let exit_code = result
                .exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "?".to_string());
            if result.output.trim().is_empty() {
                format!("- [{status}] {} (exit {exit_code})", result.command)
            } else {
                format!(
                    "- [{status}] {} (exit {exit_code})\n  {}",
                    result.command,
                    result.output.trim().replace('\n', "\n  ")
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
