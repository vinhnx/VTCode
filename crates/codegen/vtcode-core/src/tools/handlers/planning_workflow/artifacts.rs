//! Pure planning-artifact logic: plan/tracker marker handling, section
//! parsing, validation, and tracker generation.
//!
//! Everything here is side-effect-free and depends only on `std`/`serde`, so it
//! is independently testable (see `super::tests`). I/O and tool wiring live in
//! `persistence.rs` / `start.rs` / `finish.rs`.

use std::path::{Path, PathBuf};

pub(super) const PLAN_TRACKER_START: &str = "<!-- vtcode:plan-tracker:start -->";
pub(super) const PLAN_TRACKER_END: &str = "<!-- vtcode:plan-tracker:end -->";

pub(super) const REQUIRED_PLAN_SECTIONS: [&str; 4] = [
    "Summary",
    "Implementation Steps",
    "Test Cases and Validation",
    "Assumptions and Defaults",
];

pub(super) const PLACEHOLDER_TOKENS: [&str; 14] = [
    "[step]",
    "[paths]",
    "[check]",
    "[explicit assumption]",
    "[default chosen when user did not specify]",
    "[out-of-scope items intentionally not changed]",
    "[file, symbol, or behavior confirmed from the repo]",
    "[existing pattern or constraint verified before planning]",
    "[if any], otherwise: no remaining scope decisions",
    "[project build and lint command",
    "[project test command",
    "[2-4 lines: goal, user impact, what will change, what will not]",
    "[explicit commands/manual checks]",
    "[what must not break]",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanValidationReport {
    pub missing_sections: Vec<String>,
    pub placeholder_tokens: Vec<String>,
    pub open_decisions: Vec<String>,
    pub implementation_step_count: usize,
    pub validation_item_count: usize,
    pub assumption_count: usize,
    pub summary_present: bool,
}

impl PlanValidationReport {
    pub fn is_ready(&self) -> bool {
        self.missing_sections.is_empty()
            && self.placeholder_tokens.is_empty()
            && self.open_decisions.is_empty()
            && self.summary_present
            && self.implementation_step_count > 0
            && self.validation_item_count > 0
            && self.assumption_count > 0
    }
}

pub fn tracker_file_for_plan_file(plan_file: &Path) -> Option<PathBuf> {
    let stem = plan_file.file_stem()?.to_str()?;
    Some(plan_file.with_file_name(format!("{stem}.tasks.md")))
}

pub fn plan_file_for_tracker_file(tracker_file: &Path) -> Option<PathBuf> {
    let file_name = tracker_file.file_name()?.to_str()?;
    let stem = file_name.strip_suffix(".tasks.md")?;
    Some(tracker_file.with_file_name(format!("{stem}.md")))
}

fn strip_embedded_tracker(plan_content: &str) -> String {
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

pub(super) fn extract_embedded_tracker(plan_content: &str) -> Option<String> {
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

pub(super) fn render_plan_with_tracker(
    plan_markdown: &str,
    tracker_markdown: Option<&str>,
) -> String {
    let base_plan = strip_embedded_tracker(plan_markdown);
    let Some(tracker_markdown) = tracker_markdown.map(str::trim).filter(|value| !value.is_empty())
    else {
        return format!("{}\n", base_plan.trim_end());
    };
    format!(
        "{}\n\n{}\n{}\n{}\n",
        base_plan.trim_end(),
        PLAN_TRACKER_START,
        tracker_markdown,
        PLAN_TRACKER_END
    )
}

/// Merge plan markdown with an optional tracker sidecar into the canonical
/// on-disk representation.
///
/// This deliberately delegates to [`render_plan_with_tracker`] so the result is
/// identical to what `persist_plan_draft` writes: the plan body with the
/// tracker embedded between `PLAN_TRACKER_START`/`PLAN_TRACKER_END` markers.
/// Previously this module appended the tracker as a bare trailing block, which
/// produced a *different* serialization than `persist_plan_draft` and could
/// double-embed the tracker when the plan file was already persisted.
pub fn merge_plan_content(
    plan_content: Option<String>,
    tracker_content: Option<String>,
) -> Option<String> {
    match (plan_content, tracker_content) {
        (Some(plan), Some(tracker)) => Some(render_plan_with_tracker(&plan, Some(&tracker))),
        (Some(plan), None) => Some(render_plan_with_tracker(&plan, None)),
        (None, Some(tracker)) => Some(render_plan_with_tracker("", Some(&tracker))),
        (None, None) => None,
    }
}

fn section_body(content: &str, header: &str) -> Option<String> {
    let mut capture = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(found) = trimmed.strip_prefix("## ") {
            if capture {
                break;
            }
            capture = found.trim().eq_ignore_ascii_case(header);
            continue;
        }
        if capture {
            lines.push(line.to_string());
        }
    }
    let body = lines.join("\n").trim().to_string();
    (!body.is_empty()).then_some(body)
}

fn meaningful_section_lines(body: &str) -> Vec<&str> {
    body.lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with('>')
                && !line.starts_with("<!--")
                && *line != PLAN_TRACKER_START
                && *line != PLAN_TRACKER_END
        })
        .collect()
}

