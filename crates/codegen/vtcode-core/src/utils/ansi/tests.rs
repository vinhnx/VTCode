use super::links::current_file_opener;
use super::sink::INLINE_JSON_COLLAPSE_LINES;
use super::*;
use crate::ui::tui::{InlineHandle, InlineMessageKind, InlineSegment, InlineTextStyle};
use anstyle::{AnsiColor, Color as AnsiColorEnum, Effects, RgbColor};
use anyhow::Context as _;
use anyhow::Result;
use std::sync::{LazyLock, Mutex};
use unicode_width::UnicodeWidthStr;
use url::Url;

static FILE_OPENER_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn lock_file_opener_test_guard() -> std::sync::MutexGuard<'static, ()> {
    match FILE_OPENER_TEST_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[test]
fn test_styles_construct() {
    let info = MessageStyle::Info.style();
    assert_eq!(info, MessageStyle::Info.style());
    let resp = MessageStyle::Response.style();
    assert_eq!(resp, MessageStyle::Response.style());
    let tool = MessageStyle::Tool.style();
    assert_eq!(tool, MessageStyle::Tool.style());
    let reasoning = MessageStyle::Reasoning.style();
    assert_eq!(reasoning, MessageStyle::Reasoning.style());
}

/// `is_compact_display` is the single compact-vs-expanded predicate: it must
/// agree with the raw mode comparison and normalize `Unknown` to expanded.
#[test]
fn is_compact_display_matches_normalized_mode() {
    let mut renderer = AnsiRenderer::stdout();
    for (mode, expected) in [
        (ToolDisplayMode::Compact, true),
        (ToolDisplayMode::Expanded, false),
        (ToolDisplayMode::Unknown, false),
    ] {
        renderer.set_tool_display_mode(mode);
        assert_eq!(renderer.is_compact_display(), expected, "mode {mode:?}");
        assert_eq!(renderer.is_compact_display(), renderer.tool_display_mode() == ToolDisplayMode::Compact);
    }
}

#[test]
fn test_renderer_buffer() {
    let mut r = AnsiRenderer::stdout();
    r.push("hello");
    assert_eq!(r.buffer, "hello");
}

#[test]
fn convert_plain_lines_preserves_ansi_styles() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let sink = InlineSink::new(InlineHandle::new_for_tests(sender), SyntaxHighlightingConfig::default());
    let fallback = InlineTextStyle {
        color: Some(AnsiColorEnum::Ansi(AnsiColor::Green)),
        bg_color: None,
        effects: Effects::new(),
    };

    let (converted, plain) = sink.convert_plain_lines("\u{1b}[31mred\u{1b}[0m plain", &fallback);

    assert_eq!(plain, vec!["red plain".to_owned()]);
    assert_eq!(converted.len(), 1);
    let segments = &converted[0];
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].text, "red");
    assert_eq!(segments[0].style.color, Some(AnsiColorEnum::Ansi(AnsiColor::Red)));
    assert_eq!(segments[1].text, " plain");
    assert_eq!(segments[1].style.color, None);
}

#[test]
fn convert_plain_lines_maps_underline_and_dim_effects() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let sink = InlineSink::new(InlineHandle::new_for_tests(sender), SyntaxHighlightingConfig::default());
    let fallback = InlineTextStyle {
        color: None,
        bg_color: None,
        effects: Effects::DIMMED,
    };

    // Expand notices underline their click target with SGR 4; the effect
    // must survive into the segment or TUI hit-region detection (which
    // scans for UNDERLINED) cannot see it.
    let (converted, _plain) = sink.convert_plain_lines("… +2 lines · \u{1b}[4mclick to expand\u{1b}[0m", &fallback);
    let segments = &converted[0];
    let action = segments
        .iter()
        .find(|segment| segment.text.contains("click to expand"))
        .expect("action segment");
    assert!(
        action.style.effects.contains(Effects::UNDERLINE),
        "underline must survive ANSI→segment conversion: {:?}",
        action.style
    );
    assert!(
        action.style.effects.contains(Effects::DIMMED),
        "fallback dim must remain on the action: {:?}",
        action.style
    );
}

