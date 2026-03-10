use anyhow::{Context, Result, anyhow, bail};
use std::str::FromStr;
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::models::ModelId;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runner::{AgentRunner, RunnerSettings};
use vtcode_core::core::agent::types::AgentType;
use vtcode_core::utils::file_utils::write_file_with_context_sync;

use crate::startup::ensure_full_auto_workspace_trust;

use super::BenchmarkCommandOptions;
use super::report::{BenchmarkReport, BenchmarkTaskReport};
use super::spec::load_prepared_tasks;

const ERROR_FULL_AUTO_REQUIRED: &str =
    "Benchmark runs require --full-auto/--auto with [automation.full_auto] enabled.";
const ERROR_SPEC_EMPTY: &str = "Benchmark specification is empty.";

pub async fn handle_benchmark_command(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    options: BenchmarkCommandOptions,
    full_auto_requested: bool,
) -> Result<()> {
    if !full_auto_requested {
        bail!(ERROR_FULL_AUTO_REQUIRED);
    }

    let automation_cfg = &vt_cfg.automation.full_auto;
    if !automation_cfg.enabled {
        bail!(ERROR_FULL_AUTO_REQUIRED);
    }

    if !ensure_full_auto_workspace_trust(&config.workspace).await? {
        return Ok(());
    }

    let mut tasks = load_prepared_tasks(&options, &config.workspace)?;
    if tasks.is_empty() {
        bail!(ERROR_SPEC_EMPTY);
    }

    if let Some(limit) = options.max_tasks {
        if limit == 0 {
            bail!("--max-tasks must be greater than zero when provided.");
        }
        if tasks.len() > limit {
            tasks.truncate(limit);
        }
    }

    let model_id = ModelId::from_str(&config.model).with_context(|| {
        format!(
            "Model '{}' is not recognized for benchmark execution. Update vtcode.toml to a supported identifier.",
            config.model
        )
    })?;

    let session_id = format!(
        "benchmark-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|err| anyhow!("Failed to derive session identifier timestamp: {}", err))?
            .as_secs()
    );

    let mut runner = AgentRunner::new(
        AgentType::Single,
        model_id,
        config.api_key.clone(),
        config.workspace.clone(),
        session_id,
        RunnerSettings {
            reasoning_effort: Some(config.reasoning_effort),
            verbosity: None,
        },
        None,
    )
    .await?;
    runner.enable_full_auto(&automation_cfg.allowed_tools).await;

    let mut reports = Vec::with_capacity(tasks.len());
    let max_retries = vt_cfg.agent.max_task_retries;
    for prepared in &tasks {
        let result = runner
            .execute_task_with_retry(&prepared.task, &prepared.contexts, max_retries)
            .await
            .with_context(|| {
                format!(
                    "Failed to execute task '{}' after retries",
                    prepared.task.id
                )
            })?;
        reports.push(BenchmarkTaskReport::from_task_result(
            &prepared.task,
            result,
        ));
    }

    let report = BenchmarkReport {
        model: config.model.clone(),
        provider: config.provider.clone(),
        workspace: config.workspace.display().to_string(),
        task_count: reports.len(),
        tasks: reports,
    };

    let serialized = serde_json::to_string_pretty(&report)
        .context("Failed to serialize benchmark report to JSON")?;

    if let Some(path) = &options.output {
        write_file_with_context_sync(path, &serialized, "benchmark report")?;
    }

    println!("{}", serialized);
    Ok(())
}
