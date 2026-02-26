use once_cell::sync::Lazy;
use std::collections::HashMap;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use tracing::warn;

const MAX_THEME_CACHE_SIZE: usize = 32;
const DEFAULT_THEME_NAME: &str = "base16-ocean.dark";

static SHARED_SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);

static SHARED_THEME_CACHE: Lazy<parking_lot::RwLock<HashMap<String, Theme>>> = Lazy::new(|| {
    match ThemeSet::load_defaults() {
        defaults if !defaults.themes.is_empty() => {
            let mut entries: Vec<(String, Theme)> = defaults.themes.into_iter().collect();
            if entries.len() > MAX_THEME_CACHE_SIZE {
                entries.truncate(MAX_THEME_CACHE_SIZE);
            }
            let themes: HashMap<_, _> = entries.into_iter().collect();
            parking_lot::RwLock::new(themes)
        }
        _ => {
            warn!(
                "Failed to load default syntax highlighting themes; syntax highlighting will be disabled"
            );
            parking_lot::RwLock::new(HashMap::new())
        }
    }
});

pub fn syntax_set() -> &'static SyntaxSet {
    &SHARED_SYNTAX_SET
}

pub fn find_syntax_by_token(token: &str) -> &'static SyntaxReference {
    SHARED_SYNTAX_SET
        .find_syntax_by_token(token)
        .unwrap_or_else(|| SHARED_SYNTAX_SET.find_syntax_plain_text())
}

pub fn find_syntax_by_name(name: &str) -> Option<&'static SyntaxReference> {
    SHARED_SYNTAX_SET.find_syntax_by_name(name)
}

pub fn find_syntax_by_extension(ext: &str) -> Option<&'static SyntaxReference> {
    SHARED_SYNTAX_SET.find_syntax_by_extension(ext)
}

pub fn find_syntax_plain_text() -> &'static SyntaxReference {
    SHARED_SYNTAX_SET.find_syntax_plain_text()
}

pub fn load_theme(theme_name: &str, cache: bool) -> Theme {
    if let Some(theme) = SHARED_THEME_CACHE.read().get(theme_name).cloned() {
        return theme;
    }

    let defaults = ThemeSet::load_defaults();
    if let Some(theme) = defaults.themes.get(theme_name).cloned() {
        if cache {
            let mut guard = SHARED_THEME_CACHE.write();
            if guard.len() >= MAX_THEME_CACHE_SIZE
                && let Some(first_key) = guard.keys().next().cloned()
            {
                guard.remove(&first_key);
            }
            guard.insert(theme_name.to_owned(), theme.clone());
        }
        theme
    } else {
        warn!(
            theme = theme_name,
            "Unknown syntax highlighting theme, falling back to first available theme"
        );
        if defaults.themes.is_empty() {
            warn!("No syntax highlighting themes available at all");
            Theme::default()
        } else {
            defaults
                .themes
                .into_iter()
                .next()
                .map(|(_, theme)| theme)
                .unwrap_or_default()
        }
    }
}

pub fn default_theme_name() -> String {
    DEFAULT_THEME_NAME.to_string()
}

pub fn available_themes() -> Vec<String> {
    SHARED_THEME_CACHE.read().keys().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syntax_set_loaded() {
        let ss = syntax_set();
        assert!(!ss.syntaxes().is_empty(), "Syntax set should not be empty");
    }

    #[test]
    fn test_find_syntax_by_token() {
        let rust = find_syntax_by_token("rust");
        assert!(rust.name.contains("Rust"), "Should find Rust syntax");
    }

    #[test]
    fn test_find_syntax_plain_text() {
        let plain = find_syntax_plain_text();
        assert!(
            plain.name.contains("Plain Text"),
            "Should find Plain Text syntax"
        );
    }

    #[test]
    fn test_load_default_theme() {
        let theme = load_theme("base16-ocean.dark", false);
        assert!(theme.name.is_some());
    }

    #[test]
    fn test_load_unknown_theme_falls_back() {
        let theme = load_theme("nonexistent-theme-xyz", false);
        assert!(theme.name.is_some());
    }

    #[test]
    fn test_theme_caching() {
        let theme1 = load_theme("base16-ocean.dark", true);
        let theme2 = load_theme("base16-ocean.dark", true);
        assert_eq!(theme1.name, theme2.name);
    }
}
