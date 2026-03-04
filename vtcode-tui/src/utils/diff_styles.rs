//! Unified diff styles for TUI rendering
//!
//! Re-exports diff theme from vtcode-commons and provides
//! ratatui-specific style helpers for diff rendering.

// Re-export diff theme from vtcode-commons
pub use vtcode_commons::diff_theme::{
    DiffColorLevel, DiffTheme, diff_add_bg, diff_del_bg, diff_gutter_bg_add_light,
    diff_gutter_bg_del_light, diff_gutter_fg_light,
};
pub use vtcode_commons::styling::DiffColorPalette;

use crate::ui::syntax_highlight::{DiffScopeBackgroundRgbs, diff_scope_background_rgbs};
use ratatui::style::{Color as RatatuiColor, Modifier, Style as RatatuiStyle};
use vtcode_commons::color256_theme::rgb_to_ansi256_for_theme;

// ── Conversion helpers ─────────────────────────────────────────────────────

/// Convert anstyle Color to ratatui Color
fn ratatui_color_from_anstyle(color: anstyle::Color) -> RatatuiColor {
    match color {
        anstyle::Color::Ansi(c) => match c {
            anstyle::AnsiColor::Black => RatatuiColor::Black,
            anstyle::AnsiColor::Red => RatatuiColor::Red,
            anstyle::AnsiColor::Green => RatatuiColor::Green,
            anstyle::AnsiColor::Yellow => RatatuiColor::Yellow,
            anstyle::AnsiColor::Blue => RatatuiColor::Blue,
            anstyle::AnsiColor::Magenta => RatatuiColor::Magenta,
            anstyle::AnsiColor::Cyan => RatatuiColor::Cyan,
            anstyle::AnsiColor::White => RatatuiColor::White,
            anstyle::AnsiColor::BrightBlack => RatatuiColor::DarkGray,
            anstyle::AnsiColor::BrightRed => RatatuiColor::LightRed,
            anstyle::AnsiColor::BrightGreen => RatatuiColor::LightGreen,
            anstyle::AnsiColor::BrightYellow => RatatuiColor::LightYellow,
            anstyle::AnsiColor::BrightBlue => RatatuiColor::LightBlue,
            anstyle::AnsiColor::BrightMagenta => RatatuiColor::LightMagenta,
            anstyle::AnsiColor::BrightCyan => RatatuiColor::LightCyan,
            anstyle::AnsiColor::BrightWhite => RatatuiColor::White,
        },
        anstyle::Color::Ansi256(c) => RatatuiColor::Indexed(c.0),
        anstyle::Color::Rgb(c) => RatatuiColor::Rgb(c.0, c.1, c.2),
    }
}

// ── TUI-specific diff line styling ─────────────────────────────────────────

/// Diff line type for style selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLineType {
    Insert,
    Delete,
    Context,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ResolvedDiffBackgrounds {
    add: Option<RatatuiColor>,
    del: Option<RatatuiColor>,
}

/// Snapshot of diff styling inputs that can be reused while rendering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiffRenderStyleContext {
    theme: DiffTheme,
    level: DiffColorLevel,
    backgrounds: ResolvedDiffBackgrounds,
}

/// Resolve the current terminal and syntax-theme styling into one context.
pub fn current_diff_render_style_context() -> DiffRenderStyleContext {
    let theme = DiffTheme::detect();
    let level = DiffColorLevel::detect();
    diff_render_style_context_for(theme, level, scope_backgrounds_for_level(level))
}

fn diff_render_style_context_for(
    theme: DiffTheme,
    level: DiffColorLevel,
    scope_backgrounds: DiffScopeBackgroundRgbs,
) -> DiffRenderStyleContext {
    DiffRenderStyleContext {
        theme,
        level,
        backgrounds: resolve_diff_backgrounds_for(theme, level, scope_backgrounds),
    }
}

fn resolve_diff_backgrounds_for(
    theme: DiffTheme,
    level: DiffColorLevel,
    scope_backgrounds: DiffScopeBackgroundRgbs,
) -> ResolvedDiffBackgrounds {
    let mut resolved = fallback_diff_backgrounds(theme, level);
    if level == DiffColorLevel::Ansi16 {
        return resolved;
    }

    if let Some(rgb) = scope_backgrounds.inserted
        && let Some(color) = color_from_rgb_for_level(rgb, theme, level)
    {
        resolved.add = Some(color);
    }

    if let Some(rgb) = scope_backgrounds.deleted
        && let Some(color) = color_from_rgb_for_level(rgb, theme, level)
    {
        resolved.del = Some(color);
    }

    resolved
}

