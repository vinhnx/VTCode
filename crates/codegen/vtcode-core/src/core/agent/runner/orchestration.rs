use super::AgentRunner;
use super::continuation::VerificationResult;
use super::evaluator_types::{EvaluatorResponse, GeneralizationNote, SkepticPanelAggregate, SkepticPanelEntry};
use super::planner_types::{PlannerResponse, ReplanResponse};
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::harness_artifacts;
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::task::Task;
use crate::exec::events::HarnessEventKind;
use crate::llm::provider::{LLMRequest, Message, ToolDefinition};
use crate::tools::handlers::TaskTrackerTool;
use crate::tools::traits::Tool;
use anyhow::{Context, Error, Result};
use serde::Deserialize;
use serde_json::json;
use std::fmt::Write;

const EVIDENCE_BOUNDED_GUIDANCE: &str = "Prefer the smallest claim the evidence supports: keep every claim within its explicit scope, cite concrete evidence, and state a falsifier. These observations are task-scoped and must not become global beliefs or persistent memory automatically.";

/// Shared by the single evaluator and every skeptic-panel model so both judge
/// against the same contract.
const EVALUATOR_SYSTEM_PROMPT: &str = "You are the VT Code exec harness evaluator. You are not the builder. Judge the candidate skeptically and prefer failing borderline cases. Return strict JSON only with keys verdict, summary, high_severity_findings, scorecard, findings, unmet_contract_items, residual_risks, required_tracker_updates, generalization_notes. generalization_notes must be an array of objects with non-empty claim, scope, evidence, and falsifier strings, with at most 8 notes. The scorecard must contain 1-5 scores for contract_fidelity, functionality, code_quality, and verification_integrity. Use verdict=pass only when every provided score is at least 4, the tracker/spec/contract all agree, verification evidence is credible, and there are no high-severity issues. If you discover new acceptance criteria through testing, add them to required_tracker_updates so the replanner can update the feature list.";

#[derive(Debug, Clone)]
pub(super) struct PlannerArtifacts {
    pub spec_path: std::path::PathBuf,
    pub contract_path: std::path::PathBuf,
    pub tracker_path: std::path::PathBuf,
    pub feature_list_path: std::path::PathBuf,
}

#[derive(Debug, Clone)]
pub(super) struct EvaluationArtifacts {
    pub(super) generalization_notes: Vec<GeneralizationNote>,
    pub evaluation_path: std::path::PathBuf,
    pub passed: bool,
    pub summary: String,
    /// Tracker updates the evaluator requires (parsed from the LLM response
    /// but previously dropped). Applied during replanning.
    pub required_tracker_updates: Vec<String>,
    /// Contract items the evaluator found unmet.
    pub unmet_contract_items: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) enum EvaluatorGateOutcome {
    Accept,
    Continue { prompt: String },
    Exhausted { reason: String },
}

impl AgentRunner {
    pub(super) fn harness_plan_build_evaluate_enabled(&self, full_auto_active: bool, review_like: bool) -> bool {
        full_auto_active
            && !review_like
            && !self.tool_registry.is_planning_active()
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
            Some("Generating execution spec, contract, and task tracker.".to_string()),
            None,
            None,
            None,
            None,
            None,
        );

