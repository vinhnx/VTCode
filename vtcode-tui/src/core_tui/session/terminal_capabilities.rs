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

    static OVERRIDES: LazyLock<Mutex<HashMap<String, Option<String>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

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

/// Gets the appropriate border type based on terminal capabilities
///
/// Returns `BorderType::Rounded` if Unicode is supported, otherwise
/// returns `BorderType::Plain` for maximum compatibility.
pub fn get_border_type() -> ratatui::widgets::BorderType {
    if supports_unicode_box_drawing() {
        ratatui::widgets::BorderType::Rounded
    } else {
        ratatui::widgets::BorderType::Plain
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        // Test with different environment variable combinations

        // Save original values
        let original_term = env::var("TERM").ok();
        let original_lang = env::var("LANG").ok();
        let original_lc_all = env::var("LC_ALL").ok();
        let original_no_unicode = env::var("VTCODE_NO_UNICODE").ok();

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
        assert!(!supports_unicode_box_drawing());

        // Restore original values
        match original_term {
            Some(val) => set_var("TERM", &val),
            None => clear_var("TERM"),
        }
        match original_lang {
            Some(val) => set_var("LANG", &val),
            None => clear_var("LANG"),
        }
        match original_lc_all {
            Some(val) => set_var("LC_ALL", &val),
            None => clear_var("LC_ALL"),
        }
        match original_no_unicode {
            Some(val) => set_var("VTCODE_NO_UNICODE", &val),
            None => clear_var("VTCODE_NO_UNICODE"),
        }
    }

    #[test]
    fn test_get_border_type() {
        // Save original TERM
        let original_term = env::var("TERM").ok();

        // Test with Unicode-supporting terminal
        set_var("TERM", "xterm-256color");
        let border_type = get_border_type();
        assert!(matches!(border_type, ratatui::widgets::BorderType::Rounded));

        // Test with basic terminal
        set_var("TERM", "dumb");
        let border_type = get_border_type();
        assert!(matches!(border_type, ratatui::widgets::BorderType::Plain));

        // Restore original TERM
        match original_term {
            Some(val) => set_var("TERM", &val),
            None => clear_var("TERM"),
        }
    }
}
