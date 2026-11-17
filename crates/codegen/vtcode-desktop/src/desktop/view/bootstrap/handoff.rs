//! Handoff URL delivery to the live main window.

use super::*;

/// Main window for `vtcode://` delivery. Weak: a closed window must drop
/// its view instead of leaking it through the global.
pub(super) struct MainWindow {
    pub(super) view: WeakEntity<DesktopView>,
    pub(super) window: gpui_kit::AnyWindowHandle,
}

impl gpui_kit::Global for MainWindow {}

/// Route handoff links to the live main window. Unknown sessions and
/// workspaces fail through the same notices as the argv path. A handoff
/// with no live window reopens first: dropping the links silently would
/// strand CLI `/desktop` invocations against a windowless app, and
/// reopening also raises a hidden or backgrounded window since the user
/// invoked the app. Only a failed reopen drops the links: there is no
/// surface to report on.
pub(in crate::desktop::view) fn dispatch_handoff_links(cx: &mut App, links: Vec<protocol::handoff_url::HandoffLink>) {
    if links.is_empty() {
        return;
    }
    if open_or_activate_main_window(cx, None, None).is_err() {
        vtcode_commons::diagnostics::stderr(format_args!("handoff delivery missed its window"));
        return;
    }
    let Some(main) = cx.try_global::<MainWindow>() else {
        return;
    };
    let Some(view) = main.view.upgrade() else {
        return;
    };
    let window = main.window;
    if window
        .update(cx, |_, window, cx| {
            view.update(cx, |view, cx| {
                for link in links {
                    view.handle_handoff_link(link, window, cx);
                }
            });
        })
        .is_err()
    {
        vtcode_commons::diagnostics::stderr(format_args!("handoff delivery missed its window"));
    }
}

pub(in crate::desktop::view) fn show_main_window(cx: &mut App) {
    if let Err(error) = open_or_activate_main_window(cx, None, None) {
        vtcode_commons::diagnostics::stderr(format_args!("failed to open VT Code Desktop window: {error}"));
    }
}