fn is_numbered_line(line: &str) -> bool {
    let mut seen_digit = false;
    for ch in line.chars() {
        if ch.is_ascii_digit() {
            seen_digit = true;
            continue;
        }
        return seen_digit && (ch == '.' || ch == ')');
    }
    false
}

fn find_placeholder_tokens(content: &str) -> Vec<String> {
    let lower = content.to_ascii_lowercase();
    PLACEHOLDER_TOKENS
        .iter()
        .filter(|token| lower.contains(**token))
        .map(|token| token.to_string())
        .collect()
}

fn find_open_decisions(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("next open decision")
                && ![
                    "none",
                    "no remaining",
                    "no further",
                    "resolved",
                    "closed",
                    "n/a",
                    "not applicable",
                ]
                .iter()
                .any(|needle| lower.contains(needle))
        })
        .map(ToString::to_string)
        .collect()
}

pub fn validate_plan_content(content: &str) -> PlanValidationReport {
    let stripped = strip_embedded_tracker(content);
    let mut report = PlanValidationReport {
        placeholder_tokens: find_placeholder_tokens(&stripped),
        open_decisions: find_open_decisions(&stripped),
        ..PlanValidationReport::default()
    };

    let summary_body = section_body(&stripped, "Summary");
    let implementation_body = section_body(&stripped, "Implementation Steps");
    let validation_body = section_body(&stripped, "Test Cases and Validation");
    let assumptions_body = section_body(&stripped, "Assumptions and Defaults");

    for section in REQUIRED_PLAN_SECTIONS {
        if section_body(&stripped, section).is_none() {
            report.missing_sections.push(section.to_string());
        }
    }

    if let Some(body) = summary_body.as_deref() {
        report.summary_present = !meaningful_section_lines(body).is_empty();
    }
    if !report.summary_present && !report.missing_sections.iter().any(|s| s == "Summary") {
        report.missing_sections.push("Summary".to_string());
    }

    if let Some(body) = implementation_body.as_deref() {
        report.implementation_step_count = meaningful_section_lines(body)
            .into_iter()
            .filter(|line| is_numbered_line(line))
            .count();
    }
    if report.implementation_step_count == 0
        && !report.missing_sections.iter().any(|s| s == "Implementation Steps")
    {
        report.missing_sections.push("Implementation Steps".to_string());
    }

    if let Some(body) = validation_body.as_deref() {
        report.validation_item_count = meaningful_section_lines(body)
            .into_iter()
            .filter(|line| is_numbered_line(line) || line.starts_with("- "))
            .count();
    }
    if report.validation_item_count == 0
        && !report.missing_sections.iter().any(|s| s == "Test Cases and Validation")
    {
        report.missing_sections.push("Test Cases and Validation".to_string());
    }

    if let Some(body) = assumptions_body.as_deref() {
        report.assumption_count = meaningful_section_lines(body)
            .into_iter()
            .filter(|line| is_numbered_line(line) || line.starts_with("- "))
            .count();
    }
    if report.assumption_count == 0
        && !report.missing_sections.iter().any(|s| s == "Assumptions and Defaults")
    {
        report.missing_sections.push("Assumptions and Defaults".to_string());
    }

    report
}

fn parse_bracket_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim().trim_start_matches('[').trim_end_matches(']');
    trimmed
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub(super) fn tracker_has_progress_or_notes(tracker: &str) -> bool {
    let lower = tracker.to_ascii_lowercase();
    if lower.contains("## notes") {
        return true;
    }
    ["[x]", "[~]", "[!]", "[/]"].iter().any(|marker| lower.contains(marker))
}

pub fn generate_tracker_markdown_from_plan(plan_markdown: &str) -> Option<String> {
    let implementation = section_body(plan_markdown, "Implementation Steps")?;
    let title = plan_markdown
        .lines()
        .find_map(|line| line.trim().strip_prefix("# ").map(str::trim))
        .filter(|line| !line.is_empty())
        .unwrap_or("Implementation Plan");

    let mut items = Vec::new();
    for line in implementation.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if !is_numbered_line(line) {
            continue;
        }
        let description = line.split_once(['.', ')']).map(|(_, rest)| rest.trim()).unwrap_or(line);
        let segments = description.split("->").map(str::trim).collect::<Vec<_>>();
        let main = segments.first().copied().unwrap_or_default();
        if main.is_empty() {
            continue;
        }

        let mut entry = format!("- [ ] {main}\n");
        for segment in segments.iter().skip(1) {
            if let Some(files) = segment.strip_prefix("files:") {
                let values = parse_bracket_list(files);
                if !values.is_empty() {
                    entry.push_str(&format!("  files: {}\n", values.join(", ")));
                }
                continue;
            }
            if let Some(outcome) = segment.strip_prefix("outcome:") {
                let outcome = outcome.trim().trim_start_matches('[').trim_end_matches(']');
                if !outcome.is_empty() {
                    entry.push_str(&format!("  outcome: {outcome}\n"));
                }
                continue;
            }
            if let Some(verify) = segment.strip_prefix("verify:") {
                let values = parse_bracket_list(verify);
                if values.is_empty() {
                    let trimmed = verify.trim();
                    if !trimmed.is_empty() {
                        entry.push_str(&format!("  verify: {trimmed}\n"));
                    }
                } else {
                    for value in values {
                        entry.push_str(&format!("  verify: {value}\n"));
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
