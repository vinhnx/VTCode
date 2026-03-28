use anstyle::{Color, Effects, RgbColor, Style};
use vtcode_config::constants::{defaults, ui};

use crate::color_math::{balance_text_luminance, ensure_contrast, lighten, mix};

/// Identifier for the default theme.
pub const DEFAULT_THEME_ID: &str = defaults::DEFAULT_THEME;

const DEFAULT_MIN_CONTRAST: f64 = ui::THEME_MIN_CONTRAST_RATIO;

/// Color accessibility configuration loaded from vtcode.toml.
#[derive(Clone, Debug)]
pub struct ColorAccessibilityConfig {
    pub minimum_contrast: f64,
    pub bold_is_bright: bool,
    pub safe_colors_only: bool,
}

impl Default for ColorAccessibilityConfig {
    fn default() -> Self {
        Self {
            minimum_contrast: DEFAULT_MIN_CONTRAST,
            bold_is_bright: false,
            safe_colors_only: false,
        }
    }
}

/// Palette describing UI colors for the terminal experience.
#[derive(Clone, Debug)]
pub struct ThemePalette {
    pub primary_accent: RgbColor,
    pub background: RgbColor,
    pub foreground: RgbColor,
    pub secondary_accent: RgbColor,
    pub alert: RgbColor,
    pub logo_accent: RgbColor,
}

impl ThemePalette {
    fn style_from(color: RgbColor, bold: bool, bold_is_bright: bool) -> Style {
        let mut style = Style::new().fg_color(Some(Color::Rgb(color)));
        if bold && !bold_is_bright {
            style = style.bold();
        }
        style
    }

