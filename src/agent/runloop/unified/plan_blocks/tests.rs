use super::{
    ProposedPlanStreamParser, extract_any_plan, extract_proposed_plan, has_exactly_one_proposed_plan_block,
    has_unclosed_plan_block, prepare_plan_markdown_for_display, strip_plan_persistence_policy_line,
};

#[test]
fn drafting_state_tracks_suppressed_plan_body() {
    let mut parser = ProposedPlanStreamParser::new();
    assert!(!parser.is_drafting_plan());
    assert_eq!(parser.drafted_plan_len(), 0);

    // Asymmetric: visible intro vs suppressed plan body. The parser holds
    // back a short tail for split-tag detection, so visible output appears
    // after `finish`, while drafting state flips immediately on open tag.
    let _ = parser.consume("Intro\n<proposed_plan>\n- Step 1\n");
    assert!(parser.is_drafting_plan());

    // Feed enough body to exceed the close-tag tail buffer.
    let long_body = "x".repeat(128);
    let _ = parser.consume(&long_body);
    assert!(parser.drafted_plan_len() > 0);

    let _ = parser.consume("\n</proposed_plan>\nOutro");
    assert!(!parser.is_drafting_plan());
    let trailing = parser.finish();
    assert!(trailing.plan_text.is_some());
    assert!(trailing.stripped_text.contains("Outro"));
}

#[test]
fn drafting_state_ignores_closed_and_tagless_text() {
    let mut parser = ProposedPlanStreamParser::new();
    let _ = parser.consume("No plan here");
    let trailing = parser.finish();
    assert_eq!(trailing.stripped_text, "No plan here");
    assert!(!parser.is_drafting_plan());
    assert_eq!(trailing.plan_text, None);

    // Alternate tag also drafts; closed block stops drafting.
    let mut parser = ProposedPlanStreamParser::new();
    let _ = parser.consume("Before<plan>\n- A");
    assert!(parser.is_drafting_plan());
    let _ = parser.consume("\n</plan>\nAfter");
    assert!(!parser.is_drafting_plan());
}

#[test]
fn extracts_single_proposed_plan_block() {
    let extraction = extract_proposed_plan("Intro\n<proposed_plan>\n- A\n- B\n</proposed_plan>\nOutro");
    assert_eq!(extraction.stripped_text, "Intro\n\nOutro");
    assert_eq!(extraction.plan_text.as_deref(), Some("- A\n- B"));
}

#[test]
fn keeps_text_when_no_plan_block_exists() {
    let extraction = extract_proposed_plan("No plan here");
    assert_eq!(extraction.stripped_text, "No plan here");
    assert!(extraction.plan_text.is_none());
}

#[test]
fn handles_unterminated_plan_block() {
    let extraction = extract_proposed_plan("Before<proposed_plan>\n- Step 1\n- Step 2");
    assert_eq!(extraction.stripped_text, "Before");
    assert_eq!(extraction.plan_text.as_deref(), Some("- Step 1\n- Step 2"));
    assert!(has_unclosed_plan_block("Before<proposed_plan>\n- Step 1\n- Step 2"));
}

#[test]
fn extracts_alternate_plan_block_through_shared_parser() {
    let extraction = extract_any_plan("Intro\n<plan>\n- A\n- B\n</plan>\nOutro");
    assert_eq!(extraction.stripped_text, "Intro\n\nOutro");
    assert_eq!(extraction.plan_text.as_deref(), Some("- A\n- B"));
    assert!(!has_unclosed_plan_block("Intro\n<plan>\n- A\n- B\n</plan>\nOutro"));
}

#[test]
fn handles_unterminated_alternate_plan_block() {
    let extraction = extract_any_plan("Before<plan>\n- Step 1\n- Step 2");
    assert_eq!(extraction.stripped_text, "Before");
    assert_eq!(extraction.plan_text.as_deref(), Some("- Step 1\n- Step 2"));
    assert!(has_unclosed_plan_block("Before<plan>\n- Step 1\n- Step 2"));
    assert!(has_unclosed_plan_block("Before<plan"));
}

#[test]
fn supports_streaming_chunks_with_split_tags() {
    let mut parser = ProposedPlanStreamParser::new();
    let mut visible = String::new();
    visible.push_str(&parser.consume("Intro\n<propo"));
    visible.push_str(&parser.consume("sed_plan>\n- Step"));
    visible.push_str(&parser.consume(" 1\n</proposed_plan>\nOutro"));
    let trailing = parser.finish();
    visible.push_str(&trailing.stripped_text);

    assert_eq!(visible, "Intro\n\nOutro");
    assert_eq!(trailing.plan_text.as_deref(), Some("- Step 1"));
}

