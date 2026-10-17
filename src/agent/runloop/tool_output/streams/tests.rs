use vtcode_core::ui::InlineHandle;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use anstyle::AnsiColor;
use vtcode_commons::diff_theme::{DiffColorLevel, DiffTheme};
use vtcode_diff::{DiffDisplayKind, DiffDisplayLine, SideBySideRow};

use crate::agent::runloop::tool_output::collect_inline_output;

use super::super::styles::{GitStyles, LsStyles};
use super::{
    EXEC_SESSION_OUTPUT_MAX_LINES, HiddenLinesNoticeKind, MAX_LINE_LENGTH, collect_run_command_preview,
    diff_language_hint_from_content, format_diff_line_with_gutter_and_syntax,
    format_diff_line_with_gutter_and_syntax_to_width, format_side_by_side_row_ansi, hidden_lines_notice,
    hidden_lines_notice_with, highlight_diff_body_with_syntax, highlight_diff_content, is_exec_session_tool,
    language_hint_for_display_line, render_diff_content_block, render_preview_line, render_stream_section,
    select_render_line_style, should_show_diff_gutter, strip_ansi_codes, syntax_segments_for_diff_body, trim_to_tail,
};
use smallvec::SmallVec;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::tools as tool_names;

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

#[tokio::test]
async fn exec_session_body_is_bounded_and_points_at_the_transcript() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let content = (1..=40).map(|n| format!("build log line {n}")).collect::<Vec<_>>().join("\n");

    let git_styles = GitStyles::new();
    let ls_styles = LsStyles::from_env();

    render_stream_section(
        &mut renderer,
        "",
        &content,
        ToolOutputMode::Compact,
        30,
        Some(tool_names::WRITE_STDIN),
        &git_styles,
        &ls_styles,
        MessageStyle::ToolOutput,
        false,
        true,
        None,
    )
    .await
    .expect("session body should render");

    let collected = collect_inline_output(&mut receiver);
    let output = strip_ansi_codes(&collected);
    let content_lines = output.lines().filter(|line| line.contains("build log line")).count();
    assert_eq!(
        content_lines, EXEC_SESSION_OUTPUT_MAX_LINES,
        "session body must be capped at {EXEC_SESSION_OUTPUT_MAX_LINES} rows: {output:?}"
    );
    // Tail-biased: the exit/summary rows survive the trim.
    assert!(output.contains("build log line 40"), "tail should survive: {output:?}");
    assert!(!output.contains("build log line 1\n"), "head should be trimmed: {output:?}");
    // The hidden-row count and the expand affordance stay discoverable.
    assert!(output.contains("+30 lines"), "hidden count should be shown: {output:?}");
    assert!(output.contains("click to expand"), "expand affordance should be shown: {output:?}");
}

#[tokio::test]
async fn exec_session_body_keeps_short_output_whole() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    render_stream_section(
        &mut renderer,
        "",
        "line 1\nline 2\nline 3",
        ToolOutputMode::Compact,
        30,
        Some(tool_names::WRITE_STDIN),
        &GitStyles::new(),
        &LsStyles::from_env(),
        MessageStyle::ToolOutput,
        false,
        true,
        None,
    )
    .await
    .expect("short session body should render");

    let collected = collect_inline_output(&mut receiver);
    let output = strip_ansi_codes(&collected);
    assert!(output.contains("line 1") && output.contains("line 2") && output.contains("line 3"));
    assert!(!output.contains("truncated") && !output.contains("lines ("), "no notice expected: {output:?}");
}