#[test]
fn convert_plain_lines_preserves_explicit_backgrounds() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let sink = InlineSink::new(InlineHandle::new_for_tests(sender), SyntaxHighlightingConfig::default());
    let fallback = InlineTextStyle {
        color: Some(AnsiColorEnum::Ansi(AnsiColor::Green)),
        bg_color: None,
        effects: Effects::new(),
    };

    // Diff rows arrive as ANSI with an explicit tinted bg (e.g. 48;2;…).
    // Dropping it paints half-tinted rows in the transcript.
    let (converted, plain) =
        sink.convert_plain_lines("\u{1b}[1m\u{1b}[91m\u{1b}[48;2;70;38;42m- old\u{1b}[0m tail", &fallback);

    assert_eq!(plain, vec!["- old tail".to_owned()]);
    let segments = &converted[0];
    assert_eq!(segments[0].text, "- old");
    assert_eq!(
        segments[0].style.bg_color,
        Some(AnsiColorEnum::Rgb(RgbColor(70, 38, 42))),
        "explicit ANSI background must survive into transcript segments"
    );
    assert_eq!(segments[1].style.bg_color, None);
}

#[test]
fn convert_plain_lines_retains_trailing_newline() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let sink = InlineSink::new(InlineHandle::new_for_tests(sender), SyntaxHighlightingConfig::default());
    let fallback = InlineTextStyle::default();

    let (converted, plain) = sink.convert_plain_lines("hello\n", &fallback);

    assert_eq!(plain, vec!["hello".to_owned(), String::new()]);
    assert_eq!(converted.len(), 2);
    assert!(!converted[0].is_empty());
    assert!(converted[1].is_empty());
}

#[test]
fn write_multiline_combines_tool_lines() {
    use crate::ui::InlineCommand;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut sink = InlineSink::new(InlineHandle::new_for_tests(sender), SyntaxHighlightingConfig::default());
    let style = InlineTextStyle::default();
    // Use Tool kind to verify that multiple lines are combined into a single AppendLine command
    let kind = InlineMessageKind::Tool;
    let text = "one\ntwo\nthree";
    sink.write_multiline(style.to_ansi_style(None), "", text, kind).unwrap();

    // We should receive exactly one AppendLine command
    let mut count = 0;
    while let Ok(command) = receiver.try_recv() {
        if let InlineCommand::AppendLine { .. } = command {
            count += 1;
        }
    }
    assert_eq!(count, 1);
}

#[test]
fn prepare_markdown_lines_uses_syntax_highlighting_config() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let config = SyntaxHighlightingConfig {
        enabled: true,
        enabled_languages: vec!["rust".to_string()],
        ..Default::default()
    };
    let sink = InlineSink::new(InlineHandle::new_for_tests(sender), config);
    let base_style = MessageStyle::Response.style();
    let markdown = "```rust\nlet value = 1;\n```";

    let (prepared, plain, _) = sink.prepare_markdown_lines(markdown, "", base_style, true, false);

    let (segments, plain_line) = prepared
        .iter()
        .zip(plain.iter())
        .find(|(_, line)| line.contains("let value = 1;"))
        .expect("code line exists");

    assert!(segments.len() > 2, "expected highlighted segments, got {}, line: {}", segments.len(), plain_line);
}

#[test]
fn prepare_markdown_lines_strips_local_path_underlines() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let sink = InlineSink::new(InlineHandle::new_for_tests(sender), SyntaxHighlightingConfig::default());
    let base_style = MessageStyle::Response.style();
    let markdown = "See README.md for details.";

    let (prepared, _, _) = sink.prepare_markdown_lines(markdown, "", base_style, true, false);
    let readme_segment = prepared
        .iter()
        .flat_map(|line| line.iter())
        .find(|segment| segment.text.contains("README.md"))
        .expect("README segment should be present");

    assert!(
        !readme_segment.style.effects.contains(Effects::UNDERLINE),
        "local file-like path text should not keep markdown underline in inline UI"
    );
}

#[test]
fn prepare_markdown_lines_keeps_https_link_underlines() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let sink = InlineSink::new(InlineHandle::new_for_tests(sender), SyntaxHighlightingConfig::default());
    let base_style = MessageStyle::Response.style();
    let markdown = "[docs](https://example.com)";

    let (prepared, _, _) = sink.prepare_markdown_lines(markdown, "", base_style, true, false);
    let docs_segment = prepared
        .iter()
        .flat_map(|line| line.iter())
        .find(|segment| segment.text.contains("docs"))
        .expect("docs segment should be present");

    assert!(
        docs_segment.style.effects.contains(Effects::UNDERLINE),
        "https markdown links should keep underline styling"
    );
}

