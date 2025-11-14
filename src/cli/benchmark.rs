use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::config::models::ModelId;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runner::{AgentRunner, ContextItem, Task};
use vtcode_core::core::agent::types::AgentType;
use vtcode_core::{RunnerTaskOutcome, RunnerTaskResults};

use crate::workspace_trust::{WorkspaceTrustGateResult, ensure_workspace_trust};

const ERROR_SPEC_REQUIRED: &str =
    "Provide a benchmark specification via --task-file, --task, or STDIN.";
const ERROR_FULL_AUTO_REQUIRED: &str =
    "Benchmark runs require --full-auto/--auto with [automation.full_auto] enabled.";
const ERROR_SPEC_EMPTY: &str = "Benchmark specification is empty.";
const CONTEXT_PREFIX: &str = "ctx";
const TASK_PREFIX: &str = "task";
const TASK_SECTION_SEPARATOR: &str = "\n\n";
const DEFAULT_TASK_TITLE: &str = "Benchmark Task";
const DEFAULT_DESCRIPTION_PLACEHOLDER: &str = "No description provided.";

/// Options passed from the CLI layer for running the benchmark command.
#[derive(Debug, Clone)]
pub struct BenchmarkCommandOptions {
    pub task_file: Option<PathBuf>,
    pub inline_task: Option<String>,
    pub output: Option<PathBuf>,
    pub max_tasks: Option<usize>,
}

#[derive(Debug)]
struct PreparedTask {
    task: Task,
    contexts: Vec<ContextItem>,
}

#[derive(Debug, Deserialize, Default)]
struct RawSpecWrapper {
    #[serde(default)]
    tasks: Vec<RawTaskSpec>,
    #[serde(default)]
    cases: Vec<RawTaskSpec>,
    #[serde(default)]
    task: Option<RawTaskSpec>,
}

#[derive(Debug, Deserialize, Default)]
struct RawTaskSpec {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    instructions: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    problem: Option<String>,
    #[serde(default)]
    bug_description: Option<String>,
    #[serde(default)]
    contexts: Vec<RawContextEntry>,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    reference_context: Vec<RawContextEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawContextEntry {
    Text(String),
    Detailed(RawContextDetail),
}

#[derive(Debug, Deserialize, Default)]
struct RawContextDetail {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    model: String,
    provider: String,
    workspace: String,
    task_count: usize,
    tasks: Vec<BenchmarkTaskReport>,
}

#[derive(Debug, Serialize)]
struct BenchmarkTaskReport {
    id: String,
    title: String,
    summary: String,
    modified_files: Vec<String>,
    executed_commands: Vec<String>,
    warnings: Vec<String>,
    outcome: RunnerTaskOutcome,
    turns_executed: usize,
    total_duration_ms: u128,
    average_turn_duration_ms: Option<f64>,
    max_turn_duration_ms: Option<u128>,
    turn_durations_ms: Vec<u128>,
    success: bool,
}

impl BenchmarkTaskReport {
    fn from(task: &Task, result: RunnerTaskResults) -> Self {
        let success = matches!(
            result.outcome,
            RunnerTaskOutcome::Success | RunnerTaskOutcome::StoppedNoAction
        ) && result.warnings.is_empty();
        Self {
            id: task.id.clone(),
            title: task.title.clone(),
            summary: result.summary,
            modified_files: result.modified_files,
            executed_commands: result.executed_commands,
            warnings: result.warnings,
            outcome: result.outcome,
            turns_executed: result.turns_executed,
            total_duration_ms: result.total_duration_ms,
            average_turn_duration_ms: result.average_turn_duration_ms,
            max_turn_duration_ms: result.max_turn_duration_ms,
            turn_durations_ms: result.turn_durations_ms,
            success,
        }
    }
}

pub async fn handle_benchmark_command(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    options: BenchmarkCommandOptions,
    full_auto_requested: bool,
) -> Result<()> {
    match ensure_workspace_trust(&config.workspace, true).await? {
        WorkspaceTrustGateResult::Trusted(level) => {
            if level != WorkspaceTrustLevel::FullAuto {
                bail!(
                    "Benchmark command requires workspace trust level 'full_auto'. Upgrade trust before proceeding."
                );
            }
        }
        WorkspaceTrustGateResult::Aborted => {
            return Ok(());
        }
    }

    if !full_auto_requested {
        bail!(ERROR_FULL_AUTO_REQUIRED);
    }

    let automation_cfg = &vt_cfg.automation.full_auto;
    if !automation_cfg.enabled {
        bail!(ERROR_FULL_AUTO_REQUIRED);
    }

    let spec_source = load_spec_source(&options)?;
    let mut tasks = parse_spec(&spec_source, &config.workspace)?;
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
        Some(config.reasoning_effort),
        None,
    )
    .await?;

