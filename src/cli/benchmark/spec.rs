use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use std::io::{self, Read};
use std::path::Path;
use vtcode_core::core::agent::task::{ContextItem, Task};
use vtcode_core::utils::file_utils::read_file_with_context_sync;
use vtcode_core::utils::path::resolve_workspace_path;
use vtcode_core::utils::tty::TtyExt;

use super::BenchmarkCommandOptions;

const ERROR_SPEC_REQUIRED: &str =
    "Provide a benchmark specification via --task-file, --task, or STDIN.";
const ERROR_SPEC_EMPTY: &str = "Benchmark specification is empty.";
const CONTEXT_PREFIX: &str = "ctx";
const TASK_PREFIX: &str = "task";
const TASK_SECTION_SEPARATOR: &str = "\n\n";
const DEFAULT_TASK_TITLE: &str = "Benchmark Task";
const DEFAULT_DESCRIPTION_PLACEHOLDER: &str = "No description provided.";

#[derive(Debug)]
pub(super) struct PreparedTask {
    pub(super) task: Task,
    pub(super) contexts: Vec<ContextItem>,
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

pub(super) fn load_prepared_tasks(
    options: &BenchmarkCommandOptions,
    workspace: &Path,
) -> Result<Vec<PreparedTask>> {
    let spec_source = load_spec_source(options)?;
    parse_spec(&spec_source, workspace)
}

fn load_spec_source(options: &BenchmarkCommandOptions) -> Result<String> {
    if let Some(inline) = &options.inline_task {
        let trimmed = inline.trim();
        if !trimmed.is_empty() {
            return Ok(inline.clone());
        }
    }

    if let Some(path) = &options.task_file {
        let contents = read_file_with_context_sync(path, "benchmark specification")?;
        return Ok(contents);
    }

    let mut buffer = String::new();
    let stdin = io::stdin();
    if stdin.is_tty_ext() {
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
                let canonical =
                    resolve_workspace_path(workspace, Path::new(&path)).with_context(|| {
                        format!(
                            "Failed to resolve benchmark context path '{}' relative to workspace {}",
                            path,
                            workspace.display()
                        )
                    })?;

                content = read_file_with_context_sync(&canonical, "benchmark context file")?;
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
