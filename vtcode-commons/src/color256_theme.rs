//! Theme-aware 256-color helpers.
//!
//! The terminal 256-color palette can be "non-harmonious" on light themes when
//! palette semantics are intentionally flipped for compatibility. In that mode,
//! cube/gray indices need to be reflected to keep visual intent stable.

use std::sync::atomic::{AtomicU8, Ordering};

const HARMONIOUS_FALSE: u8 = 0;
const HARMONIOUS_TRUE: u8 = 1;
const HARMONIOUS_UNSET: u8 = 2;

static RUNTIME_HARMONIOUS_HINT: AtomicU8 = AtomicU8::new(HARMONIOUS_UNSET);

fn resolve_harmony(
    is_light_theme: bool,
    env_override: Option<bool>,
    runtime_hint: Option<bool>,
) -> bool {
    env_override.or(runtime_hint).unwrap_or(!is_light_theme)
}

/// Determine harmony mode for the current theme.
///
/// Precedence:
/// 1. `VTCODE_256_HARMONIOUS` environment override (`1/0`, `true/false`, `yes/no`, `on/off`)
/// 2. Runtime hint (typically from OSC probe cached at startup)
/// 3. Default behavior: light themes are treated as non-harmonious for compatibility.
fn is_harmonious_for_theme(is_light_theme: bool) -> bool {
    resolve_harmony(
        is_light_theme,
        harmonious_override(),
        harmonious_runtime_hint(),
    )
}

fn harmonious_runtime_hint() -> Option<bool> {
    match RUNTIME_HARMONIOUS_HINT.load(Ordering::Relaxed) {
        HARMONIOUS_TRUE => Some(true),
        HARMONIOUS_FALSE => Some(false),
        _ => None,
    }
}

/// Store a runtime harmony hint.
///
/// This is intended to be populated once at startup by terminal OSC probing.
/// Set `None` to clear the runtime hint.
pub fn set_harmonious_runtime_hint(value: Option<bool>) {
    let encoded = match value {
        Some(true) => HARMONIOUS_TRUE,
        Some(false) => HARMONIOUS_FALSE,
        None => HARMONIOUS_UNSET,
    };
    RUNTIME_HARMONIOUS_HINT.store(encoded, Ordering::Relaxed);
}

/// Reflected gray-ramp index (maps `0..=23` onto `232..=255`).
fn gray_index(level: u8) -> u8 {
    232 + (23 - level.min(23))
}

/// Reflected cube index (maps `r,g,b` in `0..=5` onto `16..=231`).
fn cube_index(r: u8, g: u8, b: u8) -> u8 {
    let r = r.min(5);
    let g = g.min(5);
    let b = b.min(5);

    let max = r.max(g).max(b) as i16;
    let min = r.min(g).min(b) as i16;
    let offset = 5 - max - min;

    let r = ((r as i16 + offset).clamp(0, 5)) as u8;
    let g = ((g as i16 + offset).clamp(0, 5)) as u8;
    let b = ((b as i16 + offset).clamp(0, 5)) as u8;

    16 + 36 * r + 6 * g + b
}

/// Adjust an existing ANSI256 index for palette harmony.
///
/// - `16..=231` is treated as cube space.
/// - `232..=255` is treated as grayscale ramp.
/// - `0..=15` is left unchanged.
fn adjust_index(index: u8, is_harmonious: bool) -> u8 {
    if is_harmonious {
        return index;
    }

    match index {
        16..=231 => {
            let adjusted = index - 16;
            let r = adjusted / 36;
            let g = (adjusted % 36) / 6;
            let b = adjusted % 6;
            cube_index(r, g, b)
        }
        232..=255 => gray_index(index - 232),
        _ => index,
    }
}

/// Adjust an ANSI256 index based on a light/dark theme hint.
pub fn adjust_index_for_theme(index: u8, is_light_theme: bool) -> u8 {
    adjust_index(index, is_harmonious_for_theme(is_light_theme))
}

/// Convert RGB to ANSI256 and apply theme-aware palette adjustment.
pub fn rgb_to_ansi256_for_theme(r: u8, g: u8, b: u8, is_light_theme: bool) -> u8 {
    let base_index = if r == g && g == b {
        if r < 8 {
            16
        } else if r > 248 {
            231
        } else {
            ((r as u16 - 8) / 10) as u8 + 232
        }
    } else {
        let r_index = ((r as u16 * 5) / 255) as u8;
        let g_index = ((g as u16 * 5) / 255) as u8;
        let b_index = ((b as u16 * 5) / 255) as u8;
        16 + 36 * r_index + 6 * g_index + b_index
    };

    adjust_index_for_theme(base_index, is_light_theme)
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn harmonious_override() -> Option<bool> {
    std::env::var("VTCODE_256_HARMONIOUS")
        .ok()
        .and_then(|value| parse_bool(&value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harmonious_indices_are_identity() {
        assert_eq!(adjust_index(16, true), 16);
        assert_eq!(adjust_index(231, true), 231);
        assert_eq!(adjust_index(232, true), 232);
        assert_eq!(adjust_index(255, true), 255);
        assert_eq!(adjust_index(194, true), 194);
    }

    #[test]
    fn non_harmonious_gray_and_cube_reflect() {
        assert_eq!(gray_index(0), 255);
        assert_eq!(gray_index(23), 232);
        assert_eq!(cube_index(0, 0, 0), 231);
        assert_eq!(cube_index(5, 5, 5), 16);
    }

    #[test]
    fn non_harmonious_adjusts_existing_indices() {
        assert_eq!(adjust_index(194, false), 22);
        assert_eq!(adjust_index(224, false), 52);
        assert_eq!(adjust_index(233, false), 254);
        assert_eq!(adjust_index(14, false), 14);
    }

    #[test]
    fn rgb_to_ansi256_applies_theme_adjustment() {
        assert_eq!(rgb_to_ansi256_for_theme(0, 0, 0, false), 16);
        assert_eq!(rgb_to_ansi256_for_theme(0, 0, 0, true), 231);
        assert_eq!(rgb_to_ansi256_for_theme(255, 255, 255, false), 231);
        assert_eq!(rgb_to_ansi256_for_theme(255, 255, 255, true), 16);
    }

    #[test]
    fn resolve_harmony_precedence_is_env_then_runtime_then_default() {
        assert!(resolve_harmony(true, Some(true), Some(false)));
        assert!(!resolve_harmony(false, Some(false), Some(true)));
        assert!(resolve_harmony(true, None, Some(true)));
        assert!(!resolve_harmony(true, None, None));
        assert!(resolve_harmony(false, None, None));
    }
}
