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
