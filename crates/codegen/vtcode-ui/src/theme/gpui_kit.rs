//! Ports of the longbridge/gpui-kit theme sets (<https://github.com/longbridge/gpui-kit/tree/main/themes>)
//! as terminal palettes.
//!
//! Each entry keeps the upstream `background`, `foreground` and `primary`
//! colors and records the six ANSI hue slots (`red` … `cyan`) converted from
//! the upstream `base.*` palette or its syntax highlight colors. The CLI
//! registry and the desktop app both derive their accents from these rows, so a
//! theme looks the same in the TUI and in the GPUI client.

use anstyle::RgbColor;
use vtcode_config::constants::ui;

use crate::theme::color_math::{contrast_ratio, mix};

/// RGB triple serialized as `0xRRGGBB` in the source table.
pub(crate) const fn rgb(value: u32) -> RgbColor {
    RgbColor((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

/// Upstream theme palette: real colors for every slot the UI needs.
pub(crate) struct UiThemePalette {
    pub background: RgbColor,
    pub foreground: RgbColor,
    /// Upstream `primary.background` (brand accent for buttons and badges).
    pub accent: RgbColor,
    pub red: RgbColor,
    pub green: RgbColor,
    pub yellow: RgbColor,
    pub blue: RgbColor,
    pub magenta: RgbColor,
    pub cyan: RgbColor,
}

/// One upstream theme variant, keyed by its VT Code theme id.
pub(crate) struct UiThemeSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub dark: bool,
    pub palette: UiThemePalette,
}

macro_rules! spec {
    (
        $id:literal, $label:literal, $dark:expr,
        bg $bg:literal, fg $fg:literal, accent $accent:literal,
        red $red:literal, green $green:literal, yellow $yellow:literal,
        blue $blue:literal, magenta $magenta:literal, cyan $cyan:literal,
    ) => {
        UiThemeSpec {
            id: $id,
            label: $label,
            dark: $dark,
            palette: UiThemePalette {
                background: rgb($bg),
                foreground: rgb($fg),
                accent: rgb($accent),
                red: rgb($red),
                green: rgb($green),
                yellow: rgb($yellow),
                blue: rgb($blue),
                magenta: rgb($magenta),
                cyan: rgb($cyan),
            },
        }
    };
}

/// Adapt an upstream brand accent to terminal text usage.
///
/// Upstream `primary.background` is a *filled surface*; the CLI prints accents
/// as text. An accent that already reads against `background` is returned
/// untouched; otherwise it steps toward the theme foreground just until it
/// clears the contrast floor, which keeps the upstream hue instead of washing
/// the accent out to plain white or black.
pub(crate) fn terminal_accent(accent: RgbColor, background: RgbColor, foreground: RgbColor) -> RgbColor {
    let min_contrast = ui::THEME_MIN_CONTRAST_RATIO;
    if contrast_ratio(accent, background) >= min_contrast {
        return accent;
    }
    for step in 1..=ACCENT_LIFT_STEPS {
        let candidate = mix(accent, foreground, f32::from(step) / f32::from(ACCENT_LIFT_STEPS));
        if contrast_ratio(candidate, background) >= min_contrast {
            return candidate;
        }
    }
    foreground
}

/// Granularity of the accent readability lift; 20 steps keep the upstream hue
/// recognizable while still landing above the contrast floor.
const ACCENT_LIFT_STEPS: u8 = 20;

/// Every upstream theme variant, grouped by upstream theme set.
pub(crate) fn theme_specs() -> &'static [UiThemeSpec] {
    static SPECS: &[UiThemeSpec] = &[
        spec!(
            "adventure", "Adventure", true,
            bg 0x040404, fg 0xFEFFFF, accent 0x4384AD,
            red 0xD84A33, green 0x5DA602, yellow 0xAA7900,
            blue 0x417AB3, magenta 0x97D7EF, cyan 0x41B3A9,
        ),
        spec!(
            "adventure-time", "Adventure Time", true,
            bg 0x1F1D45, fg 0xC7C7D4, accent 0x5F72C6,
            red 0xE19773, green 0x549235, yellow 0xCE7837,
            blue 0xE1D797, magenta 0xA76D3B, cyan 0x26977B,
        ),
        spec!(
            "alduin", "Alduin", true,
            bg 0x1C1C1C, fg 0x9E9E9E, accent 0x458588,
            red 0xA87880, green 0x7A875F, yellow 0x9D906C,
            blue 0x87AFAF, magenta 0xAF8787, cyan 0x878787,
        ),
        spec!(
            "asciinema", "Asciinema", true,
            bg 0x121314, fg 0xCCCCCC, accent 0x26B0D7,
            red 0xDD3C69, green 0x4EBF22, yellow 0xDDAF3C,
            blue 0x26B0D7, magenta 0xB954E1, cyan 0x54E1B9,
        ),
        spec!(
            "aurora-light", "Aurora Light", false,
            bg 0xFFFFFF, fg 0x020617, accent 0x1E293B,
            red 0x0433FF, green 0x036A07, yellow 0x6F42C1,
            blue 0x0000A2, magenta 0x0433FF, cyan 0x0000A2,
        ),
        spec!(
            "ayu-light", "Ayu Light", false,
            bg 0xFCFCFC, fg 0x5C6166, accent 0x55B4D3,
            red 0x835AAC, green 0x5C6166, yellow 0x835AAC,
            blue 0x5C6166, magenta 0x7351D0, cyan 0x5C6166,
        ),
        spec!(
            "ayu-dark", "Ayu Dark", true,
            bg 0x0D1016, fg 0xB3B1AD, accent 0x5AC1FE,
            red 0xEF7177, green 0xAAD84C, yellow 0xFEB454,
            blue 0x5AC1FE, magenta 0xD2A6FF, cyan 0x5A728B,
        ),
        spec!(
            "catppuccin-latte", "Catppuccin Latte", false,
            bg 0xE5E9EF, fg 0x4C4F69, accent 0x7287FD,
            red 0x6819CF, green 0x0046D5, yellow 0x0046D5,
            blue 0x0046D5, magenta 0x4C4F69, cyan 0x0046D5,
        ),
        spec!(
            "catppuccin-frappe", "Catppuccin Frappé", true,
            bg 0x232634, fg 0xC6D0F5, accent 0x8CAAEE,
            red 0xE78284, green 0xA6D189, yellow 0xE7D682,
            blue 0x8CAAEE, magenta 0x9882E7, cyan 0x81C8BE,
        ),
        spec!(
            "catppuccin-macchiato", "Catppuccin Macchiato", true,
            bg 0x1E2030, fg 0xCAD3F5, accent 0x8AADF4,
            red 0xED8796, green 0xA6DA95, yellow 0xEED49F,
            blue 0x8AADF4, magenta 0xF5BDE6, cyan 0x8BD5CA,
        ),
        spec!(
            "catppuccin-mocha", "Catppuccin Mocha", true,
            bg 0x181825, fg 0xCDD6F4, accent 0x89B4FA,
            red 0xF38BA8, green 0xA6E3A1, yellow 0xF9E2AF,
            blue 0x89B4FA, magenta 0xF5C2E7, cyan 0x94E2D5,
        ),
        spec!(
            "everforest-light", "Everforest Light", false,
            bg 0xFEFCEE, fg 0x5F6D75, accent 0xE69875,
            red 0x5F6D75, green 0x5F6D75, yellow 0x5F6D75,
            blue 0x5F6D75, magenta 0x5F6D75, cyan 0x5F6D75,
        ),
        spec!(
            "everforest-dark", "Everforest Dark", true,
            bg 0x262E34, fg 0xD3C6AA, accent 0xE69875,
            red 0xE67E80, green 0xA7C080, yellow 0xDBBC7F,
            blue 0x7FBBB3, magenta 0xA76D3B, cyan 0x83C092,
        ),
        spec!(
            "fahrenheit", "Fahrenheit", true,
            bg 0x000000, fg 0xFFFFCE, accent 0x720202,
            red 0xCB4A05, green 0x027225, yellow 0x726302,
            blue 0xFD9F4D, magenta 0xCDA074, cyan 0x027272,
        ),
        spec!(
            "flexoki-light", "Flexoki Light", false,
            bg 0xFFFCF0, fg 0x100F0F, accent 0x3AA99F,
            red 0xB12D21, green 0x677A19, yellow 0x8D6300,
            blue 0x23659E, magenta 0xAE3D77, cyan 0x04635B,
        ),
        spec!(
            "flexoki-dark", "Flexoki Dark", true,
            bg 0x100F0F, fg 0xCECDC3, accent 0x24837B,
            red 0x879A39, green 0x66800B, yellow 0xAD8301,
            blue 0xDA702C, magenta 0xD0A215, cyan 0x24837B,
        ),
        spec!(
            "gruvbox-light", "Gruvbox Light", false,
            bg 0xFBF1C7, fg 0x3C3836, accent 0xD79921,
            red 0xDB2914, green 0x595400, yellow 0x955600,
            blue 0x076678, magenta 0x8F3F71, cyan 0x225B38,
        ),
        spec!(
            "gruvbox-dark", "Gruvbox Dark", true,
            bg 0x1D2021, fg 0xEBDBB2, accent 0xD79921,
            red 0xF06555, green 0x98971A, yellow 0xD79921,
            blue 0x458588, magenta 0xB16286, cyan 0x689D6A,
        ),
        spec!(
            "harper", "Harper", true,
            bg 0x010101, fg 0xA8A49D, accent 0xB196C6,
            red 0xFF5874, green 0x489E48, yellow 0xF8B63F,
            blue 0x7FB5E1, magenta 0xB296C6, cyan 0xBFF5E5,
        ),
        spec!(
            "hybrid-light", "Hybrid Light", false,
            bg 0xE4E4E4, fg 0x1C1C1C, accent 0x005F87,
            red 0x5F0000, green 0x005F00, yellow 0x746000,
            blue 0x00195F, magenta 0x5F1C51, cyan 0x005A5F,
        ),
        spec!(
            "hybrid-dark", "Hybrid Dark", true,
            bg 0x1D1F21, fg 0xE8E8E8, accent 0x15678A,
            red 0xE19773, green 0x7C8A15, yellow 0x8A7C15,
            blue 0xE1D797, magenta 0xA76D3B, cyan 0xA86D3B,
        ),
        spec!(
            "jellybeans", "Jellybeans", true,
            bg 0x151515, fg 0xE8E8D3, accent 0x97BEDC,
            red 0xE27373, green 0x94B979, yellow 0xFFBA7B,
            blue 0x97BEDC, magenta 0xB294BB, cyan 0x00988E,
        ),
        spec!(
            "kibble", "Kibble", true,
            bg 0x0E100A, fg 0xF7F7F7, accent 0x6CE05C,
            red 0xC70231, green 0x2BCF13, yellow 0xC7A302,
            blue 0xC7A302, magenta 0x8400FF, cyan 0x0798AB,
        ),
        spec!(
            "macos-classic-light", "macOS Classic Light", false,
            bg 0xF9F9F9, fg 0x000000, accent 0x0060DE,
            red 0xD21F07, green 0x117A00, yellow 0x6F42C1,
            blue 0x0060DE, magenta 0x9A0068, cyan 0x005E6A,
        ),
        spec!(
            "macos-classic-dark", "macOS Classic Dark", true,
            bg 0x131313, fg 0xDEDEDE, accent 0x077CFD,
            red 0xFF5257, green 0x30D158, yellow 0xFFC600,
            blue 0x419CFF, magenta 0xA550A7, cyan 0x07FDD3,
        ),
        spec!(
            "mellifluous-light", "Mellifluous Light", false,
            bg 0xE7E7E7, fg 0x383A42, accent 0x5A6599,
            red 0xA93934, green 0x626020, yellow 0x525252,
            blue 0x3A4579, magenta 0x525252, cyan 0x3A4579,
        ),
        spec!(
            "mellifluous-dark", "Mellifluous Dark", true,
            bg 0x1A1A1A, fg 0xABB2BF, accent 0x5A6599,
            red 0xC95954, green 0x828040, yellow 0xC98D54,
            blue 0x5481C9, magenta 0x9C6995, cyan 0x54C9BD,
        ),
        spec!(
            "molokai-light", "Molokai Light", false,
            bg 0xFEFAF9, fg 0x0A0A0A, accent 0xE14774,
            red 0xC15027, green 0x067D49, yellow 0x006C88,
            blue 0xC14012, magenta 0x7058BE, cyan 0xAC5A00,
        ),
        spec!(
            "molokai-dark", "Molokai Dark", true,
            bg 0x1B1D1E, fg 0xF8F8F2, accent 0xDC1860,
            red 0xDC1860, green 0x82CC0A, yellow 0x807607,
            blue 0xE1D797, magenta 0xA76D3B, cyan 0x07807E,
        ),
        spec!(
            "solarized-light", "Solarized Light", false,
            bg 0xFDF6E3, fg 0x586E75, accent 0x586E75,
            red 0x755858, green 0x515538, yellow 0x756E58,
            blue 0x586975, magenta 0x755866, cyan 0x587573,
        ),
        spec!(
            "solarized-dark", "Solarized Dark", true,
            bg 0x002B36, fg 0xFDF6E3, accent 0x0D667D,
            red 0xE19773, green 0x76BA53, yellow 0xA86D3B,
            blue 0xE1D797, magenta 0xA76D3B, cyan 0xA86D3B,
        ),
        spec!(
            "spaceduck", "Spaceduck", true,
            bg 0x0F111B, fg 0xA3A49D, accent 0x089CC5,
            red 0xE33400, green 0x5CCC96, yellow 0xB89C00,
            blue 0x00A3CC, magenta 0xC86D8C, cyan 0x089CC5,
        ),
        spec!(
            "tokyo-night", "Tokyo Night", true,
            bg 0x1A1B26, fg 0xC0CAF5, accent 0x7AA2F7,
            red 0xF7768E, green 0x9ECE6A, yellow 0xE0AF68,
            blue 0x7AA2F7, magenta 0x7AA2F7, cyan 0x7DCFFF,
        ),
        spec!(
            "tokyo-storm", "Tokyo Storm", true,
            bg 0x24283B, fg 0xC0CAF5, accent 0x7AA2F7,
            red 0xF7768E, green 0x9ECE6A, yellow 0xE0AF68,
            blue 0x7AA2F7, magenta 0xB283F8, cyan 0x7DCFFF,
        ),
        spec!(
            "tokyo-moon", "Tokyo Moon", true,
            bg 0x222436, fg 0xC0CAF5, accent 0x82AAFF,
            red 0xFF757F, green 0xC3E88D, yellow 0xFFC777,
            blue 0x82AAFF, magenta 0x6E738D, cyan 0x86E1FC,
        ),
        spec!(
            "twilight", "Twilight", true,
            bg 0x141414, fg 0xDCDCDC, accent 0xCDA869,
            red 0xC06D44, green 0xAFB97A, yellow 0xC2A86C,
            blue 0xE1D797, magenta 0xB4BE7C, cyan 0x778385,
        ),
    ];
    SPECS
}
