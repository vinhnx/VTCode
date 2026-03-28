use anstyle::RgbColor;
use vtcode_config::constants::ui;

pub(crate) const MAX_DARK_BG_TEXT_LUMINANCE: f64 = 0.92;
pub(crate) const MIN_DARK_BG_TEXT_LUMINANCE: f64 = 0.20;
pub(crate) const MAX_LIGHT_BG_TEXT_LUMINANCE: f64 = 0.68;

pub(crate) fn relative_luminance(color: RgbColor) -> f64 {
    fn channel(value: u8) -> f64 {
        let c = (value as f64) / 255.0;
        if c <= ui::THEME_RELATIVE_LUMINANCE_CUTOFF {
            c / ui::THEME_RELATIVE_LUMINANCE_LOW_FACTOR
        } else {
            ((c + ui::THEME_RELATIVE_LUMINANCE_OFFSET)
                / (1.0 + ui::THEME_RELATIVE_LUMINANCE_OFFSET))
                .powf(ui::THEME_RELATIVE_LUMINANCE_EXPONENT)
        }
    }

    let r = channel(color.0);
    let g = channel(color.1);
    let b = channel(color.2);

    ui::THEME_RED_LUMINANCE_COEFFICIENT * r
        + ui::THEME_GREEN_LUMINANCE_COEFFICIENT * g
        + ui::THEME_BLUE_LUMINANCE_COEFFICIENT * b
}

pub(crate) fn contrast_ratio(foreground: RgbColor, background: RgbColor) -> f64 {
    let fg = relative_luminance(foreground);
    let bg = relative_luminance(background);
    let (lighter, darker) = if fg > bg { (fg, bg) } else { (bg, fg) };
    (lighter + ui::THEME_CONTRAST_RATIO_OFFSET) / (darker + ui::THEME_CONTRAST_RATIO_OFFSET)
}

fn darken(color: RgbColor, ratio: f64) -> RgbColor {
    mix(color, RgbColor(0, 0, 0), ratio)
}

fn adjust_luminance_to_target(color: RgbColor, target: f64) -> RgbColor {
    let current = relative_luminance(color);
    if (current - target).abs() < 1e-3 {
        return color;
    }

    if current < target {
        let denom = (1.0 - current).max(1e-6);
        let ratio = ((target - current) / denom).clamp(0.0, 1.0);
        lighten(color, ratio)
    } else {
        let denom = current.max(1e-6);
        let ratio = ((current - target) / denom).clamp(0.0, 1.0);
        darken(color, ratio)
    }
}

pub(crate) fn balance_text_luminance(
    color: RgbColor,
    background: RgbColor,
    min_contrast: f64,
) -> RgbColor {
    let bg_luminance = relative_luminance(background);
    let mut candidate = color;
    let current = relative_luminance(candidate);
    if bg_luminance < 0.5 {
        if current < MIN_DARK_BG_TEXT_LUMINANCE {
            candidate = adjust_luminance_to_target(candidate, MIN_DARK_BG_TEXT_LUMINANCE);
        } else if current > MAX_DARK_BG_TEXT_LUMINANCE {
            candidate = adjust_luminance_to_target(candidate, MAX_DARK_BG_TEXT_LUMINANCE);
        }
    } else if current > MAX_LIGHT_BG_TEXT_LUMINANCE {
        candidate = adjust_luminance_to_target(candidate, MAX_LIGHT_BG_TEXT_LUMINANCE);
    }

    ensure_contrast(candidate, background, min_contrast, &[color])
}

pub(crate) fn ensure_contrast(
    candidate: RgbColor,
    background: RgbColor,
    min_ratio: f64,
    fallbacks: &[RgbColor],
) -> RgbColor {
    if contrast_ratio(candidate, background) >= min_ratio {
        return candidate;
    }

    for &fallback in fallbacks {
        if contrast_ratio(fallback, background) >= min_ratio {
            return fallback;
        }
    }

    let black = RgbColor(0, 0, 0);
    let white = RgbColor(255, 255, 255);
    if contrast_ratio(black, background) >= contrast_ratio(white, background) {
        black
    } else {
        white
    }
}

pub(crate) fn mix(color: RgbColor, target: RgbColor, ratio: f64) -> RgbColor {
    let ratio = ratio.clamp(ui::THEME_MIX_RATIO_MIN, ui::THEME_MIX_RATIO_MAX);
    let blend = |c: u8, t: u8| -> u8 {
        let c = c as f64;
        let t = t as f64;
        ((c + (t - c) * ratio).round()).clamp(ui::THEME_BLEND_CLAMP_MIN, ui::THEME_BLEND_CLAMP_MAX)
            as u8
    };

    RgbColor(
        blend(color.0, target.0),
        blend(color.1, target.1),
        blend(color.2, target.2),
    )
}

pub(crate) fn lighten(color: RgbColor, ratio: f64) -> RgbColor {
    mix(
        color,
        RgbColor(
            ui::THEME_COLOR_WHITE_RED,
            ui::THEME_COLOR_WHITE_GREEN,
            ui::THEME_COLOR_WHITE_BLUE,
        ),
        ratio,
    )
}
