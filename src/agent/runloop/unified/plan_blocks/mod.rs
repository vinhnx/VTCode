const OPEN_TAG: &str = "<proposed_plan>";
const CLOSE_TAG: &str = "</proposed_plan>";
const ALT_OPEN_TAG: &str = "<plan>";
const ALT_CLOSE_TAG: &str = "</plan>";

/// Detect whether a planning-mode text response is a clarifying question
/// posed to the user rather than a plan or research prose. The deterministic
/// interview-denial recovery must NOT force plan synthesis when the model is
/// legitimately asking the user a question in plain text (the text-mode
/// equivalent of the unavailable `request_user_input` modal). Without this
/// check, the retry directive suppresses the question and the agent proceeds
/// to propose a plan without waiting for the user's answer (checkpoint
/// turn_856).
///
/// Heuristic: the last non-empty line ends with `?`. This is a strong signal
/// that the model is asking a question, and it does not match completed plans
/// (which end with Assumptions/Validation prose) or research dumps.
pub(crate) fn looks_like_clarifying_question(text: &str) -> bool {
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .is_some_and(|last_line| last_line.trim().ends_with('?'))
}

/// Detect whether a planning-mode text response is an attempted plan emitted
/// without the required `<proposed_plan>` tags (checkpoint turn_1075: a
/// `## Simple plan` markdown answer with seven numbered steps but no tags,
/// no `files:` targets, and no `verify:` checks). Without this check the
/// turn ends with only the generic "no approval-ready plan" hint and the
/// model never learns the canonical step contract, so it repeats the same
/// tagless shape every turn.
///
/// Heuristic (conservative by design): the text has a markdown heading naming
/// an implementation section (`## Implementation Steps`, or `## Goal`
/// together with `## Plan`) or contains at least two numbered-step lines
/// (`1. ...`, `1) ...`). Single numbered lines in research prose do not
/// trigger; clarifying questions are excluded by the caller, not here, so
/// this stays a pure shape check.
pub(crate) fn looks_like_attempted_plan(text: &str) -> bool {
    fn heading_rest(line: &str) -> Option<&str> {
        let trimmed = line.trim().trim_start_matches('>').trim_start();
        let without_hashes = trimmed.trim_start_matches('#');
        if without_hashes.len() == trimmed.len() {
            return None;
        }
        Some(without_hashes.trim_start().trim_start_matches(['*', '_', '`']).trim_start())
    }

    fn is_heading_name(line: &str, name: &str) -> bool {
        let Some(rest) = heading_rest(line) else {
            return false;
        };
        if !rest.get(..name.len()).is_some_and(|prefix| prefix.eq_ignore_ascii_case(name)) {
            return false;
        }
        // Word boundary: `## Planning` must not match a `## Plan` heading,
        // and `## Goals` must not match `## Goal`.
        rest[name.len()..]
            .chars()
            .next()
            .is_none_or(|next| !next.is_ascii_alphanumeric())
    }

    let mut has_implementation_heading = false;
    let mut has_plan_heading = false;
    let mut has_goal_heading = false;
    for line in text.lines() {
        if !has_implementation_heading && is_heading_name(line, "implementation steps") {
            has_implementation_heading = true;
        }
        if !has_plan_heading && is_heading_name(line, "plan") {
            has_plan_heading = true;
        }
        if !has_goal_heading && is_heading_name(line, "goal") {
            has_goal_heading = true;
        }
        if has_implementation_heading || (has_plan_heading && has_goal_heading) {
            break;
        }
    }
    if has_implementation_heading {
        return true;
    }
    if has_plan_heading && has_goal_heading {
        return true;
    }

    let mut numbered_steps = 0usize;
    for line in text.lines() {
        let trimmed = line.trim().trim_start_matches('>').trim_start();
        // Tolerate a `Step ` prefix (`Step 1: ...`), mirroring
        // `numbered_line_parts` in planning_workflow/artifacts.rs: only strip
        // when a digit follows so `stepwise` is never mistaken for a step.
        let trimmed = trimmed
            .get(..4)
            .filter(|prefix| prefix.eq_ignore_ascii_case("step"))
            .map(|_| trimmed[4..].trim_start())
            .filter(|rest| rest.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
            .unwrap_or(trimmed);
        let mut digits_len = 0usize;
        for ch in trimmed.chars() {
            if ch.is_ascii_digit() {
                digits_len += ch.len_utf8();
            } else {
                break;
            }
        }
        if digits_len == 0 {
            continue;
        }
        let rest = trimmed[digits_len..].trim_start();
        let mut chars = rest.chars();
        // Accept the same step punctuation as the artifact validator
        // (`numbered_line_parts` in planning_workflow/artifacts.rs).
        match chars.next() {
            Some('.') | Some(')') | Some(':') if chars.next().is_some_and(|next| next.is_whitespace()) => {
                numbered_steps += 1;
                if numbered_steps >= 2 {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProposedPlanExtraction {
    pub stripped_text: String,
    pub plan_text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseMode {
    Normal,
    InPlan { close_tag: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanTag {
    Proposed,
    Alternate,
}

impl PlanTag {
    fn open_tag(self) -> &'static str {
        match self {
            Self::Proposed => OPEN_TAG,
            Self::Alternate => ALT_OPEN_TAG,
        }
    }

    fn close_tag(self) -> &'static str {
        match self {
            Self::Proposed => CLOSE_TAG,
            Self::Alternate => ALT_CLOSE_TAG,
        }
    }
}

/// Streaming parser that removes either supported plan block form from
/// assistant-visible text while collecting the plan body.
#[derive(Debug, Default)]
pub(crate) struct ProposedPlanStreamParser {
    mode: Option<ParseMode>,
    pending: String,
    policy_pending: String,
    plan_buffer: String,
    saw_plan_block: bool,
}

impl ProposedPlanStreamParser {
    pub(crate) fn new() -> Self {
        Self {
            mode: Some(ParseMode::Normal),
            pending: String::new(),
            policy_pending: String::new(),
            plan_buffer: String::new(),
            saw_plan_block: false,
        }
    }

    /// Consume streamed text and return only content that should remain visible
    /// to the assistant transcript.
    pub(crate) fn consume(&mut self, chunk: &str) -> String {
        let chunk = self.filter_policy_chunk(chunk, false);
        self.consume_plan_markup(&chunk)
    }

    fn consume_plan_markup(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        let mut visible = String::new();

        loop {
            match self.mode.unwrap_or(ParseMode::Normal) {
                ParseMode::Normal => {
                    if let Some((index, tag)) = find_next_open_tag(&self.pending) {
                        visible.push_str(&self.pending[..index]);
                        self.pending.drain(..index + tag.open_tag().len());
                        self.mode = Some(ParseMode::InPlan { close_tag: tag.close_tag() });
                        self.saw_plan_block = true;
                        continue;
                    }

                    let keep_tail = OPEN_TAG.len().max(ALT_OPEN_TAG.len()).saturating_sub(1).min(self.pending.len());
                    let emit_len = safe_char_boundary(&self.pending, self.pending.len().saturating_sub(keep_tail));
                    visible.push_str(&self.pending[..emit_len]);
                    self.pending.drain(..emit_len);
                    break;
                }
                ParseMode::InPlan { close_tag } => {
                    if let Some(index) = self.pending.find(close_tag) {
                        self.plan_buffer.push_str(&self.pending[..index]);
                        self.pending.drain(..index + close_tag.len());
                        self.mode = Some(ParseMode::Normal);
                        continue;
                    }

                    let keep_tail = close_tag.len().saturating_sub(1).min(self.pending.len());
                    let append_len = safe_char_boundary(&self.pending, self.pending.len().saturating_sub(keep_tail));
                    self.plan_buffer.push_str(&self.pending[..append_len]);
                    self.pending.drain(..append_len);
                    break;
                }
            }
        }

        visible
    }

    /// Finish parsing and return any remaining visible text plus optional plan.
    pub(crate) fn finish(&mut self) -> ProposedPlanExtraction {
        let policy_trailing = self.flush_policy_pending();
        let mut trailing_visible = self.consume_plan_markup(&policy_trailing);
        match self.mode.unwrap_or(ParseMode::Normal) {
            ParseMode::Normal => {
                trailing_visible.push_str(&self.pending);
            }
            ParseMode::InPlan { .. } => {
                // Unterminated block: treat the remainder as plan content.
                self.plan_buffer.push_str(&self.pending);
            }
        }
        self.pending.clear();
        self.mode = Some(ParseMode::Normal);

        ProposedPlanExtraction {
            stripped_text: trailing_visible,
            plan_text: finalize_plan_text(self.saw_plan_block, &self.plan_buffer),
        }
    }

    pub(crate) fn has_unclosed_plan_block(&self) -> bool {
        matches!(self.mode, Some(ParseMode::InPlan { .. })) || has_partial_open_tag(&self.pending)
    }

    /// Whether the parser is currently buffering a `<proposed_plan>` / `<plan>`
    /// body. While true, streamed tokens are intentionally hidden from the
    /// transcript, so callers should show explicit drafting progress.
    pub(crate) fn is_drafting_plan(&self) -> bool {
        matches!(self.mode, Some(ParseMode::InPlan { .. }))
    }

    /// Number of plan body characters buffered so far. Used for throttled
    /// `Drafting plan... (N chars)` spinner updates without exposing content.
    /// Callers should only invoke this when an update is actually due: counting
    /// is linear in the buffered body, while [`Self::is_drafting_plan`] is O(1)
    /// and suitable for per-token checks.
    pub(crate) fn drafted_plan_len(&self) -> usize {
        self.plan_buffer.chars().count()
    }

    fn filter_policy_chunk(&mut self, chunk: &str, flush: bool) -> String {
        self.policy_pending.push_str(chunk);
        filter_policy_text(&mut self.policy_pending, flush)
    }

    fn flush_policy_pending(&mut self) -> String {
        self.filter_policy_chunk("", true)
    }
}

pub(crate) fn extract_proposed_plan(text: &str) -> ProposedPlanExtraction {
    let mut parser = ProposedPlanStreamParser::new();
    let mut stripped = parser.consume(text);
    let trailing = parser.finish();
    stripped.push_str(&trailing.stripped_text);

    ProposedPlanExtraction {
        stripped_text: stripped,
        plan_text: trailing.plan_text,
    }
}

pub(crate) fn extract_any_plan(text: &str) -> ProposedPlanExtraction {
    extract_proposed_plan(text)
}

/// Recovery synthesis accepts one canonical plan block only. Normal planning
/// responses continue to support the legacy `<plan>` alias, but the bounded
/// tool-free recovery contract must be unambiguous before it can be persisted
/// or shown for approval.
pub(crate) fn has_exactly_one_proposed_plan_block(text: &str) -> bool {
    let text = strip_plan_persistence_policy_line(text);
    let Some(open_index) = text.find(OPEN_TAG) else {
        return false;
    };
    let Some(close_index) = text.find(CLOSE_TAG) else {
        return false;
    };
    open_index < close_index
        && text.matches(OPEN_TAG).count() == 1
        && text.matches(CLOSE_TAG).count() == 1
        && !text.contains(ALT_OPEN_TAG)
        && !text.contains(ALT_CLOSE_TAG)
}

pub(crate) fn has_unclosed_plan_block(text: &str) -> bool {
    let mut parser = ProposedPlanStreamParser::new();
    parser.consume(text);
    parser.has_unclosed_plan_block()
}

/// Remove the runtime-owned persistence policy when a provider repeats it as
/// assistant prose around a plan block. The exact line is intentionally
/// bounded and prompt-owned; arbitrary model text is left untouched.
pub(crate) fn strip_plan_persistence_policy_line(text: &str) -> String {
    let stripped = text.replace(vtcode_core::prompts::system::PLANNING_WORKFLOW_PLAN_PERSISTENCE_POLICY_LINE, "");
    if stripped == text {
        text.to_string()
    } else {
        stripped.trim().to_string()
    }
}

/// Harness display contract for propose-plan markdown.
///
/// The harness must guarantee that any markdown content shown to the user is
/// parsed and rendered accurately: plan wrappers are removed, headings/lists
/// keep their structure, and failures produce clear, actionable feedback
/// instead of raw `<proposed_plan>` text.
///
/// Returns `(display_markdown, warnings)` where `display_markdown` is safe to
/// hand to the markdown renderer and `warnings` describes anything that had to
/// be repaired (unclosed blocks, empty plans, stray markup).
pub(crate) fn prepare_plan_markdown_for_display(raw: &str) -> (String, Vec<String>) {
    let mut warnings = Vec::new();
    if raw.trim().is_empty() {
        warnings.push("Plan content was empty; nothing to render.".to_string());
        return (String::new(), warnings);
    }

    let had_open = contains_plan_tag_outside_fences(raw, &[OPEN_TAG, ALT_OPEN_TAG]);
    let had_close = contains_plan_tag_outside_fences(raw, &[CLOSE_TAG, ALT_CLOSE_TAG]);
    if had_open && !had_close {
        warnings.push("Plan block was missing its closing tag; showing the partial draft.".to_string());
    }

    // Prefer the extracted plan body, but fall back to tag-stripped prose so a
    // tag-less structured synthesis still renders instead of vanishing.
    let mut body = extract_plan_body_for_display(raw).unwrap_or_else(|| strip_all_plan_tags(raw));
    body = strip_plan_persistence_policy_line(&body);

    if body.trim().is_empty() {
        warnings.push("Plan block was empty after removing markup; check the model output.".to_string());
        return (String::new(), warnings);
    }

    let normalized = normalize_plan_display_markdown(&body);

    if contains_plan_tag_outside_fences(&normalized, &[OPEN_TAG, CLOSE_TAG, ALT_OPEN_TAG, ALT_CLOSE_TAG]) {
        warnings.push("Stray plan markup remained after cleanup and was left as-is.".to_string());
    }

    (normalized, warnings)
}

/// Strip every plan wrapper tag outside fenced code for display purposes,
/// regardless of pairing. Literal tags in code examples remain untouched.
pub(crate) fn strip_all_plan_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fenced_code = false;
    let mut inline_code_ticks = None;
    for (index, raw_line) in text.split_inclusive('\n').enumerate() {
        if index > 0 && !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let is_fence = is_fence_delimiter(line);
        if in_fenced_code || is_fence {
            out.push_str(raw_line);
        } else {
            out.push_str(&strip_plan_tags(raw_line, &mut inline_code_ticks));
        }
        if is_fence {
            in_fenced_code = !in_fenced_code;
            inline_code_ticks = None;
        }
    }
    out.trim().to_string()
}

fn strip_plan_tags(text: &str, inline_code_ticks: &mut Option<usize>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    while cursor < text.len() {
        let Some((index, tag)) = find_plan_tag_outside_inline_code(
            &text[cursor..],
            &[OPEN_TAG, CLOSE_TAG, ALT_OPEN_TAG, ALT_CLOSE_TAG],
            inline_code_ticks,
        ) else {
            out.push_str(&text[cursor..]);
            break;
        };
        out.push_str(&text[cursor..cursor + index]);
        cursor += index + tag.len();
    }
    out
}

fn contains_plan_tag_outside_fences(text: &str, tags: &[&'static str]) -> bool {
    let mut in_fenced_code = false;
    let mut inline_code_ticks = None;
    for raw_line in text.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let is_fence = is_fence_delimiter(line);
        if !in_fenced_code
            && !is_fence
            && find_plan_tag_outside_inline_code(line, tags, &mut inline_code_ticks).is_some()
        {
            return true;
        }
        if is_fence {
            in_fenced_code = !in_fenced_code;
            inline_code_ticks = None;
        }
    }
    false
}

/// Find a plan wrapper outside inline-code spans while carrying the span state
/// across lines. The returned tag is static because all callers pass one of
/// the module's canonical wrapper constants.
fn find_plan_tag_outside_inline_code(
    text: &str,
    tags: &[&'static str],
    inline_code_ticks: &mut Option<usize>,
) -> Option<(usize, &'static str)> {
    let mut cursor = 0;
    while cursor < text.len() {
        let remainder = &text[cursor..];
        if remainder.starts_with('`') {
            let run_length = remainder.bytes().take_while(|byte| *byte == b'`').count();
            if inline_code_ticks.is_some_and(|ticks| ticks == run_length) {
                *inline_code_ticks = None;
            } else if inline_code_ticks.is_none() {
                *inline_code_ticks = Some(run_length);
            }
            cursor += run_length;
            continue;
        }

        if inline_code_ticks.is_none()
            && let Some(tag) = tags.iter().find(|tag| {
                remainder
                    .get(..tag.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(tag))
            })
        {
            return Some((cursor, *tag));
        }

        let character = remainder.chars().next().expect("cursor is on a character boundary");
        cursor += character.len_utf8();
    }
    None
}

/// Extract a plan body while ignoring wrapper-looking text inside fenced code.
/// The streaming parser remains responsible for live transcript suppression;
/// this full-input path is used for the approval display, where preserving the
/// exact code block is more important than incremental output.
fn extract_plan_body_for_display(text: &str) -> Option<String> {
    let mut close_tag: Option<&'static str> = None;
    let mut body = String::new();
    let mut in_fenced_code = false;
    let mut inline_code_ticks = None;
    let mut saw_plan = false;

    for raw_line in text.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let is_fence = is_fence_delimiter(line);
        if in_fenced_code || is_fence {
            if saw_plan {
                body.push_str(raw_line);
            }
            if is_fence {
                in_fenced_code = !in_fenced_code;
                inline_code_ticks = None;
            }
            continue;
        }

        let mut remaining = raw_line;
        loop {
            if let Some(expected_close) = close_tag {
                if let Some((index, tag)) =
                    find_plan_tag_outside_inline_code(remaining, &[expected_close], &mut inline_code_ticks)
                {
                    body.push_str(&remaining[..index]);
                    remaining = &remaining[index + tag.len()..];
                    close_tag = None;
                    continue;
                }
                body.push_str(remaining);
                break;
            }

            let Some((index, tag)) =
                find_plan_tag_outside_inline_code(remaining, &[OPEN_TAG, ALT_OPEN_TAG], &mut inline_code_ticks)
            else {
                break;
            };
            saw_plan = true;
            close_tag = Some(if tag == OPEN_TAG { CLOSE_TAG } else { ALT_CLOSE_TAG });
            remaining = &remaining[index + tag.len()..];
        }
    }

    saw_plan.then(|| body.trim().to_string())
}

/// Normalize plan prose so the markdown renderer preserves structure:
/// - `•` bullets become `-` (the marker pulldown-cmark recognizes)
/// - bare `Summary:` / `Test Cases and Validation:` / `Assumptions…` labels
///   become `##` headings so they read as sections instead of a wall of text
/// - long `1. Action -> files: […] -> verify: […]` steps are split so the
///   metadata renders as nested bullets instead of one unwrapped wall of text
/// - missing blank lines around headings/lists are repaired so the parser
///   emits distinct blocks instead of one collapsed paragraph.
fn normalize_plan_display_markdown(body: &str) -> String {
    const HEADING_ALIASES: &[(&str, &str)] = &[
        ("summary", "## Summary"),
        ("implementation steps", "## Implementation Steps"),
        ("test cases and validation", "## Test Cases and Validation"),
        ("validation", "## Test Cases and Validation"),
        ("assumptions and defaults", "## Assumptions and Defaults"),
        ("assumptions", "## Assumptions and Defaults"),
        ("open questions", "## Open Questions"),
    ];

    struct DisplayLine {
        text: String,
        structural: bool,
    }

    let mut lines: Vec<DisplayLine> = Vec::with_capacity(body.lines().count() + 8);
    let mut in_fenced_code = false;
    for line in body.lines() {
        let trimmed_start = line.trim_start();
        let is_fence = is_fence_delimiter(line);
        if in_fenced_code || is_fence {
            lines.push(DisplayLine { text: line.to_string(), structural: false });
            if is_fence {
                in_fenced_code = !in_fenced_code;
            }
            continue;
        }

        let indent_len = line.len() - trimmed_start.len();
        let indent = &line[..indent_len];

        // Normalize bullets first so list structure survives rendering.
        let mut current = if let Some(rest) = trimmed_start.strip_prefix("• ") {
            format!("{indent}- {rest}")
        } else if trimmed_start == "•" {
            format!("{indent}-")
        } else {
            line.to_string()
        };

        // Promote sparse `Label:` lines to headings. Only exact label matches
        // (case-insensitive, optional trailing colon) or `Label: prose` inline
        // forms are rewritten; numbered steps and prose containing those words
        // elsewhere are left untouched.
        let trimmed_owned = current.trim().to_string();
        let trimmed: &str = &trimmed_owned;
        let label = trimmed.strip_suffix(':').unwrap_or(trimmed).trim();
        let lowered = label.to_ascii_lowercase();
        if !label.is_empty() && label.len() <= 64 {
            if let Some((_, heading)) = HEADING_ALIASES.iter().find(|(alias, _)| *alias == lowered) {
                if trimmed.ends_with(':') {
                    current = heading.to_string();
                }
            }
        }

        if trimmed.len() <= 256
            && let Some((_, heading)) = HEADING_ALIASES
                .iter()
                .find(|(alias, _)| trimmed.to_ascii_lowercase().starts_with(&format!("{alias}:")))
        {
            // e.g. `Summary: the fix ...` -> heading + paragraph.
            if let Some(colon) = trimmed.find(':') {
                let after = trimmed[colon + 1..].trim();
                let heading_owned = heading.to_string();
                if !after.is_empty() {
                    lines.push(DisplayLine { text: heading_owned, structural: true });
                    lines.push(DisplayLine { text: String::new(), structural: true });
                    lines.push(DisplayLine { text: after.to_string(), structural: true });
                    continue;
                }
                current = heading_owned;
            }
        }

        // Split long `1. Action -> files: […] -> verify: […]` steps so the
        // metadata renders as nested bullets. A single-line step with two
        // `->` clauses wraps without a hanging indent and reads as a wall of
        // text (Sep-04 screenshot); nested `- files:` / `- verify:` lines keep
        // the ordered-list structure while staying scannable. Fenced code is
        // already excluded above. Splits happen only at ` -> files:` /
        // ` -> verify:` outside inline-code spans so `` `a -> files: b` ``
        // and prose arrows (`A -> B`) stay intact.
        if is_ordered_list_item(current.trim())
            && let Some(parts) = split_ordered_step_metadata(current.trim())
        {
            let marker_width = ordered_marker_width(current.trim());
            let nested_prefix = format!("{indent}{:width$}- ", "", width = marker_width);
            lines.push(DisplayLine {
                text: format!("{indent}{}", parts[0].trim_end()),
                structural: true,
            });
            for part in &parts[1..] {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                lines.push(DisplayLine {
                    text: format!("{nested_prefix}{part}"),
                    structural: true,
                });
            }
            continue;
        }

        lines.push(DisplayLine { text: current, structural: true });
    }

    // Ensure blank lines around headings and lists so the markdown parser does
    // not collapse them into a single paragraph.
    let mut spaced: Vec<String> = Vec::with_capacity(lines.len() * 2);
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.text.trim();
        let is_heading = line.structural && trimmed.starts_with("## ");
        let is_list = line.structural
            && (trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("+ ")
                || is_ordered_list_item(trimmed));
        let prev_blank = spaced.last().is_some_and(|prev: &String| prev.trim().is_empty());
        if (is_heading || is_list) && idx > 0 && !prev_blank {
            spaced.push(String::new());
        }
        spaced.push(line.text.clone());
        let next_is_list_or_heading = lines.get(idx + 1).map(|next| {
            let t = next.text.trim();
            next.structural
                && (t.starts_with("## ") || t.starts_with("- ") || t.starts_with("* ") || is_ordered_list_item(t))
        });
        if (is_heading || is_list) && next_is_list_or_heading == Some(false) {
            if let Some(next) = lines.get(idx + 1)
                && !next.text.trim().is_empty()
            {
                spaced.push(String::new());
            }
        }
    }

    spaced.join("\n").trim().to_string()
}

fn is_fence_delimiter(line: &str) -> bool {
    vtcode_commons::formatting::is_markdown_fence_delimiter(line)
}

fn is_ordered_list_item(trimmed: &str) -> bool {
    let mut chars = trimmed.chars();
    let mut saw_digit = false;
    for ch in chars.by_ref() {
        if ch.is_ascii_digit() {
            saw_digit = true;
        } else {
            break;
        }
    }
    if !saw_digit {
        return false;
    }
    let rest: String = trimmed.chars().skip_while(|c| c.is_ascii_digit()).collect();
    rest.starts_with(". ") || rest.starts_with(") ")
}

/// Width of the ordered marker (`1. ` -> 3, `10. ` -> 4) so nested bullets
/// align under the item content instead of a fixed 3-space guess.
fn ordered_marker_width(trimmed: &str) -> usize {
    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    digits + 2
}

/// Split `1. Action -> files: […] -> verify: […]` at metadata boundaries
/// outside inline-code spans. Returns head + metadata parts; `None` when no
/// ` -> files:` / ` -> verify:` marker exists outside backticks.
fn split_ordered_step_metadata(trimmed: &str) -> Option<Vec<String>> {
    let mut splits = Vec::new();
    let mut inline_ticks: Option<usize> = None;
    let mut cursor = 0;
    while cursor < trimmed.len() {
        let remainder = &trimmed[cursor..];
        if remainder.starts_with('`') {
            let run = remainder.bytes().take_while(|byte| *byte == b'`').count();
            if inline_ticks.is_some_and(|ticks| ticks == run) {
                inline_ticks = None;
            } else if inline_ticks.is_none() {
                inline_ticks = Some(run);
            }
            cursor += run;
            continue;
        }
        if inline_ticks.is_none() && (remainder.starts_with(" -> files:") || remainder.starts_with(" -> verify:")) {
            splits.push(cursor);
            cursor += " -> ".len();
            continue;
        }
        let character = remainder.chars().next().expect("cursor is on a character boundary");
        cursor += character.len_utf8();
    }
    if splits.is_empty() {
        return None;
    }
    let mut parts = Vec::with_capacity(splits.len() + 1);
    let mut start = 0;
    for split in splits {
        parts.push(trimmed[start..split].to_string());
        start = split + " -> ".len();
    }
    parts.push(trimmed[start..].to_string());
    Some(parts)
}

fn find_next_open_tag(text: &str) -> Option<(usize, PlanTag)> {
    [PlanTag::Proposed, PlanTag::Alternate]
        .into_iter()
        .filter_map(|tag| text.find(tag.open_tag()).map(|index| (index, tag)))
        .min_by_key(|(index, _)| *index)
}

fn has_partial_open_tag(text: &str) -> bool {
    [OPEN_TAG, ALT_OPEN_TAG]
        .into_iter()
        .any(|tag| (1..tag.len()).any(|prefix_len| text.ends_with(&tag[..prefix_len])))
}

fn filter_policy_text(buffer: &mut String, flush: bool) -> String {
    let policy = vtcode_core::prompts::system::PLANNING_WORKFLOW_PLAN_PERSISTENCE_POLICY_LINE;
    let mut visible = String::new();

    loop {
        let Some(start) = find_policy_candidate_start(buffer, policy) else {
            visible.push_str(buffer);
            buffer.clear();
            break;
        };

        visible.push_str(&buffer[..start]);
        buffer.drain(..start);
        if buffer.starts_with(policy) {
            buffer.drain(..policy.len());
            continue;
        }
        if !flush && policy.starts_with(buffer.as_str()) {
            break;
        }

        visible.push_str(buffer);
        buffer.clear();
        break;
    }

    visible
}

fn find_policy_candidate_start(text: &str, policy: &str) -> Option<usize> {
    let mut line_start = 0;
    loop {
        let candidate = &text[line_start..];
        if policy.starts_with(candidate) || candidate.starts_with(policy) {
            return Some(line_start);
        }
        let newline = text[line_start..].find('\n')?;
        line_start += newline + 1;
        if line_start >= text.len() {
            return None;
        }
    }
}

fn finalize_plan_text(saw_plan_block: bool, raw: &str) -> Option<String> {
    if !saw_plan_block {
        return None;
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn safe_char_boundary(text: &str, idx: usize) -> usize {
    if idx >= text.len() {
        return text.len();
    }
    if text.is_char_boundary(idx) {
        return idx;
    }
    text.char_indices()
        .take_while(|(pos, _)| *pos < idx)
        .last()
        .map(|(pos, _)| pos)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