#[tokio::test]
async fn exec_session_body_and_notice_are_dimmed() {
    use anstyle::Effects;
    use vtcode_core::ui::InlineCommand;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let content = (1..=40).map(|n| format!("build log line {n}")).collect::<Vec<_>>().join("\n");

    render_stream_section(
        &mut renderer,
        "",
        &content,
        ToolOutputMode::Compact,
        30,
        Some(tool_names::WRITE_STDIN),
        &GitStyles::new(),
        &LsStyles::from_env(),
        MessageStyle::ToolOutput,
        false,
        true,
        None,
    )
    .await
    .expect("session body should render");

    let mut body_segment = None;
    let mut notice_segment = None;
    while let Ok(command) = receiver.try_recv() {
        let segments = match command {
            InlineCommand::AppendLine { segments, .. } | InlineCommand::AppendToolOutputLine { segments, .. } => {
                segments
            }
            _ => continue,
        };
        let text = segments.iter().map(|s| s.text.as_str()).collect::<String>();
        if body_segment.is_none() && text.contains("build log line") {
            body_segment = segments.first().cloned();
        }
        if notice_segment.is_none() && text.contains("click to expand") {
            // The underlined action phrase is its own segment.
            notice_segment = segments.into_iter().find(|s| s.text.contains("click to expand"));
        }
    }

    let body = body_segment.expect("session body segment");
    assert!(body.style.effects.contains(Effects::DIMMED), "session body must be dimmed: {:?}", body.style);
    let notice = notice_segment.expect("expand notice segment");
    assert!(
        notice.style.effects.contains(Effects::UNDERLINE),
        "expand action must be underlined as the click target: {:?}",
        notice.style
    );
}

#[tokio::test]
async fn exec_session_expand_notice_binds_tool_output_id() {
    use vtcode_core::ui::InlineCommand;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_session_expand_anchor(42);
    let content = (1..=40).map(|n| format!("build log line {n}")).collect::<Vec<_>>().join("\n");

    render_stream_section(
        &mut renderer,
        "",
        &content,
        ToolOutputMode::Compact,
        30,
        Some(tool_names::WRITE_STDIN),
        &GitStyles::new(),
        &LsStyles::from_env(),
        MessageStyle::ToolOutput,
        false,
        true,
        None,
    )
    .await
    .expect("session body should render");

    let mut bound_notice_id = None;
    while let Ok(command) = receiver.try_recv() {
        if let InlineCommand::AppendToolOutputLine { id, segments, .. } = command {
            let text = segments.iter().map(|s| s.text.as_str()).collect::<String>();
            if text.contains("click to expand") {
                bound_notice_id = Some(id);
            }
        }
    }
    assert_eq!(
        bound_notice_id,
        Some(42),
        "expand notice must carry the recorded capture id so click opens the viewer"
    );
}

#[tokio::test]
async fn exec_session_body_skips_ls_and_diff_coloring() {
    // Build-test lines mentioning `.rs` must not pick up LS_COLORS file-type
    // colors, and a diff-shaped session capture stays plain: the body is
    // terminal text, not an `ls` listing or a tool diff.
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let content = "PASS  [0.013s] vtcode-core/src/tools/exec_session.rs\n+let added = 1;\n-let removed = 2;\n";

    render_stream_section(
        &mut renderer,
        "",
        content,
        ToolOutputMode::Compact,
        30,
        Some(tool_names::WRITE_STDIN),
        &GitStyles::new(),
        &LsStyles::from_env(),
        MessageStyle::ToolOutput,
        false,
        true,
        None,
    )
    .await
    .expect("session body should render");

    // Styles live on the emitted segments; `collect_inline_output` drops them,
    // so read the inline commands directly. Both the line text and its indent
    // prefix carry the applied style, so per-line coloring changes either.
    let session_styles = collect_inline_line_styles(&mut receiver).await;
    let unique: std::collections::HashSet<_> = session_styles.iter().cloned().collect();
    assert_eq!(
        unique.len(),
        1,
        "session body must use one plain style, not per-line git/ls coloring: {session_styles:?}"
    );

    // Control: the same content rendered as a real diff body does vary per
    // line, so the assertion above cannot pass vacuously.
    let control_styles = collect_diff_body_styles(content).await;
    assert!(
        control_styles.iter().collect::<std::collections::HashSet<_>>().len() > 1,
        "control must show per-line coloring for the assertion to mean anything: {control_styles:?}"
    );
}

