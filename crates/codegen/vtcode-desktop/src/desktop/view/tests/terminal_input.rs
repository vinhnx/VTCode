use super::*;

fn press_keys(window: AnyWindowHandle, keys: &[&str], cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        for key in keys {
            window.press(key, cx);
        }
    })
    .unwrap();
}

fn grid_text(view: &DesktopView) -> String {
    view.terminal
        .as_ref()
        .unwrap()
        .screen
        .grid()
        .display_iter()
        .map(|cell| cell.square.c())
        .collect()
}

/// Pump until the grid shows `marker` `count` times (login shells source
/// profiles; wait generously, assert fast), then settle a frame. Typed input
/// echoes, so executed output needs a count of two to prove the shell ran it.
fn pump_until_grid_count(
    window: AnyWindowHandle,
    view: &Entity<DesktopView>,
    marker: &str,
    count: usize,
    cx: &mut TestAppContext,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let (found, text) = view.update(cx, |view, cx| {
            assert!(view.pump_terminal_once(view.terminal_pump_generation, cx));
            let text = grid_text(view);
            (text.match_indices(marker).count() >= count, text)
        });
        if found {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "grid never showed {marker} {count}x; grid was {text:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn terminal_typing_executes_in_shell(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    press_keys(window, &["e", "c", "h", "o", "space", "z", "x", "q", "j", "9", "enter"], cx);
    pump_until_grid_count(window, &view, "zxqj9", 2, cx);
}

#[gpui_kit::test]
fn terminal_backspace_erases_before_execute(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    press_keys(
        window,
        &[
            "e",
            "c",
            "h",
            "o",
            "space",
            "z",
            "x",
            "q",
            "X",
            "backspace",
            "j",
            "9",
            "enter",
        ],
        cx,
    );
    pump_until_grid_count(window, &view, "zxqj9", 1, cx);
    view.update(cx, |view, _| {
        assert!(!grid_text(view).contains("zxqXqj9"), "X must be erased, not executed");
    });
}

#[gpui_kit::test]
fn terminal_ctrl_c_interrupts_foreground_job(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    // Byte order, not timing: the shell forks sleep, then reads ETX while it
    // runs. If the interrupt failed, the echo below would queue behind the
    // 30s sleep and the pump below would time out instead of passing.
    press_keys(window, &["s", "l", "e", "e", "p", "space", "3", "0", "enter"], cx);
    press_keys(window, &["ctrl-c"], cx);
    press_keys(window, &["e", "c", "h", "o", "space", "z", "x", "q", "j", "9", "enter"], cx);
    pump_until_grid_count(window, &view, "zxqj9", 2, cx);
}

#[gpui_kit::test]
fn terminal_cmd_v_pastes_clipboard(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    cx.update(|cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("PASTE_ME".to_string())));
    cx.update_window(window, |_, window, cx| {
        window.press("cmd-v", cx);
    })
    .unwrap();
    pump_until_grid_count(window, &view, "PASTE_ME", 1, cx);
}

#[gpui_kit::test]
fn terminal_select_all_and_copy_round_trips_viewport_text(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    press_keys(window, &["e", "c", "h", "o", "space", "c", "o", "p", "y", "m", "e", "enter"], cx);
    pump_until_grid_count(window, &view, "copyme", 2, cx);
    cx.update_window(window, |_, window, cx| {
        window.press("cmd-a", cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.press("cmd-c", cx);
    })
    .unwrap();
    cx.update(|cx| {
        let copied = cx.read_from_clipboard().and_then(|item| item.text()).unwrap();
        assert!(copied.contains("copyme"), "{copied:?}");
    });
    view.update(cx, |view, _| {
        assert!(view.terminal.as_ref().unwrap().selected_text().is_some(), "copy keeps the selection");
    });
}

#[gpui_kit::test]
fn terminal_escape_clears_selection(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    press_keys(window, &["e", "c", "h", "o", "space", "s", "e", "l", "x", "enter"], cx);
    pump_until_grid_count(window, &view, "selx", 2, cx);
    cx.update_window(window, |_, window, cx| {
        window.press("cmd-a", cx);
    })
    .unwrap();
    view.update(cx, |view, _| {
        assert!(view.terminal.as_ref().unwrap().selected_text().is_some());
    });
    cx.update_window(window, |_, window, cx| {
        window.press("escape", cx);
    })
    .unwrap();
    view.update(cx, |view, _| {
        assert!(view.terminal.as_ref().unwrap().selected_text().is_none());
    });
}

#[gpui_kit::test]
fn terminal_escape_reaches_shell_without_selection(cx: &mut TestAppContext) {
    let (window, view, _dir) = open_workspace_terminal(cx);
    press_keys(window, &["e", "c", "h", "o", "space", "r", "d", "y", "enter"], cx);
    pump_until_grid_count(window, &view, "rdy", 2, cx);
    // `command` bypasses user aliases/functions (a `cat=bat` alias turns
    // `cat -v` into a clap error and the escape never reaches a reader).
    press_keys(
        window,
        &[
            "c", "o", "m", "m", "a", "n", "d", "space", "c", "a", "t", "space", "-", "v", "enter",
        ],
        cx,
    );
    // Let the fork land: an escape pressed while zsh still holds the line
    // is swallowed by line editing instead of reaching cat.
    std::thread::sleep(Duration::from_millis(500));
    press_keys(window, &["escape"], cx);
    pump_until_grid_count(window, &view, "^[", 1, cx);
}
