use super::*;

#[test]
fn run_command_preview_uses_head_tail_three_lines() {
    let content = "l1\nl2\nl3\nl4\nl5\nl6\nl7\n";
    let (preview, total, hidden) = collect_run_command_preview(content);
    assert_eq!(total, 7);
    assert_eq!(hidden, 1);
    assert_eq!(preview.as_slice(), ["l1", "l2", "l3", "l5", "l6", "l7"]);
}
#[test]
fn run_command_preview_keeps_short_output_unmodified() {
    let content = "l1\nl2\nl3\n";
    let (preview, total, hidden) = collect_run_command_preview(content);
    assert_eq!(total, 3);
    assert_eq!(hidden, 0);
    assert_eq!(preview.as_slice(), ["l1", "l2", "l3"]);
}
#[test]
fn hidden_lines_notice_preserves_existing_variants() {
    assert_eq!(
        hidden_lines_notice(2, HiddenLinesNoticeKind::CommandPreview),
        "    … +2 lines (/share html for full transcript)"
    );
    assert_eq!(
        strip_ansi_codes(&hidden_lines_notice(2, HiddenLinesNoticeKind::ExecSessionExpand)),
        "… +2 lines · click to expand"
    );
    assert_eq!(hidden_lines_notice(1, HiddenLinesNoticeKind::Generic), "[... 1 line truncated ...]");
    assert_eq!(
        hidden_lines_notice(3, HiddenLinesNoticeKind::TokenBudget),
        "[... content truncated by token budget ...]"
    );
}
#[test]
fn trim_to_tail_keeps_newest_rows() {
    let mut lines: SmallVec<[&str; 32]> = ["l1", "l2", "l3", "l4", "l5"].iter().copied().collect();
    assert!(trim_to_tail(&mut lines, 3));
    assert_eq!(lines.as_slice(), ["l3", "l4", "l5"]);
    // Already within budget: no rows dropped, so callers do not flag truncation.
    assert!(!trim_to_tail(&mut lines, 3));
    assert_eq!(lines.as_slice(), ["l3", "l4", "l5"]);
}
#[test]
fn trim_to_tail_zero_cap_drops_everything() {
    let mut lines: SmallVec<[&str; 32]> = ["l1"].iter().copied().collect();
    assert!(trim_to_tail(&mut lines, 0));
    assert!(lines.is_empty());
    // Empty input with a zero cap is already conformant.
    assert!(!trim_to_tail(&mut lines, 0));
}
#[test]
fn exec_session_output_stays_within_display_budget() {
    assert_eq!(EXEC_SESSION_OUTPUT_MAX_LINES, 10);
    let content = (1..=40).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
    let borrowed = content.lines().collect::<SmallVec<[&str; 32]>>();
    assert_eq!(borrowed.len(), 40);

    let mut lines = borrowed;
    assert!(trim_to_tail(&mut lines, EXEC_SESSION_OUTPUT_MAX_LINES));
    assert_eq!(lines.len(), EXEC_SESSION_OUTPUT_MAX_LINES);
    // Terminal rows (exit status, failures, summary) survive the trim.
    assert_eq!(lines.last().copied(), Some("line 40"));
}
#[test]
fn exec_session_hidden_lines_advertise_expand() {
    // A bounded session body must advertise the in-TUI expand affordance
    // rather than the command-preview share hint.
    let notice = hidden_lines_notice(30, HiddenLinesNoticeKind::ExecSessionExpand);
    assert!(notice.contains("+30 lines"), "got: {notice}");
    assert!(notice.contains("click to expand"), "got: {notice}");
    assert!(!notice.contains("share html"), "session overflow must not push the share hint: {notice}");
    // The action phrase is underlined so TUI hit-region detection can find it.
    assert!(strip_ansi_codes(&notice).contains("click to expand"));
    assert!(notice.contains("\u{1b}["), "underline SGR expected in: {notice:?}");
}
#[test]
fn exec_session_expand_notice_plain_without_underline() {
    // CLI sinks have no hit regions; raw SGR would leak into plain output.
    let notice = hidden_lines_notice_with(2, HiddenLinesNoticeKind::ExecSessionExpand, false);
    assert_eq!(notice, "… +2 lines · click to expand");
    assert!(!notice.contains('\u{1b}'), "no SGR expected in: {notice:?}");
}
#[test]
fn is_exec_session_tool_covers_session_readers_only() {
    assert!(is_exec_session_tool(Some(tool_names::WRITE_STDIN)));
    assert!(is_exec_session_tool(Some(tool_names::SEND_PTY_INPUT)));
    assert!(is_exec_session_tool(Some(tool_names::READ_PTY_SESSION)));
    // Launches are routed through the run-command preview instead.
    assert!(!is_exec_session_tool(Some(tool_names::EXEC_COMMAND)));
    assert!(!is_exec_session_tool(Some(tool_names::RUN_PTY_CMD)));
    assert!(!is_exec_session_tool(Some(tool_names::UNIFIED_EXEC)));
    assert!(!is_exec_session_tool(None));
}
