//! Plan artifact I/O: persisting drafts, syncing the embedded tracker,
//! and detecting workspace validation commands.
//!
//! Depends on `artifacts` for pure content shaping and on `state` for the
//! plan-file location. Tool wiring lives in `start.rs` / `finish.rs`.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::tools::handlers::planning_workflow::artifacts::{
    PlanValidationReport, extract_embedded_tracker, generate_tracker_markdown_from_plan, render_plan_with_tracker,
    tracker_file_for_plan_file, tracker_has_progress_or_notes, validate_plan_content,
};
use crate::tools::handlers::planning_workflow::state::PlanningWorkflowState;
use crate::tools::handlers::task_tracking::{
    TrackerArtifact, persist_tracker_artifacts, read_tracker_artifact as read_optional_file, write_tracker_artifact,
};
use crate::utils::file_utils::{ensure_dir_exists, read_file_with_context, write_file_with_context};

#[derive(Debug, Clone)]
pub struct PersistedPlanDraft {
    pub plan_file: PathBuf,
    pub tracker_file: Option<PathBuf>,
    pub validation: PlanValidationReport,
}

/// Allocate the workspace-local plan file when none exists.
///
/// Shared by `persist_plan_draft` (active-planning synthesis without a prior
/// `start_planning` call) and the execution-mode approval path, which must
/// persist a valid first draft without activating the planning workflow.
/// Sets the file pointer and baseline and ensures the parent directory.
/// Returns the existing path when one is already set.
pub async fn allocate_plan_file_if_missing(state: &PlanningWorkflowState) -> Result<PathBuf> {
    if let Some(existing) = state.get_plan_file().await {
        return Ok(existing);
    }
    // The dedicated plan agent can enter planning without invoking the
    // `start_planning` tool first. Plan synthesis must still have a
    // durable artifact before approval, so lazily allocate the same
    // workspace-local plan location used by `start_planning`.
    let plan_file = state
        .plans_dir()
        .join(format!("{}.md", vtcode_commons::slug::create_timestamped()));
    if let Some(parent) = plan_file.parent() {
        ensure_dir_exists(parent)
            .await
            .with_context(|| format!("Failed to create plans directory: {}", parent.display()))?;
    }
    state.set_plan_file(Some(plan_file.clone())).await;
    state.set_plan_baseline(Some(SystemTime::now())).await;
    tracing::info!(
        plan_file = %plan_file.display(),
        "Initialized missing plan file during plan synthesis"
    );
    Ok(plan_file)
}

pub async fn sync_tracker_into_plan_file(plan_file: &Path, tracker_markdown: &str) -> Result<()> {
    let plan_content = read_file_with_context(plan_file, "plan file")
        .await
        .with_context(|| format!("Failed to read plan file: {}", plan_file.display()))?;
    let updated = render_plan_with_tracker(&plan_content, Some(tracker_markdown));
    if updated == plan_content {
        return Ok(());
    }
    write_tracker_artifact(plan_file, &updated, "plan file")
        .await
        .with_context(|| format!("Failed to write plan file: {}", plan_file.display()))?;
    Ok(())
}

pub async fn persist_plan_draft(state: &PlanningWorkflowState, plan_markdown: &str) -> Result<PersistedPlanDraft> {
    let validation = validate_plan_content(plan_markdown);
    if !validation.is_ready() {
        bail!("plan draft is not ready for persistence: {}", validation.reasons().join("; "));
    }

    let plan_file = match state.get_plan_file().await {
        Some(path) => path,
        None if state.is_active() => allocate_plan_file_if_missing(state).await?,
        None => bail!("No active plan file. Call start_planning first."),
    };
    let existing_plan = read_optional_file(&plan_file, "plan file").await?;
    let tracker_file = tracker_file_for_plan_file(&plan_file);
    let (existing_tracker, tracker_from_sidecar) = if let Some(path) = tracker_file.as_ref() {
        let sidecar = read_optional_file(path, "plan tracker file").await?;
        if sidecar.is_some() {
            let existing_tracker = sidecar.clone().filter(|value| !value.trim().is_empty());
            (existing_tracker, true)
        } else {
            (
                existing_plan
                    .as_deref()
                    .and_then(extract_embedded_tracker)
                    .filter(|content: &String| !content.trim().is_empty()),
                false,
            )
        }
    } else {
        (
            existing_plan
                .as_deref()
                .and_then(extract_embedded_tracker)
                .filter(|content: &String| !content.trim().is_empty()),
            false,
        )
    };
    let workspace_root = state.workspace_root().unwrap_or_default();
    let global_tracker_file = (!workspace_root.as_os_str().is_empty())
        .then(|| workspace_root.join(".vtcode").join("tasks").join("current_task.md"));
    let existing_global_tracker = match global_tracker_file.as_ref() {
        Some(path) => read_optional_file(path, "task checklist").await?,
        None => None,
    };

    let should_refresh_embedded = !tracker_from_sidecar
        && existing_tracker
            .as_deref()
            .is_some_and(|tracker| !tracker_has_progress_or_notes(tracker));
    let generated_tracker = generate_tracker_markdown_from_plan(plan_markdown);
    let tracker_to_persist = if should_refresh_embedded {
        generated_tracker.or(existing_tracker.clone())
    } else {
        existing_tracker.clone().or(generated_tracker)
    };
    let Some(tracker_markdown) = tracker_to_persist.as_deref().filter(|content| !content.trim().is_empty()) else {
        bail!("unable to generate a non-empty plan task tracker");
    };
    let Some(tracker_file_path) = tracker_file.as_ref() else {
        bail!("unable to derive a plan task tracker path");
    };

    let canonical_plan = render_plan_with_tracker(plan_markdown, Some(tracker_markdown));
    let mut artifacts =
        vec![TrackerArtifact::new(tracker_file_path.clone(), tracker_markdown.to_owned(), "plan tracker file").await?];
    if let Some(global_tracker_file) = global_tracker_file
        && existing_global_tracker
            .as_deref()
            .is_none_or(|content| content.trim().is_empty())
    {
        artifacts.push(TrackerArtifact::new(global_tracker_file, tracker_markdown.to_owned(), "task checklist").await?);
    }
    artifacts.push(TrackerArtifact::new(plan_file.clone(), canonical_plan, "plan file").await?);
    persist_tracker_artifacts(artifacts).await?;

    Ok(PersistedPlanDraft {
        plan_file,
        tracker_file: Some(tracker_file_path.clone()),
        validation,
    })
}

