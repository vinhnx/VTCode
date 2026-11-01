use super::*;

#[gpui_kit::test]
fn transcript_arrows_navigate_messages_reveal_actions_and_escape_returns_to_composer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            for (id, role, label) in [
                ("nav-user-1", "user", "You"),
                ("nav-assistant-1", "assistant", "VT Code"),
                ("nav-assistant-2", "assistant", "VT Code"),
            ] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), role.into(), label.into(), format!("Body {id}")));
            }
        });
        window.render_frame(cx);
        focus_element_by_tab(window, "transcript-scroll", cx);
        assert!(view.read(cx).transcript_focus.is_focused(window));
        let row = window.find("transcript-row-nav-assistant-1").bounds();
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("nav-user-1"));
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("nav-assistant-1"));
        window.render_frame(cx);
        assert!(
            window.try_find("copy-message-nav-assistant-1").is_some(),
            "highlight reveals row actions like hover"
        );
        assert!(
            window.try_find("transcript-focus-rail-nav-assistant-1").is_some(),
            "highlight paints a visible rail"
        );
        assert_eq!(window.find("transcript-row-nav-assistant-1").bounds(), row, "highlight must not reflow the row");
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("nav-assistant-2"));
        window.press("down", cx);
        assert_eq!(
            view.read(cx).keyboard_focused_message.as_deref(),
            Some("nav-assistant-2"),
            "navigation clamps at the last message"
        );
        window.press("up", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("nav-assistant-1"));
        window.render_frame(cx);
        assert!(window.try_find("copy-message-nav-assistant-2").is_none(), "reveal follows the highlight");
        window.press("up", cx);
        window.press("up", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("nav-user-1"));
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "escape returns to the composer"
        );
        assert_eq!(
            view.read(cx).keyboard_focused_message.as_deref(),
            Some("nav-user-1"),
            "escape keeps the highlight as position memory"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn transcript_nav_skips_collapsed_activity_members(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            for (id, hidden) in [("nav-first", false), ("nav-hidden", true), ("nav-last", false)] {
                let mut item =
                    TranscriptItem::new(id.into(), "assistant".into(), "VT Code".into(), format!("Body {id}"));
                if hidden {
                    item.activity_grouped = true;
                    item.activity_group_head = false;
                    item.activity_expanded = false;
                }
                view.transcript.push(item);
            }
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.transcript_focus.focus(window, cx));
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("nav-first"));
        window.press("down", cx);
        assert_eq!(
            view.read(cx).keyboard_focused_message.as_deref(),
            Some("nav-last"),
            "collapsed members are skipped"
        );
        view.update(cx, |view, _| view.keyboard_focused_message = None);
        window.press("up", cx);
        assert_eq!(
            view.read(cx).keyboard_focused_message.as_deref(),
            Some("nav-last"),
            "up from nothing starts at the last visible message"
        );
    })
    .unwrap();
}
