use super::*;

#[gpui_kit::test]
fn terminal_keyboard_resize_adjusts_height_and_grid(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let dir = tempfile::tempdir().unwrap();
    view.update(cx, |view, cx| {
        view.prepare_workspace(dir.path(), cx);
    });
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::ToggleTerminal), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let before = cx.update(|cx| view.read(cx).terminal_panel_height);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.terminal_resize_focus.focus(window, cx);
        });
        window.press("up", cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(view.read(cx).terminal_panel_height, before + 10.0);
    });
    view.update(cx, |view, cx| {
        assert!(view.pump_terminal_once(view.terminal_pump_generation, cx));
        let expected =
            terminal_rows_for_height(view.terminal_panel_height, terminal_row_height(cx.theme().mono_font_size))
                .unwrap();
        assert_eq!(view.terminal.as_ref().unwrap().grid_size().rows, expected);
    });
}

#[gpui_kit::test]
fn terminal_measured_row_matches_laid_out_height(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let dir = tempfile::tempdir().unwrap();
    view.update(cx, |view, cx| {
        view.prepare_workspace(dir.path(), cx);
    });
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::ToggleTerminal), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let laid_out = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.find(("terminal-row", 0usize)).bounds().size.height.as_f32()
        })
        .unwrap();
    let measured = cx.update(|cx| terminal_row_height(cx.theme().mono_font_size));
    assert_eq!(laid_out, measured);
}

#[gpui_kit::test]
fn terminal_drag_resizes_panel_height(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let dir = tempfile::tempdir().unwrap();
    view.update(cx, |view, cx| {
        view.prepare_workspace(dir.path(), cx);
    });
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::ToggleTerminal), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let before = cx.update(|cx| view.read(cx).terminal_panel_height);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let start = window.find("terminal-resize").bounds().center();
        window.drag(start, point(start.x, start.y - px(60.)), cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(view.read(cx).terminal_panel_height - before, 60.0);
    });
}

#[gpui_kit::test]
fn terminal_keyboard_resize_clamps_at_bounds(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let dir = tempfile::tempdir().unwrap();
    view.update(cx, |view, cx| {
        view.prepare_workspace(dir.path(), cx);
    });
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::ToggleTerminal), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.terminal_resize_focus.focus(window, cx);
        });
        for _ in 0..6 {
            window.press("shift-down", cx);
        }
    })
    .unwrap();
    cx.update(|cx| {
        let height = view.read(cx).terminal_panel_height;
        assert!(
            (height - crate::desktop::preferences::MIN_TERMINAL_PANEL_HEIGHT).abs() < 0.001,
            "clamps at minimum: {height}"
        );
    });
    cx.update_window(window, |_, window, cx| {
        for _ in 0..12 {
            window.press("shift-up", cx);
        }
    })
    .unwrap();
    cx.update(|cx| {
        let height = view.read(cx).terminal_panel_height;
        assert!(
            (height - crate::desktop::preferences::MAX_TERMINAL_PANEL_HEIGHT).abs() < 0.001,
            "clamps at maximum: {height}"
        );
    });
}