fn collect_append_lines(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<crate::ui::InlineCommand>,
) -> Vec<Vec<InlineSegment>> {
    let mut lines = Vec::new();
    while let Ok(command) = receiver.try_recv() {
        if let crate::ui::InlineCommand::AppendLine { segments, .. } = command {
            lines.push(segments);
        }
    }
    lines
}

fn collect_replacement_lines(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<crate::ui::InlineCommand>,
) -> Option<(usize, Vec<Vec<InlineSegment>>)> {
    let mut replacement = None;
    while let Ok(command) = receiver.try_recv() {
        if let crate::ui::InlineCommand::ReplaceLast { count, lines, .. } = command {
            replacement = Some((count, lines));
        }
    }
    replacement
}

fn inline_line_texts(lines: &[Vec<InlineSegment>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| line.iter().map(|segment| segment.text.as_str()).collect::<String>())
        .collect()
}

fn render_normal_markdown_fixture(source: &str, terminal_width: usize) -> Vec<Vec<InlineSegment>> {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    handle.set_message_labels(Some("Agent".to_owned()), None);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    renderer.set_table_max_width(Some(terminal_width));
    renderer
        .render_markdown_output(MessageStyle::Response, source)
        .expect("normal Markdown fixture should render");
    collect_append_lines(&mut receiver)
}

fn render_streamed_markdown_fixture(source: &str, terminal_width: usize) -> (usize, usize, Vec<Vec<InlineSegment>>) {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    handle.set_message_labels(Some("Agent".to_owned()), None);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    renderer.set_table_max_width(Some(terminal_width));
    let line_count = renderer
        .stream_markdown_response(source, 2)
        .expect("streamed Markdown fixture should render");
    let (replaced_count, lines) = collect_replacement_lines(&mut receiver).expect("stream should replace lines");
    (line_count, replaced_count, lines)
}

fn assert_markdown_table_fixture(source: &str, expected: &str, terminal_width: usize, expect_table_separators: bool) {
    let normal = render_normal_markdown_fixture(source, terminal_width);
    let normal_text = inline_line_texts(&normal);
    assert_eq!(normal_text.join("\n"), expected.trim_end_matches('\n'));

    let (line_count, replaced_count, streamed) = render_streamed_markdown_fixture(source, terminal_width);
    assert_eq!(line_count, streamed.len());
    assert_eq!(replaced_count, 2);
    assert_eq!(inline_line_texts(&streamed), normal_text);

    let agent_frame_width = UnicodeWidthStr::width(" • ") + UnicodeWidthStr::width("Agent") + 1;
    assert!(
        normal_text
            .iter()
            .all(|line| UnicodeWidthStr::width(line.as_str()) + agent_frame_width <= terminal_width),
        "a rendered line exceeded the framed terminal width: {normal_text:?}"
    );
    let has_table_separator = normal_text.iter().any(|line| line.contains('━'));
    assert_eq!(has_table_separator, expect_table_separators, "unexpected table layout: {normal_text:?}");
}

#[test]
fn markdown_table_wide_layout_snapshot_matches_normal_and_streaming() {
    assert_markdown_table_fixture(
        include_str!("../fixtures/markdown_table_wide.md"),
        include_str!("../fixtures/markdown_table_wide.snap"),
        40,
        true,
    );
}

#[test]
fn markdown_table_narrow_layout_snapshot_matches_normal_and_streaming() {
    let source = include_str!("../fixtures/markdown_table_narrow.md");
    let expected = include_str!("../fixtures/markdown_table_narrow.snap");
    assert_markdown_table_fixture(source, expected, 31, false);

    let normal = render_normal_markdown_fixture(source, 31);
    assert!(
        normal
            .iter()
            .flat_map(|line| line.iter())
            .any(|segment| segment.text.contains("Details") && segment.style.effects.contains(Effects::BOLD)),
        "fallback heading labels should be bold"
    );
}

