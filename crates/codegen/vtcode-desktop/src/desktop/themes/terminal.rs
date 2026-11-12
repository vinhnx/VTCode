//! Fixed ANSI color tables for the embedded terminal panel.
//!
//! Hex color data lives under `themes/` by the desktop hard rule, even
//! though only the terminal consumes these slots. The dark table keeps
//! Rio's defaults untouched; the light table holds WCAG AA against every
//! built-in light theme (proven by
//! `terminal_tab::tests::terminal_palette_light_table_holds_contrast_on_every_light_theme`).

use rio_vt::config::colors::{ColorArray, ColorBuilder, Format};

/// Light-mode ANSI slots 0-15 as sRGB hex: darkened hues readable on
/// light grounds.
const LIGHT_ANSI_HEX: [&str; 16] = [
    "#1A1A1A", "#B00020", "#006E31", "#7C5C00", "#0B57D0", "#7B1FA2", "#006A77", "#5A5A5A", "#5F5F5F", "#B02718",
    "#296A2D", "#755A00", "#125AA9", "#9C27B0", "#006B76", "#616161",
];

/// Parse the light ANSI table. The hex constants are valid; a parse
/// failure falls back to black, which stays readable on light grounds.
pub(crate) fn light_ansi_slots() -> [ColorArray; 16] {
    LIGHT_ANSI_HEX.map(|hex| {
        ColorBuilder::from_hex(hex.to_owned(), Format::SRGB0_1)
            .map(|built| built.to_arr())
            .unwrap_or([0.0, 0.0, 0.0, 1.0])
    })
}
