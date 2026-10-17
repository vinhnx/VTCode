//! Plan shape validation and validator-owned repair feedback.

use super::sections::{
    ASSUMPTIONS_SECTION_ALIASES, IMPLEMENTATION_SECTION_ALIASES, ImplementationStepBlock, SUMMARY_SECTION_ALIASES,
    VALIDATION_SECTION_ALIASES, collect_implementation_step_blocks, find_placeholder_tokens, is_numbered_line,
    labeled_body_for_aliases, meaningful_section_lines, section_body_for_aliases,
};
use super::steps::{PLAN_TARGET_LABELS, marker_value, step_action_segments};
use super::tracker::strip_embedded_tracker;
use super::verification::{
    VerificationValidationError, is_concrete_target, is_markdown_target, is_optional_markdown_verification,
    validate_concrete_verification,
};

/// The canonical one-line step format that produces reliable validation and
/// tracker generation. All repair directives reference this so the model gets
/// the same contract from every prompt surface.
pub const CANONICAL_STEP_FORMAT: &str = "1. Action -> files: [path/to/file.rs] -> verify: [cargo check]";

/// Shared valid `verify:` examples for planning synthesis/repair prompts.
/// `repair_feedback()` embeds this list; binary runloop constants stay
/// compile-time literals but must keep these examples via presence tests.
pub const PLANNING_VERIFY_VALID_EXAMPLES: &str = "`verify: [cargo nextest run -p vtcode]`, `verify: [cargo check --locked]`, `verify: [rg -n 'symbol' src/file.rs]`, `verify: [sed -n '1,40p' docs/file.md]`, `verify: [grep -n 'symbol' src/file.rs]`, `verify: [git show --stat HEAD]`";

/// Shared invalid `verify:` examples. Kept adjacent to
/// [`PLANNING_VERIFY_VALID_EXAMPLES`] so every prompt surface pairs them.
pub const PLANNING_VERIFY_INVALID_EXAMPLES: &str =
    "`verify: [run checks]`, `verify: [check later]`, `verify: [git diff --check]`";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanValidationReport {
    pub missing_sections: Vec<String>,
    pub placeholder_tokens: Vec<String>,
    pub open_decisions: Vec<String>,
    pub invalid_implementation_steps: Vec<String>,
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
            && self.invalid_implementation_steps.is_empty()
            && self.summary_present
            && self.implementation_step_count > 0
            && self.validation_item_count > 0
            && self.assumption_count > 0
    }

    pub fn reasons(&self) -> Vec<String> {
        let mut reasons = Vec::new();
        if !self.missing_sections.is_empty() {
            reasons.push(format!("missing sections: {}", self.missing_sections.join(", ")));
        }
        if !self.placeholder_tokens.is_empty() {
            reasons.push(format!("placeholder tokens: {}", self.placeholder_tokens.join(", ")));
        }
        if !self.open_decisions.is_empty() {
            reasons.push(format!("unresolved decisions: {}", self.open_decisions.join("; ")));
        }
        if !self.invalid_implementation_steps.is_empty() {
            reasons.push(format!("invalid implementation steps: {}", self.invalid_implementation_steps.join("; ")));
        }
        if !self.summary_present {
            reasons.push("summary is empty".to_string());
        }
        if self.implementation_step_count == 0 {
            reasons.push("no implementation steps".to_string());
        }
        if self.validation_item_count == 0 {
            reasons.push("no validation items".to_string());
        }
        if self.assumption_count == 0 {
            reasons.push("no assumptions or defaults".to_string());
        }
        reasons
    }

    /// Produce bounded, validator-owned diagnostics for a repair directive.
    ///
    /// Unlike `reasons()`, which joins raw `open_decisions` lines (user/model
    /// controlled text), this method emits only validator-owned category
    /// summaries and step counts. It always includes the canonical step format
    /// so the model knows the exact contract to follow. This is safe to inject
    /// into a system message because every string here is validator-authored.
    pub fn repair_feedback(&self) -> String {
        let mut feedback = Vec::new();

        if !self.missing_sections.is_empty() {
            feedback.push(format!("missing required section(s): {}", self.missing_sections.join(", ")));
        }
        if !self.placeholder_tokens.is_empty() {
            feedback.push(format!("contains {} unresolved placeholder token(s)", self.placeholder_tokens.len()));
        }
        if !self.open_decisions.is_empty() {
            feedback.push(format!("contains {} unresolved decision marker(s)", self.open_decisions.len()));
        }
        if !self.invalid_implementation_steps.is_empty() {
            // Step numbers are parsed digits and reason strings are
            // validator-owned, so this is safe to echo.
            feedback.push(format!(
                "{} of {} implementation step(s) lack a concrete target or verification: {}",
                self.invalid_implementation_steps.len(),
                self.implementation_step_count,
                self.invalid_implementation_steps.join("; ")
            ));
        }
        if !self.summary_present {
            feedback.push("summary is empty".to_string());
        }
        if self.implementation_step_count == 0 {
            feedback.push("no implementation steps".to_string());
        }
        if self.validation_item_count == 0 {
            feedback.push("no validation items".to_string());
        }
        if self.assumption_count == 0 {
            feedback.push("no assumptions or defaults".to_string());
        }

        let mut result = if feedback.is_empty() {
            "The plan has validation issues".to_string()
        } else {
            format!("Plan validation issues: {}", feedback.join("; "))
        };
        result.push_str("\n\nRewrite every implementation step in this canonical one-line form:\n");
        result.push_str(CANONICAL_STEP_FORMAT);
        result.push_str(&format!(
            "\nEach step must name a concrete file path or symbol (not prose) and one concrete verify command or observable check. \
             List separate verification commands as comma-separated entries, not a semicolon chain; each must be a command or an observable check. Commas inside single or double quotes stay inside one item. \
             Valid examples: {PLANNING_VERIFY_VALID_EXAMPLES}. \
             Invalid examples: {PLANNING_VERIFY_INVALID_EXAMPLES}; vague prose and generic VCS-only checks do not satisfy this validator. \
             Command heads that are common English words (`file`, `sort`, `find`, `ls`, `wc`, …) also need a flag or path-like argument.",
        ));
        result
    }
}

