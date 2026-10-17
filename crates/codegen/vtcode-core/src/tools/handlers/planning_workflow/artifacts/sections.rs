//! Plan section discovery and numbered implementation-step blocks.

use super::{PLAN_TRACKER_END, PLAN_TRACKER_START};

pub(super) const SUMMARY_SECTION_ALIASES: &[&str] = &["Summary"];

pub(super) const IMPLEMENTATION_SECTION_ALIASES: &[&str] = &["Implementation Steps", "Steps"];

pub(super) const VALIDATION_SECTION_ALIASES: &[&str] = &["Test Cases and Validation", "Validation"];

pub(super) const ASSUMPTIONS_SECTION_ALIASES: &[&str] = &["Assumptions and Defaults", "Assumptions"];

fn section_body(content: &str, header: &str) -> Option<String> {
    let mut capture = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if capture && is_plan_section_boundary(trimmed) {
            break;
        }
        if let Some(found) = trimmed.strip_prefix("## ") {
            if capture {
                break;
            }
            capture = strip_emphasis(found.trim().trim_end_matches(':'))
                .trim()
                .eq_ignore_ascii_case(header);
            continue;
        }
        if capture {
            lines.push(line.to_string());
        }
    }
    let body = lines.join("\n").trim().to_string();
    (!body.is_empty()).then_some(body)
}

pub(super) fn section_body_for_aliases(content: &str, headers: &[&str]) -> Option<String> {
    headers
        .iter()
        .find_map(|header| section_body(content, header).or_else(|| standalone_section_body(content, header)))
}

/// Strip Markdown emphasis markers (`**bold**`, `` `code` ``) that models
/// frequently wrap around plan labels such as `**Files/symbols:**`. Emphasis
/// carries no semantics for validation; leaving it in place makes label
/// prefix matching reject well-formed plans (checkpoint turn_912).
fn strip_emphasis(value: &str) -> &str {
    value.trim_matches(['*', '`'])
}

/// Leading-edge variant for label prefixes: `**Files/symbols:** value`.
pub(super) fn strip_leading_emphasis(value: &str) -> &str {
    value.trim_start_matches(['*', '`'])
}

fn normalized_section_label(line: &str) -> &str {
    let mut normalized = line.trim().trim_start_matches('>').trim_start();
    while let Some(stripped) = normalized.strip_prefix('#') {
        normalized = stripped.trim_start();
    }
    strip_emphasis(strip_list_marker(normalized).trim()).trim()
}

fn is_standalone_section_label(line: &str, header: &str) -> bool {
    let normalized = normalized_section_label(line);
    normalized.eq_ignore_ascii_case(header)
}

fn standalone_section_body(content: &str, header: &str) -> Option<String> {
    let mut capture = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if is_standalone_section_label(trimmed, header) {
            if capture {
                break;
            }
            capture = true;
            continue;
        }
        if capture && is_plan_section_boundary(trimmed) {
            break;
        }
        if capture {
            lines.push(line.to_string());
        }
    }
    let body = lines.join("\n").trim().to_string();
    (!body.is_empty()).then_some(body)
}

pub(super) fn strip_ascii_case_insensitive_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let prefix_end = prefix.len();
    value
        .get(..prefix_end)
        .filter(|candidate| candidate.eq_ignore_ascii_case(prefix))
        .and_then(|_| value.get(prefix_end..).map(str::trim_start))
}

