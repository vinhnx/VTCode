//! Glass primary buttons: the decision-area commit with a translucent rest fill.
//!
//! Same primary semantics, geometry, border, text, and Kit-owned states as
//! [`desktop_primary_button`]; only the resting fill turns to glass, derived
//! per theme and per seat by the [`themes`](super::themes) glass recipes.
//! Hover and press solidify to the theme's opaque primary tokens (Kit owns
//! those slots). The builder owns the whole disabled composition: Kit replays
//! the instance fill inside its disabled arm, so the wash cannot be a
//! call-site afterthought.
//!
//! One builder per seat; the seat names the surface the button sits on, so a
//! wrong-seat call reads wrong at the call site. In-layer dialog commits take
//! no elevation: the dialog already carries it.

use super::*;

/// Shared glass composition: opaque primary semantics with the resting fill
/// replaced by `fill`. Every seat funnels through here so geometry, text,
/// and disabled handling cannot drift between seats.
fn glass_primary_button_with(id: impl Into<gpui_kit::ElementId>, fill: gpui_kit::Hsla, disabled: bool) -> Button {
    desktop_primary_button(id).bg(fill).desktop_disabled(disabled)
}

/// Decision-area default commit with a translucent rest fill: the one
/// emphasized action per view, normally the Enter action. Canvas seat: the
/// floating composer send/cancel cluster. Never use for destructive actions
/// even when destruction is the most likely choice.
pub(super) fn desktop_glass_primary_button(
    id: impl Into<gpui_kit::ElementId>,
    colors: &gpui_kit::component::theme::ThemeColor,
    disabled: bool,
) -> Button {
    let fill = if disabled {
        themes::glass_disabled_wash(colors)
    } else {
        themes::glass_primary_rest(colors)
    };
    glass_primary_button_with(id, fill, disabled)
}

/// Decision-area default commit on a popover surface: interaction approvals,
/// About, changelog, rename, and the attachment picker. Same tier contract
/// as [`desktop_glass_primary_button`], composited over the popover seat.
pub(super) fn desktop_glass_popover_primary_button(
    id: impl Into<gpui_kit::ElementId>,
    colors: &gpui_kit::component::theme::ThemeColor,
    disabled: bool,
) -> Button {
    let fill = if disabled {
        themes::glass_disabled_wash(colors)
    } else {
        themes::glass_primary_rest_on_popover(colors)
    };
    glass_primary_button_with(id, fill, disabled)
}