        let planner_response = self.request_planner_response(task).await?;
        let spec_markdown = planner_response
            .spec_markdown
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| self.fallback_spec_markdown(task));
        let spec_path = harness_artifacts::write_spec(&self._workspace, &spec_markdown).await?;

        let tracker_items = self.build_planner_tracker_items(task, planner_response.items);
        let contract_markdown = planner_response
            .contract_markdown
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| self.render_contract_markdown(task, &tracker_items));
        let contract_path = harness_artifacts::write_contract(&self._workspace, &contract_markdown).await?;
        let tracker_title = planner_response
            .task_title
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| task.title.clone());
        let tracker_tool = TaskTrackerTool::new(self._workspace.clone(), self.tool_registry.planning_workflow_state());
        tracker_tool
            .execute(json!({
                "action": "create",
                "title": tracker_title,
                "items": tracker_items,
            }))
            .await
            .context("seed planner task tracker")?;

        let tracker_path = harness_artifacts::current_task_path(&self._workspace);

        // Build and write the feature list artifact. The planner may provide
        // it directly; otherwise we derive a fallback from the tracker items.
        let feature_list_markdown = planner_response
            .feature_list_markdown
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| self.fallback_feature_list_markdown(&tracker_items));
        let feature_list_path = harness_artifacts::write_feature_list(&self._workspace, &feature_list_markdown).await?;

        event_recorder.harness_event(
            HarnessEventKind::PlanningCompleted,
            Some(format!(
                "Planner wrote {}, {}, {}, and seeded {}.",
                spec_path.display(),
                contract_path.display(),
                feature_list_path.display(),
                tracker_path.display()
            )),
            None,
            Some(spec_path.display().to_string()),
            None,
            None,
            None,
        );

        Ok(PlannerArtifacts {
            spec_path,
            contract_path,
            tracker_path,
            feature_list_path,
        })
    }

    /// Re-plan from the current state after an evaluator rejection.
    ///
    /// Appends evaluator feedback to the existing spec and contract files so the
    /// generator can see what went wrong. Uses an LLM-based replanner when
    /// available to produce a revised feature list, contract addendum, and new
    /// tracker items. Falls back to annotation-only if the replanner fails.
    async fn run_evaluator_phase(
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
            None,
            None,
        );

        let evaluator = if self.config().agent.harness.skeptic_panel.enabled
            && !self.config().agent.harness.skeptic_panel.models.is_empty()
        {
            let aggregate = self.run_skeptic_panel(task, session_state, verification_results).await?;
            EvaluatorResponse {
                verdict: aggregate.verdict,
                summary: aggregate.summary,
                high_severity_findings: aggregate.high_severity_findings,
                scorecard: Some(aggregate.scorecard),
                findings: Vec::new(),
                unmet_contract_items: Vec::new(),
                residual_risks: Vec::new(),
                required_tracker_updates: Vec::new(),
                generalization_notes: aggregate.generalization_notes,
            }
        } else {
            self.request_evaluator_response(task, session_state, verification_results)
                .await?
        };
        let summary = evaluator.effective_summary();
        let passed = evaluator.passed();
        let generalization_notes = evaluator
            .validated_generalization_notes()
            .map_err(|error| anyhow::anyhow!(error))
            .context("validate evaluator generalization notes")?;
        let evaluation_path = harness_artifacts::write_evaluation(
            &self._workspace,
            &self.render_evaluation(&evaluator, &generalization_notes),
        )
        .await?;

        Ok(EvaluationArtifacts {
            evaluation_path,
            passed,
            summary,
            required_tracker_updates: evaluator.required_tracker_updates,
            unmet_contract_items: evaluator.unmet_contract_items,
            generalization_notes,
        })
    }

    fn evaluation_retry_prompt(&self, evaluation: &EvaluationArtifacts, revision_round: usize) -> String {
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
        if evaluation.passed {
            event_recorder.harness_event(
                HarnessEventKind::EvaluationPassed,
                Some(evaluation.summary.clone()),
                None,
                Some(evaluation.evaluation_path.display().to_string()),
                Some(0),
                None,
                None,
            );
            return Ok(EvaluatorGateOutcome::Accept);
        }

        event_recorder.harness_event(
            HarnessEventKind::EvaluationFailed,
            Some(evaluation.summary.clone()),
            None,
            Some(evaluation.evaluation_path.display().to_string()),
            None,
            None,
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
            Some(format!("Starting revision round {} after evaluator rejection.", *revision_rounds_used)),
            None,
            Some(evaluation.evaluation_path.display().to_string()),
            None,
            None,
            None,
        );

        // Re-plan from the current state using evaluator feedback.
        // Best-effort: if re-planning fails (e.g. mock provider in tests),
        // fall back to the original retry prompt without updated artifacts.
        let revised_artifacts = self.replan_from_failure(task, &evaluation, *revision_rounds_used).await;

        if let Some(ref artifacts) = revised_artifacts {
            let prompt = format!(
                "Evaluator rejected the candidate implementation in round {}.\n\n\
                 The plan has been revised based on evaluator feedback.\n\
                 Updated spec: {}\n\
                 Updated contract: {}\n\
                 Updated feature list: {}\n\
                 Updated tracker: {}\n\n\
                 Latest evaluation summary:\n{}\n\n\
                 Evaluation artifact: {}\n\n\
                 Work through the updated tracker items.",
                *revision_rounds_used,
                artifacts.spec_path.display(),
                artifacts.contract_path.display(),
                artifacts.feature_list_path.display(),
                artifacts.tracker_path.display(),
                evaluation.summary,
                evaluation.evaluation_path.display(),
            );

            Ok(EvaluatorGateOutcome::Continue { prompt })
        } else {
            Ok(EvaluatorGateOutcome::Continue {
                prompt: self.evaluation_retry_prompt(&evaluation, *revision_rounds_used),
            })
        }
    }

    /// Issue a tool-less, single-turn JSON-only request against the active
    /// provider and decode the response into `T`. Used by harness sub-roles
    /// (planner, evaluator) that need structured output with no tool calls.
    async fn request_json_only<T>(
        &mut self,
        system_prompt: &str,
        user_prompt: String,
        temperature: f32,
        max_tokens: u32,
        request_label: &'static str,
        parse_label: &'static str,
    ) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let model = self.get_selected_model();
        let response = self
            .provider_client
            .generate(LLMRequest {
                messages: std::sync::Arc::new(vec![Message::user(user_prompt)]),
                system_prompt: Some(std::sync::Arc::from(system_prompt)),
                tools: Some(std::sync::Arc::new(Vec::<ToolDefinition>::new())),
                model,
                stream: false,
                temperature: Some(temperature),
                max_tokens: Some(max_tokens),
                ..Default::default()
            })
            .await
            .context(request_label)?;
        let content = response.content.unwrap_or_default();
        parse_json_response::<T>(content.as_str()).context(parse_label)
    }

    async fn request_planner_response(&mut self, task: &Task) -> Result<PlannerResponse> {
        const SYSTEM_PROMPT: &str = "You are the VT Code exec harness planner. Expand the task into a concise execution spec, a concrete execution contract, a feature list, and a tracker. Return strict JSON only with keys: spec_markdown, contract_markdown, feature_list_markdown, task_title, items. Keep spec_markdown high-level and implementation-agnostic. Use contract_markdown and items to define observable done conditions and verification. feature_list_markdown should enumerate the project's features with acceptance criteria as a markdown checklist. Each item must include description, outcome, and verify; files is optional. Keep scope tight to the user request and do not invent speculative work.";
        let system_prompt = format!("{SYSTEM_PROMPT}\n{EVIDENCE_BOUNDED_GUIDANCE}");
        let user_prompt = format!(
            "Plan this task.\n\nTitle: {}\nDescription: {}\nInstructions: {}\n\nProduce:\n- a concise execution spec\n- a concrete execution contract with observable done signals\n- a feature list with acceptance criteria as a markdown checklist\n- tracker items with explicit verification commands",
            task.title,
            task.description,
            task.instructions.as_deref().unwrap_or("(none)")
        );
        self.request_json_only(
            &system_prompt,
            user_prompt,
            0.2,
            4096,
            "planner request failed",
            "parse planner response",
        )
        .await
    }

    /// Build a fallback feature list from tracker items when the planner LLM
    /// doesn't provide one directly.
    fn fallback_feature_list_markdown(&self, tracker_items: &[serde_json::Value]) -> String {
        let mut md = String::from("# Feature List\n\n");
        for item in tracker_items {
            if let Some(desc) = item.get("description").and_then(|v| v.as_str()) {
                let outcome = item.get("outcome").and_then(|v| v.as_str()).unwrap_or("(unspecified)");
                md.push_str(&format!("- [ ] {desc} — acceptance: {outcome}\n"));
            }
        }
        if md.lines().count() <= 1 {
            md.push_str("(No features derived from tracker.)\n");
        }
        md
    }

    /// Request a structured replan from the LLM after an evaluator rejection.
    ///
    /// This addresses the long-running harness pattern: "the evaluator takes on
    /// part of the local planner role for feedback-driven replanning." The
    /// replanner receives the current artifacts and evaluator feedback, then
    /// produces a revised feature list, contract addendum, and new tracker
    /// items. Falls back to `None` on any error (caller uses annotation-only).
    pub(super) async fn request_replan_response(
        &mut self,
        task: &Task,
        evaluation: &EvaluationArtifacts,
        revision_round: usize,
    ) -> Option<ReplanResponse> {
        let (spec_content, contract_content, _tracker_content, feature_list_content) =
            read_harness_artifacts(&self._workspace).await;

        const SYSTEM_PROMPT: &str = "You are the VT Code exec harness replanner. The evaluator rejected the current implementation. Revise the plan based on evaluator feedback. Return strict JSON only with keys: revised_feature_list, contract_addendum, new_tracker_items, preserved_scopes, rationale. revised_feature_list should be the complete updated feature list markdown (replacing the old one). contract_addendum should be a short markdown section appended to the contract. new_tracker_items should be an array of {description, outcome, verify} objects for newly discovered acceptance criteria. preserved_scopes must repeat every supplied generalization-note scope verbatim. Every supplied falsifier must become a verification step in the revised tracker. rationale should explain your changes.";
        let system_prompt = format!("{SYSTEM_PROMPT}\n{EVIDENCE_BOUNDED_GUIDANCE}");

        let user_prompt = format!(
            "Replan after evaluator rejection (round {}).\n\nTask: {}\n{}\n\nCurrent spec:\n{}\n\nCurrent contract:\n{}\n\nCurrent feature list:\n{}\n\nEvaluator feedback:\n{}\n\nUnmet contract items:\n{}\n\nRequired tracker updates:\n{}\n\nGeneralization notes (preserve every scope and falsify each note in the tracker):\n{}\n\nProduce a revised plan. Return JSON only.",
            revision_round,
            task.title,
            task.description,
            spec_content,
            contract_content,
            feature_list_content,
            evaluation.summary,
            evaluation.unmet_contract_items.join("; "),
            evaluation.required_tracker_updates.join("; "),
            format_generalization_notes(&evaluation.generalization_notes),
        );

        self.request_json_only(&system_prompt, user_prompt, 0.2, 4096, "replan request failed", "parse replan response")
            .await
            .ok()
    }

    /// System and user prompts for an evaluator request, shared by the single
    /// evaluator and the skeptic panel.
    async fn evaluator_prompts(
        &self,
        task: &Task,
        session_state: &AgentSessionState,
        verification_results: &[VerificationResult],
    ) -> (String, String) {
        let (spec_content, contract_content, tracker_content, feature_list_content) =
            read_harness_artifacts(&self._workspace).await;
        let changed_files = load_changed_file_snapshots(&self._workspace, &session_state.modified_files).await;
        let verification_summary = format_verification_results(verification_results);
        let system_prompt = format!("{EVALUATOR_SYSTEM_PROMPT}\n{EVIDENCE_BOUNDED_GUIDANCE}");
        let user_prompt = format!(
            "Evaluate this run against the current execution contract.\n\nTask title: {}\nTask description: {}\n\nCurrent spec:\n{}\n\nCurrent contract:\n{}\n\nCurrent feature list:\n{}\n\nCurrent tracker:\n{}\n\nVerification results:\n{}\n\nModified files:\n{}\n\nWarnings:\n{}\n\nScoring guidance:\n- contract_fidelity: Did the implementation satisfy the spec and contract rather than a looser interpretation?\n- functionality: Do the implemented paths actually work beyond stubs and happy-path claims?\n- code_quality: Are the changes coherent, scoped, and consistent with local patterns?\n- verification_integrity: Do the tracker state and verification evidence really justify completion?\n\nIf you find new acceptance criteria that should be tracked, list them in required_tracker_updates. If a local observation might help the next round, add a bounded generalization note with its claim, scope, evidence, and falsifier.\n\nReturn JSON only.",
            task.title,
            task.description,
            spec_content,
            contract_content,
            feature_list_content,
            tracker_content,
            verification_summary,
            changed_files,
            format_string_list(&session_state.warnings)
        );
        (system_prompt, user_prompt)
    }

    async fn request_evaluator_response(
        &mut self,
        task: &Task,
        session_state: &AgentSessionState,
        verification_results: &[VerificationResult],
    ) -> Result<EvaluatorResponse> {
        let (system_prompt, user_prompt) = self.evaluator_prompts(task, session_state, verification_results).await;
        self.request_json_only(
            &system_prompt,
            user_prompt,
            0.1,
            1800,
            "evaluator request failed",
            "parse evaluator response",
        )
        .await
    }

    /// Run the adversarial skeptic panel: query every configured skeptic model
    /// in parallel and aggregate the strictest verdict/scorecard.
    async fn run_skeptic_panel(
        &mut self,
        task: &Task,
        session_state: &AgentSessionState,
        verification_results: &[VerificationResult],
    ) -> Result<SkepticPanelAggregate> {
        let models: Vec<String> = self
            .config()
            .agent
            .harness
            .skeptic_panel
            .models
            .clone()
            .into_iter()
            .filter(|m| !m.is_empty())
            .collect();
        if models.is_empty() {
            return self
                .request_evaluator_response(task, session_state, verification_results)
                .await
                .map(|r| SkepticPanelAggregate::from_entries(vec![SkepticPanelEntry { response: r }]));
        }

        let (system_prompt, user_prompt) = self.evaluator_prompts(task, session_state, verification_results).await;

        let base_request = LLMRequest {
            messages: std::sync::Arc::new(vec![Message::user(user_prompt)]),
            system_prompt: Some(std::sync::Arc::from(system_prompt)),
            tools: Some(std::sync::Arc::new(Vec::<ToolDefinition>::new())),
            model: String::new(),
            stream: false,
            temperature: Some(0.1),
            max_tokens: Some(1800),
            ..Default::default()
        };

        let mut handles = Vec::with_capacity(models.len());
        for model in models {
            let req = LLMRequest { model: model.clone(), ..base_request.clone() };
            let provider = self.provider_client.as_ref();
            handles.push(async move {
                let response = provider
                    .generate(req)
                    .await
                    .context(format!("skeptic evaluator request failed for model {model}"));
                (model, response)
            });
        }

        let mut entries = Vec::with_capacity(handles.len());
        for (model, response) in futures::future::join_all(handles).await {
            let response = response?;
            let content = response.content.unwrap_or_default();
            let parsed: Result<EvaluatorResponse> = parse_json_response(content.as_str())
                .context(format!("parse skeptic evaluator response for model {model}"));
            match parsed {
                Ok(evaluator) => {
                    entries.push(SkepticPanelEntry { response: evaluator });
                }
                Err(err) => {
                    tracing::warn!(model = %model, error = %err, "skeptic evaluator parse failed");
                }
            }
        }

        if entries.is_empty() {
            anyhow::bail!("skeptic panel produced no valid responses");
        }

        Ok(SkepticPanelAggregate::from_entries(entries))
    }
}

