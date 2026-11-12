use super::*;

fn grid_geometry(view: &Entity<DesktopView>, cx: &mut gpui_kit::App) -> (gpui_kit::Point<gpui_kit::Pixels>, f32, f32) {
    let grid = view.read(cx).terminal_grid_bounds.get().expect("probe must have painted");
    let origin = grid.origin;
    let theme = cx.theme();
    let row_height = terminal_row_height(theme.mono_font_size);
    // Headless advances are exactly 0.6 per em (NoopTextSystem); the handler
    // measures the same value through the real text-system path.
    let char_width = theme.mono_font_size.as_f32() * 0.6;
    (origin, row_height, char_width)
}

fn cell_center(
    origin: gpui_kit::Point<gpui_kit::Pixels>,
    row_height: f32,
    char_width: f32,
    row: usize,
    column: usize,
) -> gpui_kit::Point<gpui_kit::Pixels> {
    point(origin.x + px((column as f32 + 0.5) * char_width), origin.y + px((row as f32 + 0.5) * row_height))
}

fn open_terminal(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<DesktopView>, tempfile::TempDir) {
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
    view.update(cx, |view, _| {
        let tab = view.terminal.as_mut().unwrap();
        let _events = tab.screen.process(b"\x1b[2J\x1b[Halpha\r\nbravo\r\ncharlie");
    });
    (window, view, dir)
}

#[gpui_kit::test]
fn terminal_drag_selects_forward_across_rows(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_terminal(cx);
    let (from, to) = cx.update(|cx| {
        let (origin, row_height, char_width) = grid_geometry(&view, cx);
        (cell_center(origin, row_height, char_width, 0, 1), cell_center(origin, row_height, char_width, 1, 3))
    });
    cx.update_window(window, |_, window, cx| {
        window.drag(from, to, cx);
    })
    .unwrap();
    cx.update(|cx| {
        let selected = view.read(cx).terminal.as_ref().unwrap().selected_text().unwrap();
        assert!(selected.contains("lpha"), "starts inside row 0: {selected:?}");
        assert!(selected.contains("brav"), "ends inside row 1: {selected:?}");
        assert!(!selected.contains("charlie"), "must not overshoot: {selected:?}");
    });
}

#[gpui_kit::test]
fn terminal_drag_selects_backward_to_same_span(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_terminal(cx);
    let (from, to) = cx.update(|cx| {
        let (origin, row_height, char_width) = grid_geometry(&view, cx);
        (cell_center(origin, row_height, char_width, 1, 3), cell_center(origin, row_height, char_width, 0, 1))
    });
    cx.update_window(window, |_, window, cx| {
        window.drag(from, to, cx);
    })
    .unwrap();
    cx.update(|cx| {
        let selected = view.read(cx).terminal.as_ref().unwrap().selected_text().unwrap();
        assert!(selected.contains("lpha"), "backward drag keeps span: {selected:?}");
        assert!(selected.contains("brav"), "backward drag keeps span: {selected:?}");
    });
}

#[gpui_kit::test]
fn terminal_move_without_button_heals_stuck_drag(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_terminal(cx);
    let at = cx.update(|cx| {
        let (origin, row_height, char_width) = grid_geometry(&view, cx);
        cell_center(origin, row_height, char_width, 0, 2)
    });
    // Raw dispatch: no headless helper presses without releasing, so the
    // down travels the real hit-test path to the panel handler.
    cx.update_window(window, |_, window, cx| {
        let _dispatched = window.dispatch_event(
            gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                button: MouseButton::Left,
                position: at,
                modifiers: window.modifiers(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).terminal_select_drag.is_some());
    });
    // Extend with the button held: row 0 cols 2..=4 ("pha").
    let stretched = cx.update(|cx| {
        let (origin, row_height, char_width) = grid_geometry(&view, cx);
        cell_center(origin, row_height, char_width, 0, 4)
    });
    cx.update_window(window, |_, window, cx| {
        let _dispatched = window.dispatch_event(
            gpui_kit::PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                position: stretched,
                pressed_button: Some(MouseButton::Left),
                modifiers: window.modifiers(),
            }),
            cx,
        );
    })
    .unwrap();
    // Release happened outside the panel, so no up event arrives; the next
    // buttonless move must end the drag while keeping the selection.
    cx.update_window(window, |_, window, cx| {
        let _dispatched = window.dispatch_event(
            gpui_kit::PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                position: stretched,
                pressed_button: None,
                modifiers: window.modifiers(),
            }),
            cx,
        );
    })
    .unwrap();
    cx.update(|cx| {
        let state = view.read(cx);
        assert!(state.terminal_select_drag.is_none());
        assert_eq!(
            state.terminal.as_ref().unwrap().selected_text().as_deref(),
            Some("pha"),
            "healing ends the drag but keeps the selection"
        );
    });
}