/// Decision-area default commit on a secondary surface: the CLI config
/// editor dialog and settings saves (group-box fills share the secondary
/// seat on every theme). Same tier contract as
/// [`desktop_glass_primary_button`], composited over the secondary seat.
pub(super) fn desktop_glass_secondary_primary_button(
    id: impl Into<gpui_kit::ElementId>,
    colors: &gpui_kit::component::theme::ThemeColor,
    disabled: bool,
) -> Button {
    let fill = if disabled {
        themes::glass_disabled_wash(colors)
    } else {
        themes::glass_primary_rest_on_secondary(colors)
    };
    glass_primary_button_with(id, fill, disabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic dark seat: black canvas, white primary text, black accent —
    /// the composite holds AA at the glass floor, pinning the wiring without
    /// depending on any real palette.
    fn glass_colors() -> gpui_kit::component::theme::ThemeColor {
        let mut colors = gpui_kit::component::theme::ThemeColor::default();
        colors.background = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
        colors.button_primary = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
        colors.button_primary_foreground = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
        colors
    }

    /// Popover engages, siblings fall back: mid-gray accent with white text on
    /// a black popover seat (dark composites engage the glass floor) with a
    /// white canvas and white secondary seat (every composite stays too
    /// light for white text, so both fall back to opaque). Proves the
    /// popover builder reads its own seat: a builder wired to either
    /// sibling seat would carry the opaque fallback instead.
    fn popover_roomy_colors() -> gpui_kit::component::theme::ThemeColor {
        let mut colors = gpui_kit::component::theme::ThemeColor::default();
        colors.background = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
        colors.popover = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
        colors.secondary = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
        colors.button_primary = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 0.5, a: 1.0 };
        colors.button_primary_foreground = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
        colors
    }

    /// Secondary engages, siblings fall back: the mirror of
    /// [`popover_roomy_colors`] with the roomy and stuck seats exchanged,
    /// proving the secondary builder reads its own seat.
    fn secondary_roomy_colors() -> gpui_kit::component::theme::ThemeColor {
        let mut colors = gpui_kit::component::theme::ThemeColor::default();
        colors.background = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
        colors.popover = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
        colors.secondary = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
        colors.button_primary = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 0.5, a: 1.0 };
        colors.button_primary_foreground = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
        colors
    }

    #[test]
    fn glass_primary_wires_enabled_fill_and_cursor() {
        let colors = glass_colors();
        let mut button = desktop_glass_primary_button("glass-enabled", &colors, false);
        assert_eq!(
            button.style().mouse_cursor,
            Some(CursorStyle::PointingHand),
            "glass delegates to desktop_button like every tier"
        );
        assert_eq!(
            button.style().background,
            Some(themes::glass_primary_rest(&colors).into()),
            "the instance fill is the theme's glass rest fill"
        );
    }

    #[test]
    fn glass_primary_disabled_takes_the_wash_and_arrow() {
        let colors = glass_colors();
        let mut button = desktop_glass_primary_button("glass-disabled", &colors, true);
        assert_eq!(
            button.style().mouse_cursor,
            Some(CursorStyle::Arrow),
            "disabled glass keeps the arrow like every tier"
        );
        assert_eq!(
            button.style().background,
            Some(themes::glass_disabled_wash(&colors).into()),
            "a disabled glass button must carry the wash, never the rest fill"
        );
    }

    #[test]
    fn glass_popover_wires_its_own_seat_fill_and_cursor() {
        let colors = popover_roomy_colors();
        for sibling in [
            themes::glass_primary_rest(&colors),
            themes::glass_primary_rest_on_secondary(&colors),
        ] {
            assert_ne!(
                themes::glass_primary_rest_on_popover(&colors),
                sibling,
                "the fixture must discriminate all three seats, else the wiring assert below proves nothing"
            );
        }
        let mut button = desktop_glass_popover_primary_button("glass-popover-enabled", &colors, false);
        assert_eq!(
            button.style().mouse_cursor,
            Some(CursorStyle::PointingHand),
            "glass delegates to desktop_button like every tier"
        );
        assert_eq!(
            button.style().background,
            Some(themes::glass_primary_rest_on_popover(&colors).into()),
            "the instance fill is the popover seat's glass rest fill"
        );
    }

    #[test]
    fn glass_secondary_wires_its_own_seat_fill_and_cursor() {
        let colors = secondary_roomy_colors();
        for sibling in [
            themes::glass_primary_rest(&colors),
            themes::glass_primary_rest_on_popover(&colors),
        ] {
            assert_ne!(
                themes::glass_primary_rest_on_secondary(&colors),
                sibling,
                "the fixture must discriminate all three seats, else the wiring assert below proves nothing"
            );
        }
        let mut button = desktop_glass_secondary_primary_button("glass-secondary-enabled", &colors, false);
        assert_eq!(
            button.style().mouse_cursor,
            Some(CursorStyle::PointingHand),
            "glass delegates to desktop_button like every tier"
        );
        assert_eq!(
            button.style().background,
            Some(themes::glass_primary_rest_on_secondary(&colors).into()),
            "the instance fill is the secondary seat's glass rest fill"
        );
    }

    #[test]
    fn every_glass_seat_shares_the_disabled_wash_and_arrow() {
        // Seats are irrelevant here: the wash is seat-independent by design.
        let colors = popover_roomy_colors();
        for mut button in [
            desktop_glass_primary_button("glass-canvas-disabled", &colors, true),
            desktop_glass_popover_primary_button("glass-popover-disabled", &colors, true),
            desktop_glass_secondary_primary_button("glass-secondary-disabled", &colors, true),
        ] {
            assert_eq!(
                button.style().mouse_cursor,
                Some(CursorStyle::Arrow),
                "disabled glass keeps the arrow like every tier"
            );
            assert_eq!(
                button.style().background,
                Some(themes::glass_disabled_wash(&colors).into()),
                "a disabled glass button must carry the wash, never a rest fill"
            );
        }
    }
}
