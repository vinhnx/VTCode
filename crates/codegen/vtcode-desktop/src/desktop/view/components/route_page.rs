//! Route-page width container shared by settings, browser, and review.

use super::*;

/// Width behavior for a route page. `Centered` keeps the legacy padded,
/// max-width panel (browser and review); `FullWidth` lets the panel span the
/// center container edge-to-edge horizontally (settings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::desktop::view) enum RoutePageLayout {
    Centered,
    FullWidth,
}

/// Route container shared by the settings, browser, and review pages.
/// The toolbar (normal mode) or the zen drag strip (windowed zen) owns the
/// traffic-light clearance above the slot, so the page adds none itself.
/// Titles align to the content spine in every mode; no horizontal dodge is
/// needed once the lights clear the panel vertically.
///
/// Both variants keep the bottom gutter: it is
/// the pointer-dismiss area for full-width routes, whose horizontal gutters
/// are gone. Vertical clearance stays owned by the toolbar/drag strip.
pub(in crate::desktop::view) fn route_page_with_layout(
    panel: impl IntoElement,
    layout: RoutePageLayout,
) -> impl IntoElement {
    let page = div().flex_1().min_w_0().h_full().pb_6().flex();
    match layout {
        RoutePageLayout::Centered => page
            .px_6()
            .justify_center()
            .child(div().w_full().h_full().max_w(px(1500.)).child(panel)),
        RoutePageLayout::FullWidth => page.child(div().w_full().h_full().child(panel)),
    }
}
