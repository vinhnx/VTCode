//! Glass primary buttons: the decision-area commit with a translucent rest fill.
//!
//! Same primary semantics, geometry, border, text, and Kit-owned states as
//! [`desktop_primary_button`]; only the resting fill turns to glass, derived
//! per theme by [`themes::glass_primary_rest`]. Hover and press solidify to
//! the theme's opaque primary tokens (Kit owns those slots). The builder owns
//! the whole disabled composition: Kit replays the instance fill inside its
//! disabled arm, so the wash cannot be a call-site afterthought.

use super::*;

/// Decision-area default commit with a translucent rest fill: the one
/// emphasized action per view, normally the Enter action. Never use for
/// destructive actions even when destruction is the most likely choice.
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
    desktop_primary_button(id).bg(fill).desktop_disabled(disabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic dark seat: black canvas, white primary text, black accent —
    /// the composite holds AA at the glass floor, pinning the wiring without
    /// depending on any real palette.
    fn glass_colors() -> gpui_kit::component::theme::ThemeColor {
        gpui_kit::component::theme::ThemeColor {
            background: gpui_kit::Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 },
            button_primary: gpui_kit::Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 },
            button_primary_foreground: gpui_kit::Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 },
            ..Default::default()
        }
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
}