#[test]
fn supports_streaming_chunks_with_split_alternate_tags() {
    let mut parser = ProposedPlanStreamParser::new();
    let mut visible = String::new();
    visible.push_str(&parser.consume("Intro\n<pl"));
    visible.push_str(&parser.consume("an>\n- Step"));
    visible.push_str(&parser.consume(" 1\n</plan>\nOutro"));
    let trailing = parser.finish();
    visible.push_str(&trailing.stripped_text);

    assert_eq!(visible, "Intro\n\nOutro");
    assert_eq!(trailing.plan_text.as_deref(), Some("- Step 1"));
}

#[test]
fn handles_multibyte_text_without_panicking() {
    let mut parser = ProposedPlanStreamParser::new();
    let mut visible = String::new();
    visible.push_str(&parser.consume("an’t exit on my"));
    let trailing = parser.finish();
    visible.push_str(&trailing.stripped_text);

    assert_eq!(visible, "an’t exit on my");
    assert!(trailing.plan_text.is_none());
}

#[test]
fn strips_only_the_known_plan_persistence_policy_line() {
    let policy = vtcode_core::prompts::system::PLANNING_WORKFLOW_PLAN_PERSISTENCE_POLICY_LINE;
    let text = format!("{policy}\n\nResearch summary");
    assert_eq!(strip_plan_persistence_policy_line(&text), "Research summary");
    assert_eq!(strip_plan_persistence_policy_line("unrelated prose"), "unrelated prose");
}

#[test]
fn policy_echo_does_not_look_like_an_unclosed_plan_block() {
    let policy = vtcode_core::prompts::system::PLANNING_WORKFLOW_PLAN_PERSISTENCE_POLICY_LINE;
    let response = format!("{policy}\n<plan>\n- Step 1\n</plan>");
    let extraction = extract_any_plan(&response);
    assert_eq!(extraction.plan_text.as_deref(), Some("- Step 1"));
    assert!(!extraction.stripped_text.contains("Emit exactly one final"));

    let split_at = policy.find("<proposed_plan>").expect("policy tag");
    let mut parser = ProposedPlanStreamParser::new();
    let mut visible = parser.consume(&policy[..split_at]);
    visible.push_str(&parser.consume(&policy[split_at..]));
    visible.push_str(&parser.consume("\n<plan>\n- Step 1\n</plan>"));
    let trailing = parser.finish();
    visible.push_str(&trailing.stripped_text);
    assert!(!visible.contains("Emit exactly one final"));
    assert_eq!(trailing.plan_text.as_deref(), Some("- Step 1"));
}

#[test]
fn recovery_plan_shape_requires_one_canonical_block() {
    assert!(has_exactly_one_proposed_plan_block("<proposed_plan>\n- Step 1\n</proposed_plan>"));
    assert!(!has_exactly_one_proposed_plan_block("<plan>\n- Step 1\n</plan>"));
    assert!(!has_exactly_one_proposed_plan_block(
        "<proposed_plan>\n- A\n</proposed_plan>\n<proposed_plan>\n- B\n</proposed_plan>"
    ));
    assert!(!has_exactly_one_proposed_plan_block("<proposed_plan>\n- Missing close"));
    assert!(!has_exactly_one_proposed_plan_block("<proposed_plan>\n- A\n</proposed_plan>\n</proposed_plan>"));
    assert!(!has_exactly_one_proposed_plan_block("</proposed_plan>\n<proposed_plan>\n- A\n"));
}

#[test]
fn chunked_canonical_policy_echo_preserves_only_the_actual_plan() {
    let policy = vtcode_core::prompts::system::PLANNING_WORKFLOW_PLAN_PERSISTENCE_POLICY_LINE;
    let plan = "## Summary\nUpdate the parser.\n## Implementation Steps\n1. Fix boundary -> files: [src/parser.rs] -> verify: [cargo check]";
    let response = format!("{policy}\n<proposed_plan>\n{plan}\n</proposed_plan>");
    for chunk_size in [1, 7, 31] {
        let mut parser = ProposedPlanStreamParser::new();
        let mut visible = String::new();
        for chunk in response.as_bytes().chunks(chunk_size) {
            visible.push_str(&parser.consume(std::str::from_utf8(chunk).expect("ASCII fixture")));
        }
        let extraction = parser.finish();
        visible.push_str(&extraction.stripped_text);
        assert!(visible.trim().is_empty(), "policy and markup must stay suppressed");
        assert_eq!(extraction.plan_text.as_deref(), Some(plan));
        assert!(!parser.has_unclosed_plan_block());
    }
    assert!(has_exactly_one_proposed_plan_block(&strip_plan_persistence_policy_line(&response)));
}