/// One style key per rendered body line, covering every segment on that line
/// (indent prefix plus text). Text alone cannot prove styling, and per-line
/// coloring shows up on the indent prefix even when the text stays uncolored.
async fn collect_inline_line_styles(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<vtcode_core::ui::InlineCommand>,
) -> Vec<String> {
    let mut lines = Vec::new();
    while let Ok(command) = receiver.try_recv() {
        if let vtcode_core::ui::InlineCommand::AppendLine { segments, .. } = command {
            let key = segments
                .iter()
                .filter(|segment| !segment.text.is_empty())
                .map(|segment| style_key(segment.style.as_ref()))
                .collect::<Vec<_>>()
                .join("|");
            if !key.is_empty() {
                lines.push(key);
            }
        }
    }
    lines
}

/// The same content rendered through the styled diff path, proving per-line
/// coloring is observable through this seam when it is applied.
async fn collect_diff_body_styles(content: &str) -> Vec<String> {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    render_stream_section(
        &mut renderer,
        "",
        content,
        ToolOutputMode::Compact,
        30,
        Some(vtcode_core::config::constants::tools::APPLY_PATCH),
        &GitStyles::new(),
        &LsStyles::from_env(),
        MessageStyle::ToolDetail,
        false,
        true,
        None,
    )
    .await
    .expect("control diff body should render");
    collect_inline_line_styles(&mut receiver).await
}

fn style_key(style: &vtcode_commons::ui_protocol::InlineTextStyle) -> String {
    format!("{:?}/{:?}", style.color, style.effects)
}

#[test]
fn render_preview_line_truncates_and_prefixes() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let line = "x".repeat(MAX_LINE_LENGTH + 10);

    render_preview_line(&mut renderer, &line, None, Some("  "), true, MessageStyle::ToolOutput, None)
        .expect("preview line should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.starts_with("  "));
    assert!(inline_output.ends_with("..."));
}

#[test]
fn diff_stream_preview_keeps_head_tail_and_omission_marker() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let mut diff = String::from("@@ -1,600 +1,600 @@\n");
    for index in 0..600 {
        diff.push_str(&format!("-old-{index}\n+new-{index}\n"));
    }

    render_diff_content_block(
        &mut renderer,
        &diff,
        Some("apply_patch"),
        &GitStyles::new(),
        &LsStyles::from_env(),
        MessageStyle::ToolDetail,
        ToolOutputMode::Compact,
        5,
    )
    .expect("bounded diff should render");

    let collected = collect_inline_output(&mut receiver);
    let output = strip_ansi_codes(&collected);
    assert!(output.contains("old-0"), "head should be retained: {output:?}");
    assert!(output.contains("new-599"), "tail should be retained: {output:?}");
    assert!(output.contains("review full diff"), "vertical omission must advertise expand: {output:?}");
    assert!(
        output.contains("lines") && (output.contains("omitted") || output.contains('+')),
        "omission count should remain discoverable: {output:?}"
    );
}

#[tokio::test]
async fn safety_capped_diff_body_advertises_expand_and_records_anchor() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    // One logical addition row far above DIFF_WRAP_SOURCE_MAX_WIDTH (2000).
    let long_row = format!("+{}", "x".repeat(2_050));
    let diff = format!("@@ -1,1 +1,1 @@\n{long_row}\n");

    render_diff_content_block(
        &mut renderer,
        &diff,
        Some("apply_patch"),
        &GitStyles::new(),
        &LsStyles::from_env(),
        MessageStyle::ToolDetail,
        ToolOutputMode::Full,
        50,
    )
    .expect("safety-capped diff should render");

    let collected = collect_inline_output(&mut receiver);
    let output = strip_ansi_codes(&collected);
    assert!(output.contains("review full diff"), "safety-cap truncation must advertise expand: {output:?}");
    assert!(
        output.contains("RecordDiffReview") || output.contains("diff truncated"),
        "safety-cap path must record an expand payload: {output:?}"
    );
}

fn test_diff_line(kind: DiffDisplayKind, old_line: Option<u32>, new_line: Option<u32>, text: &str) -> DiffDisplayLine {
    DiffDisplayLine {
        kind,
        old_line,
        new_line,
        text: text.to_string(),
        changed: Vec::new(),
    }
}

