//! Fixed ANSI color tables for the embedded terminal panel.
//!
//! Hex color data lives under `themes/` by the desktop hard rule, even
//! though only the terminal consumes these slots. The dark table keeps
//! Rio's defaults untouched; the light table holds WCAG AA against every
//! built-in light theme (proven by
//! `terminal_tab::tests::terminal_palette_light_table_holds_contrast_on_every_light_theme`);
//! the Ciapre table is the flagship colorscheme, hue-true and warm-leaning
//! like the Ciapre theme, holding AA on the Ciapre ground (proven by
//! `terminal_tab::tests::terminal_palette_ciapre_table_holds_contrast_on_ciapre_theme`);
//! mid-dark grounds may soften the muted red, like every fixed scheme. The
//! Latte table is its light-ground sibling in the Catppuccin Latte family,
//! holding AA on the latte ground (proven by
//! `terminal_tab::tests::terminal_palette_latte_table_holds_contrast_on_latte_theme`).

use rio_vt::config::colors::{ColorArray, ColorBuilder, Format};

/// Light-mode ANSI slots 0-15 as sRGB hex: darkened hues readable on
/// light grounds.
const LIGHT_ANSI_HEX: [&str; 16] = [
    "#1A1A1A", "#B00020", "#006E31", "#7C5C00", "#0B57D0", "#7B1FA2", "#006A77", "#5A5A5A", "#5F5F5F", "#B02718",
    "#296A2D", "#755A00", "#125AA9", "#9C27B0", "#006B76", "#616161",
];

/// Ciapre-scheme ANSI slots 0-15 as sRGB hex: a fixed dark-ground
/// colorscheme in the Ciapre family (alert red, dark success-seed green,
/// khaki white), holding AA on the Ciapre ground. Tradeoff, by design:
/// slot 0 reads light-gray (AA needs it), so explicit black backgrounds
/// render as dim slabs, lighter than slot 7 — readability wins over the
/// traditional black-slot expectation, as in the light table.
const CIAPRE_ANSI_HEX: [&str; 16] = [
    "#AEAEAE", "#C16A68", "#63C779", "#C9A227", "#6B9BD1", "#B18FCF", "#6FB3BF", "#AEA47F", "#C0C0C0", "#D98A88",
    "#7FD694", "#D9B13B", "#8AB4E0", "#C4A5DB", "#84C6D1", "#E8E4D8",
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

/// Parse the Ciapre ANSI table. The hex constants are valid; a parse
/// failure falls back to white, which stays readable on dark grounds.
pub(crate) fn ciapre_ansi_slots() -> [ColorArray; 16] {
    CIAPRE_ANSI_HEX.map(|hex| {
        ColorBuilder::from_hex(hex.to_owned(), Format::SRGB0_1)
            .map(|built| built.to_arr())
            .unwrap_or([1.0, 1.0, 1.0, 1.0])
    })
}

/// Latte-scheme ANSI slots 0-15 as sRGB hex: a fixed light-ground
/// colorscheme in the Catppuccin Latte family (pink red, vivid blue,
/// lavender magenta), holding AA on the latte ground. The yellow and
/// brights lean darker than upstream so every slot clears AA.
const LATTE_ANSI_HEX: [&str; 16] = [
    "#1A1A1A", "#C20E33", "#256B1D", "#7E5A00", "#1A5CD8", "#7E2FE0", "#0B6B74", "#5A5A5A", "#5F5F5F", "#C42338",
    "#2E7A23", "#836200", "#2063DE", "#8040DE", "#0D767E", "#616161",
];

/// Parse the Latte ANSI table. The hex constants are valid; a parse
/// failure falls back to black, which stays readable on light grounds.
pub(crate) fn latte_ansi_slots() -> [ColorArray; 16] {
    LATTE_ANSI_HEX.map(|hex| {
        ColorBuilder::from_hex(hex.to_owned(), Format::SRGB0_1)
            .map(|built| built.to_arr())
            .unwrap_or([0.0, 0.0, 0.0, 1.0])
    })
}