pub(super) fn render_markdown_list(markdown: &mut String, header: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    let _ = writeln!(markdown, "\n## {header}");
    for item in items {
        let _ = writeln!(markdown, "- {}", item.trim());
    }
}

pub(super) fn json_string_list(item: &serde_json::Value, key: &str) -> Vec<String> {
    item.get(key)
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .collect()
}

async fn read_harness_artifacts(workspace: &std::path::Path) -> (String, String, String, String) {
    let spec = tokio::fs::read_to_string(harness_artifacts::current_spec_path(workspace));
    let contract = tokio::fs::read_to_string(harness_artifacts::current_contract_path(workspace));
    let tracker = tokio::fs::read_to_string(harness_artifacts::current_task_path(workspace));
    let feature_list = tokio::fs::read_to_string(harness_artifacts::current_feature_list_path(workspace));
    let (spec, contract, tracker, feature_list) = tokio::join!(spec, contract, tracker, feature_list);
    (
        spec.unwrap_or_default(),
        contract.unwrap_or_default(),
        tracker.unwrap_or_default(),
        feature_list.unwrap_or_default(),
    )
}

fn format_generalization_notes(notes: &[GeneralizationNote]) -> String {
    if notes.is_empty() {
        return "(none)".to_string();
    }

    notes
        .iter()
        .enumerate()
        .map(|(index, note)| {
            format!(
                "{}. Claim: {}\n   Scope: {}\n   Evidence: {}\n   Falsifier: {}",
                index + 1,
                note.claim,
                note.scope,
                note.evidence,
                note.falsifier
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Hard byte cap on raw model output retained in parse-failure diagnostics.
/// Keeps a full response from inflating a tracing record when the model
/// ignores the JSON-only contract and emits prose.
const PLANNER_PARSE_FAILURE_SAMPLE_BYTES: usize = 2_048;

/// Bound a failure sample at the cap, stopping before a multi-byte UTF-8
/// character boundary instead of panicking on a truncated one.
fn bounded_failure_sample(text: &str) -> String {
    let mut end = PLANNER_PARSE_FAILURE_SAMPLE_BYTES.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let marker = if end < text.len() { "…" } else { "" };
    format!("{}{marker}", &text[..end])
}

/// Record why a harness structured response failed to decode. Without a raw
/// sample the stop reason is bare, so serializer drift or an unexpected prose
/// wrapper cannot be diagnosed from `parse planner response` alone.
fn log_parse_failure(text: &str, parse_error: &Error) {
    tracing::warn!(sample = bounded_failure_sample(text.trim()), error = %parse_error, "structured harness response failed to parse");
}

fn parse_json_response<T>(text: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let parsed = parse_json_payload::<T>(text);
    if let Err(error) = &parsed {
        log_parse_failure(text, error);
    }
    parsed
}

fn parse_json_payload<T>(text: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(Error::msg("empty model response"));
    }

    if let Ok(parsed) = serde_json::from_str::<T>(trimmed) {
        return Ok(parsed);
    }

    let stripped = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    let stripped = stripped.strip_suffix("```").map(str::trim).unwrap_or(stripped);
    serde_json::from_str::<T>(stripped).context("decode json payload")
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
        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
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
    vtcode_commons::formatting::truncate_within(text, limit, "...")
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
            let exit_code = result.exit_code.map(|code| code.to_string()).unwrap_or_else(|| "?".to_string());
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

#[cfg(test)]
mod parse_failure_tests {
    use super::*;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Probe {
        ok: bool,
    }

    /// Asymmetric both directions: a JSON-only response parses without any
    /// warning-worthy detour, while the same payload wrapped in a fenced
    /// block must still parse through the fence-strip fallback.
    #[test]
    fn direct_json_matches_fenced_json_result() {
        let direct: Probe = parse_json_response(r#"{"ok":true}"#).expect("direct json parses");
        let fenced: Probe = parse_json_response("```json\n{\"ok\":true}\n```").expect("fenced json parses");
        assert_eq!(direct, fenced);
        assert!(direct.ok);
    }

    /// The planner-response failure that terminated a harness thread
    /// (session 20261016T033259Z) must remain an error, and the failure
    /// path must not mutate the payload it reports.
    #[test]
    fn prose_payload_is_rejected_as_a_parse_error() {
        let error = parse_json_response::<Probe>("Sure! Here is the plan:\n\n1. Do the task").unwrap_err();
        assert!(error.to_string().contains("decode json payload"), "{error}");
    }

    /// Empty responses keep their dedicated message so callers can
    /// distinguish an absent payload from a malformed one.
    #[test]
    fn empty_payload_error_differs_from_malformed_payload_error() {
        assert_eq!(parse_json_response::<Probe>("   ").unwrap_err().to_string(), "empty model response");
        let malformed = parse_json_response::<Probe>("{").unwrap_err().to_string();
        assert_ne!(malformed, "empty model response", "{malformed}");
    }

    /// The diagnostic sample is bounded, keeps its full length when under
    /// the cap, and stops before splitting a multi-byte character.
    #[test]
    fn bounded_failure_sample_respects_cap_and_char_boundaries() {
        let short = "short";
        assert_eq!(bounded_failure_sample(short), short);

        let long = "a".repeat(PLANNER_PARSE_FAILURE_SAMPLE_BYTES + 500);
        let sample = bounded_failure_sample(&long);
        assert!(sample.ends_with('…'), "truncated samples carry a marker");
        assert_eq!(sample.chars().count(), PLANNER_PARSE_FAILURE_SAMPLE_BYTES + 1);

        let multibyte = "嗯".repeat(PLANNER_PARSE_FAILURE_SAMPLE_BYTES);
        let sample = bounded_failure_sample(&multibyte);
        assert!(sample.is_char_boundary(sample.len()), "sample never splits a character");
        assert_eq!(sample.chars().position(|c| c != '嗯'), Some(sample.chars().count() - 1));
    }
}