fn implementation_step_shape_error(step: &ImplementationStepBlock) -> Option<String> {
    let first_line = step.lines.first().map(String::as_str).unwrap_or_default();
    let action = first_line.trim();
    if action.is_empty() {
        return Some("action is empty".to_string());
    }

    let segments = step_action_segments(action);
    let verify_index = segments
        .iter()
        .position(|segment| marker_value(segment, &["verify", "verification"]).is_some());
    let mut has_target = false;
    let mut invalid_target = false;
    let mut markdown_targets_only = true;

    if let Some(index) = verify_index {
        if index < 2 {
            return Some("must include a concrete target before the verification marker".to_string());
        }
        for target in segments.iter().skip(1).take(index.saturating_sub(1)) {
            if marker_value(target, &["outcome"]).is_some() {
                continue;
            }
            has_target = true;
            invalid_target |= !is_concrete_target(target);
            markdown_targets_only &= is_markdown_target(target);
        }
    } else if segments.len() > 1 {
        for target in segments.iter().skip(1) {
            if marker_value(target, &["outcome"]).is_some() {
                continue;
            }
            has_target = true;
            invalid_target |= !is_concrete_target(target);
            markdown_targets_only &= is_markdown_target(target);
        }
    }

    let mut has_verification = verify_index.is_some();
    let mut has_regular_verification = verify_index
        .and_then(|index| marker_value(&segments[index], &["verify", "verification"]))
        .is_some_and(|verify| !is_optional_markdown_verification(verify));
    let mut verification_error = verify_index
        .and_then(|index| marker_value(&segments[index], &["verify", "verification"]))
        .and_then(|verify| validate_concrete_verification(verify).err());
    let trailing_inline_fields = segments.iter().skip(verify_index.map_or(segments.len(), |index| index + 1));
    for continuation in trailing_inline_fields.chain(step.lines.iter().skip(1)) {
        if let Some(target) = marker_value(continuation, PLAN_TARGET_LABELS) {
            has_target = true;
            invalid_target |= !is_concrete_target(target);
            markdown_targets_only &= is_markdown_target(target);
        }
        if let Some(verify) = marker_value(continuation, &["verify", "verification"]) {
            has_verification = true;
            has_regular_verification |= !is_optional_markdown_verification(verify);
            if verification_error.is_none() {
                verification_error = validate_concrete_verification(verify).err();
            }
        }
    }

    if !has_target || invalid_target {
        return Some("must name a concrete file, symbol, or behavior target".to_string());
    }
    if !has_verification {
        return Some("must include a `verify:` or `verification:` marker".to_string());
    }
    if let Some(error) = verification_error {
        return Some(match error {
            VerificationValidationError::NotConcrete => {
                "verification marker must include a concrete command or check".to_string()
            }
            VerificationValidationError::InvalidItem { ordinal } => {
                format!("verification item {ordinal} must be a concrete command or check")
            }
        });
    }
    if !markdown_targets_only && !has_regular_verification {
        return Some(
            "non-Markdown targets require a concrete command or observable check beyond skipped Markdown lint"
                .to_string(),
        );
    }
    None
}