    runner
        .apply_workspace_configuration(vt_cfg)
        .await
        .context("Failed to apply workspace configuration to benchmark runner")?;
    runner.enable_full_auto(&automation_cfg.allowed_tools).await;

    let mut reports = Vec::with_capacity(tasks.len());
    for prepared in &tasks {
        let result = runner
            .execute_task(&prepared.task, &prepared.contexts)
            .await
            .with_context(|| format!("Failed to execute task '{}'", prepared.task.id))?;
        reports.push(BenchmarkTaskReport::from(&prepared.task, result));
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
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create benchmark report directory {}",
                    parent.display()
                )
            })?;
        }

        fs::write(path, serialized.as_bytes())
            .with_context(|| format!("Failed to write benchmark report to {}", path.display()))?;
    }

    println!("{}", serialized);

    Ok(())
}

fn load_spec_source(options: &BenchmarkCommandOptions) -> Result<String> {
    if let Some(inline) = &options.inline_task {
        let trimmed = inline.trim();
        if !trimmed.is_empty() {
            return Ok(inline.clone());
        }
    }

    if let Some(path) = &options.task_file {
        let contents = fs::read_to_string(path).with_context(|| {
            format!(
                "Failed to read benchmark specification from {}",
                path.display()
            )
        })?;
        return Ok(contents);
    }

    let mut buffer = String::new();
    let stdin = io::stdin();
    if stdin.is_terminal() {
        bail!(ERROR_SPEC_REQUIRED);
    }

    stdin
        .lock()
        .read_to_string(&mut buffer)
        .context("Failed to read benchmark specification from STDIN")?;
    Ok(buffer)
}

fn parse_spec(source: &str, workspace: &Path) -> Result<Vec<PreparedTask>> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        bail!(ERROR_SPEC_EMPTY);
    }

    if let Ok(task_list) = serde_json::from_str::<Vec<RawTaskSpec>>(trimmed) {
        return convert_tasks(task_list, workspace);
    }

    if let Ok(wrapper) = serde_json::from_str::<RawSpecWrapper>(trimmed) {
        let mut tasks = Vec::new();
        tasks.extend(wrapper.tasks);
        tasks.extend(wrapper.cases);
        if let Some(task) = wrapper.task {
            tasks.push(task);
        }
        if !tasks.is_empty() {
            return convert_tasks(tasks, workspace);
        }
    }

    if let Ok(single) = serde_json::from_str::<RawTaskSpec>(trimmed) {
        return convert_tasks(vec![single], workspace);
    }

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        // Validate JSON to return a clearer error message.
        serde_json::from_str::<Value>(trimmed)
            .context("Failed to parse benchmark specification JSON structure")?;
        bail!(
            "Unsupported benchmark JSON structure. Expected either an array of tasks or an object containing a \"tasks\" array."
        );
    }

    Ok(vec![PreparedTask {
        task: Task {
            id: format!("{}-1", TASK_PREFIX),
            title: DEFAULT_TASK_TITLE.to_string(),
            description: trimmed.to_string(),
            instructions: None,
        },
        contexts: Vec::new(),
    }])
}

