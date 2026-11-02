use super::*;

#[test]
fn eviction_drops_pastes_inside_evicted_prefix_without_panicking() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    // Register a collapsed paste near the transcript front so the first
    // eviction chunk (the 1000 oldest lines) covers its line index.
    let json = collapsed_json_payload();
    let line_count = json.lines().count();
    session.append_pasted_message(InlineMessageKind::Tool, json, line_count);
    assert_eq!(session.collapsed_pastes.len(), 1);
    assert!(session.collapsed_pastes[0].line_index < ui::TUI_TRANSCRIPT_EVICT_CHUNK);

    for idx in 0..ui::TUI_TRANSCRIPT_MAX_MSGS {
        session.push_line(InlineMessageKind::Info, vec![make_segment(&format!("filler-{idx}"))]);
    }

    assert_eq!(session.lines.len(), ui::TUI_TRANSCRIPT_MAX_MSGS + 1 - ui::TUI_TRANSCRIPT_EVICT_CHUNK);
    // The placeholder line was evicted, so its paste record must be gone too.
    assert!(session.collapsed_pastes.is_empty());
}
#[test]
fn eviction_shifts_surviving_paste_indices() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let lines_before_paste = 1500usize;
    for idx in 0..lines_before_paste {
        session.push_line(InlineMessageKind::Info, vec![make_segment(&format!("filler-{idx}"))]);
    }

    let json = collapsed_json_payload();
    let line_count = json.lines().count();
    session.append_pasted_message(InlineMessageKind::Tool, json, line_count);
    let original_index = session.collapsed_pastes[0].line_index;
    assert_eq!(original_index, lines_before_paste);

    let remaining = ui::TUI_TRANSCRIPT_MAX_MSGS + 1 - session.lines.len();
    for idx in 0..remaining {
        session.push_line(InlineMessageKind::Info, vec![make_segment(&format!("tail-{idx}"))]);
    }

    let shifted_index = session.collapsed_pastes[0].line_index;
    assert_eq!(shifted_index, original_index - ui::TUI_TRANSCRIPT_EVICT_CHUNK);
    let preview_line = session.lines.get(shifted_index).expect("paste placeholder survives");
    let text: String = preview_line.segments.iter().map(|segment| segment.text.as_str()).collect();
    assert!(text.contains("showing last"));
}
#[test]
fn agent_prose_links_stay_aligned_when_rows_justify() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_workspace_root(Some(vtcode_tui_workspace_root()));
    // Plain single-style prose: rows stay single-span so mid-length rows are
    // justification candidates. The inline path must keep exact link bounds
    // even when its row is padded out to full width.
    let path = transcript_file_fixture_relative_path();
    let text = format!(
        "Verified the implementation across all of the modules and then confirmed the behavior in {path}, and continued checking the remaining pieces of the transcript afterwards today"
    );
    session.push_line(InlineMessageKind::Agent, vec![make_segment(&text)]);

    let rows = session.reflow_message_lines(0, 80, false);
    assert!(rows.len() > 1, "expected the prose to wrap");

    let mut found = 0;
    for row in &rows {
        let row_text: String = row.line.spans.iter().map(|span| span.content.as_ref()).collect();
        for link in &row.explicit_links {
            let slice = row_text.get(link.start..link.end).unwrap_or_default();
            assert_eq!(slice, path, "link underline misaligned after reflow: {slice:?} in {row_text:?}");
            found += 1;
        }
    }
    assert_eq!(found, 1, "expected the inline path to stay linked");
}
#[test]
fn tool_command_header_wraps_in_full_without_truncation() {
    // Screenshot 2026-09-24 16:37: a piped `• Ran grep ... | grep -v ...`
    // header must wrap across lines with every segment intact and no `…`.
    // Wrapping may only insert whitespace (hanging indent); comparing with
    // whitespace stripped proves no characters are lost or truncated.
    // Fixture uses the exact screenshot bytes: `||` inside the quoted pattern
    // and the backslash-escaped `\.backup` must both survive wrapping.
    let command = "grep -rn \"@vinhnx/vtcode|npm install -g||npx @vinhnx\" docs | grep -v node_modules | grep -v package-lock | grep -v \"\\.backup\"";
    let expected_flat: String = format!("• Ran {command}").chars().filter(|c| !c.is_whitespace()).collect();
    for width in [80u16, 50, 40] {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
        session.push_line(InlineMessageKind::Tool, vec![make_segment(&format!("• Ran {command}"))]);
        let rows = session.reflow_transcript_lines(width);
        let joined: String = rows
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<Vec<_>>()
            .join("");
        assert!(!joined.contains('…'), "width {width}: header must not truncate, got: {joined:?}");
        let flat: String = joined.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(flat, expected_flat, "width {width}: wrapped header lost content, got: {joined:?}");
        // Proper line wrapping shape: the header must actually wrap at narrow
        // widths, the first content row keeps the bullet+verb, continuations
        // hang under it (gutter + hanging indent, never a mid-token restart),
        // and no row overflows the viewport. Spacing blanks are skipped.
        let content_rows: Vec<String> = rows
            .iter()
            .map(|row| row.spans.iter().map(|span| span.content.as_ref()).collect())
            .filter(|text: &String| !text.trim().is_empty())
            .collect();
        assert!(content_rows.len() > 1, "width {width}: long header must wrap, got: {joined:?}");
        assert!(
            content_rows[0].starts_with("• Ran "),
            "width {width}: first row keeps bullet+verb, got: {:?}",
            content_rows[0]
        );
        for text in content_rows.iter().skip(1) {
            assert!(text.starts_with("  "), "width {width}: continuation hangs under the header, got: {text:?}");
            assert!(text.chars().count() <= usize::from(width), "width {width}: row overflows viewport, got: {text:?}");
        }
    }
}
#[test]
fn pty_command_header_wraps_in_full_without_truncation() {
    // Same screenshot command through the live PTY path: `• Ran` plus its
    // `  │ ` continuations must keep every pipe segment with no `…`. Exact
    // screenshot bytes (`||`, `\.backup`); the `│` stream glyphs are content
    // and must survive reflow rather than being stripped or truncated.
    let header = "• Ran grep -rn \"@vinhnx/vtcode|npm install -g||npx @vinhnx\" docs |";
    let continuation = "  │ grep -v node_modules | grep -v package-lock | grep -v \"\\.backup\"";
    let expected_flat: String = format!("{header}{continuation}")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    for width in [80u16, 50, 40] {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
        push_pty_line(&mut session, header);
        push_pty_line(&mut session, continuation);
        let first = session.reflow_pty_lines(0, width);
        let second = session.reflow_pty_lines(1, width);
        let joined: String = first
            .iter()
            .chain(second.iter())
            .flat_map(|line| line.line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<Vec<_>>()
            .join("");
        assert!(!joined.contains('…'), "width {width}: PTY header must not truncate, got: {joined:?}");
        let flat: String = joined.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(flat, expected_flat, "width {width}: PTY header lost content, got: {joined:?}");
        // Stream `│` glyphs survive reflow and no row overflows the viewport.
        assert!(joined.contains('│'), "width {width}: continuation glyph lost, got: {joined:?}");
        for line in first.iter().chain(second.iter()) {
            let text: String = line.line.spans.iter().map(|span| span.content.as_ref()).collect();
            assert!(text.chars().count() <= usize::from(width), "width {width}: row overflows viewport, got: {text:?}");
        }
    }
}
#[test]
fn streaming_append_preserves_header_cache() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let _ = session.header_lines();
    assert!(session.header_lines_cache.is_some(), "header cache should be populated");

    // First chunk creates the line; later chunks stream into it.
    session.push_line(InlineMessageKind::Agent, vec![make_segment("hello")]);
    assert!(session.header_lines_cache.is_some(), "push_line must not drop header cache");

    session.append_inline(
        InlineMessageKind::Agent,
        InlineSegment {
            text: " world".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        },
    );
    assert!(
        session.header_lines_cache.is_some(),
        "streaming append must not drop header cache (invalidation thrash)"
    );
}
#[test]
fn eviction_keeps_surviving_reflow_entries_valid() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    for idx in 0..(ui::TUI_TRANSCRIPT_MAX_MSGS + 1) {
        session.push_line(InlineMessageKind::Info, vec![make_segment(&format!("row-{idx}"))]);
    }
    // Force a reflow so the cache is populated (this path uses ensure_reflow_cache).
    let _ = session.total_transcript_rows(80);
    assert!(session.transcript_cache.is_some());
    let cache = session.transcript_cache.as_ref().expect("cache");
    assert_eq!(cache.messages.len(), session.lines.len());
    assert!(!cache.needs_reflow(0, session.lines[0].revision), "pre-eviction cache entry should be valid");

    // One more push triggers a chunked eviction from the front.
    session.push_line(InlineMessageKind::Info, vec![make_segment("after-evict")]);
    assert!(session.lines.len() <= ui::TUI_TRANSCRIPT_MAX_MSGS);

    // Refresh the cache (same path the renderer uses).
    let _ = session.total_transcript_rows(80);
    let cache = session.transcript_cache.as_ref().expect("cache after eviction");
    assert_eq!(
        cache.messages.len(),
        session.lines.len(),
        "reflow cache must track live lines after prefix eviction"
    );
    assert!(
        !cache.needs_reflow(0, session.lines[0].revision),
        "surviving reflow entries must stay valid — full invalidate would reflow 5000 msgs"
    );
    assert!(cache.total_rows() > 0);
}
/// Smoke-timing: 200 frames of a large transcript should stay well under a
/// 16ms frame budget on TestBackend. Prints averages for the performance Report.
#[test]
fn large_transcript_render_stays_under_frame_budget() {
    use ratatui::{Terminal, backend::TestBackend};
    use std::time::Instant;

    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    for idx in 0..800 {
        session.push_line(
            InlineMessageKind::Agent,
            vec![make_segment(&format!(
                "line {idx}: the quick brown fox jumps over the lazy dog while streaming tool output"
            ))],
        );
    }
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
    // Warm caches.
    for _ in 0..5 {
        terminal.draw(|frame| session.render(frame)).expect("warm render");
    }
    let frames = 200usize;
    let started = Instant::now();
    for _ in 0..frames {
        session.mark_visual_dirty();
        terminal.draw(|frame| session.render(frame)).expect("render");
    }
    let elapsed = started.elapsed();
    let avg_us = elapsed.as_micros() as f64 / frames as f64;
    eprintln!("large_transcript_render: {frames} frames in {elapsed:?} ({avg_us:.1} us/frame avg)");
    // Generous ceiling so loaded CI machines do not flake; the printed avg is
    // the number to watch for regressions (typically ~0.2ms on TestBackend).
    assert!(avg_us < 50_000.0, "avg frame {avg_us:.1}us is pathologically slow on TestBackend");
}
#[test]
fn capture_blocks_and_activity_entries_stay_bounded() {
    use crate::tui::core_tui::app::session::AppSession;

    let mut session = AppSession::new_with_logs(
        InlineTheme::default(),
        None,
        VIEW_ROWS,
        false,
        None,
        Vec::new(),
        "vtcode".to_string(),
    );

    // Flooding full captures must not grow past the FIFO block bound.
    for id in 0..(ui::TUI_TOOL_OUTPUT_BLOCKS_MAX + 8) {
        let lines: Vec<String> = (0..32).map(|row| format!("capture-{id}-row-{row}")).collect();
        session.record_tool_output_block(id as u64, lines);
    }
    assert!(session.tool_output_blocks.len() <= ui::TUI_TOOL_OUTPUT_BLOCKS_MAX);

    // Per-capture line count is tail-bounded.
    let huge: Vec<String> = (0..(ui::TUI_TOOL_OUTPUT_CAPTURE_MAX_LINES + 50))
        .map(|row| format!("huge-{row}"))
        .collect();
    session.record_tool_output_block(9_999, huge);
    let last = session.tool_output_blocks.last().expect("capture");
    assert!(last.lines.len() <= ui::TUI_TOOL_OUTPUT_CAPTURE_MAX_LINES);
}
#[test]
fn collapsed_paste_payload_is_tail_bounded() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let mut json = String::from("{\n");
    for i in 0..20_000 {
        json.push_str(&format!("  \"k{i}\": \"{}\",\n", "x".repeat(40)));
    }
    json.push_str("  \"end\": true\n}");
    let line_count = json.lines().count();
    let oversized = json.len() > ui::TUI_COLLAPSED_PASTE_MAX_BYTES;
    assert!(oversized, "fixture must exceed the paste bound");

    session.append_pasted_message(InlineMessageKind::Tool, json, line_count);
    assert_eq!(session.collapsed_pastes.len(), 1);
    assert!(session.collapsed_pastes[0].full_text.len() <= ui::TUI_COLLAPSED_PASTE_MAX_BYTES + 4);
}
#[test]
fn eviction_shifts_dirty_hint_to_appended_line() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    // Agent lines are not info-grouped, so each line is its own dirty unit.
    for idx in 0..ui::TUI_TRANSCRIPT_MAX_MSGS {
        session.push_line(InlineMessageKind::Agent, vec![make_segment(&format!("row-{idx}"))]);
    }
    let _ = session.total_transcript_rows(80);
    session.first_dirty_line = None;

    // This push exceeds the cap, so it also triggers a front-eviction chunk.
    session.push_line(InlineMessageKind::Agent, vec![make_segment("after-evict")]);
    assert_eq!(
        session.first_dirty_line,
        Some(session.lines.len() - 1),
        "eviction must shift the dirty hint to the appended line, not invent dirty=0"
    );
}
#[test]
fn input_render_cache_rejects_same_length_content_change() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.input_manager.set_content("aaa".to_string());
    session.input_manager.set_cursor(3);
    let _ = session.build_input_render_for_test(40, 3);
    session.input_manager.set_content("bbb".to_string());
    session.input_manager.set_cursor(3);
    let rebuilt = session.build_input_render_for_test(40, 3);
    let text: String = rebuilt
        .text
        .lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref().to_string()))
        .collect();
    assert!(text.contains("bbb"), "same-length edit must not serve cached glyphs: {text:?}");
    assert!(!text.contains("aaa"), "stale cached glyphs leaked: {text:?}");
}
