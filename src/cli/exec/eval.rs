//! Eval suite runner for `vtcode exec eval`.
//!
//! The orchestration core lives in `vtcode_eval::run_suite`, which depends
//! only on the [`vtcode_eval::EvalExecutor`] trait. This file wires the
//! production executor ([`AgentRunnerExecutor`]) to that trait, applies the
//! trust/automation guardrails, and handles I/O.

use crate::startup::require_full_auto_workspace_trust;
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::str::FromStr;
use std::time::Instant;
use vtcode_core::cli::input_hardening::validate_agent_safe_text;
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::models::ModelId;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runner::{AgentRunner, RunnerSettings};
use vtcode_core::core::agent::task::{ContextItem, Task, TaskOutcome};
use vtcode_core::core::agent::types::AgentType;
use vtcode_core::core::threads::ThreadBootstrap;
use vtcode_eval::{
    EvalRunResult, EvalSuite, EvalTask, RunOutcome,
    environment::{CommandProbe, EnvironmentProbe},
    run_suite,
};

use super::ExecCommandKind;
use super::run::task_spec;

/// Handle the `vtcode exec eval --suite <path>` command.
pub(crate) async fn handle_eval_command(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    suite_path: &Path,
    output_path: Option<&Path>,
) -> Result<()> {
    let suite_json = std::fs::read_to_string(suite_path)
        .with_context(|| format!("read eval suite from {}", suite_path.display()))?;
    let suite: EvalSuite = serde_json::from_str(&suite_json)
        .with_context(|| format!("parse eval suite JSON from {}", suite_path.display()))?;

    if suite.attempts < 1 {
        bail!("eval suite requires attempts >= 1");
    }

    eprintln!(
        "Running eval suite: {} ({} tasks, {} attempts each)",
        suite.name,
        suite.tasks.len(),
        suite.attempts
    );

    // H1: require full-auto workspace trust
    require_full_auto_workspace_trust(config.workspace.as_path(), "eval runs", "eval").await?;

    // H2: require full-auto enabled
    if !vt_cfg.automation.full_auto.enabled {
        bail!(
            "Automation is disabled in configuration. Enable [automation.full_auto] to run eval."
        );
    }

    let allowed_tools = vt_cfg.automation.full_auto.allowed_tools.clone();
    let executor = AgentRunnerExecutor::new(config, vt_cfg, &allowed_tools);

    let report = run_suite(&executor, &suite).await?;

    let markdown = report.to_markdown();
    if let Some(path) = output_path {
        std::fs::write(path, &markdown)
            .with_context(|| format!("write report to {}", path.display()))?;
        eprintln!("\nReport written to {}", path.display());
    } else {
        println!("{markdown}");
    }
    Ok(())
}

/// Production executor: runs each task through the agent runner and applies
/// environment probes to verify the claimed outcome.
struct AgentRunnerExecutor {
    config: CoreAgentConfig,
    vt_cfg: VTCodeConfig,
    allowed_tools: Vec<String>,
}

impl AgentRunnerExecutor {
    fn new(config: &CoreAgentConfig, vt_cfg: &VTCodeConfig, allowed_tools: &[String]) -> Self {
        Self {
            config: config.clone(),
            vt_cfg: vt_cfg.clone(),
            allowed_tools: allowed_tools.to_vec(),
        }
    }
}

#[async_trait::async_trait]
impl vtcode_eval::EvalExecutor for AgentRunnerExecutor {
    async fn execute_task(&self, eval_task: &EvalTask) -> Result<EvalRunResult> {
        Ok(run_eval_task(&self.config, &self.vt_cfg, eval_task, &self.allowed_tools).await)
    }
}

