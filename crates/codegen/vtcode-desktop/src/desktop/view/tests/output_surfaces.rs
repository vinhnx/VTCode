use super::*;
use crate::desktop::inspector as output_inspector;

#[test]
fn output_tail_strips_terminal_controls_without_changing_capture() {
    let raw: gpui_kit::SharedString =
        "start \u{1b}[31mred\u{1b}[0m\n\u{1b}]8;;https://example.com\u{7}link\u{1b}]8;;\u{7}\u{7} end".into();
    assert_eq!(output_inspector::display_output_tail(&raw), "start red\nlink end");
    assert!(raw.contains("\u{1b}[31m"));
    let incomplete: gpui_kit::SharedString = "before\u{1b}[31".into();
    assert_eq!(output_inspector::display_output_tail(&incomplete), "before");
    assert_eq!(output_inspector::display_output_tail(&"plain\u{7}text".into()), "plaintext");
    assert_eq!(output_inspector::output_tail(&raw), raw);
}

#[gpui_kit::test]
fn output_surfaces_show_clean_terminal_text_and_copy_the_capture(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let raw = "first \u{1b}[31merror\u{1b}[0m\nsecond\u{7}";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut item =
                TranscriptItem::new("ansi-output".into(), "tool".into(), "Command".into(), "completed".into());
            item.output = Some(raw.into());
            item.expanded = true;
            view.transcript.push(item);
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
        });
        window.render_frame(cx);
        assert_eq!(window.find("output-body").label(), Some("first error\nsecond"));
        assert!(window.try_find("tool-output-ansi-output").is_none());
        window.click("copy-output", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(raw));
    })
    .unwrap();
}

#[gpui_kit::test]
fn empty_tool_capture_does_not_fall_back_to_metadata(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut item = TranscriptItem::new(
                "empty-output".into(),
                "tool".into(),
                "Command".into(),
                "running\n\n{\"type\":\"tool_output\"}".into(),
            );
            item.output = Some("".into());
            item.expanded = true;
            view.transcript.push(item);
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
        });
        window.render_frame(cx);
        assert!(window.try_find("tool-output-empty-output").is_none());
        assert_eq!(window.find("output-body").label().unwrap_or_default(), "");
    })
    .unwrap();
}

#[gpui_kit::test]
fn transcript_opens_full_capture_in_inspector(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut item = TranscriptItem::new("compact".into(), "tool".into(), "Command".into(), "running".into());
            item.output = Some(numbered_output_lines(20).into());
            item.expanded = true;
            view.transcript.push(item);
            view.inspector_open = false;
        });
        window.render_frame(cx);
        assert!(window.try_find("tool-output-compact").is_none());
        assert!(window.find("tool-open-output-compact").visible());
        focus_element_by_tab(window, "tool-open-output-compact", cx);
        window.press("enter", cx);
        window.render_frame(cx);
        let full_element = window.find("output-body");
        let full = full_element.label().unwrap();
        assert!(full.contains("output line 0\n"));
        assert!(full.contains("output line 19"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn long_output_stays_out_of_transcript_and_remains_available(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 800., 900.);
    let output = format!("start {} end", "long output ".repeat(150));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut item = TranscriptItem::new("long-preview".into(), "tool".into(), "Command".into(), "done".into());
            item.output = Some(output.clone().into());
            item.expanded = true;
            view.transcript.push(item);
        });
        window.render_frame(cx);
        assert!(window.try_find("tool-output-long-preview").is_none());
        window.click("tool-open-output-long-preview", cx);
        window.render_frame(cx);
        assert_eq!(window.find("output-body").label(), Some(output.as_str()));
    })
    .unwrap();
}

#[gpui_kit::test]
fn long_command_wraps_and_copies_without_overlaying_code(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 1000., 1400.);
    let command = format!("printf '%s' {}", "argument ".repeat(30));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.sidebar_open = false;
            view.inspector_open = false;
            let mut item =
                TranscriptItem::new("long-command".into(), "tool".into(), "Run command".into(), "done".into());
            item.command = Some(command.clone().into());
            item.expanded = true;
            view.transcript.push(item);
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            view.keyboard_focused_message = Some("long-command".into());
            cx.notify();
        });
        window.render_frame(cx);
        let code = window.find("tool-command-surface-long-command");
        assert_eq!(code.label(), Some(command.as_str()));
        assert!(code.bounds().size.height > px(60.), "long command wraps inside its surface");
        let copy = window.find("copy-tool-command-long-command");
        assert!(
            copy.visible(),
            "copy {:?}, code {:?}, selected {:?}",
            copy.bounds(),
            code.bounds(),
            view.read(cx).keyboard_focused_message
        );
        assert!(copy.bounds().bottom() <= code.bounds().top());
        window.click("copy-tool-command-long-command", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(command.as_str()));
    })
    .unwrap();
}

