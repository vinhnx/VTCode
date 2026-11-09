use super::*;

use crate::desktop::inspector::InspectorTab;

#[gpui_kit::test]
fn inspector_card_sits_beside_the_workspace_with_an_even_gap(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open, "suite baseline opens the inspector");
        let pane = window.find("pane-root").bounds();
        let workspace = window.find("workspace-surface").bounds();
        let inspector = window.find("inspector-panel").bounds();
        assert!(
            inspector.left() > workspace.right(),
            "inspector must separate from the workspace card: workspace={workspace:?}, inspector={inspector:?}"
        );
        let outer = workspace.left() - pane.left();
        assert!(
            outer > px(0.),
            "workspace card must sit inside the pane padding: workspace={workspace:?}, pane={pane:?}"
        );
        assert_eq!(
            inspector.left() - workspace.right(),
            outer,
            "card gap must match the pane rhythm: workspace={workspace:?}, inspector={inspector:?}, pane={pane:?}"
        );
        assert_eq!(
            pane.right() - inspector.right(),
            outer,
            "inspector must hug the trailing pane band: inspector={inspector:?}, pane={pane:?}"
        );
        assert_eq!(
            inspector.top(),
            workspace.top(),
            "sibling cards must share the top edge: workspace={workspace:?}, inspector={inspector:?}"
        );
        assert_eq!(
            inspector.bottom(),
            workspace.bottom(),
            "sibling cards must share the bottom edge: workspace={workspace:?}, inspector={inspector:?}"
        );
        assert!(window.try_find("resize-inspector").is_some(), "the divider must move with the separated card");
        let conversation = window.find("conversation-column").bounds();
        let horizontal = conversation.left() - workspace.left();
        assert_eq!(
            workspace.right() - conversation.right(),
            horizontal,
            "conversation must sit centered in its card: conversation={conversation:?}, workspace={workspace:?}"
        );
        assert!(
            horizontal > px(0.),
            "conversation must keep the card padding: conversation={conversation:?}, workspace={workspace:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn each_inspector_tab_mounts_its_own_container(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let tabs = [
        (InspectorTab::Changes, "inspector-changes-tab"),
        (InspectorTab::Files, "inspector-files-tab"),
        (InspectorTab::Activity, "activity-list"),
        (InspectorTab::Output, "inspector-output-tab"),
        (InspectorTab::Utilities, "utilities-pane"),
    ];
    cx.update_window(window, |_, window, cx| {
        for (tab, id) in tabs {
            view.update(cx, |view, cx| {
                view.inspector_tab = tab;
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.find(id).visible(), "{tab:?} must mount its container");
            for (other_tab, other_id) in tabs {
                if other_tab != tab {
                    assert!(
                        window.try_find(other_id).is_none(),
                        "{other_tab:?} container must unmount while {tab:?} is selected"
                    );
                }
            }
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn inspector_close_button_collapses_the_card_and_round_trips_through_the_toolbar(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open, "suite baseline opens the inspector");
        assert_eq!(window.find("close-inspector").role(), Some(gpui_kit::Role::Button));
        assert_eq!(window.find("close-inspector").label(), Some("Hide panel"));
        view.update(cx, |view, cx| view.inspector_focus.focus(window, cx));
        window.render_frame(cx);

        window.click("close-inspector", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).inspector_open);
        assert_eq!(view.read(cx).preferences.inspector_open, Some(false));
        assert!(window.try_find("inspector-panel").is_none());
        assert!(window.try_find("resize-inspector").is_none());
        assert!(
            view.read(cx).focus_handle.is_focused(window),
            "closing a focused inspector must restore the desktop focus path"
        );

        // Close is a collapse, not a teardown: the toolbar toggle brings the card back.
        window.click("toggle-inspector", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).preferences.inspector_open, Some(true));
        assert!(window.try_find("inspector-panel").is_some());
        assert!(window.try_find("close-inspector").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn toggle_inspector_in_zen_reveals_instead_of_flipping(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        // Closed in zen: the toggle reveals rather than flipping
        // invisible state.
        view.update(cx, |view, _| {
            view.zen_mode = true;
            view.inspector_open = false;
            view.inspector_tab = InspectorTab::Utilities;
        });
        window.render_frame(cx);
        assert!(window.try_find("inspector-panel").is_none());
        view.update(cx, |view, cx| view.toggle_inspector(cx));
        window.render_frame(cx);
        assert!(!view.read(cx).zen_mode, "toggle reveals by exiting zen");
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).preferences.inspector_open, Some(true));
        assert!(window.try_find("utilities-pane").is_some());

        // Open-but-hidden in zen: a naive flip would close (and persist)
        // something the user never saw; reveal keeps it open.
        view.update(cx, |view, _| {
            view.zen_mode = true;
            view.inspector_open = true;
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.toggle_inspector(cx));
        window.render_frame(cx);
        assert!(!view.read(cx).zen_mode);
        assert!(view.read(cx).inspector_open, "reveal must not close hidden state");
        assert_eq!(view.read(cx).preferences.inspector_open, Some(true));
        assert!(window.try_find("inspector-panel").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn closing_the_inspector_gives_the_workspace_the_full_row(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("inspector-panel").is_some());
        view.update(cx, |view, cx| view.toggle_inspector(cx));
        window.render_frame(cx);
        assert!(!view.read(cx).inspector_open);
        assert!(window.try_find("inspector-panel").is_none());
        assert!(window.try_find("resize-inspector").is_none());
        let pane = window.find("pane-root").bounds();
        let workspace = window.find("workspace-surface").bounds();
        let outer = workspace.left() - pane.left();
        assert_eq!(
            pane.right() - workspace.right(),
            outer,
            "closed inspector must return the full row to the workspace: workspace={workspace:?}, pane={pane:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn inspector_overflow_has_a_name_and_selects_a_tab(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 900.);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("inspector-tabs-menu").label(), Some("Context inspector tabs"));
        window.click("inspector-tabs-menu", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("popup-menu").role(), Some(gpui_kit::Role::Menu));
        let trigger = window.find("inspector-tabs-menu").bounds();
        let popup = window.find("popup-menu").bounds();
        let trigger_right = trigger.origin.x + trigger.size.width;
        let popup_right = popup.origin.x + popup.size.width;
        assert!(
            popup_right <= trigger_right + px(1.) && popup_right >= trigger_right - px(1.),
            "right-edge trigger opens its menu leftward: popup={popup:?}, trigger={trigger:?}"
        );
        for _ in 0..5 {
            window.press("down", cx);
        }
        window.press("enter", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Utilities);
    })
    .unwrap();
}