#[test]
fn format_diff_line_styles_gutter_for_additions() {
    let style = anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green)));
    let mut buf = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::Addition, None, Some(1377), "let x = 1;"),
        Some(style),
        5,
        None,
        &GitStyles::new(),
        &mut buf,
    );
    assert!(rendered.contains("\u{1b}["));
    let stripped = strip_ansi_codes(rendered);
    // Blank old column (5) + ` │ ` + ` 1377` + ` │ ` + `+ content`.
    assert!(stripped.contains("+ 1377 │ let x = 1;"), "got: {stripped:?}");
}

#[test]
fn format_diff_line_preserves_code_indentation() {
    let mut buf = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::Addition, None, Some(1384), "    line,"),
        None,
        5,
        None,
        &GitStyles::new(),
        &mut buf,
    );
    let stripped = strip_ansi_codes(rendered);
    assert!(stripped.contains("+ 1384 │     line,"));
}

#[test]
fn format_diff_line_keeps_blank_line_spacing() {
    let mut buf = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::Addition, None, Some(42), ""),
        None,
        5,
        None,
        &GitStyles::new(),
        &mut buf,
    );
    let stripped = strip_ansi_codes(rendered);
    assert!(stripped.contains("+   42 │ "), "got: {stripped:?}");
}

#[test]
fn format_diff_line_clears_reused_buffer_for_metadata() {
    let mut buf = String::new();
    let _ = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::Addition, None, Some(1), "let x = 1;"),
        None,
        5,
        None,
        &GitStyles::new(),
        &mut buf,
    );
    let rendered = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::Metadata, None, None, "diff --git a/src/lib.rs b/src/lib.rs"),
        None,
        5,
        None,
        &GitStyles::new(),
        &mut buf,
    );

    assert_eq!(strip_ansi_codes(rendered), "diff --git a/src/lib.rs b/src/lib.rs");
}

#[test]
fn format_diff_line_keeps_markdown_bullet_distinct_from_marker() {
    let mut buf = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::Addition, None, Some(53), "- **Agent-first by design*: prose"),
        None,
        5,
        None,
        &GitStyles::new(),
        &mut buf,
    );
    let stripped = strip_ansi_codes(rendered);
    assert!(stripped.contains("+   53 │ - **Agent-first"), "got: {stripped:?}");
}

#[test]
fn format_diff_line_truncates_long_addition() {
    let mut buf = String::new();
    let long_text = "y".repeat(MAX_LINE_LENGTH * 2);
    // Single gutter: sign(1) + number(5) + " │ "(3) = 9.
    let gutter_width = 9;
    let mut line = test_diff_line(DiffDisplayKind::Addition, None, Some(9), &long_text);
    // Simulate a chip that would be invalid after truncation.
    line.changed = vec![(0, long_text.len())];
    let word_bg = Some(anstyle::Color::Rgb(anstyle::RgbColor(36, 100, 70)));
    let rendered = format_diff_line_with_gutter_and_syntax(&line, None, 5, word_bg, &GitStyles::new(), &mut buf);
    let stripped = strip_ansi_codes(rendered);
    assert!(
        vtcode_commons::preview::display_width(&stripped) <= MAX_LINE_LENGTH + gutter_width,
        "rendered diff line must not exceed MAX_LINE_LENGTH + gutter width"
    );
    assert!(stripped.contains("..."));
    // Truncated rows must not paint word chips with stale offsets.
    assert!(!rendered.contains("48;2;36;100;70"), "no chip on truncated line");
}

#[test]
fn diff_bodies_stay_solid_no_syntax_brightness() {
    let bg = Some(anstyle::Color::Rgb(anstyle::RgbColor(20, 58, 45)));
    let rendered = highlight_diff_content("- **bold** and `code`", bg, &[], None).expect("solid body");
    // Solid tint only: no bright syntax SGR, no bold, no extra fg.
    assert!(rendered.contains("48;2;20;58;45"));
    assert!(!rendered.contains("38;2;"));
    assert!(!rendered.contains("\u{1b}[1m"));
    assert!(!rendered.contains("\u{1b}[91m"));
    assert!(!rendered.contains("\u{1b}[92m"));
    assert!(highlight_diff_content("plain", None, &[], None).is_none());
}

