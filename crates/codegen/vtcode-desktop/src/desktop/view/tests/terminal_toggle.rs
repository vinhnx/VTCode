use super::*;
use crate::desktop::strings::key_context;

/// Dispatch the terminal toggle and settle the frame so the panel (and focus)
/// can be observed.
#[gpui_kit::test]
fn terminal_panel_click_focuses_it(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let dir = tempfile::tempdir().unwrap();
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.prepare_workspace(dir.path(), cx);
        });
    })
    .unwrap();
    toggle_terminal(window, cx);
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::FocusComposer), cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        assert!(!view.read(cx).terminal_focus.is_focused(window), "setup must unfocus the panel");
        window.click("terminal-panel", cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        assert!(view.read(cx).terminal_focus.is_focused(window), "click must focus the panel");
    })
    .unwrap();
}

fn toggle_terminal(window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::ToggleTerminal), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();
}

fn prepare_workspace(view: &Entity<DesktopView>, workspace: &Path, cx: &mut TestAppContext) {
    view.update(cx, |view, cx| {
        view.prepare_workspace(workspace, cx);
    });
}

#[gpui_kit::test]
fn terminal_toggle_binds_cmd_j(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    cx.update(|cx| {
        let preferences = view.read(cx).preferences.clone();
        super::super::bootstrap::init_keybindings(cx, &preferences);
        assert_chord_resolves_to(cx, "cmd-j", key_context::DESKTOP, &super::super::ToggleTerminal);
    });
}

#[gpui_kit::test]
fn terminal_toggle_spawns_workspace_shell_and_focuses_panel(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let dir = tempfile::tempdir().unwrap();
    prepare_workspace(&view, dir.path(), cx);
    toggle_terminal(window, cx);
    cx.update_window(window, |_, window, cx| {
        assert!(window.try_find("terminal-panel").is_some(), "toggle must render the panel");
        let view = view.read(cx);
        assert!(view.terminal_visible, "toggle must show the panel");
        assert!(view.terminal.is_some(), "first toggle must spawn the workspace shell");
        assert!(view.terminal_focus.is_focused(window), "toggle must focus the panel");
    })
    .unwrap();
}

#[gpui_kit::test]
fn terminal_second_toggle_hides_panel_but_keeps_session(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let dir = tempfile::tempdir().unwrap();
    prepare_workspace(&view, dir.path(), cx);
    toggle_terminal(window, cx);
    toggle_terminal(window, cx);
    cx.update_window(window, |_, window, cx| {
        assert!(window.try_find("terminal-panel").is_none(), "second toggle must hide the panel");
        let view = view.read(cx);
        assert!(!view.terminal_visible, "panel must be hidden");
        assert!(view.terminal.is_some(), "hiding must keep the session alive");
    })
    .unwrap();
}

#[gpui_kit::test]
fn terminal_workspace_switch_closes_session_and_hides_panel(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let dir = tempfile::tempdir().unwrap();
    prepare_workspace(&view, dir.path(), cx);
    toggle_terminal(window, cx);
    prepare_workspace(&view, Path::new("/work/other"), cx);
    cx.run_until_parked();
    view.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("terminal-panel").is_none(), "switch must hide the panel");
        let view = view.read(cx);
        assert!(!view.terminal_visible, "panel must be hidden after switch");
        assert!(view.terminal.is_none(), "switch must close the session");
    })
    .unwrap();
}

#[gpui_kit::test]
fn terminal_toggle_without_workspace_spawns_at_home(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = None;
    });
    toggle_terminal(window, cx);
    cx.update_window(window, |_, window, cx| {
        assert!(window.try_find("terminal-panel").is_some(), "panel must render without a workspace");
        let view = view.read(cx);
        assert!(view.terminal_visible, "toggle must show the panel");
        assert!(view.terminal.is_some(), "shell must spawn at the home fallback");
    })
    .unwrap();
    assert!(!grid_text_settled(&view, cx).trim().is_empty(), "home shell must produce output");
    // First workspace keeps the running shell; only real switches close it.
    let dir = tempfile::tempdir().unwrap();
    prepare_workspace(&view, dir.path(), cx);
    cx.update(|cx| {
        let state = view.read(cx);
        assert!(state.terminal_visible, "first workspace must keep the panel");
        assert!(state.terminal.is_some(), "first workspace must keep the shell");
    });
}

#[gpui_kit::test]
fn terminal_rail_button_toggles_panel(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let dir = tempfile::tempdir().unwrap();
    prepare_workspace(&view, dir.path(), cx);
    cx.update_window(window, |_, window, cx| {
        window.click("nav-terminal", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let state = view.read(cx);
        assert!(state.terminal_visible, "rail click must show the panel");
        assert!(state.terminal.is_some(), "rail click must spawn the shell");
    });
    cx.update_window(window, |_, window, cx| {
        window.click("nav-terminal", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(!view.read(cx).terminal_visible, "second rail click must hide the panel");
    });
}

#[gpui_kit::test]
fn terminal_connections_cta_runs_fork_in_panel(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let dir = tempfile::tempdir().unwrap();
    prepare_workspace(&view, dir.path(), cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("sess-1".to_string());
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        select_settings_category(window, 4, cx);
        window.render_frame(cx);
        assert!(window.try_find("continue-builtin-terminal").is_some());
        window.click("continue-builtin-terminal", cx);
    })
    .unwrap();
    cx.update(|cx| {
        let state = view.read(cx);
        assert!(state.terminal_visible, "CTA must show the panel");
        assert!(state.terminal.is_some(), "CTA must spawn the shell");
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        pump_settle(cx);
        let text = cx.update(|cx| grid_text(&view, cx));
        if text.contains("--fork-session") && text.contains("sess-1") {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "shell must receive the fork command for sess-1: {text:?}");
    }
}

#[gpui_kit::test]
fn terminal_pump_loop_ticks_without_manual_pumps(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let dir = tempfile::tempdir().unwrap();
    prepare_workspace(&view, dir.path(), cx);
    toggle_terminal(window, cx);
    // Retention pin: dropping the GPUI task cancels the loop, which froze
    // the real panel while every manual-pump test stayed green.
    cx.update(|cx| {
        assert!(view.read(cx).terminal_pump_task.is_some(), "show must retain the pump loop");
    });
    assert!(!grid_text_settled(&view, cx).trim().is_empty(), "spawned loop must drain shell output");
    toggle_terminal(window, cx);
    cx.update(|cx| {
        assert!(view.read(cx).terminal_pump_task.is_none(), "hide must cancel the pump loop");
    });
}

/// Advance the headless clock past several pump intervals so the spawned
/// loop (not a manual pump call) drains pending shell output.
fn pump_settle(cx: &mut TestAppContext) {
    let background = cx.update(|cx| cx.background_executor().clone());
    background.advance_clock(Duration::from_millis(100));
    cx.run_until_parked();
}

/// Grid text after the spawned loop drains, waiting out slow shell startup.
fn grid_text_settled(view: &Entity<DesktopView>, cx: &mut TestAppContext) -> String {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        pump_settle(cx);
        let text = cx.update(|cx| grid_text(view, cx));
        if !text.trim().is_empty() || std::time::Instant::now() >= deadline {
            return text;
        }
    }
}

fn grid_text(view: &Entity<DesktopView>, cx: &gpui_kit::App) -> String {
    view.read(cx)
        .terminal
        .as_ref()
        .unwrap()
        .display_rows()
        .into_iter()
        .flatten()
        .map(|run| run.text)
        .collect()
}
