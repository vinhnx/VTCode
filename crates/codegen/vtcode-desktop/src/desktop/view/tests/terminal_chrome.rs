use super::*;
use crate::desktop::preferences::TerminalPosition;

#[gpui_kit::test]
fn terminal_dock_left_places_panel_beside_content(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    view.update(cx, |view, cx| {
        view.set_terminal_position(TerminalPosition::Left, cx);
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("terminal-resize-left").is_some());
        let panel = window.find("terminal-panel").bounds();
        let content = window.find("pane-root").bounds();
        assert!(panel.origin.x < content.origin.x, "panel must sit left of content");
        assert!((panel.origin.x + panel.size.width - content.origin.x).abs() < px(1.), "panel must abut content");
        assert!((panel.size.height - content.size.height).abs() < px(1.), "side panel must span content height");
    })
    .unwrap();
}

#[gpui_kit::test]
fn terminal_dock_right_places_panel_beside_content(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    view.update(cx, |view, cx| {
        view.set_terminal_position(TerminalPosition::Right, cx);
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("terminal-resize-right").is_some());
        let panel = window.find("terminal-panel").bounds();
        let content = window.find("pane-root").bounds();
        assert!(panel.origin.x > content.origin.x, "panel must sit right of content");
        assert!((panel.origin.x - (content.origin.x + content.size.width)).abs() < px(1.), "panel must abut content");
        assert!((panel.size.height - content.size.height).abs() < px(1.), "side panel must span content height");
    })
    .unwrap();
}

#[gpui_kit::test]
fn terminal_header_tab_close_hides_panel(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("terminal-tab").is_some(), "header tab must render");
        window.click("terminal-tab-close", cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(!view.read(cx).terminal_visible, "tab close must hide the panel");
    });
}

#[gpui_kit::test]
fn terminal_header_collapse_hides_panel(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    cx.update_window(window, |_, window, cx| {
        window.click("terminal-collapse", cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(!view.read(cx).terminal_visible, "collapse must hide the panel");
    });
}

#[gpui_kit::test]
fn terminal_grid_has_reference_padding(cx: &mut TestAppContext) {
    let (window, _view, _dir) = open_workspace_terminal(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let scroll = window.find("terminal-scroll").bounds();
        let row = window.find(("terminal-row", 0usize)).bounds();
        assert_eq!(row.origin.x - scroll.origin.x, px(16.));
        assert_eq!(row.origin.y - scroll.origin.y, px(8.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn terminal_tab_tracks_live_osc7_directory(cx: &mut TestAppContext) {
    let (_window, view, _dir) = open_workspace_terminal(cx);
    view.update(cx, |view, _| {
        let tab = view.terminal.as_mut().unwrap();
        assert_eq!(tab.current_directory(), None);
        let _events = tab.screen.process(b"\x1b]7;file://localhost/tmp/osc7-live\x07");
        assert_eq!(tab.current_directory().as_deref(), Some(Path::new("/tmp/osc7-live")));
    });
}

#[gpui_kit::test]
fn utilities_terminal_cta_opens_panel(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::ToggleTerminal), cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(!view.read(cx).terminal_visible, "setup must start hidden");
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_utilities(cx));
        window.render_frame(cx);
        window.click("utilities-terminal", cx);
    })
    .unwrap();
    cx.update(|cx| {
        let state = view.read(cx);
        assert!(state.terminal_visible, "CTA must show the panel");
        assert!(state.terminal.is_some(), "CTA must spawn the shell");
    });
}