#[test]
fn diff_language_hint_infers_extension_from_headers() {
    let diff = "diff --git a/src/main.rs b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n";
    assert_eq!(diff_language_hint_from_content(diff).as_deref(), Some("rs"));
    let markers = "--- a/app.ts\n+++ b/app.ts\n@@ -1 +1 @@\n-old\n+new\n";
    assert_eq!(diff_language_hint_from_content(markers).as_deref(), Some("ts"));
    assert_eq!(diff_language_hint_from_content("@@ -1 +1 @@\n-old\n+new\n"), None);
    let patch = "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old\n+new\n";
    assert_eq!(diff_language_hint_from_content(patch).as_deref(), Some("rs"));
    let quoted = "diff --git \"a/my file.rs\" \"b/my file.rs\"\n@@ -1 +1 @@\n-old\n+new\n";
    assert_eq!(diff_language_hint_from_content(quoted).as_deref(), Some("rs"));
}

#[test]
fn per_file_language_tracking_switches_grammars() {
    let git_line = DiffDisplayLine::body(
        DiffDisplayKind::Metadata,
        None,
        None,
        "diff --git a/src/main.rs b/src/main.rs".to_string(),
    );
    assert_eq!(language_hint_for_display_line(&git_line).as_deref(), Some("rs"));
    let marker_line = DiffDisplayLine::body(DiffDisplayKind::Metadata, None, None, "+++ b/app.ts".to_string());
    assert_eq!(language_hint_for_display_line(&marker_line).as_deref(), Some("ts"));
    let hunk = DiffDisplayLine::body(DiffDisplayKind::HunkHeader, None, None, "@@ -1 +1 @@".to_string());
    assert_eq!(language_hint_for_display_line(&hunk), None);
}

#[test]
fn syntax_segments_skip_prose_and_plain_but_keep_code() {
    assert!(syntax_segments_for_diff_body("fn main() {}", Some("rs")).is_some());
    assert!(syntax_segments_for_diff_body("hello", Some("md")).is_none());
    assert!(syntax_segments_for_diff_body("hello", Some("txt")).is_none());
    assert!(syntax_segments_for_diff_body("hello", None).is_none());
    assert!(syntax_segments_for_diff_body("", Some("rs")).is_none());
}

#[test]
fn rust_diff_body_layers_syntax_over_row_tint() {
    let bg = Some(anstyle::Color::Rgb(anstyle::RgbColor(20, 58, 45)));
    let word_bg = Some(anstyle::Color::Rgb(anstyle::RgbColor(36, 100, 70)));
    let content = "fn main() { let value = 2; }";
    let rendered = highlight_diff_body_with_syntax(content, Some("rs"), bg, &[], word_bg, None).expect("syntax body");
    assert!(rendered.contains("48;2;20;58;45"), "row tint must survive syntax: {rendered:?}");
    assert!(rendered.contains("38;2;"), "syntax foreground must layer over tint: {rendered:?}");
    // ANSI16/no-color has no tint, so syntax must fall back to solid handling.
    assert!(highlight_diff_body_with_syntax(content, Some("rs"), None, &[], word_bg, None).is_none());
    assert!(highlight_diff_body_with_syntax(content, Some("md"), bg, &[], word_bg, None).is_none());
}

#[test]
fn word_chips_apply_stronger_bg_on_changed_spans() {
    let content = "let a = 1;";
    let word_bg = Some(anstyle::Color::Rgb(anstyle::RgbColor(36, 100, 70)));
    let line_bg = Some(anstyle::Color::Rgb(anstyle::RgbColor(20, 58, 45)));
    let rendered = highlight_diff_content(content, line_bg, &[(8, 9)], word_bg).expect("rendered");
    assert!(rendered.contains("48;2;36;100;70"));
    assert!(rendered.contains("48;2;20;58;45"));
    // Default fg on both spans: no bright syntax leak.
    assert!(!rendered.contains("38;2;"));
}

