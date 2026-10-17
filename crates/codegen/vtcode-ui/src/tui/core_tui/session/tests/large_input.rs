#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::helpers::*;
use crate::tui::core_tui::types::ContentPart;

#[test]
fn long_single_line_paste_collapses_to_summary() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let pasted = "a".repeat(ui::INLINE_INPUT_COMPACT_CHAR_THRESHOLD + 100);
    session.insert_paste_text(&pasted);
    assert!(session.input_manager.compact_paste_range().is_some());
    assert!(session.input_compact_placeholder().is_some());

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Pasted Content"), "expected summary, got: {rendered}");
    assert!(!rendered.contains(&pasted), "full single-line flood must not render");
    // Full fidelity preserved for submit.
    assert_eq!(session.input_manager.content(), pasted);
}

#[test]
fn long_typed_content_collapses_without_paste_range() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let typed = "b".repeat(ui::INLINE_INPUT_COMPACT_CHAR_THRESHOLD + 50);
    session.set_input(typed.clone());
    assert!(session.input_manager.compact_paste_range().is_none());
    assert!(session.input_compact_placeholder().is_some());

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Pasted Content"), "expected summary, got: {rendered}");
    assert!(!rendered.contains(&typed));
    assert_eq!(session.input_manager.content(), typed);
}

#[test]
fn short_input_does_not_collapse() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello world".to_string());
    assert!(session.input_compact_placeholder().is_none());

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("hello world"));
    assert!(!rendered.contains("[Pasted Content"));
}

#[test]
fn many_images_collapse_to_summary_with_counts() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let content = "[Image #1][Image #2][Image #3] describe them".to_string();
    session.set_input(content.clone());
    session.input_manager.set_attachments(vec![
        ContentPart::image("a", "image/png"),
        ContentPart::image("b", "image/png"),
        ContentPart::image("c", "image/png"),
    ]);
    // Refresh collapse state after attachments change (mirrors image-paste path).
    session.refresh_input_edit_state();

    let placeholder = session.input_compact_placeholder().expect("should collapse");
    assert!(placeholder.contains("[Pasted Content"), "got: {placeholder}");
    assert!(placeholder.contains("3 images"), "got: {placeholder}");

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("3 images"), "got: {rendered}");
    assert!(!rendered.contains("[Image #1][Image #2][Image #3]"));
    // Attachments preserved.
    assert_eq!(session.input_manager.attachments().len(), 3);
}

#[test]
fn many_file_tokens_collapse_to_summary_with_counts() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let content = "@src/a.rs @src/b.rs @src/c.rs @src/d.rs @src/e.rs @src/f.rs".to_string();
    session.set_input(content.clone());

    let placeholder = session.input_compact_placeholder().expect("should collapse");
    assert!(placeholder.contains("[Pasted Content"), "got: {placeholder}");
    assert!(placeholder.contains("files"), "got: {placeholder}");

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Pasted Content"), "got: {rendered}");
    assert!(!rendered.contains("@src/a.rs @src/b.rs"));
    assert_eq!(session.input_manager.content(), content);
}

#[test]
fn few_file_tokens_do_not_collapse() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("check @src/main.rs please".to_string());
    assert!(session.input_compact_placeholder().is_none());
}

#[test]
fn large_char_paste_sets_compact_range_and_backspace_clears_atomically() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let pasted = "x".repeat(ui::INLINE_INPUT_COMPACT_CHAR_THRESHOLD + 10);
    session.insert_paste_text(&pasted);
    assert!(session.input_manager.compact_paste_range().is_some());
    session.delete_char();
    assert_eq!(session.input_manager.content(), "");
    assert!(session.input_manager.compact_paste_range().is_none());
}

#[test]
fn large_input_summary_shows_head_and_tail() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let head = "HEAD-".repeat(40);
    let tail = "TAIL-".repeat(40);
    let middle = "m".repeat(ui::INLINE_INPUT_COMPACT_CHAR_THRESHOLD);
    let content = format!("{head}{middle}{tail}");
    session.set_input(content.clone());

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Pasted Content"), "got: {rendered}");
    assert!(rendered.contains("HEAD-"), "head snippet missing: {rendered}");
    assert!(rendered.contains("TAIL-"), "tail snippet missing: {rendered}");
    // Middle flood summarized, not rendered in full.
    assert!(rendered.len() < content.len() / 2, "rendered should be bounded");
}

#[test]
fn collapse_threshold_boundaries_are_asymmetric_per_dimension() {
    // Lines: 9 short lines stay expanded, 10 collapse.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let nine = (0..9).map(|i| format!("l{i}")).collect::<Vec<_>>().join("\n");
    session.set_input(nine);
    assert!(session.input_compact_placeholder().is_none());
    let ten = (0..10).map(|i| format!("l{i}")).collect::<Vec<_>>().join("\n");
    session.set_input(ten);
    assert!(session.input_compact_placeholder().is_some());

    // Chars: 999 stay expanded, 1000 collapse (single line, no other signals).
    session.set_input("c".repeat(999));
    assert!(session.input_compact_placeholder().is_none());
    session.set_input("c".repeat(1000));
    assert!(session.input_compact_placeholder().is_some());

    // Images: 2 placeholders stay expanded, 3 collapse.
    session.set_input("[Image #1][Image #2] ok".to_string());
    assert!(session.input_compact_placeholder().is_none());
    session.set_input("[Image #1][Image #2][Image #3] ok".to_string());
    assert!(session.input_compact_placeholder().is_some());

    // Files: 4 refs stay expanded, 5 collapse.
    session.set_input("@src/a.rs @src/b.rs @src/c.rs @src/d.rs".to_string());
    assert!(session.input_compact_placeholder().is_none());
    session.set_input("@src/a.rs @src/b.rs @src/c.rs @src/d.rs @src/e.rs".to_string());
    assert!(session.input_compact_placeholder().is_some());
}

