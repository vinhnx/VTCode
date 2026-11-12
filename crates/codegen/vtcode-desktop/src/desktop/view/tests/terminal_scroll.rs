use super::*;

#[gpui_kit::test]
fn terminal_shift_navigation_scrolls_history(cx: &mut TestAppContext) {
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
    // Fill scrollback directly: deterministic, no shell timing.
    view.update(cx, |view, _| {
        let tab = view.terminal.as_mut().unwrap();
        for line in 0..30 {
            let _events = tab.screen.process(format!("history line {line:02}\n").as_bytes());
        }
        assert_eq!(tab.screen.grid().display_offset(), 0);
    });
    cx.update_window(window, |_, window, cx| {
        window.press("shift-pageup", cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).terminal.as_ref().unwrap().screen.grid().display_offset() > 0);
    });
    cx.update_window(window, |_, window, cx| {
        window.press("shift-end", cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(view.read(cx).terminal.as_ref().unwrap().screen.grid().display_offset(), 0);
    });
}

#[gpui_kit::test]
fn terminal_wheel_scrolls_history_both_directions(cx: &mut TestAppContext) {
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
        for line in 0..30 {
            let _events = tab.screen.process(format!("history line {line:02}\n").as_bytes());
        }
        assert_eq!(tab.screen.grid().display_offset(), 0);
    });
    // Wheel-up is positive delta.y; ten rows up at any default mono size.
    cx.update_window(window, |_, window, cx| {
        window.scroll("terminal-scroll", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(210.))), cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).terminal.as_ref().unwrap().screen.grid().display_offset() > 0);
    });
    // Symmetric wheel-down returns exactly to live.
    cx.update_window(window, |_, window, cx| {
        window.scroll("terminal-scroll", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-210.))), cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(view.read(cx).terminal.as_ref().unwrap().screen.grid().display_offset(), 0);
    });
}

#[gpui_kit::test]
fn terminal_typing_snaps_scrolled_viewport_home(cx: &mut TestAppContext) {
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
        for line in 0..30 {
            let _events = tab.screen.process(format!("history line {line:02}\n").as_bytes());
        }
    });
    cx.update_window(window, |_, window, cx| {
        window.press("shift-pageup", cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).terminal.as_ref().unwrap().screen.grid().display_offset() > 0);
    });
    cx.update_window(window, |_, window, cx| {
        window.press("a", cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(view.read(cx).terminal.as_ref().unwrap().screen.grid().display_offset(), 0);
    });
}
