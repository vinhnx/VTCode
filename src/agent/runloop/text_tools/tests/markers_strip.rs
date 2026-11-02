use super::*;

#[test]
fn enclosed_regions_keep_unicode_byte_offsets_and_unclosed_tail() {
    let text = "é<tool_call>a</tool_call>日本<tool_call>b</tool_call>尾<tool_call>未完";
    let mut regions = Vec::new();
    collect_enclosed_regions(text, "<tool_call>", "</tool_call>", &mut regions);
    assert_eq!(regions, vec![(2, 26), (32, 56), (59, text.len())]);
    assert_eq!(&text[regions[2].0..regions[2].1], "<tool_call>未完");

    let mut absent = Vec::new();
    collect_enclosed_regions("日本</tool_call>", "<tool_call>", "</tool_call>", &mut absent);
    assert!(absent.is_empty());
}
// ── contains_pseudo_tool_call_markers ─────────────────────────────────────────

#[test]
fn pseudo_markers_detect_bare_tool_call_tag() {
    assert!(contains_pseudo_tool_call_markers(
        "Let me apply:\n<tool_call>\n<function=apply_patch>...</function>\n</tool_call>"
    ));
}
#[test]
fn pseudo_markers_detect_function_eq_without_tool_call_wrapper() {
    assert!(contains_pseudo_tool_call_markers("<function=write_file>content</function>"));
}
#[test]
fn pseudo_markers_detect_parameter_eq() {
    assert!(contains_pseudo_tool_call_markers("<parameter=patch>some diff</parameter>"));
}
#[test]
fn pseudo_markers_detect_invoke_name() {
    assert!(contains_pseudo_tool_call_markers(
        r#"<invoke name="apply_patch"><parameter name="p">x</parameter></invoke>"#
    ));
}
#[test]
fn pseudo_markers_detect_minimax_tool_call() {
    assert!(contains_pseudo_tool_call_markers("<minimax:tool_call>...</minimax:tool_call>"));
}
#[test]
fn pseudo_markers_detect_tools_call_dialect() {
    assert!(contains_pseudo_tool_call_markers(r#"<tools:call name="exec_command">...</tools:call>"#));
}
#[test]
fn pseudo_markers_detect_spaced_dsml_dialect() {
    assert!(contains_pseudo_tool_call_markers(
        "<\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} invoke name=\"exec_command\">...</\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} invoke>"
    ));
}
#[test]
fn pseudo_markers_detect_closing_tool_call_tag_alone() {
    assert!(contains_pseudo_tool_call_markers("</tool_call>"));
}
#[test]
fn pseudo_markers_is_case_insensitive() {
    assert!(contains_pseudo_tool_call_markers("<TOOL_CALL>...</TOOL_CALL>"));
    assert!(contains_pseudo_tool_call_markers("<Function=foo>"));
}
#[test]
fn pseudo_markers_returns_false_for_clean_prose() {
    assert!(!contains_pseudo_tool_call_markers(
        "I searched the codebase and found 3 matches. Here are the results."
    ));
}
#[test]
fn pseudo_markers_returns_false_for_ordinary_xml_attributes() {
    // A plain XML attribute like `name="foo"` should not trigger the check.
    assert!(!contains_pseudo_tool_call_markers(r#"<config name="foo" value="bar"/>"#));
}
// ── strip_textual_tool_call_regions — function= / parameter= coverage ─────────

#[test]
fn strip_regions_removes_bare_function_eq_block() {
    let text = "Here is what I would do:\n\
                <function=apply_patch>--- a/f\n+++ b/f\n@@ -1 +1 @@\n-old\n+new</function>\n\
                Ask me if you want me to actually run this.";
    let stripped = strip_textual_tool_call_regions(text);
    assert!(!stripped.contains("<function="), "function= block should be stripped");
    assert!(stripped.contains("Here is what I would do"), "surrounding prose should be preserved");
}
#[test]
fn strip_regions_removes_bare_parameter_eq_block() {
    let text = "Patch content:\n<parameter=patch>diff goes here</parameter>";
    let stripped = strip_textual_tool_call_regions(text);
    assert!(!stripped.contains("<parameter="));
}
/// Regression: when a model uses parameterised close tags (`</function=name>`,
/// `</parameter=name>`) the stripper must not fall back to `end = text.len()`
/// and must not eat prose that follows the block.
#[test]
fn strip_regions_parameterised_close_tags_do_not_eat_trailing_prose() {
    // Bare block with parameterised close tag, followed by trailing prose.
    let bare = "Prose before.\n\
                <function=apply_patch>diff content</function=apply_patch>\n\
                Prose after.";
    let stripped = strip_textual_tool_call_regions(bare);
    assert!(!stripped.contains("<function="), "function= block should be stripped");
    assert!(stripped.contains("Prose before."), "leading prose must survive");
    assert!(stripped.contains("Prose after."), "trailing prose must survive bare parameterised close tag");

    // When the function= block is nested inside <tool_call> with prose after
    // </tool_call>, the fallback `end=text.len()` must NOT swallow trailing prose.
    // Note: <tool_call> itself may remain (collect_enclosed_regions requires a
    // parseable payload), but the key invariant is that trailing prose is preserved.
    let wrapped = "Prose before.\n\
                   <tool_call>\n\
                   <function=apply_patch>diff</function=apply_patch>\n\
                   </tool_call>\n\
                   Prose after.";
    let stripped2 = strip_textual_tool_call_regions(wrapped);
    assert!(!stripped2.contains("<function=apply_patch>"), "function= content must be stripped");
    assert!(stripped2.contains("Prose before."), "leading prose must survive");
    assert!(stripped2.contains("Prose after."), "trailing prose must not be eaten by merged regions");
}
#[test]
fn strip_regions_removes_function_eq_block_with_parameterised_close() {
    // `</function=name>` close tag (without a tool_call wrapper) must be consumed.
    let text = "<function=write_file>hello world</function=write_file>";
    let stripped = strip_textual_tool_call_regions(text);
    assert!(!stripped.contains("<function="), "function= block should be fully stripped");
    assert!(!stripped.contains("</function="), "parameterised close tag must be consumed");
}
