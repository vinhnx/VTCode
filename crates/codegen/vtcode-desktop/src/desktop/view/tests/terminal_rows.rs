use super::super::terminal_panel::{TERMINAL_PUMP_IDLE_BACKOFF_TICKS, terminal_pump_interval};
use super::*;

#[gpui_kit::test]
fn terminal_pump_backs_off_when_quiet_and_wakes_on_activity(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        // A monitor has no shell to emit output, so the pump sees a quiet pane.
        let palette = super::super::super::terminal_tab::palette_for_theme(&cx.theme().colors, true);
        let size = portable_pty::PtySize { rows: 4, cols: 40, pixel_width: 0, pixel_height: 0 };
        view.terminal_monitor = Some(super::super::super::terminal_tab::TerminalTab::monitor(size, palette));
        view.terminal_monitor_row = Some("monitor-row".into());
        view.terminal_visible = true;
        view.terminal_pump_idle_ticks = 0;

        assert!(view.pump_terminal_once(view.terminal_pump_generation, cx));
        assert_eq!(view.terminal_pump_idle_ticks, 1);
        for expected in 2..=TERMINAL_PUMP_IDLE_BACKOFF_TICKS {
            assert!(view.pump_terminal_once(view.terminal_pump_generation, cx));
            assert_eq!(view.terminal_pump_idle_ticks, expected);
        }
        assert_eq!(terminal_pump_interval(view.terminal_pump_idle_ticks), Duration::from_millis(100));

        view.note_terminal_activity();
        assert_eq!(view.terminal_pump_idle_ticks, 0);
        assert_eq!(terminal_pump_interval(view.terminal_pump_idle_ticks), Duration::from_millis(16));
    });
}

#[gpui_kit::test]
fn terminal_activity_fires_the_pending_pump_waker(cx: &mut TestAppContext) {
    use futures::FutureExt;
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        let (wake, sleep) = futures::channel::oneshot::channel();
        view.terminal_pump_wake = Some(wake);
        view.note_terminal_activity();
        assert!(view.terminal_pump_wake.is_none(), "a fired waker is consumed once");
        assert!(matches!(sleep.now_or_never(), Some(Ok(()))), "activity must interrupt an idle sleep");
        // No pending waker: still idempotent for repeated activity.
        view.note_terminal_activity();
        assert_eq!(view.terminal_pump_idle_ticks, 0);
    });
}

#[gpui_kit::test]
fn terminal_wake_interrupts_an_idle_pump_sleep(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // A monitor has no shell output, so the spawned loop reaches backoff
    // deterministically and the only way to tick early is the waker.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_terminal_monitor("wake-row".into(), window, cx));
    })
    .unwrap();
    let background = cx.update(|cx| cx.background_executor().clone());
    background.advance_clock(Duration::from_millis(100));
    cx.run_until_parked();
    let idled = cx.update(|cx| view.read(cx).terminal_pump_idle_ticks);
    assert!(idled >= TERMINAL_PUMP_IDLE_BACKOFF_TICKS, "pump must be sleeping at idle cadence: {idled}");

    view.update(cx, |view, _| view.note_terminal_activity());
    // Far short of the 100 ms idle interval: only the waker can end this sleep.
    background.advance_clock(Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(
        cx.update(|cx| view.read(cx).terminal_pump_idle_ticks),
        1,
        "activity must wake the pump before the idle timer expires"
    );
}

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
