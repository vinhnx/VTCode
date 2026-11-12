use super::*;

#[gpui_kit::test]
fn terminal_pump_renders_drained_output_rows(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // No session: the pump exits immediately.
    view.update(cx, |view, cx| {
        assert!(!view.pump_terminal_once(view.terminal_pump_generation, cx));
    });
    let dir = tempfile::tempdir().unwrap();
    view.update(cx, |view, cx| {
        view.prepare_workspace(dir.path(), cx);
    });
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::ToggleTerminal), cx);
    })
    .unwrap();
    cx.run_until_parked();
    // Stale generations exit instead of double-pumping.
    view.update(cx, |view, cx| {
        assert!(!view.pump_terminal_once(view.terminal_pump_generation.wrapping_add(1), cx));
        assert!(view.pump_terminal_once(view.terminal_pump_generation, cx));
        view.terminal
            .as_ref()
            .unwrap()
            .input(b"echo ROW_$(echo MARKER)\n".to_vec())
            .unwrap();
    });
    // Login shells source profiles; wait generously, assert fast. The marker
    // only reads contiguous after the shell expands the substitution, so the
    // input echo alone cannot satisfy this.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let rendered = view.update(cx, |view, cx| {
            assert!(view.pump_terminal_once(view.terminal_pump_generation, cx));
            view.terminal
                .as_ref()
                .unwrap()
                .screen
                .grid()
                .display_iter()
                .map(|cell| cell.square.c())
                .collect::<String>()
                .contains("ROW_MARKER")
        });
        if rendered {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "shell output never reached the grid");
        std::thread::sleep(Duration::from_millis(10));
    }
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();
    let expected = cx.update(|cx| {
        let height = view.read(cx).terminal_panel_height;
        terminal_content_rows(height, cx.theme().mono_font_size).unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        assert!(window.try_find("terminal-panel").is_some(), "panel must render");
        assert_eq!(
            view.read(cx).terminal.as_ref().unwrap().display_rows().len(),
            usize::from(expected),
            "model must project computed rows"
        );
        let rows = (0usize..100)
            .take_while(|index| window.try_find(("terminal-row", *index)).is_some())
            .count();
        assert_eq!(rows, usize::from(expected), "panel must render one row per grid line");
    })
    .unwrap();
}
