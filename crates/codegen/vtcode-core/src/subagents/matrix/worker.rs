use super::{MatrixWorkerContext, SubagentController};
use crate::core::agent::runner::{AgentRunner, RunnerSettings};
use crate::core::agent::task::{Task, TaskOutcome};
use crate::core::threads::ThreadBootstrap;
use crate::exec::events::matrix::*;
use crate::tools::registry::ToolRegistry;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub(super) struct WorkerResult {
    pub assignment: MatrixAssignment,
    pub outcome: MatrixOutcome,
    pub evidence: Vec<MatrixCommandEvidence>,
    pub cleanup_confirmed: bool,
}

/// Every command is launched by a private worker registry; cleanup never
/// touches parent commands or discovers ownership through a process ID.
struct OwnedCommands(Option<ToolRegistry>);
impl Drop for OwnedCommands {
    fn drop(&mut self) {
        if let Some(registry) = self.0.take() {
            tokio::spawn(async move {
                let _ = registry.terminate_all_exec_sessions_for_exit_async().await;
            });
        }
    }
}

pub(super) async fn execute(
    controller: &SubagentController,
    assignment: MatrixAssignment,
    task: MatrixTaskSpec,
    cancel: CancellationToken,
) -> WorkerResult {
    let mut owned = OwnedCommands(None);
    let mut evidence = Vec::new();
    let work = async {
        let spec = controller
            .resolve_requested_spec(Some(
                if assignment.phase == MatrixPhase::Execute && task.access == WorkspaceAccess::Read {
                    "explore"
                } else {
                    "default"
                },
            ))
            .await?;
        ensure!(
            assignment.phase != MatrixPhase::Execute || task.access != WorkspaceAccess::Read || spec.is_read_only(),
            "matrix read workspace policy requires a read-only worker"
        );
        let (model, reasoning, cfg) = super::super::prepare_child_runtime_config(
            &controller.config.vt_cfg,
            &spec,
            &controller.config.parent_model,
            &controller.config.parent_provider,
            controller.config.parent_reasoning_effort,
            None,
            None,
            None,
            false,
            super::super::resolve_effective_subagent_model,
        )?;
        let session_id = format!("matrix-{}-{}", controller.parent_session_id.read().await, assignment.worker_id);
        let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
            super::super::agent_type_for_spec(&spec),
            model,
            controller.config.api_key.clone(),
            vtcode_commons::canonicalize(controller.config.workspace_root.join(&task.workspace))
                .context("resolve matrix task workspace")?,
            session_id,
            RunnerSettings { reasoning_effort: Some(reasoning), verbosity: None },
            None,
            ThreadBootstrap::new(None),
            Some(cfg.clone()),
            controller.config.openai_chatgpt_auth.clone(),
        ))
        .await?;
        runner.set_quiet(true);
        runner.set_subagent_mode(true);
        let registry = runner.tool_registry();
        let worker_context = MatrixWorkerContext::new(assignment.clone());
        registry.set_matrix_worker(worker_context.clone());
        owned.0 = Some(registry.clone());
        // Matrix control restrictions belong to the coordinator registry;
        // children inherit the baseline sandbox/allow/deny policy instead.
        let mut definitions =
            super::super::filter_child_tools(&spec, runner.build_universal_tools().await?, spec.is_read_only(), false);
        definitions.retain(|tool| tool.function_name() != "matrix");
        if assignment.phase == MatrixPhase::Execute {
            if let Some(definition) = runner
                .build_universal_tools()
                .await?
                .into_iter()
                .find(|tool| tool.function_name() == "matrix")
            {
                definitions.push(definition);
            }
        }
        runner.set_tool_definitions_override(definitions.clone());
        if cfg.automation.full_auto.enabled {
            runner.enable_full_auto(&cfg.automation.full_auto.allowed_tools).await;
        }
        if assignment.phase == MatrixPhase::Execute {
            let mut run_task =
                Task::new(assignment.task_id.clone(), format!("Matrix task {}", task.id), task.instructions.clone());
            run_task.instructions = Some(format!(
                "{}\nYour task workspace is {}. Use workdir=. for commands in this directory. Nested delegation is disabled. Execute the assigned instructions; the scheduler runs final verification after all execution tasks finish. You may report using matrix action=report; task identity is runtime-owned. An accepted report ends this execution attempt. evidence_ids are optional; include only canonical event IDs explicitly provided by the runtime, never command run IDs or invented references.",
                spec.prompt,
                runner.workspace().display()
            ));
            let result = Box::pin(runner.execute_task(&run_task, &[])).await?;
            let outcome = match result.outcome {
                TaskOutcome::Success | TaskOutcome::StoppedNoAction => MatrixOutcome::Success,
                TaskOutcome::Cancelled => MatrixOutcome::Interrupted,
                TaskOutcome::BudgetLimitReached { .. }
                | TaskOutcome::TurnLimitReached { .. }
                | TaskOutcome::ToolLoopLimitReached { .. } => MatrixOutcome::BudgetExhausted,
                _ => worker_context
                    .reported_outcome()
                    .filter(|outcome| *outcome != MatrixOutcome::Success)
                    .unwrap_or(MatrixOutcome::Failed),
            };
            return Ok(if outcome == MatrixOutcome::Success {
                worker_context.reported_outcome().unwrap_or(outcome)
            } else {
                outcome
            });
        }
        let mut checks_succeeded = true;
        let previous_checks = controller.matrix_snapshot().await.map_or(0, |snapshot| {
            snapshot
                .tasks
                .iter()
                .find(|state| state.id == assignment.task_id)
                .map_or(0, |state| {
                    state
                        .attempts
                        .iter()
                        .filter(|attempt| {
                            attempt.id != assignment.attempt_id
                                && attempt.phase == MatrixPhase::Verify
                                && attempt.launch_requested
                        })
                        .count()
                        .saturating_mul(task.checks.len())
                })
        });
        for (index, command) in task.checks.iter().enumerate() {
            if (cfg.agent.harness.max_tool_calls_per_turn > 0 && index >= cfg.agent.harness.max_tool_calls_per_turn)
                || vtcode_config::core::tools::tool_loop_limit_reached(
                    previous_checks.saturating_add(index),
                    cfg.tools.max_tool_loops,
                )
            {
                return Ok(MatrixOutcome::BudgetExhausted);
            }
            let value = runner
                .execute_scheduled_tool(
                    "exec_command",
                    json!({"command":command,"workdir":".","yield_time_ms":1000,"max_output_tokens":2000}),
                )
                .await?;
            let value = settle_command(&runner, value).await?;
            let exit_code = value
                .get("exit_code")
                .and_then(Value::as_i64)
                .and_then(|code| i32::try_from(code).ok());
            let cancelled = value.get("cancelled").and_then(Value::as_bool).unwrap_or(false);
            evidence.push(MatrixCommandEvidence {
                command: command.clone(),
                attempt_id: assignment.attempt_id.clone(),
                worker_id: assignment.worker_id.clone(),
                generation: assignment.generation.clone().context("verification generation unavailable")?,
                event_id: format!("matrix-check-{}-{index}", assignment.attempt_id),
                exit_code,
                cancelled,
            });
            checks_succeeded &= exit_code == Some(0) && !cancelled;
        }
        Ok::<_, anyhow::Error>(if checks_succeeded {
            MatrixOutcome::Success
        } else {
            MatrixOutcome::Failed
        })
    };
    let outcome = tokio::select! {
        biased;
        () = cancel.cancelled() => MatrixOutcome::Cancelled,
        result = tokio::time::timeout(Duration::from_secs(task.timeout_secs), work) => match result {
            Err(_) => MatrixOutcome::TimedOut,
            Ok(Ok(outcome)) => outcome,
            Ok(Err(error)) => {
                tracing::warn!(task_id = task.id, attempt_id = assignment.attempt_id, error = %error, "matrix worker failed");
                let message = error.to_string().to_ascii_lowercase();
                if message.contains("permission") || message.contains("policy") || message.contains("denied") { MatrixOutcome::PermissionDenied } else { MatrixOutcome::Failed }
            }
        },
    };
    let cleanup_confirmed = match owned.0.as_ref() {
        Some(registry) => matches!(
            tokio::time::timeout(Duration::from_secs(12), registry.terminate_all_exec_sessions_for_exit_async()).await,
            Ok(Ok(()))
        ),
        None => true,
    };
    if cleanup_confirmed {
        owned.0.take();
    }
    WorkerResult { assignment, outcome, evidence, cleanup_confirmed }
}

async fn settle_command(runner: &AgentRunner, mut value: Value) -> Result<Value> {
    while value.get("exit_code").is_none() {
        if let Some(error) = value.get("error") {
            let category = error.get("category").and_then(Value::as_str).unwrap_or_default();
            ensure!(!category.contains("Policy") && !category.contains("Permission"), "verification permission denied");
            anyhow::bail!("verification command failed to launch or settle");
        }
        ensure!(
            value.get("success").and_then(Value::as_bool) != Some(false),
            "matrix command was denied or failed to launch"
        );
        let session = value
            .get("session_id")
            .or_else(|| value.get("process_id"))
            .and_then(Value::as_str)
            .context("verification did not return a completed command or owned session")?
            .to_owned();
        value = runner
            .execute_scheduled_tool(
                "write_stdin",
                json!({"session_id":session,"yield_time_ms":1000,"max_output_tokens":2000}),
            )
            .await?;
    }
    Ok(value)
}