#[test]
fn multibyte_flood_collapses_without_panicking() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let flooded = "😀".repeat(ui::INLINE_INPUT_COMPACT_CHAR_THRESHOLD + 10);
    session.set_input(flooded.clone());
    assert!(session.input_compact_placeholder().is_some());

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Pasted Content"), "got: {rendered}");
    assert!(rendered.contains("😀"), "head/tail snippet missing: {rendered}");
    assert!(rendered.len() < flooded.len(), "rendered should be bounded");

    // Paste path with multibyte content must track byte ranges without panicking.
    let mut pasted_session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    pasted_session.insert_paste_text(&flooded);
    assert!(pasted_session.input_manager.compact_paste_range().is_some());
    assert_eq!(pasted_session.input_manager.content(), flooded);
}

#[test]
fn paste_with_large_before_context_truncates_far_head() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input(format!("{}NEAR", "A".repeat(500)));
    let pasted = (0..11).map(|i| format!("paste-{i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Pasted Content"), "got: {rendered}");
    assert!(rendered.contains("NEAR"), "adjacent context missing: {rendered}");
    assert!(!rendered.contains(&"A".repeat(300)), "far head must be truncated: {rendered}");
}

#[test]
fn second_paste_expands_to_full_content() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let first = (0..11).map(|i| format!("paste-{i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&first);
    assert!(session.input_compact_mode);

    let rendered = text_content(&session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS).text);
    assert!(rendered.contains("[Pasted Content"), "got: {rendered}");
    assert!(!rendered.contains("paste-0"));

    session.insert_paste_text("more");
    assert!(!session.input_compact_mode);

    // Expanded view is still height-windowed to the last lines (pre-existing
    // `visible_input_window` cap), so assert the visible tail plus no marker.
    let rendered = text_content(&session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS).text);
    assert!(rendered.contains("paste-10"), "got: {rendered}");
    assert!(rendered.contains("more"), "got: {rendered}");
    assert!(!rendered.contains("[Pasted Content"), "got: {rendered}");
    assert_eq!(session.input_manager.content(), format!("{first}more"));
}

#[test]
fn third_paste_collapses_again() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let first = (0..11).map(|i| format!("paste-{i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&first);
    session.insert_paste_text("more");
    assert!(!session.input_compact_mode);

    session.insert_paste_text("again");
    assert!(session.input_compact_mode);

    let rendered = text_content(&session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS).text);
    assert!(rendered.contains("[Pasted Content"), "got: {rendered}");
    assert_eq!(session.input_manager.content(), format!("{first}moreagain"));
}

#[test]
fn backspace_in_expanded_view_deletes_single_char() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let pasted = (0..11).map(|i| format!("paste-{i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);
    assert!(session.input_compact_mode);

    // Simulate click-to-expand: full text visible, cursor at the block end.
    session.input_compact_mode = false;
    session.delete_char();

    let expected = pasted[..pasted.len() - 1].to_string();
    assert_eq!(session.input_manager.content(), expected);
    assert!(session.input_manager.compact_paste_range().is_none());
}

#[test]
fn line_clear_in_expanded_view_clears_only_cursor_line() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let lines: Vec<String> = (0..11).map(|i| format!("paste-{i}")).collect();
    session.insert_paste_text(&lines.join("\n"));
    assert!(session.input_compact_mode);

    // Simulate click-to-expand, then park the cursor inside "paste-5".
    session.input_compact_mode = false;
    let line_start = lines[..5].iter().map(|line| line.len() + 1).sum::<usize>();
    session.input_manager.set_cursor(line_start + 1);
    session.clear_current_line_or_all();

    let mut expected = lines.clone();
    // Line clear removes the line's text but keeps its newline (same as
    // normal multiline editing), leaving an empty line behind.
    expected[5] = String::new();
    assert_eq!(session.input_manager.content(), expected.join("\n"));
}

#[test]
fn paste_with_large_single_line_after_truncates_far_tail() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let pasted = (0..11).map(|i| format!("paste-{i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);
    for ch in format!("NEAR {}", "B".repeat(2000)).chars() {
        session.insert_char(ch);
    }

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Pasted Content"), "got: {rendered}");
    assert!(rendered.contains("NEAR"), "adjacent context missing: {rendered}");
    assert!(!rendered.contains(&"B".repeat(500)), "far tail must be truncated: {rendered}");
    // Full fidelity preserved for submit.
    assert!(session.input_manager.content().contains(&"B".repeat(2000)));
}
