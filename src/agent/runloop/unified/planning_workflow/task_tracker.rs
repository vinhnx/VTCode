use anyhow::{Context, bail};
use std::collections::HashMap;
use vtcode_commons::paths::ensure_path_within_workspace_resolved;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::handlers::task_tracking::{short_task_description, split_task_description_metadata};
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_ui::tui::app::{InlineHandle, PlanContent};

use super::tracker_response::resolve_tracker_file_response;
use super::validate_plan_content;

fn render_created_task_tracker(handle: &InlineHandle, output: &serde_json::Value) {
    let (panel_lines, panel_statuses, panel_current) = crate::agent::runloop::tool_output::tracker_panel_rows(output);
    // Approval has no renderer display mode available; default to expanded so
    // each task item is visible inline instead of only the plan name.
    let progress_lines = crate::agent::runloop::tool_output::tracker_transcript_lines(output, true);
    if panel_lines.is_empty() && progress_lines.is_empty() {
        return;
    }

    // Approval creates the tracker outside the normal tool pipeline, so make
    // the same panel/transcript updates that a regular task_tracker call gets.
    // Panel body keeps the full tree with per-row statuses; transcript uses
    // the shared single-writer path (never stacks duplicates).
    handle.update_task_panel_with_statuses(
        panel_lines,
        panel_statuses,
        panel_current,
        crate::agent::runloop::tool_output::tracker_panel_metadata(output),
    );
    handle.show_task_panel();
    crate::agent::runloop::unified::tool_output_handler::write_tracker_progress_transcript(handle, progress_lines);
}

#[derive(Debug, Clone)]
pub(crate) struct TaskTrackerHandoff {
    pub(crate) plan_file: std::path::PathBuf,
    pub(crate) tracker_file: std::path::PathBuf,
    pub(crate) item_count: usize,
}

