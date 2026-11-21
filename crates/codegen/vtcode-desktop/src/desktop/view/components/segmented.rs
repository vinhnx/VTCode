//! Shared tab and choice builders: Kit's folder/segmented `TabBar` variants,
//! the clamped keyboard step for a segmented choice, and the focus wrapper
//! that gives those otherwise keyboard-inert Kit tabs a complete keyboard path.

use super::*;
use gpui_kit::component::tab::TabVariant;

/// Shared tab treatment. Both variants come from Kit; callers choose by
/// semantics, not by looks: folder tabs join canvas content below them
/// (document strips, medium size), segmented tabs are one pill track for a
/// mutually exclusive choice (compact size). No width cap: Kit truncates
/// labels only when `max_width` is set, which these builders never do —
/// overflow scrolls, and strips with many tabs chain `.menu(true)` for the
/// dropdown.
fn desktop_tab_bar(id: impl Into<gpui_kit::ElementId>, variant: TabVariant) -> TabBar {
    TabBar::new(id).with_variant(variant)
}

/// Document tabs: the selected tab joins the content below, so callers on
/// canvas place the bar flush above that content.
pub(in crate::desktop::view) fn desktop_folder_tab_bar(id: impl Into<gpui_kit::ElementId>) -> TabBar {
    desktop_tab_bar(id, TabVariant::Folder).w_full()
}

/// Mutually exclusive choices (source switches, preference choices): Kit's
/// segmented variant owns the track, sliding pill, concentric corners and
/// hover/selected states; the palette colors come from
/// `themes/fills.rs`. Kit tabs are not focusable themselves,
/// so callers wrap the bar in a focusable element with arrow/Home/End handling
/// through [`segmented_choice_index`].
pub(in crate::desktop::view) fn desktop_segmented_tab_bar(id: impl Into<gpui_kit::ElementId>) -> TabBar {
    desktop_tab_bar(id, TabVariant::Segmented).small()
}

/// Clamped index a segmented choice should move to for a navigation keystroke,
/// or `None` when the key belongs to the enclosing scope. Matches the thread
/// strip's clamp-at-both-ends behavior; modifiers are ignored by callers.
pub(in crate::desktop::view) fn segmented_choice_index(
    keystroke: &gpui_kit::Keystroke,
    current: usize,
    count: usize,
) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let last = count - 1;
    match key::Named::parse(keystroke.key.as_str())? {
        key::Named::Left | key::Named::Up => Some(current.saturating_sub(1)),
        key::Named::Right | key::Named::Down => Some((current + 1).min(last)),
        key::Named::Home => Some(0),
        key::Named::End => Some(last),
        _ => None,
    }
}

/// Focus stop and keyboard contract for a [`desktop_segmented_tab_bar`]: Kit
/// tabs cannot own focus, so this wrapper owns the handle, visible focus ring,
/// click-to-focus, and Left/Right/Home/End selection. Callers pass the selected
/// index, a state updater, an accessible name that includes the current value,
/// and the built bar; the bar itself stays a stateless Kit element. The wrapper
/// is the only focus stop, so it must carry a role and name: the inner Kit tabs
/// expose their own `Tab`/`aria_selected` semantics, but focus never lands on
/// them. Returns a ready-to-mount element with the lookup id on the wrapper
/// (Kit bars register no observable id of their own).
pub(in crate::desktop::view) fn segmented_choice_wrapper(
    id: impl Into<gpui_kit::ElementId>,
    focus: &FocusHandle,
    selected: usize,
    count: usize,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
    ring: gpui_kit::Hsla,
    label: gpui_kit::SharedString,
    bar: impl IntoElement,
) -> AnyElement {
    let focus = focus.clone();
    div()
        .id(id)
        .test_support()
        .role(gpui_kit::Role::Group)
        .aria_label(label)
        .track_focus(&focus)
        .focus_visible(move |style| style.border_1().border_color(ring))
        // Kit's Tab stops mouse-down propagation, so a normal ancestor handler
        // never sees a direct tab click. Capture before the child dispatch so
        // pointer selection also arms arrow-key navigation on this wrapper.
        .capture_any_mouse_down(move |event, window, cx| {
            if event.button == MouseButton::Left {
                focus.focus(window, cx);
            }
        })
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if event.keystroke.modifiers.control
                || event.keystroke.modifiers.alt
                || event.keystroke.modifiers.platform
                || event.keystroke.modifiers.function
            {
                return;
            }
            let Some(next) = segmented_choice_index(&event.keystroke, selected, count) else {
                return;
            };
            cx.stop_propagation();
            on_select(next, window, cx);
        })
        .child(bar)
        .into_any_element()
}
