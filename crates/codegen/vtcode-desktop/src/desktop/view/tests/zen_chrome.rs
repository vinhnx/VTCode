//! Zen floating chrome: back/forward placement and a toggle that never jumps.

use super::*;
use crate::desktop::view::InspectorTab;

#[gpui_kit::test]
fn zen_floating_chrome_keeps_the_toggle_slot_and_exposes_navigation(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // Zen hides the navigation rail, which resizes the center column. Keep
    // navigation hidden here so the comparison isolates the chrome anchoring
    // from that separate column-width change.
    view.update(cx, |view, _| view.preferences.navigation_visible = Some(false));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let docked = window.find("zen-mode-toggle").bounds();

        view.update(cx, |view, cx| view.toggle_zen_mode(window, cx));
        window.render_frame(cx);
        let floating = window.find("zen-mode-toggle").bounds();
        assert_eq!(
            floating.origin.y, docked.origin.y,
            "toggle keeps its top y across the mode switch: docked={docked:?} floating={floating:?}"
        );
        assert_eq!(
            floating.origin.x, docked.origin.x,
            "toggle keeps its trailing x across the mode switch: docked={docked:?} floating={floating:?}"
        );

        let home = window.find("nav-home").bounds();
        let new_thread = window.find("nav-new-thread").bounds();
        let browse = window.find("nav-browse-threads").bounds();
        let utilities = window.find("nav-utilities").bounds();
        let back = window.find("navigate-back").bounds();
        let forward = window.find("navigate-forward").bounds();
        assert!(home.right() <= new_thread.origin.x, "Home leads New thread: {home:?} {new_thread:?}");
        assert!(new_thread.right() <= browse.origin.x, "New thread leads Browse Threads: {new_thread:?} {browse:?}");
        assert!(browse.right() <= utilities.origin.x, "Browse Threads precedes Utilities: {browse:?} {utilities:?}");
        assert!(utilities.right() <= back.origin.x, "Utilities precedes Back: {utilities:?} {back:?}");
        assert!(back.right() <= forward.origin.x, "Back precedes Forward left-to-right: {back:?} {forward:?}");
        assert!(
            forward.right() <= floating.origin.x,
            "Forward sits before the trailing toggle: {forward:?} {floating:?}"
        );

        // The group is right-anchored: changing navigation availability must
        // not shift the trailing toggle.
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        assert_ne!(window.find("navigate-back").disabled(), Some(true), "Back is live with a route open");
        assert_eq!(window.find("zen-mode-toggle").bounds(), floating, "toggle stays pinned as arrows become live");
        window.click("navigate-back", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).current_route(), Route::Workspace);
        assert_eq!(window.find("zen-mode-toggle").bounds(), floating, "toggle stays pinned after navigation");
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_home_returns_to_workspace_without_changing_the_sidebar(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| view.preferences.navigation_visible = Some(false));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.toggle_zen_mode(window, cx));
        window.render_frame(cx);

        let home = window.find("nav-home");
        assert_eq!(home.role(), Some(gpui_kit::Role::Button));
        assert_eq!(home.label(), Some("Workspace"));

        let back = window.find("navigate-back").bounds();
        let forward = window.find("navigate-forward").bounds();
        let toggle = window.find("zen-mode-toggle").bounds();
        assert_eq!(home.bounds().size, back.size, "Home shares the zen icon band");
        assert_eq!(home.bounds().center().y, back.center().y, "Home shares the band center line");
        assert!(home.bounds().right() <= back.origin.x, "Home leads Back: {:?} {:?}", home.bounds(), back);
        assert!(back.right() <= forward.origin.x, "Back precedes Forward");
        assert!(forward.right() <= toggle.origin.x, "Forward precedes the trailing toggle");

        // Open a route the way the palette would, then let Home bring the
        // workspace landing page back without touching rail/sidebar state.
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).current_route(), Route::Settings);
        let before = view.read(cx).sidebar_open;
        window.click("nav-home", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).current_route(), Route::Workspace);
        assert!(view.read(cx).zen_mode);
        assert_eq!(view.read(cx).sidebar_open, before);
        assert!(!view.read(cx).preferences.navigation_visible());
        assert!(window.try_find("primary-navigation").is_none());
        assert_eq!(window.find("zen-mode-toggle").bounds(), toggle, "toggle stays pinned after Home");

        // Already home, Home is a no-op rather than opening a route.
        window.click("nav-home", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).current_route(), Route::Workspace);
    })
    .unwrap();
}

#[gpui_kit::test]
fn fullscreen_zen_browse_threads_keeps_the_browser_header_reachable(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 700.);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.toggle_zen_mode(window, cx));
        window.dispatch_action(Box::new(super::super::ToggleFullScreen), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.is_fullscreen());
        window.click("nav-browse-threads", cx);
        window.render_frame(cx);

        let dismiss = window.find("browser-dismiss").bounds();
        let floating = window.find("zen-mode-toggle").bounds();
        assert!(
            dismiss.top() >= floating.bottom(),
            "browser actions clear floating chrome: dismiss={dismiss:?} floating={floating:?}"
        );
        window.click("browser-dismiss", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).current_route(), Route::Workspace);
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_utilities_button_opens_the_utilities_inspector(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(std::env::temp_dir());
            view.toggle_zen_mode(window, cx);
        });
        window.render_frame(cx);
        let button = window.find("nav-utilities");
        assert_eq!(button.role(), Some(gpui_kit::Role::Button));
        assert_eq!(button.label(), Some("Workspace utilities"));
        window.click("nav-utilities", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open, "Zen Utilities opens the inspector");
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Utilities);
        assert!(!view.read(cx).zen_mode, "revealing the inspector exits Zen");
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_new_thread_button_starts_a_thread(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.workspace = Some(std::env::temp_dir());
            view.toggle_zen_mode(window, cx);
        });
        window.render_frame(cx);
        let button = window.find("nav-new-thread");
        assert_eq!(button.role(), Some(gpui_kit::Role::Button));
        assert_eq!(button.label(), Some("New thread"));
        assert_ne!(button.disabled(), Some(true), "new thread is available in Zen");
        window.click("nav-new-thread", cx);
        window.render_frame(cx);
        assert!(view.read(cx).zen_mode);
    })
    .unwrap();
}
