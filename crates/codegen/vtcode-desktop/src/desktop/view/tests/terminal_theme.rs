use super::*;
use crate::desktop::preferences::TerminalAnsiTheme;

/// White-cell foreground flips between the dark and light ANSI tables
/// when the setting changes; proves the handler reaches the live session.
#[gpui_kit::test]
fn terminal_ansi_setting_flips_live_palette(cx: &mut TestAppContext) {
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
    let white_red = |view: &Entity<DesktopView>, cx: &gpui_kit::App| {
        view.read(cx)
            .terminal
            .as_ref()
            .unwrap()
            .display_rows()
            .into_iter()
            .flatten()
            .find(|run| run.text.contains('X'))
            .map(|run| run.fg.r)
    };
    view.update(cx, |view, _| {
        let tab = view.terminal.as_mut().unwrap();
        let _events = tab.screen.process(b"\x1b[37mX");
    });
    // Dark table first: white slot is near-white.
    view.update(cx, |view, cx| {
        view.set_terminal_ansi_theme(TerminalAnsiTheme::Dark, cx);
    });
    cx.update(|cx| {
        assert!(white_red(&view, cx).unwrap() > 0.8);
    });
    // Light table: the same cell renders dark gray.
    view.update(cx, |view, cx| {
        view.set_terminal_ansi_theme(TerminalAnsiTheme::Light, cx);
    });
    cx.update(|cx| {
        assert!(white_red(&view, cx).unwrap() < 0.5);
    });
}
