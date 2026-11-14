//! Terminal capability detection for optimal rendering
//!
//! The verdict comes from the canonical probe in
//! `vtcode_commons::terminal_capabilities`, so the TUI and the CLI always
//! agree on what the terminal can draw. The environment is read once per
//! process instead of once per bordered cell.

use ratatui::symbols::border;

pub(crate) use crate::design::panel::{ASCII_BORDER_SET, border_set_with_fallback};

#[cfg(test)]
mod test_env_overrides {
    use hashbrown::HashMap;
    use std::sync::{LazyLock, Mutex};

    static OVERRIDES: LazyLock<Mutex<HashMap<String, Option<String>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

    pub(super) fn get(key: &str) -> Option<Option<String>> {
        OVERRIDES.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned()
    }

    pub(super) fn set(key: &str, value: Option<&str>) {
        if let Ok(mut map) = OVERRIDES.lock() {
            map.insert(key.to_string(), value.map(ToString::to_string));
        }
    }

    pub(super) fn clear(key: &str) {
        if let Ok(mut map) = OVERRIDES.lock() {
            map.remove(key);
        }
    }
}

fn read_env_var(key: &str) -> Option<String> {
    #[cfg(test)]
    if let Some(override_value) = test_env_overrides::get(key) {
        return override_value;
    }

    std::env::var(key).ok()
}

#[cfg(test)]
pub(crate) fn set_test_env_override(key: &str, value: Option<&str>) {
    test_env_overrides::set(key, value);
}

#[cfg(test)]
pub(crate) fn clear_test_env_override(key: &str) {
    test_env_overrides::clear(key);
}

use crate::design::panel::border_set_for_tier as border_set_for;

/// Gets the appropriate border set based on terminal capabilities
///
/// Heavy double dashed borders for terminals known to render the dashed
/// variants correctly, rounded borders for terminals with basic Unicode
/// support, and the ASCII set for terminals that cannot render Unicode.
pub(crate) fn get_border_set() -> border::Set<'static> {
    crate::design::panel::canonical_border_set()
}

pub(crate) fn queued_input_edit_uses_shift_left() -> bool {
    if read_env_var("TMUX").is_some() {
        return true;
    }

    read_env_var("TERM")
        .map(|term| term.to_lowercase().contains("tmux"))
        .unwrap_or(false)
}

pub(crate) fn queued_input_edit_hint() -> &'static str {
    if queued_input_edit_uses_shift_left() {
        if cfg!(target_os = "macos") {
            "⇧ + ← edit"
        } else {
            "Shift + ← edit"
        }
    } else if cfg!(target_os = "macos") {
        "⌥ + ↑ edit"
    } else {
        "Alt + ↑ edit"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::widgets::BorderType;
    use std::sync::{LazyLock, Mutex};
    use vtcode_commons::terminal_capabilities::BoxGlyphTier;

    static TERMINAL_ENV_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[inline]
    fn set_var(key: &str, value: &str) {
        test_env_overrides::set(key, Some(value));
    }
    #[inline]
    fn remove_var(key: &str) {
        test_env_overrides::set(key, None);
    }
    #[inline]
    fn clear_var(key: &str) {
        test_env_overrides::clear(key);
    }

    #[test]
    fn border_set_follows_the_canonical_tier() {
        assert_eq!(border_set_for(BoxGlyphTier::RichUnicode), BorderType::HeavyDoubleDashed.to_border_set());
        assert_eq!(border_set_for(BoxGlyphTier::Unicode), BorderType::Rounded.to_border_set());
        assert_eq!(border_set_for(BoxGlyphTier::Ascii), ASCII_BORDER_SET);
    }

    #[test]
    fn ascii_border_set_uses_ascii_glyphs() {
        assert_eq!(ASCII_BORDER_SET.top_left, "+");
        assert_eq!(ASCII_BORDER_SET.vertical_left, "|");
        assert_eq!(ASCII_BORDER_SET.horizontal_top, "-");
        // Every glyph must be ASCII, which is the whole point of the tier.
        for glyph in [
            ASCII_BORDER_SET.top_left,
            ASCII_BORDER_SET.top_right,
            ASCII_BORDER_SET.bottom_left,
            ASCII_BORDER_SET.bottom_right,
            ASCII_BORDER_SET.vertical_left,
            ASCII_BORDER_SET.vertical_right,
            ASCII_BORDER_SET.horizontal_top,
            ASCII_BORDER_SET.horizontal_bottom,
        ] {
            assert!(glyph.is_ascii(), "ASCII tier leaked a non-ASCII glyph: {glyph:?}");
        }
    }

    #[test]
    fn tiers_render_distinct_glyphs() {
        // Pins the glyphs the design-system doc names, so the table there can
        // never drift from what ratatui actually paints.
        let rich = border_set_for(BoxGlyphTier::RichUnicode);
        assert_eq!((rich.top_left, rich.horizontal_top, rich.top_right), ("┏", "╍", "┓"));

        let unicode = border_set_for(BoxGlyphTier::Unicode);
        assert_eq!((unicode.top_left, unicode.horizontal_top, unicode.top_right), ("╭", "─", "╮"));

        assert_ne!(rich, unicode, "the rich tier must not collapse into the Unicode tier");
        assert_ne!(ASCII_BORDER_SET, unicode, "the ASCII tier must not collapse into the Unicode tier");
    }

    #[test]
    fn border_set_matches_the_cached_capability_probe() {
        assert_eq!(border_set_for(vtcode_commons::terminal_capabilities::box_glyph_tier()), get_border_set());
    }

    #[test]
    fn cli_and_tui_agree_on_unicode_support() {
        // The CLI reads the capability through `AnsiCapabilities`; the TUI
        // reads it through the tier. They must never disagree about the same
        // machine, so both views of the probe are pinned together.
        let tier = vtcode_commons::terminal_capabilities::box_glyph_tier();
        assert_eq!(vtcode_commons::ansi_capabilities::CAPABILITIES.unicode_support, tier.supports_unicode());
    }

    #[test]
    fn queued_input_edit_binding_switches_for_tmux() {
        let _guard = TERMINAL_ENV_TEST_LOCK.lock().expect("terminal env test lock");
        // Shadow instead of clearing: clearing falls back to the host environment,
        // which leaks when the suite itself runs inside tmux.
        remove_var("TMUX");
        remove_var("TERM");
        set_var("TERM", "xterm-256color");
        assert!(!queued_input_edit_uses_shift_left());

        set_var("TMUX", "/tmp/tmux-1000/default,123,0");
        assert!(queued_input_edit_uses_shift_left());

        remove_var("TMUX");
        set_var("TERM", "tmux-256color");
        assert!(queued_input_edit_uses_shift_left());

        clear_var("TMUX");
        clear_var("TERM");
    }
}
