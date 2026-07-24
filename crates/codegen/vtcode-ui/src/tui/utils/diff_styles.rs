//! Unified diff styles for TUI rendering
//!
//! Re-exports diff theme from vtcode-commons and provides
//! ratatui-specific style helpers for diff rendering.

// Re-export diff theme from vtcode-commons
pub use vtcode_commons::diff_theme::{
    DiffColorLevel, DiffTheme, diff_add_bg, diff_del_bg, diff_gutter_bg_add_light, diff_gutter_bg_del_light,
    diff_gutter_fg_light,
};
pub use vtcode_commons::styling::DiffColorPalette;

use crate::tui::ui::syntax_highlight::{DiffScopeBackgroundRgbs, diff_scope_background_rgbs};
use ratatui::style::{Color as RatatuiColor, Modifier, Style as RatatuiStyle};
use vtcode_commons::color256_theme::rgb_to_ansi256_for_theme;

// ── WCAG AA accessible colours ─────────────────────────────────────────────
//
// Verified to pass 4.5:1 minimum contrast ratio on their respective tinted
// diff backgrounds (dark add bg #143A2D, dark del bg #46262A).

/// Foreground colour for deletion markers and content on dark themes.
/// Brighter than standard ANSI Red (#CD0000) to pass WCAG AA on dark del bg.
const DELETION_FG_DARK: RatatuiColor = RatatuiColor::Rgb(255, 90, 90);

/// Foreground colour for insertion markers and content on dark themes.
const INSERTION_FG_DARK: RatatuiColor = RatatuiColor::LightGreen;

/// Foreground colour for deletion markers on light themes.
const DELETION_FG_LIGHT: RatatuiColor = RatatuiColor::LightRed;

/// Foreground colour for insertion markers on light themes.
const INSERTION_FG_LIGHT: RatatuiColor = RatatuiColor::LightGreen;

// ── Conversion helpers ─────────────────────────────────────────────────────