fn fallback_diff_backgrounds(theme: DiffTheme, level: DiffColorLevel) -> ResolvedDiffBackgrounds {
    match level {
        DiffColorLevel::Ansi16 => ResolvedDiffBackgrounds::default(),
        DiffColorLevel::TrueColor | DiffColorLevel::Ansi256 => ResolvedDiffBackgrounds {
            add: Some(ratatui_color_from_anstyle(diff_add_bg(theme, level))),
            del: Some(ratatui_color_from_anstyle(diff_del_bg(theme, level))),
        },
    }
}

fn color_from_rgb_for_level(
    rgb: (u8, u8, u8),
    theme: DiffTheme,
    level: DiffColorLevel,
) -> Option<RatatuiColor> {
    match level {
        DiffColorLevel::TrueColor => Some(RatatuiColor::Rgb(rgb.0, rgb.1, rgb.2)),
        DiffColorLevel::Ansi256 => Some(RatatuiColor::Indexed(rgb_to_ansi256_for_theme(
            rgb.0,
            rgb.1,
            rgb.2,
            theme.is_light(),
        ))),
        DiffColorLevel::Ansi16 => None,
    }
}

pub fn content_background(
    kind: DiffLineType,
    style_context: DiffRenderStyleContext,
) -> Option<RatatuiColor> {
    match kind {
        DiffLineType::Insert => style_context.backgrounds.add,
        DiffLineType::Delete => style_context.backgrounds.del,
        DiffLineType::Context => None,
    }
}

/// Full-width line background style. Context lines use terminal default.
pub fn style_line_bg(kind: DiffLineType, style_context: DiffRenderStyleContext) -> RatatuiStyle {
    match kind {
        DiffLineType::Insert => style_context
            .backgrounds
            .add
            .map_or_else(RatatuiStyle::default, |bg| RatatuiStyle::default().bg(bg)),
        DiffLineType::Delete => style_context
            .backgrounds
            .del
            .map_or_else(RatatuiStyle::default, |bg| RatatuiStyle::default().bg(bg)),
        DiffLineType::Context => RatatuiStyle::default(),
    }
}

fn scope_backgrounds_for_level(level: DiffColorLevel) -> DiffScopeBackgroundRgbs {
    match level {
        DiffColorLevel::Ansi16 => DiffScopeBackgroundRgbs::default(),
        DiffColorLevel::TrueColor | DiffColorLevel::Ansi256 => diff_scope_background_rgbs(),
    }
}

/// Gutter (line number) style.
///
/// Keep gutter signs/numbers dim and on standard ANSI red/green without bold.
pub fn style_gutter(kind: DiffLineType) -> RatatuiStyle {
    match kind {
        DiffLineType::Insert => RatatuiStyle::default()
            .fg(RatatuiColor::Green)
            .add_modifier(Modifier::DIM)
            .remove_modifier(Modifier::BOLD),
        DiffLineType::Delete => RatatuiStyle::default()
            .fg(RatatuiColor::Red)
            .add_modifier(Modifier::DIM)
            .remove_modifier(Modifier::BOLD),
        DiffLineType::Context => RatatuiStyle::default().add_modifier(Modifier::DIM),
    }
}

/// Sign character (`+`/`-`) style.
/// Uses standard ANSI red/green without bold for consistency.
pub fn style_sign(kind: DiffLineType) -> RatatuiStyle {
    match kind {
        DiffLineType::Insert => RatatuiStyle::default()
            .fg(RatatuiColor::Green)
            .add_modifier(Modifier::DIM)
            .remove_modifier(Modifier::BOLD),
        DiffLineType::Delete => RatatuiStyle::default()
            .fg(RatatuiColor::Red)
            .add_modifier(Modifier::DIM)
            .remove_modifier(Modifier::BOLD),
        DiffLineType::Context => RatatuiStyle::default(),
    }
}