#[test]
fn markdown_table_width_accounts_for_agent_frame_and_indent() -> Result<()> {
    use crate::ui::InlineCommand;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_table_max_width(Some(18));

    let markdown = "| Name | Description |\n|------|-------------|\n| item | a long value |\n";
    renderer
        .render_markdown_output(MessageStyle::Response, markdown)
        .context("render narrow Markdown table")?;

    let mut rendered = Vec::new();
    while let Ok(command) = receiver.try_recv() {
        if let InlineCommand::AppendLine { segments, .. } = command {
            rendered.push(segments.into_iter().map(|segment| segment.text).collect::<String>());
        }
    }

    let output = rendered.join("\n");
    assert!(output.contains("Name"), "agent frame should trigger labeled records: {output}");
    assert!(output.contains("Description"), "all labels should be retained: {output}");
    assert!(!output.contains("│"), "table separators should not survive the narrow layout: {output}");
    assert!(rendered.iter().all(|line| UnicodeWidthStr::width(line.as_str()) <= 18));
    Ok(())
}

#[test]
fn markdown_table_width_accounts_for_agent_label() -> Result<()> {
    use crate::ui::InlineCommand;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    handle.set_message_labels(Some("Agent".to_owned()), None);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    renderer.set_table_max_width(Some(23));

    let markdown = "| Name | Description |\n|------|-------------|\n| item | value |\n";
    renderer
        .render_markdown_output(MessageStyle::Response, markdown)
        .context("render agent-labeled Markdown table")?;

    let mut rendered = Vec::new();
    while let Ok(command) = receiver.try_recv() {
        if let InlineCommand::AppendLine { segments, .. } = command {
            rendered.push(segments.into_iter().map(|segment| segment.text).collect::<String>());
        }
    }

    let output = rendered.join("\n");
    assert!(output.contains("Name"), "agent label should reserve its prefix width: {output}");
    assert!(!output.contains("│"), "table columns should not be rewrapped after the label: {output}");
    Ok(())
}

#[test]
fn streaming_markdown_table_uses_same_aligned_record_layout() -> Result<()> {
    use crate::ui::InlineCommand;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_table_max_width(Some(18));

    let markdown = "| Name | Description |\n|------|-------------|\n| item | a long value |\n";
    let line_count = renderer
        .stream_markdown_response(markdown, 2)
        .context("stream labeled Markdown table")?;

    let mut replacement = None;
    while let Ok(command) = receiver.try_recv() {
        if let InlineCommand::ReplaceLast { count, lines, .. } = command {
            replacement = Some((count, lines));
        }
    }

    let (replaced_count, lines) = replacement.expect("streaming should replace rendered markdown lines");
    let output = lines
        .iter()
        .map(|line| line.iter().map(|segment| segment.text.as_str()).collect::<String>())
        .collect::<Vec<_>>();
    assert_eq!(line_count, lines.len());
    assert_eq!(replaced_count, 2);
    assert!(output.iter().any(|line| line.contains("Name")), "streamed output should contain labels: {output:?}");
    assert!(
        !output.iter().any(|line| line.contains('│')),
        "streamed output should use aligned records: {output:?}"
    );
    Ok(())
}

#[test]
fn line_function_no_trailing_empty_line() {
    use crate::utils::ansi_capabilities::AnsiCapabilities;
    use anstream::{AutoStream, ColorChoice};

    // Create a renderer that doesn't output to stdout
    let choice = ColorChoice::Never;
    let mut renderer = AnsiRenderer {
        writer: AutoStream::new(io::stdout(), choice),
        buffer: String::new(),
        color: false,
        sink: None,
        last_line_was_empty: false,
        highlight_config: SyntaxHighlightingConfig::default(),
        capabilities: AnsiCapabilities::detect(),
        reasoning_visible: true,
        screen_reader_mode: false,
        show_diagnostics_in_transcript: false,
        tool_display_mode: ToolDisplayMode::default(),
        diff_preview_mode: vtcode_commons::ui_protocol::DiffPreviewMode::Inline,
        compact_command_group: None,
        next_compact_group_id: 0,
        pending_tool_output_anchor: None,
        session_expand_anchor: None,
        session_body: false,
    };

    // This should not create an extra empty line after "line 2"
    renderer.line(MessageStyle::Tool, "line 1\nline 2\n").unwrap();

    // Previously, this would have added an extra empty line due to the trailing \n
    // With our fix, it should only process the actual content lines
}

