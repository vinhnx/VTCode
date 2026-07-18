//! Terminal capability detection for optimal rendering
//!
//! This module provides utilities to detect terminal capabilities such as
//! Unicode support, color support, and other features to ensure optimal
//! rendering across different terminal environments.

use std::env;

#[cfg(test)]
mod test_env_overrides {
    use hashbrown::HashMap;
    use std::sync::{LazyLock, Mutex};

    static OVERRIDES: LazyLock<Mutex<HashMap<String, Option<String>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

    pub(super) fn get(key: &str) -> Option<Option<String>> {
        OVERRIDES.lock().ok().and_then(|map| map.get(key).cloned())
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

    env::var(key).ok()
}

#[cfg(test)]
pub(crate) fn set_test_env_override(key: &str, value: Option<&str>) {
    test_env_overrides::set(key, value);
}

#[cfg(test)]
pub(crate) fn clear_test_env_override(key: &str) {
    test_env_overrides::clear(key);
}

/// Detects if the current terminal supports Unicode box drawing characters
///
/// This function checks various environment variables and terminal settings
/// to determine if Unicode characters can be safely displayed without
/// appearing as broken ANSI sequences.
pub fn supports_unicode_box_drawing() -> bool {
    // Check if explicitly disabled via environment variable
    if read_env_var("VTCODE_NO_UNICODE").is_some() {
        return false;
    }

    // Check terminal type - many terminals support Unicode
    if let Some(term) = read_env_var("TERM") {
        let term_lower = term.to_lowercase();

        // Modern terminals that definitely support Unicode
        if term_lower.contains("unicode")
            || term_lower.contains("utf")
            || term_lower.contains("xterm-256color")
            || term_lower.contains("screen-256color")
            || term_lower.contains("tmux-256color")
            || term_lower.contains("alacritty")
            || term_lower.contains("wezterm")
            || term_lower.contains("kitty")
            || term_lower.contains("iterm")
            || term_lower.contains("hyper")
        {
            return true;
        }

        // Older or basic terminal types that likely don't support Unicode well
        if term_lower.contains("dumb")
            || term_lower.contains("basic")
            || term_lower == "xterm"
            || term_lower == "screen"
        {
            return false;
        }
    }

    // Check LANG environment variable for UTF-8 locale
    if let Some(lang) = read_env_var("LANG")
        && (lang.to_lowercase().contains("utf-8") || lang.to_lowercase().contains("utf8"))
    {
        return true;
    }

    // Check LC_ALL and LC_CTYPE for UTF-8
    for var in &["LC_ALL", "LC_CTYPE"] {
        if let Some(locale) = read_env_var(var)
            && (locale.to_lowercase().contains("utf-8") || locale.to_lowercase().contains("utf8"))
        {
            return true;
        }
    }

    // Default to plain ASCII for safety - prevents broken Unicode display
    false
}

/// Detects if the terminal supports rich Unicode box drawing characters
///
/// This includes dashed border variants (╌, ╍, ┄, ┅, etc.) used by
/// `BorderType::HeavyDoubleDashed` and similar types from ratatui 0.30.0.
/// Only terminals known to render these correctly are opted in. Unknown
/// Unicode-capable terminals receive `Rounded` borders instead.
pub fn supports_rich_unicode_box_drawing() -> bool {
    // Check if explicitly disabled via environment variable
    if read_env_var("VTCODE_NO_RICH_UNICODE").is_some() {
        return false;
    }

    // Rich Unicode requires basic Unicode support first
    if !supports_unicode_box_drawing() {
        return false;
    }

    // Only opt in for terminals known to render dashed box drawing correctly
    if let Some(term) = read_env_var("TERM") {
        let term_lower = term.to_lowercase();
        return term_lower.contains("alacritty")
            || term_lower.contains("wezterm")
            || term_lower.contains("kitty")
            || term_lower.contains("iterm")
            || term_lower.contains("hyper")
            || term_lower.contains("ghostty");
    }

    false
}

/// Gets the appropriate border type based on terminal capabilities
///
/// Returns `BorderType::HeavyDoubleDashed` for terminals with rich Unicode support,
/// `BorderType::Rounded` for terminals with basic Unicode support, or
/// `BorderType::Plain` for maximum compatibility.
pub fn get_border_type() -> ratatui::widgets::BorderType {
    if supports_rich_unicode_box_drawing() {
        ratatui::widgets::BorderType::HeavyDoubleDashed
    } else if supports_unicode_box_drawing() {
        ratatui::widgets::BorderType::Rounded
    } else {
        ratatui::widgets::BorderType::Plain
    }
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
    use std::sync::{LazyLock, Mutex};

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
    fn test_supports_unicode_box_drawing() {
        let _guard = TERMINAL_ENV_TEST_LOCK.lock().expect("terminal env test lock");
        for key in ["TERM", "LANG", "LC_ALL", "LC_CTYPE", "VTCODE_NO_UNICODE"] {
            clear_var(key);
        }

        // Test with VTCODE_NO_UNICODE set (should disable Unicode)
        set_var("VTCODE_NO_UNICODE", "1");
        assert!(!supports_unicode_box_drawing());
        remove_var("VTCODE_NO_UNICODE");

        // Test with modern terminal
        set_var("TERM", "xterm-256color");
        assert!(supports_unicode_box_drawing());

        // Test with UTF-8 locale
        set_var("LANG", "en_US.UTF-8");
        assert!(supports_unicode_box_drawing());

        // Test with basic terminal
        set_var("TERM", "dumb");
        assert!(!supports_unicode_box_drawing());

        // Test with no locale info (should default to false for safety)
        remove_var("TERM");
        remove_var("LANG");
        remove_var("LC_ALL");
        remove_var("LC_CTYPE");
        assert!(!supports_unicode_box_drawing());

        for key in ["TERM", "LANG", "LC_ALL", "LC_CTYPE", "VTCODE_NO_UNICODE"] {
            clear_var(key);
        }
    }

    #[test]
    fn test_get_border_type() {
        let _guard = TERMINAL_ENV_TEST_LOCK.lock().expect("terminal env test lock");
        clear_var("TERM");
        clear_var("VTCODE_NO_RICH_UNICODE");

        // Test with rich Unicode terminal (Alacritty)
        set_var("TERM", "alacritty");
        let border_type = get_border_type();
        assert!(matches!(border_type, ratatui::widgets::BorderType::HeavyDoubleDashed));

        // Test with Unicode-supporting terminal (xterm-256color) - not in known list
        set_var("TERM", "xterm-256color");
        clear_var("VTCODE_NO_RICH_UNICODE");
        let border_type = get_border_type();
        assert!(matches!(border_type, ratatui::widgets::BorderType::Rounded));

        // Test with basic terminal
        set_var("TERM", "dumb");
        let border_type = get_border_type();
        assert!(matches!(border_type, ratatui::widgets::BorderType::Plain));

        // Test with rich Unicode disabled
        set_var("TERM", "alacritty");
        set_var("VTCODE_NO_RICH_UNICODE", "1");
        let border_type = get_border_type();
        assert!(matches!(border_type, ratatui::widgets::BorderType::Rounded));

        clear_var("TERM");
        clear_var("VTCODE_NO_RICH_UNICODE");
    }

    #[test]
    fn queued_input_edit_binding_switches_for_tmux() {
        let _guard = TERMINAL_ENV_TEST_LOCK.lock().expect("terminal env test lock");
        clear_var("TMUX");
        clear_var("TERM");
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
