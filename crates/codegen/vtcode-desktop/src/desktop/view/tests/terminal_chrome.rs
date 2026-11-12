use super::*;
use crate::desktop::preferences::TerminalPosition;

#[gpui_kit::test]
fn terminal_dock_bottom_floats_card_on_chrome(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    view.update(cx, |view, _| {
        view.preferences.show_status_bar = true;
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let panel = window.find("terminal-panel").bounds();
        let workspace = window.find("workspace-surface").bounds();
        let status = window.find("status-bar").bounds();
        let handle = window.find("terminal-resize").bounds();
        assert!(
            handle.origin.y <= panel.origin.y && panel.origin.y <= handle.origin.y + handle.size.height,
            "drag edge must hug the panel top",
        );
        assert!(
            handle.origin.x <= panel.origin.x
                && panel.origin.x + panel.size.width <= handle.origin.x + handle.size.width,
            "drag edge must span the panel width",
        );
        assert_eq!(panel.origin.y - (workspace.origin.y + workspace.size.height), px(12.));
        assert_eq!(status.origin.y - (panel.origin.y + panel.size.height), px(12.));
        assert_eq!(panel.origin.x, workspace.origin.x, "card must share the workspace spine");
    })
    .unwrap();
}

#[gpui_kit::test]
fn terminal_dock_left_places_panel_beside_content(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    view.update(cx, |view, cx| {
        view.inspector_open = false;
        view.set_terminal_position(TerminalPosition::Left, cx);
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("terminal-resize-left").is_some());
        let panel = window.find("terminal-panel").bounds();
        let content = window.find("pane-root").bounds();
        let workspace = window.find("workspace-surface").bounds();
        let handle = window.find("terminal-resize-left").bounds();
        assert!(
            handle.origin.x <= panel.origin.x + panel.size.width
                && panel.origin.x + panel.size.width <= handle.origin.x + handle.size.width,
            "drag edge must hug the panel inner edge",
        );
        assert!(
            handle.origin.y <= panel.origin.y
                && panel.origin.y + panel.size.height <= handle.origin.y + handle.size.height,
            "drag edge must span the panel height",
        );
        assert!(panel.origin.x < content.origin.x, "panel must sit left of content");
        assert!((panel.origin.x + panel.size.width - content.origin.x).abs() < px(1.), "panel must abut content");
        assert_eq!(workspace.origin.x - (panel.origin.x + panel.size.width), px(12.));
        assert_eq!(panel.origin.y, workspace.origin.y, "card must share the workspace top spine");
        assert_eq!(
            panel.origin.y + panel.size.height,
            workspace.origin.y + workspace.size.height,
            "card must share the workspace bottom spine",
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn terminal_dock_right_places_panel_beside_content(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    view.update(cx, |view, cx| {
        view.inspector_open = false;
        view.set_terminal_position(TerminalPosition::Right, cx);
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("terminal-resize-right").is_some());
        let panel = window.find("terminal-panel").bounds();
        let content = window.find("pane-root").bounds();
        let workspace = window.find("workspace-surface").bounds();
        let handle = window.find("terminal-resize-right").bounds();
        assert!(
            handle.origin.x <= panel.origin.x && panel.origin.x <= handle.origin.x + handle.size.width,
            "drag edge must hug the panel inner edge",
        );
        assert!(
            handle.origin.y <= panel.origin.y
                && panel.origin.y + panel.size.height <= handle.origin.y + handle.size.height,
            "drag edge must span the panel height",
        );
        assert!(panel.origin.x > content.origin.x, "panel must sit right of content");
        assert!((panel.origin.x - (content.origin.x + content.size.width)).abs() < px(1.), "panel must abut content");
        assert_eq!(panel.origin.x - (workspace.origin.x + workspace.size.width), px(12.));
        assert_eq!(panel.origin.y, workspace.origin.y, "card must share the workspace top spine");
        assert_eq!(
            panel.origin.y + panel.size.height,
            workspace.origin.y + workspace.size.height,
            "card must share the workspace bottom spine",
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn terminal_dock_zen_stays_flush(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    view.update(cx, |view, cx| {
        view.zen_mode = true;
        cx.notify();
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let panel = window.find("terminal-panel").bounds();
        let content = window.find("pane-root").bounds();
        assert_eq!(panel.origin.y, content.origin.y + content.size.height, "zen panel must abut content");
        assert_eq!(panel.origin.x, content.origin.x, "zen panel must share the content left spine");
        assert_eq!(
            panel.origin.x + panel.size.width,
            content.origin.x + content.size.width,
            "zen panel must share the content right spine",
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn terminal_dock_zen_side_stays_flush(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    view.update(cx, |view, cx| {
        view.zen_mode = true;
        view.set_terminal_position(TerminalPosition::Left, cx);
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let panel = window.find("terminal-panel").bounds();
        let content = window.find("pane-root").bounds();
        assert_eq!(panel.origin.x + panel.size.width, content.origin.x, "zen panel must abut content");
        assert_eq!(panel.origin.y, content.origin.y, "zen panel must share the content top spine");
        assert_eq!(
            panel.origin.y + panel.size.height,
            content.origin.y + content.size.height,
            "zen panel must share the content bottom spine",
        );
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