#[test]
fn inline_ui_shows_error_lines_without_recording_transcript_when_disabled() {
    use crate::ui::InlineCommand;
    use crate::utils::transcript;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_show_diagnostics_in_transcript(false);
    transcript::clear();

    renderer.line(MessageStyle::Error, "fatal: hidden transcript failure").unwrap();

    let mut saw_append = false;
    while let Ok(command) = receiver.try_recv() {
        if matches!(command, InlineCommand::AppendLine { .. }) {
            saw_append = true;
        }
    }
    assert!(saw_append, "error output should still be visible in inline UI");
    assert!(
        !transcript::snapshot()
            .iter()
            .any(|line| line.contains("fatal: hidden transcript failure")),
        "error output should not be recorded in transcript when disabled"
    );
}

#[test]
fn inline_ui_shows_error_lines_when_enabled() {
    use crate::ui::InlineCommand;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_show_diagnostics_in_transcript(true);
    renderer.line(MessageStyle::Error, "fatal: visible in transcript").unwrap();

    let mut saw_append = false;
    while let Ok(command) = receiver.try_recv() {
        if matches!(command, InlineCommand::AppendLine { .. }) {
            saw_append = true;
        }
    }
    assert!(saw_append, "error output should be appended when enabled");
}

#[test]
fn inline_ui_collapses_large_json_tool_output() {
    use crate::ui::InlineCommand;
    use std::fmt::Write as _;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    let mut json = String::from("{\n");
    let line_total = INLINE_JSON_COLLAPSE_LINES + 5;
    for idx in 0..line_total {
        let _ = writeln!(&mut json, "  \"key{idx}\": \"value{idx}\",");
    }
    json.push_str("  \"end\": true\n}");

    renderer.line(MessageStyle::ToolOutput, &json).unwrap();

    let mut saw_pasted = false;
    let mut saw_append_line = false;
    while let Ok(command) = receiver.try_recv() {
        match command {
            InlineCommand::AppendPastedMessage { kind, text, line_count, .. } => {
                saw_pasted = true;
                assert_eq!(kind, InlineMessageKind::Pty);
                assert!(text.contains("\"end\": true"));
                assert!(line_count >= INLINE_JSON_COLLAPSE_LINES);
            }
            InlineCommand::AppendLine { .. } => {
                saw_append_line = true;
            }
            _ => {}
        }
    }

    assert!(saw_pasted, "expected large json to use AppendPastedMessage");
    assert!(!saw_append_line, "unexpected AppendLine for large json");
}

#[test]
fn clickable_targets_resolve_relative_paths_against_current_directory() {
    let _guard = lock_file_opener_test_guard();
    let original = current_file_opener();
    apply_file_opener_config(vtcode_config::FileOpener::Vscode);

    let cwd = std::env::current_dir().expect("current dir");
    let expected = Url::from_file_path(cwd.join("crates/codegen/vtcode-core/src/utils/ansi.rs")).expect("file url");
    let clickable =
        make_clickable_target("./crates/codegen/vtcode-core/src/utils/ansi.rs:42").expect("clickable target");

    assert_eq!(clickable, format!("vscode://file{}:42", expected.as_str().trim_start_matches("file://")));

    apply_file_opener_config(original);
}

#[test]
fn clickable_targets_translate_hash_locations_to_editor_suffixes() {
    let _guard = lock_file_opener_test_guard();
    let original = current_file_opener();
    apply_file_opener_config(vtcode_config::FileOpener::Vscode);

    let clickable = make_clickable_target("/tmp/example.rs#L12C3").expect("clickable target");

    assert_eq!(clickable, "vscode://file/tmp/example.rs:12:3");

    apply_file_opener_config(original);
}

#[test]
fn clickable_targets_decode_percent_encoded_bare_paths() {
    let _guard = lock_file_opener_test_guard();
    let original = current_file_opener();
    apply_file_opener_config(vtcode_config::FileOpener::Vscode);

    let clickable = make_clickable_target("/tmp/Example%20Folder/R%C3%A9sum%C3%A9.md:12").expect("clickable target");

    assert_eq!(clickable, "vscode://file/tmp/Example%20Folder/R%C3%A9sum%C3%A9.md:12");

    apply_file_opener_config(original);
}
