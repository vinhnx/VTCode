//! Plan/tracker embedding, path pairing, and tracker generation.

use super::sections::{IMPLEMENTATION_SECTION_ALIASES, collect_implementation_step_blocks, section_body_for_aliases};
use super::steps::{marker_value, parse_bracket_list, step_action_segments};
use super::verification::{is_scope_review_action, parse_scope_review_checks};
use super::{PLAN_TRACKER_END, PLAN_TRACKER_START};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn tracker_file_for_plan_file(plan_file: &Path) -> Option<PathBuf> {
    let stem = plan_file.file_stem()?.to_str()?;
    Some(plan_file.with_file_name(format!("{stem}.tasks.md")))
}

pub fn plan_file_for_tracker_file(tracker_file: &Path) -> Option<PathBuf> {
    let file_name = tracker_file.file_name()?.to_str()?;
    let stem = file_name.strip_suffix(".tasks.md")?;
    Some(tracker_file.with_file_name(format!("{stem}.md")))
}

pub(super) fn strip_embedded_tracker(plan_content: &str) -> String {
    let Some(start) = plan_content.find(PLAN_TRACKER_START) else {
        return plan_content.trim().to_string();
    };
    let end = plan_content[start..]
        .find(PLAN_TRACKER_END)
        .map(|offset| start + offset + PLAN_TRACKER_END.len())
        .unwrap_or(plan_content.len());
    let mut merged = String::new();
    merged.push_str(plan_content[..start].trim_end());
    if !merged.is_empty() && !plan_content[end..].trim().is_empty() {
        merged.push_str("\n\n");
    }
    merged.push_str(plan_content[end..].trim_start());
    merged.trim().to_string()
}

pub(in crate::tools::handlers::planning_workflow) fn extract_embedded_tracker(plan_content: &str) -> Option<String> {
    let start = plan_content.find(PLAN_TRACKER_START)?;
    let end = plan_content.find(PLAN_TRACKER_END)?;
    if end <= start {
        return None;
    }
    let content = plan_content[start + PLAN_TRACKER_START.len()..end].trim();
    if content.is_empty() {
        None
    } else {
        Some(content.to_string())
    }
}

pub(in crate::tools::handlers) fn render_plan_with_tracker(
    plan_markdown: &str,
    tracker_markdown: Option<&str>,
) -> String {
    let base_plan = strip_embedded_tracker(plan_markdown);
    let Some(tracker_markdown) = tracker_markdown.map(str::trim).filter(|value| !value.is_empty()) else {
        return format!("{}\n", base_plan.trim_end());
    };
    format!("{}\n\n{}\n{}\n{}\n", base_plan.trim_end(), PLAN_TRACKER_START, tracker_markdown, PLAN_TRACKER_END)
}

/// Merge plan markdown with an optional tracker sidecar into the canonical
/// on-disk representation.
///
/// This deliberately delegates to `render_plan_with_tracker` so the result is
/// identical to what `persist_plan_draft` writes: the plan body with the
/// tracker embedded between `PLAN_TRACKER_START`/`PLAN_TRACKER_END` markers.
/// Previously this module appended the tracker as a bare trailing block, which
/// produced a *different* serialization than `persist_plan_draft` and could
/// double-embed the tracker when the plan file was already persisted.
pub fn merge_plan_content(plan_content: Option<String>, tracker_content: Option<String>) -> Option<String> {
    match (plan_content, tracker_content) {
        (Some(plan), Some(tracker)) => Some(render_plan_with_tracker(&plan, Some(&tracker))),
        (Some(plan), None) => Some(render_plan_with_tracker(&plan, None)),
        (None, Some(tracker)) => Some(render_plan_with_tracker("", Some(&tracker))),
        (None, None) => None,
    }
}

pub(in crate::tools::handlers::planning_workflow) fn tracker_has_progress_or_notes(tracker: &str) -> bool {
    let lower = tracker.to_ascii_lowercase();
    if lower.contains("## notes") {
        return true;
    }
    ["[x]", "[~]", "[!]", "[/]"].iter().any(|marker| lower.contains(marker))
}

pub fn generate_tracker_markdown_from_plan(plan_markdown: &str) -> Option<String> {
    let stripped = strip_embedded_tracker(plan_markdown);
    let implementation = section_body_for_aliases(&stripped, IMPLEMENTATION_SECTION_ALIASES);
    let blocks = implementation.as_deref().map_or_else(
        || collect_implementation_step_blocks(&stripped, true),
        |body| collect_implementation_step_blocks(body, false),
    );
    let title = plan_markdown
        .lines()
        .find_map(|line| line.trim().strip_prefix("# ").map(str::trim))
        .filter(|line| !line.is_empty())
        .unwrap_or("Implementation Plan");

    let mut items = Vec::new();
    let mut seen_descriptions = HashSet::new();
    for block in &blocks {
        let Some(description) = block.lines.first() else {
            continue;
        };
        // Share marker boundaries with validation so prose arrows remain
        // part of the action and verification text in the tracker.
        let segments = step_action_segments(description);
        let main = segments.first().map(String::as_str).unwrap_or_default();
        if main.is_empty() {
            continue;
        }
        let description_key = vtcode_commons::formatting::collapse_whitespace(main).to_ascii_lowercase();
        if !seen_descriptions.insert(description_key) {
            continue;
        }

        let mut entry = format!("- [ ] {main}\n");
        let verification_label = if is_scope_review_action(main) {
            "scope-review"
        } else {
            "verify"
        };
        for segment in segments.iter().skip(1).chain(block.lines.iter().skip(1)) {
            // Reuse the validator's emphasis-tolerant marker parsing so an
            // accepted `**files:**`-style segment yields the same tracker
            // metadata the validator saw.
            if let Some(files) = marker_value(segment, &["files"]) {
                let values = parse_bracket_list(files);
                if !values.is_empty() {
                    entry.push_str(&format!("  files: {}\n", values.join(", ")));
                }
                continue;
            }
            if let Some(outcome) = marker_value(segment, &["outcome"]) {
                let outcome = outcome.trim().trim_start_matches('[').trim_end_matches(']');
                if !outcome.is_empty() {
                    entry.push_str(&format!("  outcome: {outcome}\n"));
                }
                continue;
            }
            if let Some(verify) = marker_value(segment, &["verify", "verification"]) {
                let values = if is_scope_review_action(main) {
                    parse_scope_review_checks(verify)
                } else {
                    parse_bracket_list(verify)
                };
                if values.is_empty() {
                    let trimmed = verify.trim();
                    if !trimmed.is_empty() {
                        entry.push_str(&format!("  {verification_label}: {trimmed}\n"));
                    }
                } else {
                    for value in values {
                        entry.push_str(&format!("  {verification_label}: {value}\n"));
                    }
                }
            }
        }
        items.push(entry);
    }

    if items.is_empty() {
        return None;
    }

    Some(format!("# {}\n\n## Plan of Work\n\n{}", title, items.concat().trim_end()))
}