pub(super) fn labeled_body_for_aliases(content: &str, labels: &[&str]) -> Option<String> {
    let lines = content
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            labels
                .iter()
                .find_map(|label| strip_ascii_case_insensitive_prefix(line, &format!("{label}:")))
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

pub(super) fn meaningful_section_lines(body: &str) -> Vec<&str> {
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

pub(super) fn numbered_line_parts(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    // Tolerate a `Step ` prefix (`Step 1: ...`), a common model variant. The
    // digit check requires a leading digit after whitespace trimming so words
    // like `stepwise` are never mistaken for a step prefix.
    let trimmed = strip_ascii_case_insensitive_prefix(trimmed, "step")
        .map(str::trim_start)
        .filter(|rest| rest.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
        .unwrap_or(trimmed);
    let digit_end = trimmed
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_digit())
        .last()
        .map_or(0, |(index, ch)| index + ch.len_utf8());
    if digit_end == 0 {
        return None;
    }

    let rest = trimmed.get(digit_end..)?.trim_start();
    let punctuation = rest.chars().next()?;
    if punctuation != '.' && punctuation != ')' && punctuation != ':' {
        return None;
    }

    Some((trimmed.get(..digit_end)?, rest.get(punctuation.len_utf8()..)?.trim_start()))
}

pub(super) fn is_numbered_line(line: &str) -> bool {
    numbered_line_parts(line).is_some()
}

#[derive(Debug, Clone)]
pub(super) struct ImplementationStepBlock {
    pub(super) number: String,
    pub(super) lines: Vec<String>,
}

pub(super) fn strip_list_marker(line: &str) -> &str {
    let mut current = line.trim();
    loop {
        let Some(stripped) = current
            .strip_prefix("- ")
            .or_else(|| current.strip_prefix("* "))
            .or_else(|| current.strip_prefix("• "))
        else {
            return current;
        };
        current = stripped.trim_start();
    }
}

fn is_plan_section_boundary(line: &str) -> bool {
    let mut normalized = line.trim();
    while let Some(stripped) = normalized.strip_prefix('#') {
        normalized = stripped.trim_start();
    }
    normalized = strip_emphasis(strip_list_marker(normalized).trim()).trim();
    SUMMARY_SECTION_ALIASES
        .iter()
        .chain(IMPLEMENTATION_SECTION_ALIASES.iter())
        .chain(VALIDATION_SECTION_ALIASES.iter())
        .chain(ASSUMPTIONS_SECTION_ALIASES.iter())
        .any(|alias| {
            normalized.eq_ignore_ascii_case(alias)
                || strip_ascii_case_insensitive_prefix(normalized, &format!("{alias}:")).is_some()
        })
}

pub(super) fn collect_implementation_step_blocks(
    content: &str,
    stop_at_section_boundaries: bool,
) -> Vec<ImplementationStepBlock> {
    let mut blocks = Vec::new();
    let mut current: Option<ImplementationStepBlock> = None;
    let mut collecting = true;
    let mut started = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('>')
            || trimmed.starts_with("<!--")
            || trimmed == PLAN_TRACKER_START
            || trimmed == PLAN_TRACKER_END
        {
            continue;
        }

        if let Some((number, step)) = numbered_line_parts(trimmed) {
            if !collecting {
                continue;
            }
            if let Some(previous) = current.take() {
                blocks.push(previous);
            }
            started = true;
            current = Some(ImplementationStepBlock {
                number: number.to_string(),
                lines: vec![step.to_string()],
            });
            continue;
        }

        if stop_at_section_boundaries && is_plan_section_boundary(trimmed) {
            if started {
                if let Some(previous) = current.take() {
                    blocks.push(previous);
                }
                collecting = false;
            }
            continue;
        }

        if collecting
            && started
            && let Some(step) = current.as_mut()
        {
            step.lines.push(trimmed.to_string());
        }
    }

    if let Some(last) = current {
        blocks.push(last);
    }
    blocks
}

const PLACEHOLDER_TOKENS: [&str; 21] = [
    "[step]",
    "[paths]",
    "[check]",
    "[explicit assumption]",
    "[default chosen when user did not specify]",
    "[out-of-scope items intentionally not changed]",
    "[file, symbol, or behavior confirmed from the repo]",
    "[observed command output -> the insight it establishes]",
    "[existing pattern or constraint verified before planning]",
    "[if any], otherwise: no remaining scope decisions",
    "[project build and lint command",
    "[project test command",
    "[2-4 lines: goal, user impact, what will change, what will not]",
    "[explicit commands/manual checks]",
    "[what must not break]",
    "[observable end state the implementation must produce]",
    "[required tooling, configuration, or prior work]",
    "[todo]",
    "todo:",
    "[decision needed]",
    "tbd",
];

pub(super) fn find_placeholder_tokens(content: &str) -> Vec<String> {
    let lower = content.to_ascii_lowercase();
    PLACEHOLDER_TOKENS
        .iter()
        .filter(|token| lower.contains(**token))
        .map(|token| token.to_string())
        .collect()
}
