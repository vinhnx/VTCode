use super::provider;

const DEFAULT_MAX_FILE_SIZE_MB: usize = 10;
const DEFAULT_HIGHLIGHT_TIMEOUT_MS: u64 = 5_000;
const MIN_FILE_SIZE_MB: usize = 1;
const MIN_HIGHLIGHT_TIMEOUT_MS: u64 = 100;

/// Shared defaults for syntax highlighting configuration.
pub struct SyntaxHighlightingDefaults;

impl SyntaxHighlightingDefaults {
    /// Whether syntax highlighting is enabled by default.
    pub fn enabled() -> bool {
        true
    }

    /// Whether theme caching is enabled by default.
    pub fn cache_themes() -> bool {
        true
    }

    /// Default syntax highlighting theme identifier.
    pub fn theme() -> String {
        provider::with_config_defaults(|defaults| defaults.syntax_theme())
    }

    /// Default maximum file size (in megabytes) that will be highlighted.
    pub fn max_file_size_mb() -> usize {
        DEFAULT_MAX_FILE_SIZE_MB
    }

    /// Default timeout (in milliseconds) for highlighting operations.
    pub fn highlight_timeout_ms() -> u64 {
        DEFAULT_HIGHLIGHT_TIMEOUT_MS
    }

    /// Minimum supported highlight timeout.
    pub fn min_highlight_timeout_ms() -> u64 {
        MIN_HIGHLIGHT_TIMEOUT_MS
    }

    /// Minimum supported highlighted file size.
    pub fn min_file_size_mb() -> usize {
        MIN_FILE_SIZE_MB
    }

    /// Default list of enabled languages.
    pub fn enabled_languages() -> Vec<String> {
        provider::with_config_defaults(|defaults| defaults.syntax_languages())
    }
}

/// Serde helper returning the default enabled flag.
pub fn enabled() -> bool {
    SyntaxHighlightingDefaults::enabled()
}

/// Serde helper returning the default cache flag.
pub fn cache_themes() -> bool {
    SyntaxHighlightingDefaults::cache_themes()
}

/// Serde helper returning the default theme string.
pub fn theme() -> String {
    SyntaxHighlightingDefaults::theme()
}

/// Serde helper returning the default maximum file size.
pub fn max_file_size_mb() -> usize {
    SyntaxHighlightingDefaults::max_file_size_mb()
}

/// Serde helper returning the default highlight timeout.
pub fn highlight_timeout_ms() -> u64 {
    SyntaxHighlightingDefaults::highlight_timeout_ms()
}

/// Serde helper returning the default language list.
pub fn enabled_languages() -> Vec<String> {
    SyntaxHighlightingDefaults::enabled_languages()
}
