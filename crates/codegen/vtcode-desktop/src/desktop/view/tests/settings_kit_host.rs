//! Kit host contract: which settings surface renders at which viewport.
//!
//! Wide viewports host Kit `Settings` (sidebar rows `0-N`); narrow
//! viewports keep the legacy stacked navigation and content. These are
//! production regression tests over the real view, unlike the spike
//! probes in `settings_kit_spike` which use a minimal fixture.

use super::*;

#[gpui_kit::test]
fn settings_host_switches_at_the_narrow_breakpoint(cx: &mut TestAppContext) {
    for (width, legacy) in [(899., true), (900., false)] {
        let (window, view) = open_desktop_sized(cx, width, 900.);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            view.update(cx, |view, cx| view.open_settings(window, cx));
            window.render_frame(cx);
            assert_eq!(window.try_find("settings-navigation").is_some(), legacy, "legacy navigation at {width}px");
            assert_eq!(window.try_find("0-0").is_some(), !legacy, "Kit sidebar at {width}px");
        })
        .unwrap();
    }
}