#[test]
fn display_preparation_strips_plan_wrappers_and_preserves_structure() {
    let raw = "<proposed_plan>\nSummary: Fix scrolling.\n\n1. Locate modal -> files: [src/modal.rs] -> verify: [cargo check]\n\nTest Cases and Validation:\n\n• Down reaches the final item.\n\nAssumptions and Defaults:\n\n• Paths are stale.\n</proposed_plan>";
    let (display, warnings) = prepare_plan_markdown_for_display(raw);
    assert!(!display.contains("<proposed_plan>"));
    assert!(!display.contains("</proposed_plan>"));
    assert!(display.contains("## Summary"));
    assert!(display.contains("## Test Cases and Validation"));
    assert!(display.contains("## Assumptions and Defaults"));
    assert!(display.contains("- Down reaches the final item."));
    assert!(display.contains("1. Locate modal"));
    // Long steps are split so metadata renders as nested bullets instead
    // of one unwrapped wall of text (Sep-04 screenshot).
    assert!(display.contains("- files: [src/modal.rs]"));
    assert!(display.contains("- verify: [cargo check]"));
    assert!(warnings.is_empty(), "well-formed plan should not warn: {warnings:?}");
}

#[test]
fn display_preparation_splits_screenshot_style_long_steps() {
    let raw = "<proposed_plan>\nSummary: The approved /config scrolling fix could not be implemented.\n\n1. Locate the actual /config list-modal implementation -> files: [src/a.rs, crates/b.rs] -> verify: [rg -n \"x\" src]\n\nTest Cases and Validation:\n\n• Down reaches the final item.\n\nAssumptions and Defaults:\n\n• Paths are stale.\n</proposed_plan>";
    let (display, warnings) = prepare_plan_markdown_for_display(raw);
    assert!(!display.contains("<proposed_plan>"));
    assert!(display.contains("## Summary"));
    assert!(display.contains("1. Locate the actual /config list-modal implementation"));
    assert!(display.contains("- files: [src/a.rs, crates/b.rs]"));
    assert!(display.contains("- verify:"));
    assert!(display.contains("## Test Cases and Validation"));
    assert!(display.contains("- Down reaches the final item."));
    assert!(warnings.is_empty(), "well-formed plan should not warn: {warnings:?}");
}

#[test]
fn display_preparation_warns_on_unclosed_and_empty_plans() {
    let (partial, warnings) = prepare_plan_markdown_for_display("<proposed_plan>\n- Step 1");
    assert!(partial.contains("- Step 1"));
    assert!(warnings.iter().any(|w| w.contains("closing tag")));

    let (empty, warnings) = prepare_plan_markdown_for_display("<proposed_plan>\n   \n</proposed_plan>");
    assert!(empty.is_empty());
    assert!(!warnings.is_empty());

    let (blank, warnings) = prepare_plan_markdown_for_display("   ");
    assert!(blank.is_empty());
    assert!(!warnings.is_empty());
}

#[test]
fn display_preparation_preserves_fenced_code() {
    let raw = "<proposed_plan>\n## Summary\n```text\n• literal\nSummary:\n```\n</proposed_plan>";
    let (display, warnings) = prepare_plan_markdown_for_display(raw);

    assert!(warnings.is_empty(), "well-formed plan should not warn: {warnings:?}");
    assert!(display.contains("```text\n• literal\nSummary:\n```"));
}

#[test]
fn display_preparation_does_not_treat_fenced_plan_tags_as_wrappers() {
    let raw = "<proposed_plan>\n```text\n<proposed_plan>\nliteral\n</proposed_plan>\n```\n</proposed_plan>";
    let (display, warnings) = prepare_plan_markdown_for_display(raw);

    assert!(warnings.is_empty(), "well-formed plan should not warn: {warnings:?}");
    assert!(display.contains("<proposed_plan>\nliteral\n</proposed_plan>"));

    let code_only = "```text\n<plan>\nliteral\n</plan>\n```";
    let (display, warnings) = prepare_plan_markdown_for_display(code_only);
    assert!(warnings.is_empty(), "code-only markdown should not warn: {warnings:?}");
    assert_eq!(display, code_only);
}

#[test]
fn display_preparation_preserves_inline_plan_tags() {
    let code_only = "Use `<plan>` and `<proposed_plan>` as literal wrapper names.";
    let (display, warnings) = prepare_plan_markdown_for_display(code_only);

    assert!(warnings.is_empty(), "inline code should not be treated as plan markup: {warnings:?}");
    assert_eq!(display, code_only);
}