#[test]
fn inline_diff_uses_shared_accessible_marker_gutter_and_two_level_palette() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let mut line = test_diff_line(DiffDisplayKind::Addition, None, Some(7), "let value = 2;");
    line.changed = vec![(4, 9)];
    let mut buffer = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax(
        &line,
        git.add,
        3,
        git.add_word.and_then(|style| style.get_bg_color()),
        &git,
        &mut buffer,
    )
    .to_owned();

    assert!(rendered.contains("48;2;20;58;45"), "row tint missing: {rendered:?}");
    assert!(rendered.contains("48;2;36;100;70"), "word tint missing: {rendered:?}");
    assert!(rendered.contains("38;2;85;255;85"), "accessible addition marker missing: {rendered:?}");
    assert!(rendered.contains("38;2;165;175;170"), "accessible gutter missing: {rendered:?}");
}

#[test]
fn inline_diff_row_tint_reaches_the_requested_width() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let line = test_diff_line(DiffDisplayKind::Addition, None, Some(7), "let value = 2;");
    let mut buffer = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax_to_width(
        &line,
        git.add,
        3,
        git.add_word.and_then(|style| style.get_bg_color()),
        &git,
        Some(48),
        true,
        None,
        false,
        &mut buffer,
    );
    let stripped = strip_ansi_codes(rendered);

    assert_eq!(vtcode_commons::preview::display_width(&stripped), 48);
    assert!(rendered.contains("48;2;20;58;45"), "row tint missing: {rendered:?}");
    assert!(rendered.ends_with("\x1b[0m"), "padded row must reset cleanly: {rendered:?}");
}

#[test]
fn compact_diff_row_drops_the_visible_gutter_but_keeps_semantic_styling() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let mut line = test_diff_line(DiffDisplayKind::Addition, None, Some(7), "changed");
    line.changed = vec![(0, line.text.len())];
    let mut buffer = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax_to_width(
        &line,
        git.add,
        5,
        git.add_word.and_then(|style| style.get_bg_color()),
        &git,
        Some(28),
        false,
        None,
        false,
        &mut buffer,
    );
    let stripped = strip_ansi_codes(rendered);

    assert_eq!(vtcode_commons::preview::display_width(&stripped), 28);
    assert!(stripped.starts_with(' '));
    assert!(!stripped.contains('+'));
    assert!(!stripped.contains('│'));
    assert!(!stripped.contains('7'));
    assert!(rendered.contains("48;2;20;58;45"), "compact row tint missing: {rendered:?}");
    assert!(rendered.contains("38;2;85;255;85"), "compact semantic foreground missing: {rendered:?}");
    assert!(rendered.contains("48;2;36;100;70"), "compact word tint missing: {rendered:?}");
}

#[test]
fn compact_diff_row_truncates_before_applying_color() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let line = test_diff_line(DiffDisplayKind::Addition, None, Some(7), &"changed ".repeat(20));
    let mut buffer = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax_to_width(
        &line,
        git.add,
        5,
        git.add_word.and_then(|style| style.get_bg_color()),
        &git,
        Some(28),
        false,
        None,
        false,
        &mut buffer,
    );
    let stripped = strip_ansi_codes(rendered);

    assert_eq!(vtcode_commons::preview::display_width(&stripped), 28);
    assert!(rendered.contains("48;2;20;58;45"), "truncated row tint missing: {rendered:?}");
    assert!(rendered.contains("38;2;85;255;85"), "compact semantic foreground missing: {rendered:?}");
}

#[test]
fn compact_context_content_is_not_reclassified_as_a_deletion() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let ls = LsStyles::from_env();
    let line = test_diff_line(DiffDisplayKind::Context, Some(4), Some(4), "- bullet item");

    assert_eq!(
        select_render_line_style(&line, " - bullet item", Some("run_pty_cmd"), &git, &ls),
        None,
        "compact context rows must keep source-leading markers as content"
    );
}

#[test]
fn foreground_only_diff_rows_keep_their_visible_gutter_when_narrow() {
    assert!(should_show_diff_gutter(true, false, Some(1), 5));
    assert!(!should_show_diff_gutter(true, true, Some(28), 5));
    assert!(should_show_diff_gutter(true, true, Some(29), 5));
}

