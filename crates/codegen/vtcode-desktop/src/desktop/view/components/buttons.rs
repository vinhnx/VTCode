//! Shared button builders: the interactive baseline, the variant ladder
//! (ghost/secondary/primary/danger), the dismiss control, the zen toggle,
//! and the [`DesktopButtonExt`] interaction extensions.

use super::*;

/// Interactive button baseline: keyboard focus and a pointing-hand cursor.
fn desktop_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    Button::new(id).tab_stop(true).cursor_pointer().rounded(ButtonRounded::Large)
}

/// Quiet inline, toolbar, and navigation actions.
pub(in crate::desktop) fn desktop_ghost_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    desktop_button(id).ghost()
}

/// Ordinary visible action, or the alternative beside a primary or danger action.
pub(in crate::desktop::view) fn desktop_secondary_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    desktop_filled_button(id).secondary().shadow(themes::button_inset_edges())
}

fn desktop_filled_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    desktop_button(id).rounded(ButtonRounded::Medium).border_0().font_medium()
}

/// Decision-area default commit: the one emphasized action per view, normally
/// the Enter action. Paints the brand accent fill; never use for destructive
/// actions even when destruction is the most likely choice.
pub(in crate::desktop::view) fn desktop_primary_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    desktop_accented_button(id).primary()
}

/// Destructive commitment: irreversible or data-losing actions only. Pair with
/// an explicit confirmation for consequential work.
pub(in crate::desktop::view) fn desktop_danger_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    desktop_accented_button(id).danger()
}

fn desktop_accented_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    desktop_filled_button(id)
        .font_semibold()
        .shadow(themes::accented_button_shadows())
}

/// Quiet header dismiss control: ghost X icon with an accessible name and a
/// matching tooltip. Callers add sizing (usually `.small()`), disabled
/// state, and the click handler. Icon-only X buttons without a tooltip
/// (tab/attachment closers) and action-bound tooltips stay manual.
pub(in crate::desktop::view) fn desktop_dismiss_button(
    id: impl Into<gpui_kit::ElementId>,
    label: impl Into<gpui_kit::SharedString>,
) -> Button {
    let label = label.into();
    desktop_ghost_button(id)
        .icon(IconName::X)
        .accessibility_label(label.clone())
        .tooltip(label)
}

/// The single zen toggle: docked in the title bar normally, floating above
/// the content in zen mode. Enter and exit share id, size, and variant; only
/// the icon paint, label, and mount point change.
pub(in crate::desktop::view) fn zen_mode_toggle(zen_mode: bool, faded: gpui_kit::Hsla, focus: &FocusHandle) -> Button {
    let label = if zen_mode {
        rust_i18n::t!("app.navigation.exit_zen").to_string()
    } else {
        rust_i18n::t!("app.navigation.enter_zen").to_string()
    };
    let icon = if zen_mode {
        Icon::new(IconName::Leaf).text_color(faded)
    } else {
        Icon::new(IconName::Leaf)
    };
    desktop_ghost_button(ZEN_MODE_TOGGLE_ELEMENT_ID)
        .icon(icon)
        .small()
        .accessibility_label(label.clone())
        .tooltip_with_action(label, &ToggleZenMode, Some(key_context::DESKTOP))
        .dispatches_desktop_action(ToggleZenMode, focus.clone())
}

pub(in crate::desktop::view) trait DesktopButtonExt {
    fn desktop_disabled(self, disabled: bool) -> Self;
    fn dispatches_desktop_action(self, action: impl gpui_kit::Action + 'static, focus: FocusHandle) -> Self;
    /// Selected-option treatment for Ghost buttons: pressed-state metadata plus a stronger
    /// interaction fill when selected, quiet ghost otherwise. Every call site
    /// sits on a `secondary`-equivalent surface (settings panel, group-box fill), where
    /// a `secondary` fill would be pixel-identical to the background, so the
    /// highlight uses the readable pressed fill. Ghost-base
    /// (diff layout) and label-only (reduced motion)
    /// selections keep their bespoke chains.
    fn desktop_selected(self, selected: bool, colors: &gpui_kit::component::theme::ThemeColor) -> Self;
}

impl DesktopButtonExt for Button {
    fn desktop_disabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
            .cursor(if disabled {
                CursorStyle::Arrow
            } else {
                CursorStyle::PointingHand
            })
            // Kit replays instance shadows after its own disabled shadow reset.
            .when(disabled, |button| button.shadow_none())
    }

    fn desktop_selected(self, selected: bool, colors: &gpui_kit::component::theme::ThemeColor) -> Self {
        self.toggled(selected)
            .selected(selected)
            .when(selected, |button| button.bg(colors.button_active).text_color(colors.foreground))
    }

    fn dispatches_desktop_action(self, action: impl gpui_kit::Action + 'static, focus: FocusHandle) -> Self {
        self.on_click(move |_, window, cx| focus.dispatch_action(&action, window, cx))
    }
}