/// Convert anstyle Color to ratatui Color.
fn ratatui_color_from_anstyle(color: anstyle::Color) -> RatatuiColor {
    crate::design::color::anstyle_to_ratatui_color(color)
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
pub(crate) fn current_diff_render_style_context() -> DiffRenderStyleContext {
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

fn color_from_rgb_for_level(rgb: (u8, u8, u8), theme: DiffTheme, level: DiffColorLevel) -> Option<RatatuiColor> {
    match level {
        DiffColorLevel::TrueColor => Some(RatatuiColor::Rgb(rgb.0, rgb.1, rgb.2)),
        DiffColorLevel::Ansi256 => {
            Some(RatatuiColor::Indexed(rgb_to_ansi256_for_theme(rgb.0, rgb.1, rgb.2, theme.is_light())))
        }
        DiffColorLevel::Ansi16 => None,
    }
}

fn content_background(kind: DiffLineType, style_context: DiffRenderStyleContext) -> Option<RatatuiColor> {
    match kind {
        DiffLineType::Insert => style_context.backgrounds.add,
        DiffLineType::Delete => style_context.backgrounds.del,
        DiffLineType::Context => None,
    }
}

/// Full-width line background style. Context lines use terminal default.
pub(crate) fn style_line_bg(kind: DiffLineType, style_context: DiffRenderStyleContext) -> RatatuiStyle {
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

// ── Private colour helpers ──────────────────────────────────────────────────

/// Resolve the foreground colour for a diff indicator (gutter/sign).
fn indicator_fg(kind: DiffLineType, theme: DiffTheme) -> Option<RatatuiColor> {
    match (kind, theme) {
        (DiffLineType::Insert, DiffTheme::Dark) => Some(INSERTION_FG_DARK),
        (DiffLineType::Delete, DiffTheme::Dark) => Some(DELETION_FG_DARK),
        (DiffLineType::Insert, DiffTheme::Light) => Some(INSERTION_FG_LIGHT),
        (DiffLineType::Delete, DiffTheme::Light) => Some(DELETION_FG_LIGHT),
        (DiffLineType::Context, _) => None,
    }
}

/// Should the diff indicator use the DIM modifier?
fn indicator_dim(kind: DiffLineType, theme: DiffTheme) -> bool {
    matches!((kind, theme), (DiffLineType::Insert | DiffLineType::Delete, DiffTheme::Light))
}

/// Build a style for a line-level indicator (gutter or sign).
fn indicator_style(kind: DiffLineType, theme: DiffTheme) -> RatatuiStyle {
    let mut s = RatatuiStyle::default();
    if let Some(color) = indicator_fg(kind, theme) {
        s = s.fg(color);
    }
    if indicator_dim(kind, theme) {
        s = s.add_modifier(Modifier::DIM);
    }
    s
}

// ── Public style API ────────────────────────────────────────────────────────

/// Gutter (line number) style.
pub(crate) fn style_gutter(kind: DiffLineType, style_context: DiffRenderStyleContext) -> RatatuiStyle {
    indicator_style(kind, style_context.theme)
}

/// Sign character (`+`/`-`) style.
pub(crate) fn style_sign(kind: DiffLineType, style_context: DiffRenderStyleContext) -> RatatuiStyle {
    indicator_style(kind, style_context.theme)
}

/// Content style for plain (non-syntax-highlighted) diff lines.
pub(crate) fn style_content(kind: DiffLineType, style_context: DiffRenderStyleContext) -> RatatuiStyle {
    let bg = content_background(kind, style_context);
    let fg = indicator_fg(kind, style_context.theme);
    match (kind, style_context.theme, style_context.level, bg) {
        (DiffLineType::Context, _, _, _) => RatatuiStyle::default(),
        // Light theme: bg-only tinting; foreground stays as rendered default.
        (_, DiffTheme::Light, _, Some(bg)) => RatatuiStyle::default().bg(bg),
        (_, DiffTheme::Light, _, None) => RatatuiStyle::default(),
        // ANSI16: foreground-only — no background support.
        (_, _, DiffColorLevel::Ansi16, _) => fg.map(|c| RatatuiStyle::default().fg(c)).unwrap_or_default(),
        // TrueColor/256 + tinted bg: coloured text on tinted background.
        (_, _, _, Some(bg)) => fg
            .map(|c| RatatuiStyle::default().fg(c).bg(bg))
            .unwrap_or_else(|| RatatuiStyle::default().bg(bg)),
        (_, _, _, None) => fg.map(|c| RatatuiStyle::default().fg(c)).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_style_context(theme: DiffTheme, level: DiffColorLevel) -> DiffRenderStyleContext {
        diff_render_style_context_for(theme, level, scope_backgrounds_for_level(level))
    }

    #[test]
    fn dark_add_bg_is_subtle_green_tint() {
        let bg = diff_add_bg(DiffTheme::Dark, DiffColorLevel::TrueColor);
        assert_eq!(bg, anstyle::Color::Rgb(anstyle::RgbColor(20, 58, 45)));
    }

    #[test]
    fn dark_del_bg_is_subtle_red_tint() {
        let bg = diff_del_bg(DiffTheme::Dark, DiffColorLevel::TrueColor);
        assert_eq!(bg, anstyle::Color::Rgb(anstyle::RgbColor(70, 38, 42)));
    }

    #[test]
    fn light_add_bg_is_subtle_green_tint() {
        let bg = diff_add_bg(DiffTheme::Light, DiffColorLevel::TrueColor);
        assert_eq!(bg, anstyle::Color::Rgb(anstyle::RgbColor(218, 246, 225)));
    }

    #[test]
    fn light_del_bg_is_subtle_red_tint() {
        let bg = diff_del_bg(DiffTheme::Light, DiffColorLevel::TrueColor);
        assert_eq!(bg, anstyle::Color::Rgb(anstyle::RgbColor(255, 224, 224)));
    }

    #[test]
    fn all_levels_use_same_theme_tints() {
        for level in [
            DiffColorLevel::TrueColor,
            DiffColorLevel::Ansi256,
            DiffColorLevel::Ansi16,
        ] {
            assert_eq!(diff_add_bg(DiffTheme::Dark, level), anstyle::Color::Rgb(anstyle::RgbColor(20, 58, 45)));
            assert_eq!(diff_del_bg(DiffTheme::Dark, level), anstyle::Color::Rgb(anstyle::RgbColor(70, 38, 42)));
        }
    }

    #[test]
    fn context_line_bg_is_default() {
        let style =
            style_line_bg(DiffLineType::Context, test_style_context(DiffTheme::Dark, DiffColorLevel::TrueColor));
        assert_eq!(style, RatatuiStyle::default());
    }

    #[test]
    fn dark_gutter_context_has_no_style() {
        let ctx = test_style_context(DiffTheme::Dark, DiffColorLevel::TrueColor);
        let style = style_gutter(DiffLineType::Context, ctx);
        assert_eq!(style, RatatuiStyle::default());
    }

    #[test]
    fn insert_gutter_uses_light_green_on_dark() {
        let ctx = test_style_context(DiffTheme::Dark, DiffColorLevel::TrueColor);
        let style = style_gutter(DiffLineType::Insert, ctx);
        assert_eq!(style.fg, Some(RatatuiColor::LightGreen));
        assert!(!style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn delete_gutter_uses_custom_red_on_dark() {
        let ctx = test_style_context(DiffTheme::Dark, DiffColorLevel::TrueColor);
        let style = style_gutter(DiffLineType::Delete, ctx);
        assert_eq!(style.fg, Some(RatatuiColor::Rgb(255, 90, 90)));
    }

    #[test]
    fn dark_ansi16_content_uses_foreground_only() {
        let style = style_content(DiffLineType::Insert, test_style_context(DiffTheme::Dark, DiffColorLevel::Ansi16));
        assert_eq!(style.fg, Some(RatatuiColor::LightGreen));
        assert_eq!(style.bg, None);
    }

    #[test]
    fn sign_style_dark_uses_light_green_and_custom_red() {
        let ctx = test_style_context(DiffTheme::Dark, DiffColorLevel::TrueColor);
        let add_sign = style_sign(DiffLineType::Insert, ctx);
        let del_sign = style_sign(DiffLineType::Delete, ctx);
        assert_eq!(add_sign.fg, Some(RatatuiColor::LightGreen));
        assert_eq!(del_sign.fg, Some(RatatuiColor::Rgb(255, 90, 90)));
    }

    #[test]
    fn sign_style_light_uses_light_green_and_light_red_with_dim() {
        let ctx = test_style_context(DiffTheme::Light, DiffColorLevel::TrueColor);
        let add_sign = style_sign(DiffLineType::Insert, ctx);
        let del_sign = style_sign(DiffLineType::Delete, ctx);
        assert_eq!(add_sign.fg, Some(RatatuiColor::LightGreen));
        assert_eq!(del_sign.fg, Some(RatatuiColor::LightRed));
        assert!(add_sign.add_modifier.contains(Modifier::DIM));
        assert!(del_sign.add_modifier.contains(Modifier::DIM));
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
            DiffScopeBackgroundRgbs { inserted: Some((0, 95, 0)), deleted: None },
        );
        assert_eq!(
            style_line_bg(DiffLineType::Insert, style_context),
            RatatuiStyle::default().bg(RatatuiColor::Indexed(22))
        );
        assert_eq!(
            style_line_bg(DiffLineType::Delete, style_context),
            RatatuiStyle::default().bg(RatatuiColor::Rgb(70, 38, 42))
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
        assert_eq!(style_line_bg(DiffLineType::Insert, style_context), RatatuiStyle::default());
        assert_eq!(style_line_bg(DiffLineType::Delete, style_context), RatatuiStyle::default());
    }

    #[test]
    fn ansi16_content_has_no_background() {
        let style_context =
            diff_render_style_context_for(DiffTheme::Dark, DiffColorLevel::Ansi16, DiffScopeBackgroundRgbs::default());
        let add = style_content(DiffLineType::Insert, style_context);
        let del = style_content(DiffLineType::Delete, style_context);
        assert_eq!(add.fg, Some(INSERTION_FG_DARK));
        assert_eq!(add.bg, None);
        assert_eq!(del.fg, Some(DELETION_FG_DARK));
        assert_eq!(del.bg, None);
    }

    #[test]
    fn partial_scope_override_keeps_missing_side_fallback() {
        let style_context = diff_render_style_context_for(
            DiffTheme::Dark,
            DiffColorLevel::TrueColor,
            DiffScopeBackgroundRgbs { inserted: Some((12, 34, 56)), deleted: None },
        );
        assert_eq!(content_background(DiffLineType::Insert, style_context), Some(RatatuiColor::Rgb(12, 34, 56)));
        assert_eq!(content_background(DiffLineType::Delete, style_context), Some(RatatuiColor::Rgb(70, 38, 42)));
    }
}
