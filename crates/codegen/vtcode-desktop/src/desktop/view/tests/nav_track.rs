use super::*;

#[gpui_kit::test]
fn nav_tracks_cap_icons_and_route_the_rest_to_overflow(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(std::env::temp_dir());
        view.sidebar_open = false;
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);

        // Rail: three pinned icons plus one overflow; nothing else competes.
        for id in ["nav-home", "nav-new-thread", "nav-browse-threads", "nav-overflow"] {
            assert!(window.try_find(id).is_some(), "{id} must stay in the rail track");
        }
        for id in ["nav-utilities", "nav-terminal", "nav-update"] {
            assert!(window.try_find(id).is_none(), "{id} must live in the rail overflow");
        }

        // Toolbar: Search, one overflow, and the pinned Zen toggle.
        for id in [
            "navigate-back",
            "navigate-forward",
            "search-commands",
            "header-overflow",
            "zen-mode-toggle",
        ] {
            assert!(window.try_find(id).is_some(), "{id} must stay in the chrome track");
        }
        for id in [
            "toggle-sidebar",
            "toggle-inspector",
            "top-new-thread",
            "top-utilities",
            "open-settings",
        ] {
            assert!(window.try_find(id).is_none(), "{id} must live in the toolbar overflow");
        }

        // The rail overflow opens a real menu for the moved actions.
        window.click("nav-overflow", cx);
        window.render_frame(cx);
        assert_eq!(
            window.within("dropdown-menu:Name(\"nav-overflow\")").find("popup-menu").role(),
            Some(gpui_kit::Role::Menu)
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn overflow_menus_pin_their_items(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(std::env::temp_dir());
        view.sidebar_open = false;
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);

        window.click("nav-overflow", cx);
        window.render_frame(cx);
        let rail = window.within("dropdown-menu:Name(\"nav-overflow\")");
        for (index, label) in ["Workspace utilities", "Terminal", "Settings…"].into_iter().enumerate() {
            assert_eq!(rail.find(index).label(), Some(label), "rail overflow item {index}");
        }
        window.press("escape", cx);
        window.render_frame(cx);

        window.click("header-overflow", cx);
        window.render_frame(cx);
        let header = window.within("dropdown-menu:Name(\"header-overflow\")");
        let items = [
            "New thread",
            "Close thread",
            "Toggle navigation",
            "Toggle inspector",
            "Toggle Zen mode",
            "Search commands",
            "Toggle status bar",
            "Settings…",
            "Workspace utilities",
        ];
        for (index, label) in items.into_iter().enumerate() {
            assert_eq!(header.find(index).label(), Some(label), "header overflow item {index}");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn overflow_labels_carry_control_state(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(std::env::temp_dir());
        view.sidebar_open = false;
        view.terminal_visible = true;
        view.inspector_open = true;
        view.inspector_tab = crate::desktop::inspector::InspectorTab::Utilities;
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);

        window.click("nav-overflow", cx);
        window.render_frame(cx);
        let rail = window.within("dropdown-menu:Name(\"nav-overflow\")");
        assert_eq!(rail.find(0usize).label(), Some("Workspace utilities (selected)"));
        assert_eq!(rail.find(1usize).label(), Some("Hide terminal"));
        window.press("escape", cx);
        window.render_frame(cx);

        view.update(cx, |view, cx| view.set_navigation_visible(false, cx));
        window.render_frame(cx);
        window.click("header-overflow", cx);
        window.render_frame(cx);
        assert_eq!(
            window.within("dropdown-menu:Name(\"header-overflow\")").find(2usize).label(),
            Some("Show navigation")
        );
    })
    .unwrap();
}