/// Content style for plain (non-syntax-highlighted) diff lines.
///
/// ANSI16: foreground-only styling.
/// Light: bg only, no fg override.
/// Dark + TrueColor/256: colored fg + tinted bg.
pub fn style_content(kind: DiffLineType, style_context: DiffRenderStyleContext) -> RatatuiStyle {
    let bg = content_background(kind, style_context);
    match (kind, style_context.theme, style_context.level, bg) {
        (DiffLineType::Context, _, _, _) => RatatuiStyle::default(),
        (DiffLineType::Insert, _, DiffColorLevel::Ansi16, _) => {
            RatatuiStyle::default().fg(RatatuiColor::Green)
        }
        (DiffLineType::Delete, _, DiffColorLevel::Ansi16, _) => {
            RatatuiStyle::default().fg(RatatuiColor::Red)
        }
        (DiffLineType::Insert, DiffTheme::Light, _, Some(bg)) => RatatuiStyle::default().bg(bg),
        (DiffLineType::Delete, DiffTheme::Light, _, Some(bg)) => RatatuiStyle::default().bg(bg),
        (DiffLineType::Insert, DiffTheme::Dark, _, Some(bg)) => {
            RatatuiStyle::default().fg(RatatuiColor::Green).bg(bg)
        }
        (DiffLineType::Delete, DiffTheme::Dark, _, Some(bg)) => {
            RatatuiStyle::default().fg(RatatuiColor::Red).bg(bg)
        }
        (DiffLineType::Insert, DiffTheme::Light, _, None)
        | (DiffLineType::Delete, DiffTheme::Light, _, None) => RatatuiStyle::default(),
        (DiffLineType::Insert, DiffTheme::Dark, _, None) => {
            RatatuiStyle::default().fg(RatatuiColor::Green)
        }
        (DiffLineType::Delete, DiffTheme::Dark, _, None) => {
            RatatuiStyle::default().fg(RatatuiColor::Red)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_style_context(theme: DiffTheme, level: DiffColorLevel) -> DiffRenderStyleContext {
        diff_render_style_context_for(theme, level, scope_backgrounds_for_level(level))
    }

    #[test]
    fn dark_truecolor_add_bg_is_rgb() {
        let bg = diff_add_bg(DiffTheme::Dark, DiffColorLevel::TrueColor);
        assert!(matches!(
            bg,
            anstyle::Color::Rgb(anstyle::RgbColor(25, 45, 35))
        ));
    }

    #[test]
    fn dark_truecolor_del_bg_is_rgb() {
        let bg = diff_del_bg(DiffTheme::Dark, DiffColorLevel::TrueColor);
        assert!(matches!(
            bg,
            anstyle::Color::Rgb(anstyle::RgbColor(90, 40, 40))
        ));
    }

    #[test]
    fn light_truecolor_add_bg_is_accessible() {
        let bg = diff_add_bg(DiffTheme::Light, DiffColorLevel::TrueColor);
        assert!(matches!(
            bg,
            anstyle::Color::Rgb(anstyle::RgbColor(215, 240, 215))
        ));
    }

    #[test]
    fn light_truecolor_del_bg_is_accessible() {
        let bg = diff_del_bg(DiffTheme::Light, DiffColorLevel::TrueColor);
        assert!(matches!(
            bg,
            anstyle::Color::Rgb(anstyle::RgbColor(255, 235, 235))
        ));
    }

    #[test]
    fn dark_256_uses_indexed_colors() {
        let add = diff_add_bg(DiffTheme::Dark, DiffColorLevel::Ansi256);
        let del = diff_del_bg(DiffTheme::Dark, DiffColorLevel::Ansi256);
        assert!(matches!(
            add,
            anstyle::Color::Ansi256(anstyle::Ansi256Color(22))
        ));
        assert!(matches!(
            del,
            anstyle::Color::Ansi256(anstyle::Ansi256Color(52))
        ));
    }

    #[test]
    fn dark_ansi16_uses_named_colors() {
        let add = diff_add_bg(DiffTheme::Dark, DiffColorLevel::Ansi16);
        let del = diff_del_bg(DiffTheme::Dark, DiffColorLevel::Ansi16);
        assert_eq!(add, anstyle::Color::Ansi(anstyle::AnsiColor::Green));
        assert_eq!(del, anstyle::Color::Ansi(anstyle::AnsiColor::Red));
    }

    #[test]
    fn context_line_bg_is_default() {
        let style = style_line_bg(
            DiffLineType::Context,
            test_style_context(DiffTheme::Dark, DiffColorLevel::TrueColor),
        );
        assert_eq!(style, RatatuiStyle::default());
    }

    #[test]
    fn dark_gutter_is_dim() {
        let style = style_gutter(DiffLineType::Context);
        assert!(style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn insert_gutter_is_dim_standard_green_no_bold() {
        let style = style_gutter(DiffLineType::Insert);
        assert_eq!(style.fg, Some(RatatuiColor::Green));
        assert!(style.add_modifier.contains(Modifier::DIM));
        assert!(style.sub_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn dark_ansi16_content_uses_foreground_only() {
        let style = style_content(
            DiffLineType::Insert,
            test_style_context(DiffTheme::Dark, DiffColorLevel::Ansi16),
        );
        assert_eq!(style.fg, Some(RatatuiColor::Green));
        assert_eq!(style.bg, None);
    }

    #[test]
    fn sign_style_always_uses_standard_colors() {
        let add_sign = style_sign(DiffLineType::Insert);
        let del_sign = style_sign(DiffLineType::Delete);
        assert_eq!(add_sign.fg, Some(RatatuiColor::Green));
        assert_eq!(del_sign.fg, Some(RatatuiColor::Red));
        assert!(add_sign.add_modifier.contains(Modifier::DIM));
        assert!(del_sign.add_modifier.contains(Modifier::DIM));
        assert!(add_sign.sub_modifier.contains(Modifier::BOLD));
        assert!(del_sign.sub_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn theme_scope_backgrounds_override_truecolor_fallback_when_available() {
        let style_context = diff_render_style_context_for(
            DiffTheme::Dark,
            DiffColorLevel::TrueColor,
            DiffScopeBackgroundRgbs {
                inserted: Some((1, 2, 3)),
                deleted: Some((4, 5, 6)),
            },
        );

        assert_eq!(
            style_line_bg(DiffLineType::Insert, style_context),
            RatatuiStyle::default().bg(RatatuiColor::Rgb(1, 2, 3))
        );
        assert_eq!(
            style_line_bg(DiffLineType::Delete, style_context),
            RatatuiStyle::default().bg(RatatuiColor::Rgb(4, 5, 6))
        );
    }

    #[test]
    fn theme_scope_backgrounds_quantize_to_ansi256() {
        let style_context = diff_render_style_context_for(
            DiffTheme::Dark,
            DiffColorLevel::Ansi256,
            DiffScopeBackgroundRgbs {
                inserted: Some((0, 95, 0)),
                deleted: None,
            },
        );
        assert_eq!(
            style_line_bg(DiffLineType::Insert, style_context),
            RatatuiStyle::default().bg(RatatuiColor::Indexed(22))
        );
        assert_eq!(
            style_line_bg(DiffLineType::Delete, style_context),
            RatatuiStyle::default().bg(RatatuiColor::Indexed(52))
        );
    }

    #[test]
    fn ansi16_disables_line_backgrounds_even_with_scope_colors() {
        let style_context = diff_render_style_context_for(
            DiffTheme::Dark,
            DiffColorLevel::Ansi16,
            DiffScopeBackgroundRgbs {
                inserted: Some((8, 9, 10)),
                deleted: Some((11, 12, 13)),
            },
        );
        assert_eq!(
            style_line_bg(DiffLineType::Insert, style_context),
            RatatuiStyle::default()
        );
        assert_eq!(
            style_line_bg(DiffLineType::Delete, style_context),
            RatatuiStyle::default()
        );
    }

    #[test]
    fn ansi16_content_has_no_background() {
        let style_context = diff_render_style_context_for(
            DiffTheme::Dark,
            DiffColorLevel::Ansi16,
            DiffScopeBackgroundRgbs::default(),
        );
        let add = style_content(DiffLineType::Insert, style_context);
        let del = style_content(DiffLineType::Delete, style_context);
        assert_eq!(add.fg, Some(RatatuiColor::Green));
        assert_eq!(add.bg, None);
        assert_eq!(del.fg, Some(RatatuiColor::Red));
        assert_eq!(del.bg, None);
    }

    #[test]
    fn partial_scope_override_keeps_missing_side_fallback() {
        let style_context = diff_render_style_context_for(
            DiffTheme::Dark,
            DiffColorLevel::TrueColor,
            DiffScopeBackgroundRgbs {
                inserted: Some((12, 34, 56)),
                deleted: None,
            },
        );
        assert_eq!(
            content_background(DiffLineType::Insert, style_context),
            Some(RatatuiColor::Rgb(12, 34, 56))
        );
        assert_eq!(
            content_background(DiffLineType::Delete, style_context),
            Some(RatatuiColor::Rgb(90, 40, 40))
        );
    }
}