#[test]
fn narrow_inline_diff_rows_emit_full_logical_bodies_for_reflow() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_diff_preview_mode(vtcode_commons::ui_protocol::DiffPreviewMode::Inline);
    renderer.set_table_max_width(Some(28));
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let marker = "old ".repeat(30);
    let diff = format!("@@ -1 +1 @@\n-{}\n+{}\n", marker, "new ".repeat(30));

    render_diff_content_block(
        &mut renderer,
        &diff,
        Some("apply_patch"),
        &git,
        &LsStyles::from_env(),
        MessageStyle::ToolDetail,
        ToolOutputMode::Full,
        100,
    )
    .expect("narrow diff should render");

    let collected = collect_inline_output(&mut receiver);
    let output = strip_ansi_codes(&collected);
    assert!(!output.contains("..."), "inline TUI diff rows must not ellipsis-truncate: {output:?}");
    assert!(
        output.contains(marker.trim_end()),
        "logical body must reach the transcript so reflow can wrap it: {output:?}"
    );
}

#[test]
fn wrap_for_reflow_keeps_long_diff_bodies_whole() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let long_text = format!("| {} | {} |", "What can go wrong", "How VT Code responds".repeat(4));
    let line = test_diff_line(DiffDisplayKind::Addition, None, Some(83), &long_text);
    let mut buffer = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax_to_width(
        &line,
        git.add,
        3,
        git.add_word.and_then(|style| style.get_bg_color()),
        &git,
        Some(28),
        true,
        None,
        true,
        &mut buffer,
    );
    let stripped = strip_ansi_codes(rendered);
    assert!(!stripped.contains("..."), "wrap mode must not ellipsize: {stripped:?}");
    assert!(stripped.contains("How VT Code responds"), "body must stay whole: {stripped:?}");
    assert!(
        vtcode_commons::preview::display_width(&stripped) > 28,
        "wrap mode logical row may exceed measured width: {stripped:?}"
    );
}

#[test]
fn side_by_side_diff_falls_back_narrow_and_recovers_wide() {
    fn render(mode: vtcode_commons::ui_protocol::DiffPreviewMode, width: usize) -> String {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        renderer.set_diff_preview_mode(mode);
        renderer.set_table_max_width(Some(width));
        let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
        render_diff_content_block(
            &mut renderer,
            "--- a/file.rs\n+++ b/file.rs\n@@ -1 +1 @@\n-old value\n+new value\n",
            Some("apply_patch"),
            &git,
            &LsStyles::from_env(),
            MessageStyle::ToolDetail,
            ToolOutputMode::Full,
            100,
        )
        .expect("diff preview should render");
        strip_ansi_codes(&collect_inline_output(&mut receiver)).into_owned()
    }

    assert_eq!(
        render(vtcode_commons::ui_protocol::DiffPreviewMode::SideBySide, 50),
        render(vtcode_commons::ui_protocol::DiffPreviewMode::Inline, 50),
        "narrow side-by-side should use the inline renderer"
    );
    assert_ne!(
        render(vtcode_commons::ui_protocol::DiffPreviewMode::SideBySide, 80),
        render(vtcode_commons::ui_protocol::DiffPreviewMode::Inline, 80),
        "wide side-by-side should recover the configured layout"
    );
}

#[test]
fn wide_side_by_side_rows_are_not_clipped_to_inline_preview_limit() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_diff_preview_mode(vtcode_commons::ui_protocol::DiffPreviewMode::SideBySide);
    renderer.set_table_max_width(Some(200));
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let old = format!("-{}\n", "old ".repeat(40));
    let new = format!("+{}\n", "new ".repeat(40));
    let diff = format!("@@ -1 +1 @@\n{old}{new}");

    render_diff_content_block(
        &mut renderer,
        &diff,
        Some("apply_patch"),
        &git,
        &LsStyles::from_env(),
        MessageStyle::ToolDetail,
        ToolOutputMode::Full,
        100,
    )
    .expect("wide side-by-side diff should render");

    let collected = collect_inline_output(&mut receiver);
    let output = strip_ansi_codes(&collected);
    assert!(
        output
            .lines()
            .any(|line| vtcode_commons::preview::display_width(line) > MAX_LINE_LENGTH),
        "source panes should use the measured width instead of the inline cap: {output:?}"
    );
}