    pub(crate) fn build_styles_with_accessibility(
        &self,
        accessibility: &ColorAccessibilityConfig,
    ) -> ThemeStyles {
        let min_contrast = accessibility.minimum_contrast;
        let primary = self.primary_accent;
        let background = self.background;
        let secondary = self.secondary_accent;
        let logo_accent = self.logo_accent;
        let bold_is_bright = accessibility.bold_is_bright;

        let fallback_light = RgbColor(
            ui::THEME_COLOR_WHITE_RED,
            ui::THEME_COLOR_WHITE_GREEN,
            ui::THEME_COLOR_WHITE_BLUE,
        );

        let text_color = ensure_contrast(
            self.foreground,
            background,
            min_contrast,
            &[
                lighten(self.foreground, ui::THEME_FOREGROUND_LIGHTEN_RATIO),
                lighten(secondary, ui::THEME_SECONDARY_LIGHTEN_RATIO),
                fallback_light,
            ],
        );
        let text_color = balance_text_luminance(text_color, background, min_contrast);

        let info_color = ensure_contrast(
            secondary,
            background,
            min_contrast,
            &[
                lighten(secondary, ui::THEME_SECONDARY_LIGHTEN_RATIO),
                text_color,
                fallback_light,
            ],
        );
        let info_color = balance_text_luminance(info_color, background, min_contrast);

        let light_tool_color = lighten(text_color, ui::THEME_MIX_RATIO);
        let tool_color = ensure_contrast(
            light_tool_color,
            background,
            min_contrast,
            &[
                lighten(light_tool_color, ui::THEME_TOOL_BODY_LIGHTEN_RATIO),
                info_color,
                text_color,
            ],
        );
        let tool_body_candidate = mix(light_tool_color, text_color, ui::THEME_TOOL_BODY_MIX_RATIO);
        let tool_body_color = ensure_contrast(
            tool_body_candidate,
            background,
            min_contrast,
            &[
                lighten(light_tool_color, ui::THEME_TOOL_BODY_LIGHTEN_RATIO),
                text_color,
                fallback_light,
            ],
        );
        let tool_style = Style::new().fg_color(Some(Color::Rgb(tool_color)));
        let tool_detail_style = Style::new().fg_color(Some(Color::Rgb(tool_body_color)));

        let response_color = ensure_contrast(
            text_color,
            background,
            min_contrast,
            &[
                lighten(text_color, ui::THEME_RESPONSE_COLOR_LIGHTEN_RATIO),
                fallback_light,
            ],
        );
        let response_color = balance_text_luminance(response_color, background, min_contrast);

        let reasoning_color = ensure_contrast(
            lighten(text_color, 0.25),
            background,
            min_contrast,
            &[lighten(text_color, 0.15), text_color, fallback_light],
        );
        let reasoning_color = balance_text_luminance(reasoning_color, background, min_contrast);
        let reasoning_style =
            Self::style_from(reasoning_color, false).effects(Effects::DIMMED | Effects::ITALIC);

        let user_color = ensure_contrast(
            lighten(secondary, ui::THEME_USER_COLOR_LIGHTEN_RATIO),
            background,
            min_contrast,
            &[
                lighten(secondary, ui::THEME_SECONDARY_USER_COLOR_LIGHTEN_RATIO),
                info_color,
                text_color,
            ],
        );
        let user_color = balance_text_luminance(user_color, background, min_contrast);

        let alert_color = ensure_contrast(
            self.alert,
            background,
            min_contrast,
            &[
                lighten(self.alert, ui::THEME_LUMINANCE_LIGHTEN_RATIO),
                fallback_light,
                text_color,
            ],
        );
        let alert_color = balance_text_luminance(alert_color, background, min_contrast);

        let tool_output_style = Style::new();

        let pty_output_candidate = lighten(tool_body_color, ui::THEME_PTY_OUTPUT_LIGHTEN_RATIO);
        let pty_output_color = ensure_contrast(
            pty_output_candidate,
            background,
            min_contrast,
            &[
                lighten(text_color, ui::THEME_PTY_OUTPUT_LIGHTEN_RATIO),
                tool_body_color,
                text_color,
            ],
        );
        let pty_output_style = Style::new().fg_color(Some(Color::Rgb(pty_output_color)));

        let primary_style_color = balance_text_luminance(
            ensure_contrast(primary, background, min_contrast, &[text_color]),
            background,
            min_contrast,
        );
        let secondary_style_color = balance_text_luminance(
            ensure_contrast(
                secondary,
                background,
                min_contrast,
                &[info_color, text_color],
            ),
            background,
            min_contrast,
        );
        let logo_style_color = balance_text_luminance(
            ensure_contrast(
                logo_accent,
                background,
                min_contrast,
                &[secondary_style_color, text_color],
            ),
            background,
            min_contrast,
        );

        ThemeStyles {
            info: Self::style_from(info_color, true, bold_is_bright),
            error: Self::style_from(alert_color, true, bold_is_bright),
            output: Self::style_from(text_color, false, bold_is_bright),
            response: Self::style_from(response_color, false, bold_is_bright),
            reasoning: reasoning_style,
            tool: tool_style,
            tool_detail: tool_detail_style,
            tool_output: tool_output_style,
            pty_output: pty_output_style,
            status: Self::style_from(
                ensure_contrast(
                    lighten(primary_style_color, ui::THEME_PRIMARY_STATUS_LIGHTEN_RATIO),
                    background,
                    min_contrast,
                    &[
                        lighten(
                            primary_style_color,
                            ui::THEME_PRIMARY_STATUS_SECONDARY_LIGHTEN_RATIO,
                        ),
                        info_color,
                        text_color,
                    ],
                ),
                true,
                bold_is_bright,
            ),
            mcp: Self::style_from(
                ensure_contrast(
                    lighten(logo_style_color, ui::THEME_SECONDARY_LIGHTEN_RATIO),
                    background,
                    min_contrast,
                    &[
                        lighten(logo_style_color, ui::THEME_LOGO_ACCENT_BANNER_LIGHTEN_RATIO),
                        info_color,
                        fallback_light,
                    ],
                ),
                true,
                bold_is_bright,
            ),
            user: Self::style_from(user_color, false, bold_is_bright),
            primary: Self::style_from(primary_style_color, false, bold_is_bright),
            secondary: Self::style_from(secondary_style_color, false, bold_is_bright),
            background: Color::Rgb(background),
            foreground: Color::Rgb(text_color),
        }
    }
}

/// Styles computed from palette colors.
#[derive(Clone, Debug)]
pub struct ThemeStyles {
    pub info: Style,
    pub error: Style,
    pub output: Style,
    pub response: Style,
    pub reasoning: Style,
    pub tool: Style,
    pub tool_detail: Style,
    pub tool_output: Style,
    pub pty_output: Style,
    pub status: Style,
    pub mcp: Style,
    pub user: Style,
    pub primary: Style,
    pub secondary: Style,
    pub background: Color,
    pub foreground: Color,
}

#[derive(Clone, Debug)]
pub struct ThemeDefinition {
    pub id: &'static str,
    pub label: &'static str,
    pub palette: ThemePalette,
}

/// Logical grouping of built-in themes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeSuite {
    pub id: &'static str,
    pub label: &'static str,
    pub theme_ids: Vec<&'static str>,
}

/// Theme validation result.
#[derive(Debug, Clone)]
pub struct ThemeValidationResult {
    pub is_valid: bool,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}