fn markdown_task_description(line: &str) -> Option<(&str, bool)> {
    let trimmed = line.trim().strip_prefix("- ").unwrap_or(line.trim());
    if let Some(description) = trimmed.strip_prefix("[ ] ") {
        return Some((description.trim(), false));
    }
    if let Some(description) = trimmed.strip_prefix("[x] ").or_else(|| trimmed.strip_prefix("[X] ")) {
        return Some((description.trim(), true));
    }

    let (prefix, description) = trimmed.split_once('.').or_else(|| trimmed.split_once(')'))?;
    prefix
        .chars()
        .all(|character| character.is_ascii_digit())
        .then_some(description.trim())
        .filter(|description| !description.is_empty())
        .map(|description| (description, false))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlanSection {
    Summary,
    /// Plan context only — never distilled into tracker items.
    Scope,
    Implementation,
    Validation,
    Assumptions,
    /// Other known non-implementation sections (outcomes, deps, etc.).
    NonTracker,
}

fn normalized_plan_section_label(line: &str) -> (&str, bool, bool) {
    let mut label = line.trim();
    let mut is_quoted = false;
    while let Some(unquoted) = label.strip_prefix('>') {
        is_quoted = true;
        label = unquoted.trim_start();
    }
    let mut is_heading = false;
    while let Some(stripped) = label.strip_prefix('#') {
        is_heading = true;
        label = stripped.trim_start();
    }
    let label = label
        .strip_prefix("- ")
        .or_else(|| label.strip_prefix("* "))
        .unwrap_or(label)
        .trim()
        .trim_end_matches(':')
        .trim();

    (label, is_heading, is_quoted)
}

fn plan_section(line: &str) -> Option<PlanSection> {
    let (label, _, _) = normalized_plan_section_label(line);
    plan_section_label(label)
}

fn plan_section_label(label: &str) -> Option<PlanSection> {
    if label.eq_ignore_ascii_case("Summary") {
        Some(PlanSection::Summary)
    } else if label.eq_ignore_ascii_case("Scope") {
        Some(PlanSection::Scope)
    } else if is_phase_section_label(label)
        || label.eq_ignore_ascii_case("Implementation Steps")
        || label.eq_ignore_ascii_case("Steps")
    {
        Some(PlanSection::Implementation)
    } else if label.eq_ignore_ascii_case("Test Cases and Validation") || label.eq_ignore_ascii_case("Validation") {
        Some(PlanSection::Validation)
    } else if label.eq_ignore_ascii_case("Assumptions and Defaults") || label.eq_ignore_ascii_case("Assumptions") {
        Some(PlanSection::Assumptions)
    } else if label.eq_ignore_ascii_case("Expected Outcomes")
        || label.eq_ignore_ascii_case("Dependencies and Prerequisites")
        || label.eq_ignore_ascii_case("Repository facts checked")
    {
        Some(PlanSection::NonTracker)
    } else {
        None
    }
}

fn is_phase_section_label(label: &str) -> bool {
    let Some(rest) = label.get(..5).filter(|prefix| prefix.eq_ignore_ascii_case("phase")) else {
        return false;
    };
    let suffix = &label[rest.len()..];
    suffix.trim_start().starts_with(|character: char| character.is_ascii_digit())
}

/// Bare section labels (`Summary`) without markdown hashes are valid plan
/// markers — the same labels `PlanContent::from_markdown` accepts for display.
/// Keep the list/quote normalization aligned with `plan_section`; unknown
/// `##` headings stay non-tracker context.
fn is_bare_section_label(trimmed: &str, section: PlanSection) -> bool {
    let (label, is_heading, _) = normalized_plan_section_label(trimmed);
    !is_heading && plan_section_label(label) == Some(section)
}

fn sparse_implementation_task_lines(plan: &PlanContent) -> Vec<(&str, bool)> {
    let mut in_implementation = false;
    let mut saw_implementation = false;
    let mut compact_after_summary = false;
    let mut in_non_tracker_section = false;
    let mut tasks = Vec::new();

    for line in plan.raw_content.lines() {
        let trimmed = line.trim_start();
        let section = plan_section(line);
        let (_, is_heading, is_quoted) = normalized_plan_section_label(line);
        let is_section_marker =
            !is_quoted && (is_heading || section.is_some_and(|section| is_bare_section_label(trimmed, section)));

        if is_quoted && (is_heading || section.is_some()) {
            // A blockquoted heading is quoted plan context, not a real section
            // of the plan. Keep following unquoted numbered lines out of the
            // tracker until an actual section heading appears.
            in_implementation = false;
            in_non_tracker_section = true;
            compact_after_summary = false;
            continue;
        }

        if is_section_marker {
            if let Some(section) = section {
                match section {
                    PlanSection::Summary => {
                        in_implementation = false;
                        in_non_tracker_section = false;
                        compact_after_summary = !saw_implementation;
                    }
                    PlanSection::Scope | PlanSection::NonTracker => {
                        in_implementation = false;
                        in_non_tracker_section = true;
                        compact_after_summary = false;
                    }
                    PlanSection::Implementation => {
                        in_implementation = true;
                        saw_implementation = true;
                        in_non_tracker_section = false;
                        compact_after_summary = false;
                    }
                    PlanSection::Validation | PlanSection::Assumptions => {
                        in_implementation = false;
                        in_non_tracker_section = false;
                        compact_after_summary = false;
                    }
                }
            } else {
                // Unknown markdown heading: treat as non-implementation context so
                // numbered lines under custom sections are not distilled.
                in_implementation = false;
                in_non_tracker_section = true;
                compact_after_summary = false;
            }
            continue;
        }

        if (in_implementation || (compact_after_summary && !in_non_tracker_section))
            && let Some(task) = markdown_task_description(line)
        {
            tasks.push(task);
        }
    }

    tasks
}

fn normalized_task_description(description: &str) -> String {
    description
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn append_unique_task_item(
    items: &mut Vec<serde_json::Value>,
    index_by_description: &mut HashMap<String, usize>,
    item: serde_json::Value,
) {
    let Some(description) = item.get("description").and_then(|value| value.as_str()) else {
        return;
    };
    let key = normalized_task_description(description);
    if key.is_empty() {
        return;
    }

    if let Some(index) = index_by_description.get(&key).copied() {
        let existing = &mut items[index];
        if item["status"].as_str() == Some("completed") && existing["status"].as_str() != Some("completed") {
            existing["status"] = serde_json::Value::String("completed".to_string());
        }
        let existing_files_empty = existing
            .get("files")
            .and_then(serde_json::Value::as_array)
            .is_none_or(Vec::is_empty);
        if existing_files_empty
            && let Some(files) = item.get("files").filter(|files| !files.as_array().is_none_or(Vec::is_empty))
        {
            existing["files"] = files.clone();
        }
        let existing_verify_empty = existing
            .get("verify")
            .and_then(serde_json::Value::as_array)
            .is_none_or(Vec::is_empty);
        if existing_verify_empty
            && let Some(verify) = item.get("verify").filter(|verify| !verify.as_array().is_none_or(Vec::is_empty))
        {
            existing["verify"] = verify.clone();
        }
        return;
    }

    index_by_description.insert(key, items.len());
    items.push(item);
}

/// Build a tracker item from a plan-step description, splitting an inline
/// `Action -> files: [...] -> verify: [...]` suffix into structured fields so
/// the visible description stays clean. Explicit `step_files` (parsed from the
/// plan phase) win over inline-parsed files; inline verify is kept when the
/// caller provides none.
fn build_task_item(description: &str, completed: bool, step_files: &[String]) -> serde_json::Value {
    let (clean, parsed_files, parsed_verify) = split_task_description_metadata(description.trim());
    let description = if clean.is_empty() {
        description.trim().to_string()
    } else {
        clean
    };
    let mut item = serde_json::json!({
        "description": description,
        "status": if completed { "completed" } else { "pending" },
    });
    let files: Vec<String> = if step_files.is_empty() {
        parsed_files
    } else {
        step_files
            .iter()
            .map(|file| file.trim())
            .filter(|file| !file.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    };
    if !files.is_empty()
        && let Ok(files) = serde_json::to_value(&files)
    {
        item["files"] = files;
    }
    if !parsed_verify.is_empty()
        && let Ok(verify) = serde_json::to_value(&parsed_verify)
    {
        item["verify"] = verify;
    }
    item
}

/// Max bytes for the derived tracker title. Keeps the panel/transcript header
/// to a single scannable phrase.
const TRACKER_TITLE_MAX_BYTES: usize = 60;

/// Derive a descriptive tracker title from the approved plan.
///
/// The plan file stem is a generated codename (`1789108823046-jolly-forest`),
/// which reads as a random name in the TODO panel header and says nothing about
/// the work. Prefer the plan summary's leading clause; fall back to the first
/// task's short description and only then to the humanized file stem.
fn descriptive_tracker_title(
    plan: &PlanContent,
    plan_file: &std::path::Path,
    fallback_items: &[serde_json::Value],
) -> String {
    let clause = leading_title_clause(&plan.summary);
    if !clause.is_empty() {
        return vtcode_commons::formatting::truncate_byte_budget(&clause, TRACKER_TITLE_MAX_BYTES, "…");
    }
    for item in fallback_items {
        let description = item.get("description").and_then(|value| value.as_str()).unwrap_or_default();
        let short = short_task_description(description);
        let short = short.trim();
        if !short.is_empty() {
            return vtcode_commons::formatting::truncate_byte_budget(short, TRACKER_TITLE_MAX_BYTES, "…");
        }
    }
    let stem = plan_file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Implementation Plan");
    crate::agent::runloop::tool_output::humanize_tracker_title(stem)
}

/// Leading clause of the plan summary: the text before the first sentence end,
/// em/en dash, or clause separator (`:`, `;`). Falls back to the whole trimmed
/// line when no separator is present.
fn leading_title_clause(summary: &str) -> String {
    let first_line = summary.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    let mut cut = first_line.len();
    for separator in [". ", " — ", " – ", " - ", ": ", "; "] {
        if let Some(index) = first_line.find(separator) {
            cut = cut.min(index);
        }
    }
    first_line[..cut].trim().trim_end_matches([',', ';', ':']).to_string()
}

fn task_items_from_plan(plan: &PlanContent) -> Vec<serde_json::Value> {
    let mut items = Vec::new();
    let mut index_by_description = HashMap::new();
    for phase in &plan.phases {
        if !phase.name.trim().eq_ignore_ascii_case("Implementation Steps")
            && !phase.name.trim().eq_ignore_ascii_case("Steps")
        {
            continue;
        }
        for step in &phase.steps {
            if step.description.trim().is_empty() {
                continue;
            }
            let item = build_task_item(&step.description, step.completed, &step.files);
            append_unique_task_item(&mut items, &mut index_by_description, item);
        }
    }

    // Sparse plans may omit a phase heading. Preserve their numbered and
    // checkbox steps so approval still produces a usable tracker.
    if items.is_empty() {
        for (description, completed) in sparse_implementation_task_lines(plan) {
            let item = build_task_item(description, completed, &[]);
            append_unique_task_item(&mut items, &mut index_by_description, item);
        }
    }

    items
}

/// Create the implementation checklist while the planning gate is still
/// active. `PlanningTaskTrackerTool` deliberately rejects calls after
/// `finish_planning_workflow`, so this is the single lifecycle boundary for
/// approved-plan task handoff.
pub(crate) async fn create_task_tracker_from_active_plan(
    tool_registry: &ToolRegistry,
    handle: &InlineHandle,
) -> anyhow::Result<TaskTrackerHandoff> {
    let plan_state = tool_registry.planning_workflow_state();
    if !plan_state.is_active() {
        bail!("approved-plan task tracker creation requires an active planning workflow");
    }
    let plan_file = plan_state
        .get_plan_file()
        .await
        .context("approved plan is missing its persisted plan file")?;
    let plan_content = tokio::fs::read_to_string(&plan_file)
        .await
        .with_context(|| format!("failed to read approved plan {}", plan_file.display()))?;
    let validation = validate_plan_content(&plan_content);
    if !validation.is_ready() {
        bail!("approved plan is not ready for execution: {}", validation.reasons().join("; "));
    }

    let plan = PlanContent::from_markdown(
        plan_file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Implementation Plan")
            .to_string(),
        &plan_content,
        plan_file.to_str().map(|s| s.to_string()),
    );
    let items = task_items_from_plan(&plan);
    if items.is_empty() {
        bail!("approved plan contains no executable task items");
    }
    let item_count = items.len();

    let args = serde_json::json!({
        "action": "create",
        "title": descriptive_tracker_title(&plan, &plan_file, &items),
        "items": items,
    });

    // Registry execution adopts the checklist and its required verifier
    // identities before the approved execution turn can run other checks.
    let result = tool_registry
        .execute_tool(tools::TASK_TRACKER, args)
        .await
        .context("task_tracker failed during approved-plan handoff")?;
    if result.get("status").and_then(|value| value.as_str()) == Some("error") {
        bail!("task_tracker returned an error during approved-plan handoff: {result}");
    }
    // Tool responses intentionally expose workspace-relative paths. Resolve
    // that model-facing value back into the workspace before using it for
    // filesystem verification; otherwise a relative response would be
    // interpreted against the process working directory.
    let workspace_root = plan_state.workspace_root();
    let tracker_file = resolve_tracker_file_response(&result, workspace_root.as_deref(), &plan_file)?;
    let tracker_file = if let Some(workspace_root) = workspace_root.as_deref() {
        ensure_path_within_workspace_resolved(&tracker_file, workspace_root)
            .await
            .with_context(|| format!("failed to validate task tracker {}", tracker_file.display()))?
    } else {
        tracker_file
    };
    if !tokio::fs::try_exists(&tracker_file)
        .await
        .with_context(|| format!("failed to verify task tracker {}", tracker_file.display()))?
    {
        bail!("task_tracker reported success but did not create {}", tracker_file.display());
    }
    render_created_task_tracker(handle, &result);
    let message = result
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("Task tracker created");
    tracing::info!(message = %message, tracker_file = %tracker_file.display(), item_count, "Task tracker created during approved-plan handoff");

    Ok(TaskTrackerHandoff { plan_file, tracker_file, item_count })
}

#[cfg(test)]
mod tests {
    use super::{
        PlanContent, descriptive_tracker_title, leading_title_clause, resolve_tracker_file_response,
        task_items_from_plan,
    };
    use serde_json::json;
    use std::path::{Path, PathBuf};

    #[test]
    fn leading_title_clause_stops_at_first_separator() {
        assert_eq!(
            leading_title_clause("Refine `README.md` so it matches the CLI surface: add new flow, tighten prose."),
            "Refine `README.md` so it matches the CLI surface"
        );
        assert_eq!(leading_title_clause("Ship the change — rationale follows"), "Ship the change");
        assert_eq!(leading_title_clause("Fix cache miss."), "Fix cache miss.");
        assert_eq!(leading_title_clause(""), "");
    }

    #[test]
    fn descriptive_tracker_title_prefers_summary_over_codename() {
        let plan = PlanContent::from_markdown(
            "1789108823046-jolly-forest".to_string(),
            "## Summary\nRefine `README.md` so it matches the current CLI surface: add the exec resume flow.\n",
            None,
        );

        let title = descriptive_tracker_title(&plan, Path::new("/plans/1789108823046-jolly-forest.md"), &[]);

        assert_eq!(title, "Refine `README.md` so it matches the current CLI surface");
        assert!(!title.contains("Jolly"), "the generated codename must not surface: {title}");
    }

    #[test]
    fn descriptive_tracker_title_bounds_long_summaries() {
        let plan = PlanContent::from_markdown(
            "1789108823046-jolly-forest".to_string(),
            "## Summary\nImplement a very long refactor across many modules and services that keeps going without a separator\n",
            None,
        );

        let title = descriptive_tracker_title(&plan, Path::new("/plans/1789108823046-jolly-forest.md"), &[]);

        assert!(title.len() <= 60 + '…'.len_utf8(), "title must stay bounded: {title:?}");
        assert!(title.ends_with('…'), "bounded title keeps the ellipsis marker: {title:?}");
    }

    #[test]
    fn descriptive_tracker_title_falls_back_to_first_item_before_codename() {
        let plan = PlanContent::from_markdown("1789108823046-jolly-forest".to_string(), "## Scope\n", None);
        assert!(plan.summary.trim().is_empty(), "fixture must keep the summary empty");
        let items = vec![
            json!({"description": "Add vtcode exec resume to the Commands section – document the resume contract", "status": "pending"}),
        ];

        let title = descriptive_tracker_title(&plan, Path::new("/plans/1789108823046-jolly-forest.md"), &items);

        assert_eq!(title, "Add vtcode exec resume to the Commands section");
        assert!(!title.contains("Jolly"), "the generated codename must not surface: {title}");
    }

    #[test]
    fn descriptive_tracker_title_falls_back_to_humanized_stem() {
        // Headings only: the parser leaves `summary` empty, so the generated
        // codename stem is humanized instead of surfacing as the raw file stem.
        let plan = PlanContent::from_markdown("1789108823046-jolly-forest".to_string(), "## Scope\n", None);
        assert!(plan.summary.trim().is_empty(), "fixture must keep the summary empty");

        let title = descriptive_tracker_title(&plan, Path::new("/plans/1789108823046-jolly-forest.md"), &[]);

        assert_eq!(title, "Jolly Forest");
    }

    #[test]
    fn tracker_response_resolves_workspace_relative_path() {
        let result = json!({"tracker_file": ".vtcode/plans/task.tasks.md"});
        let workspace = Path::new("/workspace");
        let fallback = Path::new("/workspace/.vtcode/plans/fallback.tasks.md");

        let resolved = resolve_tracker_file_response(&result, Some(workspace), fallback).unwrap();

        assert_eq!(resolved, PathBuf::from("/workspace/.vtcode/plans/task.tasks.md"));
    }

    #[test]
    fn tracker_response_rejects_workspace_escape() {
        let result = json!({"tracker_file": "../outside.tasks.md"});
        let workspace = Path::new("/workspace");
        let fallback = Path::new("/workspace/.vtcode/plans/fallback.tasks.md");

        let error = resolve_tracker_file_response(&result, Some(workspace), fallback).unwrap_err();

        assert!(error.to_string().contains("escapes workspace"));
    }

    #[test]
    fn scope_section_is_not_distilled_into_tracker_items() {
        let plan = PlanContent::from_markdown(
            "scope-test".to_string(),
            "## Summary\nShip a focused change.\n\n## Scope\n- In: README intro\n- Out: unrelated modules\n\n## Implementation Steps\n1. Tighten intro -> files: [README.md] -> verify: [rg -n 'Why VT Code' README.md]\n\n## Test Cases and Validation\n- rg check\n\n## Assumptions and Defaults\n- Leave unrelated modules alone.\n",
            None,
        );
        let items = task_items_from_plan(&plan);
        assert_eq!(items.len(), 1, "only Implementation Steps become tracker items: {items:?}");
        let description = items[0].get("description").and_then(|value| value.as_str()).unwrap_or_default();
        assert!(description.contains("Tighten intro"));
        assert!(!description.contains("Out:"));
        assert!(!description.contains("In:"));
    }

    #[test]
    fn sparse_plan_scope_lines_are_not_distilled_into_tracker_items() {
        // Sparse plans without an Implementation Steps heading must still skip
        // numbered/checkbox Scope lines.
        let plan = PlanContent::from_markdown(
            "sparse-scope".to_string(),
            "## Summary\nFocused README tweak.\n\n1. Tighten the intro paragraph\n2. Verify with rg\n\n## Scope\n1. In scope: intro paragraph\n2. Out of scope: other docs\n",
            None,
        );
        let items = task_items_from_plan(&plan);
        let descriptions: Vec<String> = items
            .iter()
            .filter_map(|item| item.get("description").and_then(|value| value.as_str()).map(ToOwned::to_owned))
            .collect();
        assert!(
            descriptions.iter().any(|d| d.contains("Tighten the intro paragraph")),
            "implementation steps remain: {descriptions:?}"
        );
        assert!(
            !descriptions
                .iter()
                .any(|d| d.contains("In scope") || d.contains("Out of scope")),
            "Scope lines must not become tracker items: {descriptions:?}"
        );
    }

    #[test]
    fn sparse_plan_unknown_heading_sections_are_not_distilled() {
        let plan = PlanContent::from_markdown(
            "sparse-unknown".to_string(),
            "## Summary\nWork.\n\n1. Implement the change\n\n## Open Questions\n1. Decide migration strategy\n",
            None,
        );
        let items = task_items_from_plan(&plan);
        let descriptions: Vec<String> = items
            .iter()
            .filter_map(|item| item.get("description").and_then(|value| value.as_str()).map(ToOwned::to_owned))
            .collect();
        assert!(descriptions.iter().any(|d| d.contains("Implement the change")));
        assert!(
            !descriptions.iter().any(|d| d.contains("Decide migration")),
            "unknown sections must not become tracker items: {descriptions:?}"
        );
    }

    #[test]
    fn sparse_plan_phase_headings_are_distilled() {
        let plan = PlanContent::from_markdown(
            "sparse-phases".to_string(),
            "## Summary\nWork.\n\n## Phase 1: Implement\n1. Update the runtime\n\n## Phase 2: Verify\n1. Run focused checks\n\n## Test Cases and Validation\n1. Run the checks\n\n## Assumptions and Defaults\n1. Keep existing behavior\n",
            None,
        );
        let items = task_items_from_plan(&plan);
        let descriptions: Vec<String> = items
            .iter()
            .filter_map(|item| item.get("description").and_then(|value| value.as_str()).map(ToOwned::to_owned))
            .collect();

        assert_eq!(descriptions, vec!["Update the runtime", "Run focused checks"]);
    }

    #[test]
    fn sparse_plan_quoted_headings_are_not_distilled() {
        let plan = PlanContent::from_markdown(
            "sparse-quoted".to_string(),
            "## Summary\nWork.\n\n1. Implement the change\n\n> ## Scope\n1. Keep this context only\n\n> ## Open Questions\n1. Decide the migration strategy\n",
            None,
        );
        let items = task_items_from_plan(&plan);
        let descriptions: Vec<String> = items
            .iter()
            .filter_map(|item| item.get("description").and_then(|value| value.as_str()).map(ToOwned::to_owned))
            .collect();

        assert_eq!(descriptions, vec!["Implement the change"]);
    }

    #[test]
    fn sparse_plan_quoted_implementation_heading_does_not_open_tracker_section() {
        let plan = PlanContent::from_markdown(
            "sparse-quoted-implementation".to_string(),
            "## Summary\nWork.\n\n> ## Implementation Steps\n> 1. Quoted context\n\n## Implementation Steps\n1. Real implementation step\n",
            None,
        );
        let items = task_items_from_plan(&plan);
        let descriptions: Vec<String> = items
            .iter()
            .filter_map(|item| item.get("description").and_then(|value| value.as_str()).map(ToOwned::to_owned))
            .collect();

        assert_eq!(descriptions, vec!["Real implementation step"]);
    }

    #[test]
    fn sparse_approved_plan_is_distilled_into_tracker_items() {
        let plan = PlanContent::from_markdown(
            "Launch plan".to_string(),
            "Summary\nImprove startup latency.\n\n1. Measure startup\n2. Defer eager setup\n- [x] Verify with cargo check",
            None,
        );

        let items = task_items_from_plan(&plan);

        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["description"], "Measure startup");
        assert_eq!(items[0]["status"], "pending");
        assert_eq!(items[2]["description"], "Verify with cargo check");
        assert_eq!(items[2]["status"], "completed");
    }

    #[test]
    fn repeated_plan_steps_are_deduplicated_in_order() {
        // Dedup applies to implementation steps in named phases or sparse
        // plans after Summary.
        let plan = PlanContent::from_markdown(
            "Launch plan".to_string(),
            "## Summary\nImprove the runtime.\n\n## Phase 1\n1. Inspect the runtime\n2. Apply the fix\n\n## Phase 2\n1. inspect   the runtime\n[x] Verify the fix\n[x] APPLY THE FIX",
            None,
        );

        let items = task_items_from_plan(&plan);

        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["description"], "Inspect the runtime");
        assert_eq!(items[1]["description"], "Apply the fix");
        assert_eq!(items[1]["status"], "completed");
        assert_eq!(items[2]["description"], "Verify the fix");
    }

    #[test]
    fn tracker_only_contains_implementation_steps() {
        let plan = PlanContent::from_markdown(
            "Launch plan".to_string(),
            "## Summary\nImprove startup behavior.\n\n## Implementation Steps\n1. Update src/startup.rs -> verify: cargo nextest run -p vtcode\n\n## Test Cases and Validation\n1. Run cargo nextest run -p vtcode\n\n## Assumptions and Defaults\n1. Existing startup policy remains unchanged.",
            None,
        );

        let items = task_items_from_plan(&plan);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["description"], "Update src/startup.rs");
        assert_eq!(items[0]["verify"], serde_json::json!(["cargo nextest run -p vtcode"]));
    }

    #[test]
    fn inline_files_and_verify_are_split_into_structured_fields() {
        let plan = PlanContent::from_markdown(
            "Launch plan".to_string(),
            "## Summary\nImprove startup behavior.\n\n## Implementation Steps\n1. Emit summary -> files: [src/a.rs, src/b.rs] -> verify: [cargo check]\n\n## Test Cases and Validation\n1. Run cargo check.\n\n## Assumptions and Defaults\n1. Existing policy remains unchanged.",
            None,
        );

        let items = task_items_from_plan(&plan);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["description"], "Emit summary");
        assert_eq!(items[0]["files"], serde_json::json!(["src/a.rs", "src/b.rs"]));
        assert_eq!(items[0]["verify"], serde_json::json!(["cargo check"]));
    }
}