#[gpui_kit::test]
fn keyboard_can_expand_tool_and_copy_command_without_row_selection(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut item =
                TranscriptItem::new("keyboard-command".into(), "tool".into(), "Command".into(), "done".into());
            item.command = Some("printf 'a b'".into());
            view.transcript.push(item);
        });
        window.render_frame(cx);
        focus_element_by_tab(window, "tool-keyboard-command", cx);
        window.press("enter", cx);
        window.render_frame(cx);
        assert!(view.read(cx).keyboard_focused_message.is_none());
        focus_element_by_tab(window, "copy-tool-command-keyboard-command", cx);
        window.press("enter", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("printf 'a b'"));
    })
    .unwrap();
}

fn projected_command(id: &str, aggregated_output: &str) -> vtcode_exec_events::ThreadItem {
    use vtcode_exec_events::*;
    ThreadItem {
        id: id.into(),
        context: None,
        details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
            command: "cargo check".into(),
            arguments: None,
            aggregated_output: aggregated_output.into(),
            exit_code: Some(0),
            status: CommandExecutionStatus::Completed,
        })),
    }
}

#[gpui_kit::test]
fn output_surfaces_never_show_the_structured_envelope(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let raw = "command output\nStructured output:\n{\"lifecycle_state\": \"exited\", \"exit_code\": 0}";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.project_runtime_item(&projected_command("structured-command", raw), false);
            view.transcript.toggle_tool("structured-command");
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
        });
        window.render_frame(cx);

        // Projection strips the envelope, so neither surface can render it…
        assert!(window.try_find("output-body-structured").is_none());
        assert!(window.try_find("tool-output-structured-command-structured").is_none());
        // …and labels plus copies carry the same human text.
        assert_eq!(window.find("output-body").label(), Some("command output"));
        assert!(window.try_find("tool-output-structured-command").is_none());
        window.click("copy-output", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("command output"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn output_surfaces_leave_truncated_structured_json_as_plain_text(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let output = "command output\nStructured output:\n{\"lifecycle_state\": \"exited\",";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            // Asymmetric with the stripping test: truncated JSON never parses,
            // so the fail-open projection keeps every byte and the surfaces
            // render it literally — noise, never loss.
            view.project_runtime_item(&projected_command("truncated-output", output), false);
            view.transcript.toggle_tool("truncated-output");
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
        });
        window.render_frame(cx);

        assert!(window.try_find("output-body-structured").is_none());
        assert!(window.try_find("tool-output-truncated-output-structured").is_none());
        assert_eq!(window.find("output-body").label(), Some(output));
        assert!(window.try_find("tool-output-truncated-output").is_none());
    })
    .unwrap();
}