fn convert_tasks(raw_tasks: Vec<RawTaskSpec>, workspace: &Path) -> Result<Vec<PreparedTask>> {
    let mut prepared = Vec::with_capacity(raw_tasks.len());
    for (index, raw) in raw_tasks.into_iter().enumerate() {
        prepared.push(prepare_task(raw, index, workspace)?);
    }
    Ok(prepared)
}

fn prepare_task(mut raw: RawTaskSpec, index: usize, workspace: &Path) -> Result<PreparedTask> {
    let identifier = raw
        .id
        .clone()
        .unwrap_or_else(|| format!("{}-{}", TASK_PREFIX, index + 1));

    let title = raw
        .title
        .clone()
        .or_else(|| raw.id.clone())
        .unwrap_or_else(|| format!("{} {}", DEFAULT_TASK_TITLE, index + 1));

    let mut description_parts: Vec<String> = Vec::new();
    for text in [
        raw.description.take(),
        raw.summary.take(),
        raw.problem.take(),
        raw.bug_description.take(),
        raw.query.take(),
    ]
    .into_iter()
    .flatten()
    {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            description_parts.push(trimmed.to_string());
        }
    }

    if description_parts.is_empty() {
        description_parts.push(DEFAULT_DESCRIPTION_PLACEHOLDER.to_string());
    }

    let description = description_parts.join(TASK_SECTION_SEPARATOR);

    let instructions = raw
        .instructions
        .take()
        .or_else(|| raw.prompt.take())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let contexts = build_contexts(raw.contexts, raw.reference_context, raw.context, workspace)?;

    let task = Task {
        id: identifier,
        title,
        description,
        instructions,
    };

    Ok(PreparedTask { task, contexts })
}

fn build_contexts(
    contexts: Vec<RawContextEntry>,
    reference_context: Vec<RawContextEntry>,
    single: Option<String>,
    workspace: &Path,
) -> Result<Vec<ContextItem>> {
    let mut entries: Vec<RawContextEntry> = Vec::new();
    entries.extend(contexts);
    entries.extend(reference_context);
    if let Some(context) = single {
        let trimmed = context.trim();
        if !trimmed.is_empty() {
            entries.push(RawContextEntry::Text(trimmed.to_string()));
        }
    }

    let mut contexts = Vec::with_capacity(entries.len());
    for (index, entry) in entries.into_iter().enumerate() {
        contexts.push(convert_context_entry(entry, workspace, index)?);
    }
    Ok(contexts)
}

fn convert_context_entry(
    entry: RawContextEntry,
    workspace: &Path,
    index: usize,
) -> Result<ContextItem> {
    match entry {
        RawContextEntry::Text(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                bail!(
                    "Encountered an empty context entry at position {}",
                    index + 1
                );
            }

            Ok(ContextItem {
                id: format!("{}-{}", CONTEXT_PREFIX, index + 1),
                content: trimmed.to_string(),
            })
        }
        RawContextEntry::Detailed(detail) => {
            let mut content = detail.content.unwrap_or_default().trim().to_string();

            if content.is_empty()
                && let Some(path) = detail.path
            {
                let resolved = workspace.join(&path);
                let canonical = resolved.canonicalize().with_context(|| {
                    format!(
                        "Failed to resolve context path '{}' relative to workspace {}",
                        path,
                        workspace.display()
                    )
                })?;

                if !canonical.starts_with(workspace) {
                    bail!(
                        "Context path '{}' escapes the workspace boundary {}",
                        canonical.display(),
                        workspace.display()
                    );
                }

                content = fs::read_to_string(&canonical).with_context(|| {
                    format!("Failed to read context file {}", canonical.display())
                })?;
            }

            if content.trim().is_empty() {
                bail!(
                    "Encountered an empty context entry at position {}",
                    index + 1
                );
            }

            let identifier = detail
                .id
                .unwrap_or_else(|| format!("{}-{}", CONTEXT_PREFIX, index + 1));

            Ok(ContextItem {
                id: identifier,
                content,
            })
        }
    }
}