#[gpui_kit::test]
fn terminal_double_click_selects_word_through_full_sequence(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_terminal(cx);
    let at = cx.update(|cx| {
        let (origin, row_height, char_width) = grid_geometry(&view, cx);
        cell_center(origin, row_height, char_width, 1, 2)
    });
    // Realistic press count sequence: the word must survive both releases.
    for count in [1_usize, 2] {
        cx.update_window(window, |_, window, cx| {
            let _dispatched = window.dispatch_event(
                gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                    button: MouseButton::Left,
                    position: at,
                    modifiers: window.modifiers(),
                    click_count: count,
                    first_mouse: false,
                }),
                cx,
            );
            let _dispatched = window.dispatch_event(
                gpui_kit::PlatformInput::MouseUp(gpui_kit::MouseUpEvent {
                    button: MouseButton::Left,
                    position: at,
                    modifiers: window.modifiers(),
                    click_count: count,
                }),
                cx,
            );
        })
        .unwrap();
    }
    cx.update(|cx| {
        assert_eq!(view.read(cx).terminal.as_ref().unwrap().selected_text().as_deref(), Some("bravo"));
    });
}

#[gpui_kit::test]
fn terminal_triple_click_selects_line(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_terminal(cx);
    let at = cx.update(|cx| {
        let (origin, row_height, char_width) = grid_geometry(&view, cx);
        cell_center(origin, row_height, char_width, 1, 2)
    });
    cx.update_window(window, |_, window, cx| {
        let _dispatched = window.dispatch_event(
            gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                button: MouseButton::Left,
                position: at,
                modifiers: window.modifiers(),
                click_count: 3,
                first_mouse: false,
            }),
            cx,
        );
    })
    .unwrap();
    cx.update(|cx| {
        let selected = view.read(cx).terminal.as_ref().unwrap().selected_text().unwrap();
        assert!(selected.contains("bravo"), "line select must cover the row: {selected:?}");
        assert!(
            !selected.contains("alpha") && !selected.contains("charlie"),
            "line select must not leak rows: {selected:?}"
        );
    });
}

#[gpui_kit::test]
fn terminal_shift_click_extends_forward_from_anchor(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_terminal(cx);
    let (from, to, extend) = cx.update(|cx| {
        let (origin, row_height, char_width) = grid_geometry(&view, cx);
        (
            cell_center(origin, row_height, char_width, 0, 1),
            cell_center(origin, row_height, char_width, 1, 3),
            cell_center(origin, row_height, char_width, 2, 2),
        )
    });
    cx.update_window(window, |_, window, cx| {
        window.drag(from, to, cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        let _dispatched = window.dispatch_event(
            gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                button: MouseButton::Left,
                position: extend,
                modifiers: gpui_kit::Modifiers { shift: true, ..Default::default() },
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
    })
    .unwrap();
    cx.update(|cx| {
        let selected = view.read(cx).terminal.as_ref().unwrap().selected_text().unwrap();
        assert!(selected.contains("lpha"), "anchor row must stay: {selected:?}");
        assert!(selected.contains("cha"), "span must reach the shift-click: {selected:?}");
    });
}

#[gpui_kit::test]
fn terminal_shift_click_extends_backward_from_anchor(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_terminal(cx);
    let (from, to, extend) = cx.update(|cx| {
        let (origin, row_height, char_width) = grid_geometry(&view, cx);
        (
            cell_center(origin, row_height, char_width, 2, 4),
            cell_center(origin, row_height, char_width, 2, 2),
            cell_center(origin, row_height, char_width, 0, 1),
        )
    });
    cx.update_window(window, |_, window, cx| {
        window.drag(from, to, cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        let _dispatched = window.dispatch_event(
            gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                button: MouseButton::Left,
                position: extend,
                modifiers: gpui_kit::Modifiers { shift: true, ..Default::default() },
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
    })
    .unwrap();
    cx.update(|cx| {
        let selected = view.read(cx).terminal.as_ref().unwrap().selected_text().unwrap();
        assert!(selected.contains("lpha"), "span must reach back to row 0: {selected:?}");
        assert!(selected.contains("charl"), "anchor row must stay: {selected:?}");
    });
}

#[gpui_kit::test]
fn terminal_shift_click_without_selection_acts_as_click(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_terminal(cx);
    let at = cx.update(|cx| {
        let (origin, row_height, char_width) = grid_geometry(&view, cx);
        cell_center(origin, row_height, char_width, 0, 2)
    });
    cx.update_window(window, |_, window, cx| {
        let _dispatched = window.dispatch_event(
            gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                button: MouseButton::Left,
                position: at,
                modifiers: gpui_kit::Modifiers { shift: true, ..Default::default() },
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
    })
    .unwrap();
    cx.update(|cx| {
        let state = view.read(cx);
        assert!(state.terminal_select_drag.is_some(), "shift-click must start a drag");
        assert_eq!(state.terminal_select_anchor, Some((0, 2)));
    });
}

#[gpui_kit::test]
fn terminal_click_without_drag_clears_selection(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_terminal(cx);
    cx.update_window(window, |_, window, cx| {
        window.press("cmd-a", cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).terminal.as_ref().unwrap().selected_text().is_some());
    });
    let at = cx.update(|cx| {
        let (origin, row_height, char_width) = grid_geometry(&view, cx);
        cell_center(origin, row_height, char_width, 0, 2)
    });
    cx.update_window(window, |_, window, cx| {
        window.drag(at, at, cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).terminal.as_ref().unwrap().selected_text().is_none());
    });
}
