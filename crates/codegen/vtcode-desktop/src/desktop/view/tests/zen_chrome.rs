//! Zen floating chrome: back/forward placement and a toggle that never jumps.

use super::*;

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

        let back = window.find("navigate-back").bounds();
        let forward = window.find("navigate-forward").bounds();
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