fn numbered_output_lines(count: usize) -> String {
    (0..count)
        .map(|index| format!("output line {index}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Remaining scroll distance to the Output pane's live edge, read from the
/// raw handle state (independent of the production pinned predicate).
fn output_remaining_to_bottom(scroll: &gpui_kit::ScrollHandle) -> gpui_kit::Pixels {
    scroll.offset().y + scroll.max_offset().y
}

#[gpui_kit::test]
fn output_tab_follows_growing_output_while_pinned_to_bottom(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut item = TranscriptItem::new(
                "streaming-output".into(),
                "tool".into(),
                "Streaming command".into(),
                "running".into(),
            );
            item.output = Some(numbered_output_lines(300).into());
            view.transcript.push(item);
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
        });
        window.render_frame(cx);

        let state = view.read(cx);
        assert!(state.output_scroll.max_offset().y > px(0.), "fixture must overflow the pane");
        assert!(output_remaining_to_bottom(&state.output_scroll) <= px(1.), "fresh output pins to its live edge");
        assert!(window.try_find("output-jump-latest").is_none());

        view.update(cx, |view, _| {
            let mut grown = TranscriptItem::new(
                "streaming-output".into(),
                "tool".into(),
                "Streaming command".into(),
                "running".into(),
            );
            grown.output = Some(numbered_output_lines(400).into());
            view.transcript.upsert(grown);
        });
        window.render_frame(cx);

        let state = view.read(cx);
        assert!(
            output_remaining_to_bottom(&state.output_scroll) <= px(1.),
            "growth while pinned follows the live edge"
        );
        assert!(window.try_find("output-jump-latest").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn output_tab_follows_changed_tail_with_unchanged_byte_length(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let first = format!("{}{}", numbered_output_lines(300), " ".repeat(200));
        let next = format!("{}{}", numbered_output_lines(300), "\n".repeat(200));
        assert_eq!(first.len(), next.len());
        view.update(cx, |view, _| {
            let mut item = TranscriptItem::new("same-length".into(), "tool".into(), "Command".into(), "running".into());
            item.output = Some(first.into());
            view.transcript.push(item);
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
        });
        window.render_frame(cx);
        view.update(cx, |view, _| view.transcript.set_output("same-length", Some(next.into())));
        window.render_frame(cx);
        assert!(output_remaining_to_bottom(&view.read(cx).output_scroll) <= px(1.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn output_tab_pauses_follow_when_scrolled_up_until_jump_to_latest(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut item = TranscriptItem::new(
                "streaming-output".into(),
                "tool".into(),
                "Streaming command".into(),
                "running".into(),
            );
            item.output = Some(numbered_output_lines(300).into());
            view.transcript.push(item);
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
        });
        window.render_frame(cx);
        view.update(cx, |view, _| {
            let mid = view.output_scroll.max_offset().y / 2.;
            view.output_scroll.set_offset(point(px(0.), -mid));
        });
        window.render_frame(cx);

        let state = view.read(cx);
        assert!(state.output_scroll.max_offset().y > px(0.), "fixture must overflow the pane");
        assert!(output_remaining_to_bottom(&state.output_scroll) > px(1.), "scrolling up leaves the live edge");
        let held_offset = state.output_scroll.offset();

        view.update(cx, |view, _| {
            let mut grown = TranscriptItem::new(
                "streaming-output".into(),
                "tool".into(),
                "Streaming command".into(),
                "running".into(),
            );
            grown.output = Some(numbered_output_lines(400).into());
            view.transcript.upsert(grown);
        });
        window.render_frame(cx);

        let state = view.read(cx);
        assert_eq!(state.output_scroll.offset(), held_offset, "growth must not move a reader");
        assert_eq!(window.find("output-jump-latest").label(), Some("Jump to latest output"));

        window.click("output-jump-latest", cx);
        window.render_frame(cx);

        let state = view.read(cx);
        assert!(output_remaining_to_bottom(&state.output_scroll) <= px(1.), "jump resumes the live edge");
        assert!(window.try_find("output-jump-latest").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn output_tab_filters_lines_by_query_with_counts_and_empty_state(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut item = TranscriptItem::new(
                "filtered-output".into(),
                "tool".into(),
                "Filtered command".into(),
                "completed".into(),
            );
            item.output = Some("alpha one\nbeta two\nALPHA three\n".into());
            view.transcript.push(item);
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
        });
        window.render_frame(cx);

        assert!(window.try_find("output-filter-count").is_none(), "no count without a query");
        assert_eq!(window.find("output-body").label(), Some("alpha one\nbeta two\nALPHA three\n"));

        view.update(cx, |view, cx| {
            view.output_filter_input
                .update(cx, |input, cx| input.set_value("alpha", window, cx));
        });
        window.render_frame(cx);

        assert_eq!(window.find("output-body").label(), Some("alpha one\nALPHA three"));
        assert_eq!(window.find("output-filter-count").label(), Some("2 of 3 shown"));

        view.update(cx, |view, cx| {
            view.output_filter_input
                .update(cx, |input, cx| input.set_value("zzz", window, cx));
        });
        window.render_frame(cx);

        assert_eq!(window.find("output-filter-empty").label(), Some("No lines match this filter"));
        assert_eq!(window.find("output-filter-count").label(), Some("0 of 3 shown"));

        view.update(cx, |view, cx| {
            view.output_filter_input.update(cx, |input, cx| input.set_value("", window, cx));
        });
        window.render_frame(cx);

        assert_eq!(window.find("output-body").label(), Some("alpha one\nbeta two\nALPHA three\n"));
        assert!(window.try_find("output-filter-count").is_none(), "clearing restores the full tail");
    })
    .unwrap();
}

#[gpui_kit::test]
fn output_tab_resets_scroll_when_switching_captures(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut running =
                TranscriptItem::new("running-output".into(), "tool".into(), "Running command".into(), "running".into());
            running.output = Some(numbered_output_lines(300).into());
            view.transcript.push(running);
            let mut done =
                TranscriptItem::new("done-output".into(), "tool".into(), "Done command".into(), "completed".into());
            done.output = Some(numbered_output_lines(350).into());
            view.transcript.push(done);
            view.selected_output_id = Some("running-output".into());
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
        });
        window.render_frame(cx);

        let state = view.read(cx);
        assert!(state.output_scroll.offset().y < px(0.), "running output must have followed away from top");

        view.update(cx, |view, _| {
            view.selected_output_id = Some("done-output".into());
        });
        window.render_frame(cx);

        let state = view.read(cx);
        assert_eq!(state.output_scroll.offset().y, px(0.), "a new capture starts at the top");
        assert_eq!(window.find("output-body").label(), Some(numbered_output_lines(350).as_str()));
    })
    .unwrap();
}
