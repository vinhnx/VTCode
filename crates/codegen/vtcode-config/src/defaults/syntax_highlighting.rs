use super::provider;

const DEFAULT_MAX_FILE_SIZE_MB: usize = 10;
const DEFAULT_HIGHLIGHT_TIMEOUT_MS: u64 = 5_000;
const MIN_FILE_SIZE_MB: usize = 1;
const MIN_HIGHLIGHT_TIMEOUT_MS: u64 = 100;

/// Shared defaults for syntax highlighting configuration.
pub struct SyntaxHighlightingDefaults;

impl SyntaxHighlightingDefaults {
    /// Whether syntax highlighting is enabled by default.
    fn enabled() -> bool {
        true
    }

    /// Whether theme caching is enabled by default.
    fn cache_themes() -> bool {
        true
    }

    /// Default syntax highlighting theme identifier.
    pub(crate) fn theme() -> String {
        provider::with_config_defaults(|defaults| defaults.syntax_theme())
    }

    /// Default maximum file size (in megabytes) that will be highlighted.
    fn max_file_size_mb() -> usize {
        DEFAULT_MAX_FILE_SIZE_MB
    }

    /// Default timeout (in milliseconds) for highlighting operations.
    fn highlight_timeout_ms() -> u64 {
        DEFAULT_HIGHLIGHT_TIMEOUT_MS
    }

    /// Minimum supported highlight timeout.
    pub(crate) fn min_highlight_timeout_ms() -> u64 {
        MIN_HIGHLIGHT_TIMEOUT_MS
    }

    /// Minimum supported highlighted file size.
    pub(crate) fn min_file_size_mb() -> usize {
        MIN_FILE_SIZE_MB
    }

    /// Default list of enabled languages.
    pub(crate) fn enabled_languages() -> Vec<String> {
        provider::with_config_defaults(|defaults| defaults.syntax_languages())
    }
}

/// Serde helper returning the default enabled flag.
pub(crate) fn enabled() -> bool {
    SyntaxHighlightingDefaults::enabled()
}

/// Serde helper returning the default cache flag.
pub(crate) fn cache_themes() -> bool {
    SyntaxHighlightingDefaults::cache_themes()
}

/// Serde helper returning the default theme string.
pub(crate) fn theme() -> String {
    SyntaxHighlightingDefaults::theme()
}

/// Serde helper returning the default maximum file size.
pub(crate) fn max_file_size_mb() -> usize {
    SyntaxHighlightingDefaults::max_file_size_mb()
}

/// Serde helper returning the default highlight timeout.
pub(crate) fn highlight_timeout_ms() -> u64 {
    SyntaxHighlightingDefaults::highlight_timeout_ms()
}

/// Serde helper returning the default language list.
pub(crate) fn enabled_languages() -> Vec<String> {
    SyntaxHighlightingDefaults::enabled_languages()
}
