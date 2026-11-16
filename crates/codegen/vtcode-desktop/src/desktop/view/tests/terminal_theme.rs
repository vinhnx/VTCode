use super::*;
use crate::desktop::preferences::TerminalAnsiTheme;

/// Grid cell of the palette probe: the exact cell the test wrote, so the
/// probe never has to recognize its own marker in the grid.
#[derive(Clone, Copy)]
struct ProbeCell {
    row: usize,
    column: usize,
}

/// Write the probe cell into the live session and return its position.
///
/// The session is a real login shell, and its prompt prints the workspace
/// path (the macOS shells embed the cwd basename), so a probe that scans
/// the grid for a marker character can read the prompt's color instead of
/// the probe's. Recording the cell keeps the probe exact.
fn write_probe_cell(view: &Entity<DesktopView>, cx: &mut TestAppContext, bytes: &[u8]) -> ProbeCell {
    view.update(cx, |view, _| {
        let tab = view.terminal.as_mut().unwrap();
        let cell = tab.visible_cursor_cell().expect("the terminal must show a cursor");
        let _events = tab.screen.process(bytes);
        ProbeCell { row: cell.0, column: cell.1 }
    })
}

/// Foreground red channel of the probe cell, as the live session renders
/// it against its current palette.
fn probe_fg_red(view: &Entity<DesktopView>, cx: &gpui_kit::App, cell: ProbeCell) -> Option<f32> {
    view.read(cx)
        .terminal
        .as_ref()
        .unwrap()
        .visible_cell_fg(cell.row, cell.column)
        .map(|fg| fg.r)
}

/// White-cell foreground flips between the dark and light ANSI tables
/// when the setting changes; proves the handler reaches the live session.
#[gpui_kit::test]
fn terminal_ansi_setting_flips_live_palette(cx: &mut TestAppContext) {
    let (_window, view, _dir) = open_workspace_terminal(cx);
    let probe = write_probe_cell(&view, cx, b"\x1b[37mX");
    // Dark table first: white slot is near-white.
    view.update(cx, |view, cx| {
        view.set_terminal_ansi_theme(TerminalAnsiTheme::Dark, cx);
    });
    cx.update(|cx| {
        let red = probe_fg_red(&view, cx, probe).expect("the probe cell must stay visible");
        assert!(red > 0.8, "the dark table paints white near-white, got {red}");
    });
    // Light table: the same cell renders dark gray.
    view.update(cx, |view, cx| {
        view.set_terminal_ansi_theme(TerminalAnsiTheme::Light, cx);
    });
    cx.update(|cx| {
        let red = probe_fg_red(&view, cx, probe).expect("the probe cell must stay visible");
        assert!(red < 0.5, "the light table paints white dark gray, got {red}");
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

/// The scheme buttons fix their height like the theme swatches: the 32px
/// preview chip plus the label must fit inside the Kit button, whose
/// content row clips overflow. Regression: the grid inherited the 32px
/// button height and clipped every label.
#[gpui_kit::test]
fn terminal_scheme_buttons_fit_preview_and_label(cx: &mut TestAppContext) {
    let (window, _view, _dir) = open_workspace_terminal(cx);
    cx.update_window(window, |_, window, cx| {
        window.click("open-settings", cx);
        window.render_frame(cx);
        for id in ["auto", "dark", "light", "ciapre", "latte"] {
            let bounds = window.find(format!("terminal-scheme-{id}")).bounds();
            assert_eq!(
                bounds.size.height,
                px(80.),
                "scheme button {id} must fit the 32px preview plus label, got {:?}",
                bounds.size.height
            );
        }
    })
    .unwrap();
}

/// Clicking a scheme swatch applies it live: red renders the authored
/// latte value, and the choice persists to preferences.
#[gpui_kit::test]
fn terminal_scheme_swatch_click_applies_latte(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    let probe = write_probe_cell(&view, cx, b"\x1b[31mX");
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
        let red = probe_fg_red(&view, cx, probe).expect("the probe cell must stay visible");
        assert!((red - 0xC2 as f32 / 255.0).abs() < 0.02, "latte red must render, got {red}");
    });
}

/// Ciapre selection re-palettes the live session: red renders the
/// authored alert-family value, and switching back to Dark flips it away.
#[gpui_kit::test]
fn terminal_ciapre_setting_repalettes_live_session(cx: &mut TestAppContext) {
    let (_window, view, _dir) = open_workspace_terminal(cx);
    let probe = write_probe_cell(&view, cx, b"\x1b[31mX");
    view.update(cx, |view, cx| {
        view.set_terminal_ansi_theme(TerminalAnsiTheme::Ciapre, cx);
    });
    let mut ciapre_red = 0.0;
    cx.update(|cx| {
        ciapre_red = probe_fg_red(&view, cx, probe).expect("the probe cell must stay visible");
        assert!((ciapre_red - 0xC1 as f32 / 255.0).abs() < 0.02, "ciapre red must render, got {ciapre_red}");
    });
    view.update(cx, |view, cx| {
        view.set_terminal_ansi_theme(TerminalAnsiTheme::Dark, cx);
    });
    cx.update(|cx| {
        let dark_red = probe_fg_red(&view, cx, probe).expect("the probe cell must stay visible");
        assert!((dark_red - ciapre_red).abs() > 0.05, "dark red must flip away from {ciapre_red}, got {dark_red}");
    });
}