pub(super) fn resolve_plan_path(workspace_root: &Path, raw_path: &str) -> PathBuf {
    let trimmed = raw_path.trim();
    if Path::new(trimmed).is_absolute() {
        PathBuf::from(trimmed)
    } else {
        workspace_root.join(trimmed)
    }
}

pub(super) fn plan_title_seed(path: &Path, fallback_plan_name: &str) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_string())
        .unwrap_or_else(|| fallback_plan_name.to_string())
}

pub(super) async fn initialize_plan_file(
    plan_file: &Path,
    plan_file_display: &str,
    plan_title: &str,
    description: Option<&str>,
    validation_hints: &ValidationCommandHints,
) -> Result<()> {
    let initial_content =
        render_initial_plan_file_content(plan_title, description, plan_file_display, validation_hints);
    write_file_with_context(plan_file, &initial_content, "plan file")
        .await
        .with_context(|| format!("Failed to create plan file: {}", plan_file.display()))
}

pub(super) async fn plan_file_baseline(plan_file: &Path) -> SystemTime {
    tokio::fs::metadata(plan_file)
        .await
        .and_then(|meta| meta.modified())
        .unwrap_or_else(|_| SystemTime::now())
}

fn render_initial_plan_file_content(
    plan_title: &str,
    description: Option<&str>,
    plan_file_display: &str,
    validation_hints: &ValidationCommandHints,
) -> String {
    let mut content = format!("# {plan_title}\n\n");
    content.push_str("Status: drafting\n");
    content.push_str(&format!("Created: {}\n", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")));
    content.push_str(&format!("Plan file: `{plan_file_display}`\n"));
    if let Some(description) = description.map(str::trim).filter(|value| !value.is_empty()) {
        content.push_str(&format!("Description: {description}\n"));
    }
    content.push('\n');
    content.push_str("> Planning workflow is active. Research first, then materialize one compact `<proposed_plan>` spec here (fit ~1500 tokens; steps as `Action -> files: [path] -> verify: [command]`, prefer file:symbol refs over prose; add `## Expected Outcomes` and `## Dependencies and Prerequisites` when material).\n");
    content.push_str(&format!(
        "> Suggested validation defaults: build/lint {}; tests {}.\n",
        validation_hints.build_and_lint, validation_hints.tests
    ));
    content
}

#[derive(Debug, Clone)]
pub(super) struct ValidationCommandHints {
    pub(super) build_and_lint: String,
    pub(super) tests: String,
}

fn package_manager_for_workspace(workspace_root: &Path) -> &'static str {
    if workspace_root.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if workspace_root.join("yarn.lock").exists() {
        "yarn"
    } else if workspace_root.join("bun.lockb").exists() || workspace_root.join("bun.lock").exists() {
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

pub(super) fn detect_validation_command_hints(workspace_root: &Path) -> ValidationCommandHints {
    if workspace_root.join("Cargo.toml").exists() {
        return ValidationCommandHints {
            build_and_lint: "`cargo check`; `cargo clippy --workspace --all-targets -- -D warnings`".to_string(),
            tests: "`cargo test` (or `cargo nextest run` if nextest is configured)".to_string(),
        };
    }

    if workspace_root.join("package.json").exists() {
        let pm = package_manager_for_workspace(workspace_root);
        let has_build = package_json_has_script(workspace_root, "build");
        let has_lint = package_json_has_script(workspace_root, "lint");
        let has_test = package_json_has_script(workspace_root, "test");

        let build_and_lint = match (has_build, has_lint) {
            (true, true) => format!("`{}`; `{}`", node_script_command(pm, "build"), node_script_command(pm, "lint")),
            (true, false) => {
                format!("`{}`; plus configured lint command for the workspace", node_script_command(pm, "build"))
            }
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

        return ValidationCommandHints { build_and_lint, tests };
    }

    if workspace_root.join("pyproject.toml").exists()
        || workspace_root.join("requirements.txt").exists()
        || workspace_root.join("setup.py").exists()
    {
        return ValidationCommandHints {
            build_and_lint: "`python -m compileall .`; run configured linter (for example `ruff check .`)".to_string(),
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