/// Run a single eval task attempt through the agent runner + probes.
async fn run_eval_task(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    eval_task: &EvalTask,
    allowed_tools: &[String],
) -> EvalRunResult {
    let start = Instant::now();
    let session_id = format!("eval-{}-attempt", eval_task.id);

    if let Err(e) = validate_agent_safe_text("eval_task.prompt", &eval_task.prompt) {
        return eval_error(
            &eval_task.id,
            start,
            format!("Prompt validation failed: {e}"),
        );
    }

    let model_id = match ModelId::from_str(&config.model) {
        Ok(id) => id,
        Err(e) => return eval_error(&eval_task.id, start, format!("Model not recognized: {e}")),
    };

    let runner_result = AgentRunner::new_with_bootstrap(
        AgentType::Single,
        model_id,
        config.api_key.clone(),
        config.workspace.clone(),
        session_id.clone(),
        RunnerSettings {
            reasoning_effort: Some(config.reasoning_effort),
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(vt_cfg.clone()),
        config.openai_chatgpt_auth.clone(),
    )
    .await;

    let mut runner = match runner_result {
        Ok(r) => r,
        Err(e) => return eval_error(&eval_task.id, start, format!("Runner creation failed: {e}")),
    };

    runner.enable_full_auto(allowed_tools).await;
    runner.set_quiet(true);

    let ts = task_spec(
        &ExecCommandKind::Eval {
            suite_path: std::path::PathBuf::new(),
            output_path: None,
        },
        false,
    );
    let task = Task {
        id: eval_task.id.clone(),
        title: eval_task.name.clone(),
        description: eval_task.prompt.clone(),
        instructions: Some(ts.instructions.to_string()),
    };

    let exec_result = runner
        .execute_task_with_retry(&task, &[] as &[ContextItem], 1)
        .await;
    let duration_secs = start.elapsed().as_secs_f64();

    // M4: timeout_secs is not enforced here; runner max_turns / session budget
    // provides the practical wall-clock guard. Documented in suite schema.

    let exec_outcome = match &exec_result {
        Ok(result)
            if matches!(
                result.outcome,
                TaskOutcome::Success | TaskOutcome::StoppedNoAction
            ) =>
        {
            RunOutcome::Pass
        }
        Ok(_) => RunOutcome::Fail,
        Err(_) => RunOutcome::Error,
    };

    if exec_outcome == RunOutcome::Pass {
        let probes = build_probes(eval_task);
        if !probes.is_empty() && !probes.iter().all(|p| p.check(&config.workspace)) {
            return EvalRunResult {
                task_id: eval_task.id.clone(),
                outcome: RunOutcome::Fail,
                transcript_path: None,
                cost_usd: None,
                duration_secs,
                attempt: 0,
                error_message: Some("Environment probes failed after agent claimed success".into()),
            };
        }
    }

    EvalRunResult {
        task_id: eval_task.id.clone(),
        outcome: exec_outcome,
        transcript_path: None,
        cost_usd: None,
        duration_secs,
        attempt: 0,
        error_message: None,
    }
}

/// M1: DRY helper for error EvalRunResult construction.
fn eval_error(task_id: &str, start: Instant, message: String) -> EvalRunResult {
    EvalRunResult {
        task_id: task_id.into(),
        outcome: RunOutcome::Error,
        transcript_path: None,
        cost_usd: None,
        duration_secs: start.elapsed().as_secs_f64(),
        attempt: 0,
        error_message: Some(message),
    }
}

/// Build environment probes from an eval task's verify commands.
fn build_probes(eval_task: &EvalTask) -> Vec<Box<dyn EnvironmentProbe>> {
    eval_task
        .verify_commands
        .iter()
        .filter_map(|cmd| {
            let parts: Vec<&str> = cmd.split_whitespace().collect();
            if parts.is_empty() {
                return None;
            }
            let command = parts[0].to_string();
            let args: Vec<String> = parts[1..].iter().copied().map(str::to_string).collect();
            Some(Box::new(CommandProbe::new(command, args)) as Box<dyn EnvironmentProbe>)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_eval::EvalTask;

    #[test]
    fn build_probes_parses_command_and_args() {
        let task = EvalTask {
            id: "t".into(),
            name: "t".into(),
            category: vtcode_eval::EvalCategory::Capability,
            prompt: "p".into(),
            verify_commands: vec!["cargo test --all".into(), "".into(), "git status".into()],
            timeout_secs: None,
        };
        let probes = build_probes(&task);
        // Empty command is filtered out -> 2 probes.
        assert_eq!(probes.len(), 2);
    }

    #[test]
    fn build_probes_empty_when_no_commands() {
        let task = EvalTask {
            id: "t".into(),
            name: "t".into(),
            category: vtcode_eval::EvalCategory::Capability,
            prompt: "p".into(),
            verify_commands: vec![],
            timeout_secs: None,
        };
        assert!(build_probes(&task).is_empty());
    }
}
