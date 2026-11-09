use super::*;

fn seed_activity_threads(view: &Entity<DesktopView>, cx: &mut gpui_kit::App) {
    view.update(cx, |view, _| {
        view.inspector_open = true;
        view.inspector_tab = crate::desktop::inspector::InspectorTab::Activity;
        // Asymmetric statuses: done, failed with output, running.
        for (id, title, text) in [
            ("read", "Read Cargo.toml", "completed\nmanifest"),
            ("build", "cargo check", "failed (exit 1)\nstructured details"),
            ("retry", "cargo check", "in progress\nretrying"),
        ] {
            view.transcript
                .push(TranscriptItem::new(id.into(), "tool".into(), title.into(), text.into()));
        }
        view.transcript.set_output("build", Some("compiler error: missing name".into()));
    });
}

#[gpui_kit::test]
fn activity_arrows_follow_visible_order_paint_rail_clamp_and_heal(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_activity_threads(&view, cx);
        view.update(cx, |view, cx| view.activity_focus.focus(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).activity_focus.is_focused(window));
        // Newest leads: retry, build, read.
        let row = window.find("activity-build").bounds();
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_activity.as_deref(), Some("retry"));
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_activity.as_deref(), Some("build"));
        window.render_frame(cx);
        assert!(window.try_find("activity-focus-rail-build").is_some(), "highlight paints a rail");
        assert_eq!(window.find("activity-build").bounds(), row, "highlight must not reflow the row");
        let rail = window.find("activity-focus-rail-build").bounds();
        assert_eq!(rail.origin, row.origin, "rail starts at the row's top-left");
        assert_eq!(rail.size.height, row.size.height, "rail spans exactly the row");
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_activity.as_deref(), Some("read"));
        window.press("down", cx);
        assert_eq!(
            view.read(cx).keyboard_focused_activity.as_deref(),
            Some("read"),
            "navigation clamps at the last row"
        );
        window.press("up", cx);
        assert_eq!(view.read(cx).keyboard_focused_activity.as_deref(), Some("build"));
        window.press("home", cx);
        assert_eq!(view.read(cx).keyboard_focused_activity.as_deref(), Some("retry"));
        window.press("end", cx);
        assert_eq!(view.read(cx).keyboard_focused_activity.as_deref(), Some("read"));
        // A stale id (rows refreshed underneath) restarts from the edge.
        view.update(cx, |view, _| view.keyboard_focused_activity = Some("tool-gone".into()));
        window.press("up", cx);
        assert_eq!(view.read(cx).keyboard_focused_activity.as_deref(), Some("read"));
        // Filtering narrows movement to the visible rows.
        view.update(cx, |view, _| {
            view.activity_filter = Some(ActivityState::Failed);
            view.keyboard_focused_activity = None;
        });
        window.render_frame(cx);
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_activity.as_deref(), Some("build"));
        window.press("down", cx);
        assert_eq!(
            view.read(cx).keyboard_focused_activity.as_deref(),
            Some("build"),
            "a single visible row clamps immediately"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn activity_enter_is_inert_and_buttons_keep_tab_stops(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_activity_threads(&view, cx);
        view.update(cx, |view, cx| view.activity_focus.focus(window, cx));
        window.render_frame(cx);
        window.press("down", cx);
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_activity.as_deref(), Some("build"));
        // No Enter binding: the highlight never activates, so focus and
        // buttons keep unambiguous native behavior.
        window.press("enter", cx);
        assert_eq!(
            view.read(cx).inspector_tab,
            crate::desktop::inspector::InspectorTab::Activity,
            "enter must not open the output tab"
        );
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), None, "enter must not jump");
        // Both row actions stay Tab-reachable instead.
        focus_element_by_tab(window, "activity-output-build", cx);
        focus_element_by_tab(window, "activity-jump-build", cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn activity_vim_motions_follow_visible_order(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.toggle_vim_navigation(cx));
        seed_activity_threads(&view, cx);
        view.update(cx, |view, cx| view.activity_focus.focus(window, cx));
        window.render_frame(cx);
        window.press("j", cx);
        assert_eq!(view.read(cx).keyboard_focused_activity.as_deref(), Some("retry"));
        window.press("j", cx);
        assert_eq!(view.read(cx).keyboard_focused_activity.as_deref(), Some("build"));
        window.press("k", cx);
        assert_eq!(view.read(cx).keyboard_focused_activity.as_deref(), Some("retry"));
        window.press("shift-g", cx);
        assert_eq!(view.read(cx).keyboard_focused_activity.as_deref(), Some("read"));
        window.press("g", cx);
        assert_eq!(view.read(cx).keyboard_focused_activity.as_deref(), Some("retry"));
        window.press("i", cx);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window), "i focuses the composer");
    })
    .unwrap();
}