fn find_open_decisions(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            (lower.contains("next open decision") || lower.contains("open question"))
                && ![
                    "none",
                    "no open",
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

    let summary_body = section_body_for_aliases(&stripped, SUMMARY_SECTION_ALIASES)
        .or_else(|| labeled_body_for_aliases(&stripped, SUMMARY_SECTION_ALIASES));
    let implementation_section_body = section_body_for_aliases(&stripped, IMPLEMENTATION_SECTION_ALIASES);
    let implementation_labeled_body = labeled_body_for_aliases(&stripped, IMPLEMENTATION_SECTION_ALIASES);
    let implementation_blocks = if let Some(body) = implementation_section_body.as_deref() {
        collect_implementation_step_blocks(body, false)
    } else if implementation_labeled_body.is_some() {
        collect_implementation_step_blocks(implementation_labeled_body.as_deref().unwrap_or_default(), false)
    } else {
        // Older compact plans omit a Steps heading and put the numbered list
        // between labeled Summary/Validation/Assumptions lines. Keep that
        // compatibility, but stop collecting when the next labeled section
        // begins so validation bullets cannot masquerade as step details.
        collect_implementation_step_blocks(&stripped, true)
    };
    let validation_body = section_body_for_aliases(&stripped, VALIDATION_SECTION_ALIASES)
        .or_else(|| labeled_body_for_aliases(&stripped, VALIDATION_SECTION_ALIASES));
    let assumptions_body = section_body_for_aliases(&stripped, ASSUMPTIONS_SECTION_ALIASES)
        .or_else(|| labeled_body_for_aliases(&stripped, ASSUMPTIONS_SECTION_ALIASES));

    for (section, body) in [
        ("Summary", summary_body.as_ref()),
        ("Implementation Steps", (!implementation_blocks.is_empty()).then_some(&stripped)),
        ("Test Cases and Validation", validation_body.as_ref()),
        ("Assumptions and Defaults", assumptions_body.as_ref()),
    ] {
        if body.is_none() {
            report.missing_sections.push(section.to_string());
        }
    }

    if let Some(body) = summary_body.as_deref() {
        report.summary_present = !meaningful_section_lines(body).is_empty();
    }
    if !report.summary_present && !report.missing_sections.iter().any(|s| s == "Summary") {
        report.missing_sections.push("Summary".to_string());
    }

    report.implementation_step_count = implementation_blocks.len();
    report.invalid_implementation_steps = implementation_blocks
        .iter()
        .filter_map(|step| {
            implementation_step_shape_error(step).map(|reason| format!("step {}: {reason}", step.number))
        })
        .collect();
    if report.implementation_step_count == 0 && !report.missing_sections.iter().any(|s| s == "Implementation Steps") {
        report.missing_sections.push("Implementation Steps".to_string());
    }

    if let Some(body) = validation_body.as_deref() {
        let lines = meaningful_section_lines(body);
        report.validation_item_count = lines
            .iter()
            .filter(|line| is_numbered_line(line) || line.starts_with("- "))
            .count();
        if report.validation_item_count == 0 {
            report.validation_item_count = lines.len();
        }
    }
    if report.validation_item_count == 0 && !report.missing_sections.iter().any(|s| s == "Test Cases and Validation") {
        report.missing_sections.push("Test Cases and Validation".to_string());
    }

    if let Some(body) = assumptions_body.as_deref() {
        let lines = meaningful_section_lines(body);
        report.assumption_count = lines
            .iter()
            .filter(|line| is_numbered_line(line) || line.starts_with("- "))
            .count();
        if report.assumption_count == 0 {
            report.assumption_count = lines.len();
        }
    }
    if report.assumption_count == 0 && !report.missing_sections.iter().any(|s| s == "Assumptions and Defaults") {
        report.missing_sections.push("Assumptions and Defaults".to_string());
    }

    report
}
