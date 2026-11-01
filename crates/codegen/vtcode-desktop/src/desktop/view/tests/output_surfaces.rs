use super::*;

#[gpui_kit::test]
fn output_surfaces_highlight_structured_json_while_keeping_full_text_accessible(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let output = "command output\nStructured output:\n{\"lifecycle_state\": \"exited\", \"exit_code\": 0}".to_string();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut item = TranscriptItem::new(
                "structured-output".into(),
                "tool".into(),
                "Structured command".into(),
                "completed".into(),
            );
            item.output = Some(output.clone().into());
            item.expanded = true;
            view.transcript.push(item);
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
        });
        window.render_frame(cx);

        // The structured block renders as its own highlighted section in both
        // the Output tab and the expanded transcript row...
        assert!(window.try_find("output-body-structured").is_some());
        assert!(window.try_find("tool-output-structured-output-structured").is_some());
        // ...while accessibility labels and copies keep the unsplit capture.
        assert_eq!(window.find("output-body").label(), Some(output.as_str()));
        assert_eq!(window.find("tool-output-structured-output").label(), Some(output.as_str()));
        window.click("copy-output", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(output.as_str()));
    })
    .unwrap();
}

#[gpui_kit::test]
fn output_surfaces_leave_truncated_structured_json_as_plain_text(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let output = "command output\nStructured output:\n{\"lifecycle_state\": \"exited\",".to_string();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut item = TranscriptItem::new(
                "truncated-output".into(),
                "tool".into(),
                "Truncated command".into(),
                "completed".into(),
            );
            item.output = Some(output.clone().into());
            item.expanded = true;
            view.transcript.push(item);
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
        });
        window.render_frame(cx);

        assert!(window.try_find("output-body-structured").is_none());
        assert!(window.try_find("tool-output-truncated-output-structured").is_none());
        assert_eq!(window.find("output-body").label(), Some(output.as_str()));
        assert_eq!(window.find("tool-output-truncated-output").label(), Some(output.as_str()));
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
