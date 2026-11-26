//! Handoff URL delivery to the window chosen for activation.
//!
//! Delivery resolves the content view through that window's Kit `Root` and
//! updates it directly. A stored view/window pair goes stale when one window
//! closes while another remains, and a window-dispatched action would depend
//! on the current focus being inside the content subtree (Kit overlays live in
//! a sibling `Root` layer). Resolving through `Root::view()` avoids both.

use super::*;

/// Route handoff links to a live window. A handoff with no live window
/// reopens first: dropping the links silently would strand CLI `/desktop`
/// invocations against a windowless app, and reopening also raises a hidden
/// or backgrounded window since the user invoked the app. Only a failed
/// reopen drops the links: there is no surface to report on.
pub(in crate::desktop::view) fn dispatch_handoff_links(cx: &mut App, links: Vec<protocol::handoff_url::HandoffLink>) {
    if links.is_empty() {
        return;
    }
    let window = match open_or_activate_main_window(cx, None, None) {
        Ok(window) => window,
        Err(error) => {
            vtcode_commons::diagnostics::stderr(format_args!("handoff delivery missed its window: {error}"));
            return;
        }
    };
    // The window's root view is Kit's `Root`, not `DesktopView`; the content
    // view is reachable only through `Root::view()`. Resolving it here keeps
    // delivery on the window `open_or_activate_main_window` selected without
    // relying on focus or retaining an app-wide handle. Both reads fail if the
    // window closed in between, so no stale view is ever updated.
    let view = match window
        .read::<gpui_kit::base::Root, _, _>(cx, |root, cx| root.read(cx).view().clone().downcast::<DesktopView>().ok())
    {
        Ok(Some(view)) => view,
        Ok(None) => {
            vtcode_commons::diagnostics::stderr(format_args!("handoff delivery: window root is not a DesktopView"));
            return;
        }
        Err(error) => {
            vtcode_commons::diagnostics::stderr(format_args!("handoff delivery could not read its window: {error}"));
            return;
        }
    };
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
