use super::*;
use crate::desktop::preferences::TerminalAnsiTheme;

/// Foreground red channel of the `X` probe cell, if the live session
/// rendered it.
fn probe_fg_red(view: &Entity<DesktopView>, cx: &gpui_kit::App) -> Option<f32> {
    view.read(cx)
        .terminal
        .as_ref()
        .unwrap()
        .display_rows()
        .into_iter()
        .flatten()
        .find(|run| run.text.contains('X'))
        .map(|run| run.fg.r)
}

/// White-cell foreground flips between the dark and light ANSI tables
/// when the setting changes; proves the handler reaches the live session.
#[gpui_kit::test]
fn terminal_ansi_setting_flips_live_palette(cx: &mut TestAppContext) {
    let (_window, view, _dir) = open_workspace_terminal(cx);
    view.update(cx, |view, _| {
        let tab = view.terminal.as_mut().unwrap();
        let _events = tab.screen.process(b"\x1b[37mX");
    });
    // Dark table first: white slot is near-white.
    view.update(cx, |view, cx| {
        view.set_terminal_ansi_theme(TerminalAnsiTheme::Dark, cx);
    });
    cx.update(|cx| {
        assert!(probe_fg_red(&view, cx).unwrap() > 0.8);
    });
    // Light table: the same cell renders dark gray.
    view.update(cx, |view, cx| {
        view.set_terminal_ansi_theme(TerminalAnsiTheme::Light, cx);
    });
    cx.update(|cx| {
        assert!(probe_fg_red(&view, cx).unwrap() < 0.5);
    });
}

/// The scheme grid renders one swatch per option with the saved choice
/// toggled (fresh installs default to Ciapre).
#[gpui_kit::test]
fn terminal_scheme_grid_marks_active_swatch(cx: &mut TestAppContext) {
    let (window, _view, _dir) = open_workspace_terminal(cx);
    cx.update_window(window, |_, window, cx| {
        window.click("open-settings", cx);
        window.render_frame(cx);
        for id in ["auto", "dark", "light", "ciapre", "latte"] {
            assert!(window.try_find(format!("terminal-scheme-{id}")).is_some(), "scheme swatch {id} must render");
        }
        assert_eq!(window.find("terminal-scheme-ciapre").checked(), Some(true));
    })
    .unwrap();
}

/// Clicking a scheme swatch applies it live: red renders the authored
/// latte value, and the choice persists to preferences.
#[gpui_kit::test]
fn terminal_scheme_swatch_click_applies_latte(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    view.update(cx, |view, _| {
        let tab = view.terminal.as_mut().unwrap();
        let _events = tab.screen.process(b"\x1b[31mX");
    });
    cx.update_window(window, |_, window, cx| {
        window.click("open-settings", cx);
        window.render_frame(cx);
        scroll_settings_into_view(window, "terminal-scheme-latte", cx);
        window.click("terminal-scheme-latte", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(view.read(cx).preferences.terminal_ansi_theme(), TerminalAnsiTheme::Latte);
        let red = probe_fg_red(&view, cx).unwrap();
        assert!((red - 0xC2 as f32 / 255.0).abs() < 0.02, "latte red must render, got {red}");
    });
}

/// Ciapre selection re-palettes the live session: red renders the
/// authored alert-family value, and switching back to Dark flips it away.
#[gpui_kit::test]
fn terminal_ciapre_setting_repalettes_live_session(cx: &mut TestAppContext) {
    let (_window, view, _dir) = open_workspace_terminal(cx);
    view.update(cx, |view, _| {
        let tab = view.terminal.as_mut().unwrap();
        let _events = tab.screen.process(b"\x1b[31mX");
    });
    view.update(cx, |view, cx| {
        view.set_terminal_ansi_theme(TerminalAnsiTheme::Ciapre, cx);
    });
    let mut ciapre_red = 0.0;
    cx.update(|cx| {
        ciapre_red = probe_fg_red(&view, cx).unwrap();
        assert!((ciapre_red - 0xC1 as f32 / 255.0).abs() < 0.02, "ciapre red must render, got {ciapre_red}");
    });
    view.update(cx, |view, cx| {
        view.set_terminal_ansi_theme(TerminalAnsiTheme::Dark, cx);
    });
    cx.update(|cx| {
        let dark_red = probe_fg_red(&view, cx).unwrap();
        assert!((dark_red - ciapre_red).abs() > 0.05, "dark red must flip away from {ciapre_red}, got {dark_red}");
    });
}
