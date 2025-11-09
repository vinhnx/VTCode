//! Parse theme configuration from multiple syntaxes (Git, LS_COLORS, custom).

use anyhow::{Context, Result, anyhow};
use anstyle::Style as AnsiStyle;
use crate::utils::CachedStyleParser;

/// Parses color configuration strings in different syntaxes.
///
/// Supports:
/// - Git color syntax (e.g., "bold red", "red blue")
/// - LS_COLORS syntax (e.g., "01;34" for bold blue)
/// - Flexible parsing that tries multiple formats
pub struct ThemeConfigParser {
    /// Cached parser for performance
    cached_parser: CachedStyleParser,
}

impl ThemeConfigParser {
    /// Create a new ThemeConfigParser
    pub fn new() -> Self {
        Self {
            cached_parser: CachedStyleParser::new(),
        }
    }
}

impl Default for ThemeConfigParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeConfigParser {
    /// Parse a string in Git's color configuration syntax.
    ///
    /// # Examples
    ///
    /// ```text
    /// "bold red"       → bold red foreground
    /// "red blue"       → red foreground on blue background
    /// "#0000ee ul"     → RGB blue with underline
    /// "green"          → green foreground
    /// "dim white"      → dimmed white
    /// ```
    ///
    /// # Errors
    ///
    /// Returns error if the input doesn't match Git color syntax.
    pub fn parse_git_style(&self, input: &str) -> Result<AnsiStyle> {
        self.cached_parser.parse_git_style(input)
    }

    /// Parse a string in LS_COLORS syntax (ANSI escape codes).
    ///
    /// # Examples
    ///
    /// ```text
    /// "34"       → blue foreground
    /// "01;34"    → bold blue
    /// "34;03"    → blue with italic
    /// "30;47"    → black text on white background
    /// ```
    ///
    /// # Errors
    ///
    /// Returns error if the input doesn't match LS_COLORS syntax.
    pub fn parse_ls_colors(&self, input: &str) -> Result<AnsiStyle> {
        self.cached_parser.parse_ls_colors(input)
    }

    /// Parse a style string, attempting Git syntax first, then LS_COLORS as fallback.
    ///
    /// This is a convenience function for flexible input parsing. It tries the more
    /// human-readable Git syntax first, then falls back to ANSI codes if that fails.
    ///
    /// # Errors
    ///
    /// Returns error if the input matches neither Git nor LS_COLORS syntax.
    pub fn parse_flexible(&self, input: &str) -> Result<AnsiStyle> {
        self.cached_parser.parse_flexible(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_git_bold_red() {
        let parser = ThemeConfigParser::default();
        let style = parser.parse_git_style("bold red").unwrap();
        assert!(style.get_effects().contains(anstyle::Effects::BOLD));
    }

    #[test]
    fn test_parse_git_hex_color() {
        let parser = ThemeConfigParser::default();
        let style = parser.parse_git_style("#0000ee").unwrap();
        assert!(style.get_fg_color().is_some());
    }

    #[test]
    fn test_parse_git_red_on_blue() {
        let parser = ThemeConfigParser::default();
        let style = parser.parse_git_style("red blue").unwrap();
        assert!(style.get_fg_color().is_some());
        assert!(style.get_bg_color().is_some());
    }

    #[test]
    fn test_parse_git_dim_white() {
        let parser = ThemeConfigParser::default();
        let style = parser.parse_git_style("dim white").unwrap();
        assert!(style.get_effects().contains(anstyle::Effects::DIMMED));
    }

    #[test]
    fn test_parse_git_underline() {
        let parser = ThemeConfigParser::default();
        let style = parser.parse_git_style("ul green").unwrap();
        assert!(style.get_effects().contains(anstyle::Effects::UNDERLINE));
    }

    #[test]
    fn test_parse_ls_colors_blue() {
        let parser = ThemeConfigParser::default();
        let style = parser.parse_ls_colors("34").unwrap();
        assert!(style.get_fg_color().is_some());
    }

    #[test]
    fn test_parse_ls_colors_bold_blue() {
        let parser = ThemeConfigParser::default();
        let style = parser.parse_ls_colors("01;34").unwrap();
        assert!(style.get_effects().contains(anstyle::Effects::BOLD));
        assert!(style.get_fg_color().is_some());
    }

    #[test]
    fn test_parse_ls_colors_black_on_white() {
        let parser = ThemeConfigParser::default();
        let style = parser.parse_ls_colors("30;47").unwrap();
        assert!(style.get_fg_color().is_some());
        assert!(style.get_bg_color().is_some());
    }

    #[test]
    fn test_parse_flexible_tries_git_first() {
        let parser = ThemeConfigParser::default();
        let style = parser.parse_flexible("bold red").unwrap();
        assert!(style.get_effects().contains(anstyle::Effects::BOLD));
    }

    #[test]
    fn test_parse_flexible_fallback_to_ls() {
        let parser = ThemeConfigParser::default();
        let style = parser.parse_flexible("01;34").unwrap();
        assert!(style.get_effects().contains(anstyle::Effects::BOLD));
    }

    #[test]
    fn test_parse_git_fails_on_invalid() {
        let parser = ThemeConfigParser::default();
        let result = parser.parse_git_style("unknown-color");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_ls_colors_fails_on_invalid() {
        let parser = ThemeConfigParser::default();
        let result = parser.parse_ls_colors("invalid");
        assert!(result.is_err());
    }
}