#[test]
fn side_by_side_metadata_emits_full_paths_for_reflow() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_diff_preview_mode(vtcode_commons::ui_protocol::DiffPreviewMode::SideBySide);
    renderer.set_table_max_width(Some(80));
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let path = "very-long-directory-name/another-long-directory-name/source-file.rs";
    let diff = format!("diff --git a/{path} b/{path}\n@@ -1 +1 @@\n-old\n+new\n");

    render_diff_content_block(
        &mut renderer,
        &diff,
        Some("apply_patch"),
        &git,
        &LsStyles::from_env(),
        MessageStyle::ToolDetail,
        ToolOutputMode::Full,
        100,
    )
    .expect("side-by-side metadata should render");

    let collected = collect_inline_output(&mut receiver);
    let output = strip_ansi_codes(&collected);
    assert!(output.contains(path), "inline TUI metadata must keep the full path for reflow wrap: {output:?}");
    assert!(
        !output.contains("..."),
        "inline TUI metadata must not ellipsis-truncate under the safety cap: {output:?}"
    );
}

#[test]
fn diff_section_headers_are_foreground_only_and_highlighted() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let mut buffer = String::new();

    let file_header = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::Metadata, None, None, "--- a/README.md"),
        git.file_old,
        3,
        None,
        &git,
        &mut buffer,
    )
    .to_owned();
    assert!(!file_header.contains("48;"), "file header must not have a background: {file_header:?}");
    assert!(file_header.contains("38;2;255;180;180"), "file header foreground missing: {file_header:?}");

    let hunk_header = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::HunkHeader, None, None, "@@ -1 +1 @@"),
        git.hunk,
        3,
        None,
        &git,
        &mut buffer,
    )
    .to_owned();
    assert!(!hunk_header.contains("48;"), "hunk header must not have a background: {hunk_header:?}");
    assert!(hunk_header.contains("36m"), "hunk header foreground missing: {hunk_header:?}");
}

#[test]
fn side_by_side_diff_resets_empty_pane_and_keeps_tints_isolated() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let mut deleted = test_diff_line(DiffDisplayKind::Deletion, Some(3), None, "old value");
    deleted.changed = vec![(4, 9)];
    let mut added = test_diff_line(DiffDisplayKind::Addition, None, Some(3), "new value");
    added.changed = vec![(4, 9)];

    let mut buffer = String::new();
    let left_only = format_side_by_side_row_ansi(
        &SideBySideRow { left: Some(deleted), right: None },
        2,
        24,
        &git,
        None,
        &mut buffer,
    )
    .to_owned();
    assert!(left_only.contains("48;2;70;38;42"));
    assert!(left_only.contains("48;2;140;52;58"));
    assert!(!left_only.contains("48;2;20;58;45"));
    assert!(left_only.contains("\x1b[0m\x1b[49m"), "empty right pane must reset its background");

    let right_only =
        format_side_by_side_row_ansi(&SideBySideRow { left: None, right: Some(added) }, 2, 24, &git, None, &mut buffer)
            .to_owned();
    assert!(right_only.contains("48;2;20;58;45"));
    assert!(right_only.contains("48;2;36;100;70"));
    assert!(!right_only.contains("48;2;70;38;42"));
    assert!(right_only.starts_with("\x1b[0m\x1b[49m"), "empty left pane must reset its background");
}

#[test]
fn ansi16_side_by_side_diff_stays_foreground_only() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::Ansi16);
    let added = test_diff_line(DiffDisplayKind::Addition, None, Some(3), "new value");
    let mut buffer = String::new();
    let rendered =
        format_side_by_side_row_ansi(&SideBySideRow { left: None, right: Some(added) }, 2, 24, &git, None, &mut buffer);

    assert!(!rendered.contains("48;"), "ANSI16 must not paint a background: {rendered:?}");
    assert!(!rendered.contains("49m"), "ANSI16 must not emit background resets: {rendered:?}");
    assert!(rendered.contains("\x1b[92m"), "ANSI16 foreground should remain visible: {rendered:?}");
}
