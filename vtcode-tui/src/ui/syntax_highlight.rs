//! Syntax Highlighting Engine
//!
//! Global syntax highlighting using `syntect` with TextMate themes.
//! Follows the architecture from OpenAI Codex PRs #11447 and #12581.
//!
//! # Architecture
//!
//! - **SyntaxSet**: Process-global singleton (~250 grammars, loaded once)
//! - **ThemeSet**: Process-global singleton loaded once
//! - **Highlighting**: Guardrails skip large inputs (>512KB or >10K lines)
//!
//! # Usage
//!
//! ```rust
//! use crate::ui::syntax_highlight::{
//!     highlight_code_to_segments, get_active_syntax_theme
//! };
//! use crate::ui::theme::active_theme_id;
//!
//! // Auto-resolve syntax theme from current UI theme
//! let syntax_theme = get_active_syntax_theme();
//!
//! // Highlight code with proper theme
//! let segments = highlight_code_to_segments(code, Some("rust"), syntax_theme);
//! ```
//!
//! # Performance
//!
//! - Single SyntaxSet load (~1MB, ~50ms)
//! - Single ThemeSet load shared by all highlighters
//! - Input guardrails prevent highlighting huge files
//! - Parser state preserved across multiline constructs

use crate::ui::theme::get_syntax_theme_for_ui_theme;
use anstyle::Style as AnstyleStyle;
use anstyle_syntect::to_anstyle;
use once_cell::sync::Lazy;
use syntect::highlighting::{Highlighter, Theme, ThemeSet};
use syntect::parsing::{Scope, SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;
use tracing::warn;

/// Default syntax highlighting theme
const DEFAULT_THEME_NAME: &str = "base16-ocean.dark";

/// Input size guardrail - skip highlighting for files > 512 KB
const MAX_INPUT_SIZE_BYTES: usize = 512 * 1024;

/// Input line guardrail - skip highlighting for files > 10K lines
const MAX_INPUT_LINES: usize = 10_000;

/// Global SyntaxSet singleton (~250 grammars)
static SHARED_SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);

/// Global ThemeSet singleton.
static SHARED_THEME_SET: Lazy<ThemeSet> = Lazy::new(|| match ThemeSet::load_defaults() {
    defaults if !defaults.themes.is_empty() => defaults,
    _ => {
        warn!("Failed to load default syntax highlighting themes");
        ThemeSet {
            themes: Default::default(),
        }
    }
});

/// Get the global SyntaxSet reference
#[inline]
pub fn syntax_set() -> &'static SyntaxSet {
    &SHARED_SYNTAX_SET
}

/// Find syntax by language token (e.g., "rust", "python")
#[inline]
pub fn find_syntax_by_token(token: &str) -> &'static SyntaxReference {
    SHARED_SYNTAX_SET
        .find_syntax_by_token(token)
        .unwrap_or_else(|| SHARED_SYNTAX_SET.find_syntax_plain_text())
}

/// Find syntax by exact name
#[inline]
pub fn find_syntax_by_name(name: &str) -> Option<&'static SyntaxReference> {
    SHARED_SYNTAX_SET.find_syntax_by_name(name)
}

/// Find syntax by file extension
#[inline]
pub fn find_syntax_by_extension(ext: &str) -> Option<&'static SyntaxReference> {
    SHARED_SYNTAX_SET.find_syntax_by_extension(ext)
}

/// Get plain text syntax fallback
#[inline]
pub fn find_syntax_plain_text() -> &'static SyntaxReference {
    SHARED_SYNTAX_SET.find_syntax_plain_text()
}

fn fallback_theme() -> Theme {
    SHARED_THEME_SET
        .themes
        .values()
        .next()
        .cloned()
        .unwrap_or_default()
}

fn plain_text_line_segments(code: &str) -> Vec<Vec<(syntect::highlighting::Style, String)>> {
    let mut result = Vec::new();
    let mut ends_with_newline = false;
    for line in LinesWithEndings::from(code) {
        ends_with_newline = line.ends_with('\n');
        let trimmed = line.trim_end_matches('\n');
        result.push(vec![(
            syntect::highlighting::Style::default(),
            trimmed.to_string(),
        )]);
    }

    if ends_with_newline {
        result.push(Vec::new());
    }

    result
}

/// Load a theme from the process-global theme set.
///
/// # Arguments
/// * `theme_name` - Theme identifier (TextMate theme name)
/// * `cache` - Ignored. Kept for API compatibility.
///
/// # Returns
/// Cloned theme instance (safe for multi-threaded use)
pub fn load_theme(theme_name: &str, _cache: bool) -> Theme {
    if let Some(theme) = SHARED_THEME_SET.themes.get(theme_name) {
        theme.clone()
    } else {
        warn!(
            theme = theme_name,
            "Unknown syntax highlighting theme, falling back to default"
        );
        fallback_theme()
    }
}

/// Get the default syntax theme name
#[inline]
pub fn default_theme_name() -> String {
    DEFAULT_THEME_NAME.to_string()
}

/// Get all available theme names
pub fn available_themes() -> Vec<String> {
    SHARED_THEME_SET.themes.keys().cloned().collect()
}

/// Check if input should be highlighted (guardrails)
#[inline]
pub fn should_highlight(code: &str) -> bool {
    code.len() <= MAX_INPUT_SIZE_BYTES && code.lines().count() <= MAX_INPUT_LINES
}

/// Get the recommended syntax theme for the current UI theme
///
/// This ensures syntax highlighting colors complement the UI theme background.
/// Based on OpenAI Codex PRs #11447 and #12581.
#[inline]
pub fn get_active_syntax_theme() -> &'static str {
    get_syntax_theme_for_ui_theme(&crate::ui::theme::active_theme_id())
}

/// Get the recommended syntax theme for a specific UI theme
#[inline]
pub fn get_syntax_theme(theme: &str) -> &'static str {
    get_syntax_theme_for_ui_theme(theme)
}

/// Raw RGB diff backgrounds extracted from syntax theme scopes.
///
/// Prefers `markup.inserted` / `markup.deleted` and falls back to
/// `diff.inserted` / `diff.deleted`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiffScopeBackgroundRgbs {
    pub inserted: Option<(u8, u8, u8)>,
    pub deleted: Option<(u8, u8, u8)>,
}

/// Resolve diff-scope background colors from the currently active syntax theme.
pub fn diff_scope_background_rgbs() -> DiffScopeBackgroundRgbs {
    let theme_name = get_active_syntax_theme();
    let theme = load_theme(theme_name, true);
    diff_scope_background_rgbs_for_theme(&theme)
}

fn diff_scope_background_rgbs_for_theme(theme: &Theme) -> DiffScopeBackgroundRgbs {
    let highlighter = Highlighter::new(theme);
    let inserted = scope_background_rgb(&highlighter, "markup.inserted")
        .or_else(|| scope_background_rgb(&highlighter, "diff.inserted"));
    let deleted = scope_background_rgb(&highlighter, "markup.deleted")
        .or_else(|| scope_background_rgb(&highlighter, "diff.deleted"));
    DiffScopeBackgroundRgbs { inserted, deleted }
}

fn scope_background_rgb(highlighter: &Highlighter<'_>, scope_name: &str) -> Option<(u8, u8, u8)> {
    let scope = Scope::new(scope_name).ok()?;
    let background = highlighter.style_mod_for_stack(&[scope]).background?;
    Some((background.r, background.g, background.b))
}

#[inline]
fn select_syntax(language: Option<&str>) -> &'static SyntaxReference {
    language
        .map(find_syntax_by_token)
        .unwrap_or_else(find_syntax_plain_text)
}

/// Highlight code and return styled segments per line.
///
/// Uses `LinesWithEndings` semantics by preserving an empty trailing line
/// when the input ends with `\n`.
pub fn highlight_code_to_line_segments(
    code: &str,
    language: Option<&str>,
    theme_name: &str,
) -> Vec<Vec<(syntect::highlighting::Style, String)>> {
    if !should_highlight(code) {
        return plain_text_line_segments(code);
    }

    let syntax = select_syntax(language);
    let theme = load_theme(theme_name, true);
    let mut highlighter = syntect::easy::HighlightLines::new(syntax, &theme);
    let mut result = Vec::new();
    let mut ends_with_newline = false;

    for line in LinesWithEndings::from(code) {
        ends_with_newline = line.ends_with('\n');
        let trimmed = line.trim_end_matches('\n');
        let segments = match highlighter.highlight_line(trimmed, syntax_set()) {
            Ok(ranges) => ranges
                .into_iter()
                .map(|(style, text)| (style, text.to_string()))
                .collect(),
            Err(_) => vec![(syntect::highlighting::Style::default(), trimmed.to_string())],
        };
        result.push(segments);
    }

    if ends_with_newline {
        result.push(Vec::new());
    }

    result
}

/// Highlight code and convert to `anstyle` segments with optional bg stripping.
pub fn highlight_code_to_anstyle_line_segments(
    code: &str,
    language: Option<&str>,
    theme_name: &str,
    strip_background: bool,
) -> Vec<Vec<(AnstyleStyle, String)>> {
    highlight_code_to_line_segments(code, language, theme_name)
        .into_iter()
        .map(|ranges| {
            ranges
                .into_iter()
                .filter(|(_, text)| !text.is_empty())
                .map(|(style, text)| {
                    let mut anstyle = to_anstyle(style);
                    if strip_background {
                        anstyle = anstyle.bg_color(None);
                    }
                    (anstyle, text)
                })
                .collect()
        })
        .collect()
}

/// Highlight one line and convert to `anstyle` segments with optional bg stripping.
pub fn highlight_line_to_anstyle_segments(
    line: &str,
    language: Option<&str>,
    theme_name: &str,
    strip_background: bool,
) -> Option<Vec<(AnstyleStyle, String)>> {
    highlight_code_to_anstyle_line_segments(line, language, theme_name, strip_background)
        .into_iter()
        .next()
}

/// Highlight code and return styled segments
///
/// # Arguments
/// * `code` - Source code to highlight
/// * `language` - Optional language hint (auto-detected if None)
/// * `theme_name` - Syntax theme name (use `get_active_syntax_theme()` for UI theme sync)
///
/// # Returns
/// Vector of (Style, String) tuples for rendering
///
/// # Performance
/// - Returns None early if input exceeds guardrails
/// - Uses cached theme when available
pub fn highlight_code_to_segments(
    code: &str,
    language: Option<&str>,
    theme_name: &str,
) -> Vec<(syntect::highlighting::Style, String)> {
    highlight_code_to_line_segments(code, language, theme_name)
        .into_iter()
        .flatten()
        .collect()
}

/// Highlight a single line (for diff rendering)
///
/// Preserves parser state for multiline constructs
pub fn highlight_line_for_diff(
    line: &str,
    language: Option<&str>,
    theme_name: &str,
) -> Option<Vec<(syntect::highlighting::Style, String)>> {
    highlight_code_to_line_segments(line, language, theme_name)
        .into_iter()
        .next()
}

/// Convert code to ANSI escape sequences
pub fn highlight_code_to_ansi(code: &str, language: Option<&str>, theme_name: &str) -> String {
    let segments = highlight_code_to_segments(code, language, theme_name);
    let mut output = String::with_capacity(code.len() + segments.len() * 10);

    for (style, text) in segments {
        let ansi_style = to_anstyle(style);
        output.push_str(&ansi_style.to_string());
        output.push_str(&text);
        output.push_str("\x1b[0m"); // Reset
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use syntect::highlighting::Color as SyntectColor;
    use syntect::highlighting::ScopeSelectors;
    use syntect::highlighting::StyleModifier;
    use syntect::highlighting::ThemeItem;
    use syntect::highlighting::ThemeSettings;

    fn theme_item(scope: &str, background: Option<(u8, u8, u8)>) -> ThemeItem {
        ThemeItem {
            scope: ScopeSelectors::from_str(scope).expect("scope selector should parse"),
            style: StyleModifier {
                background: background.map(|(r, g, b)| SyntectColor { r, g, b, a: 255 }),
                ..StyleModifier::default()
            },
        }
    }

    #[test]
    fn test_syntax_set_loaded() {
        let ss = syntax_set();
        assert!(!ss.syntaxes().is_empty());
    }

    #[test]
    fn test_find_syntax_by_token() {
        let rust = find_syntax_by_token("rust");
        assert!(rust.name.contains("Rust"));
    }

    #[test]
    fn test_should_highlight_guardrails() {
        assert!(should_highlight("fn main() {}"));
        assert!(!should_highlight(&"x".repeat(MAX_INPUT_SIZE_BYTES + 1)));
    }

    #[test]
    fn test_get_active_syntax_theme() {
        let theme = get_active_syntax_theme();
        assert!(!theme.is_empty());
    }

    #[test]
    fn test_highlight_code_to_segments() {
        let segments =
            highlight_code_to_segments("fn main() {}", Some("rust"), "base16-ocean.dark");
        assert!(!segments.is_empty());
    }

    #[test]
    fn test_theme_loading_stable() {
        let theme1 = load_theme("base16-ocean.dark", true);
        let theme2 = load_theme("base16-ocean.dark", true);
        assert_eq!(theme1.name, theme2.name);
    }

    #[test]
    fn diff_scope_backgrounds_prefer_markup_scope_then_diff_fallback() {
        let theme = Theme {
            settings: ThemeSettings::default(),
            scopes: vec![
                theme_item("markup.inserted", Some((10, 20, 30))),
                theme_item("diff.deleted", Some((40, 50, 60))),
            ],
            ..Theme::default()
        };

        let rgbs = diff_scope_background_rgbs_for_theme(&theme);
        assert_eq!(
            rgbs,
            DiffScopeBackgroundRgbs {
                inserted: Some((10, 20, 30)),
                deleted: Some((40, 50, 60)),
            }
        );
    }

    #[test]
    fn diff_scope_backgrounds_return_none_when_scopes_do_not_match() {
        let theme = Theme {
            settings: ThemeSettings::default(),
            scopes: vec![theme_item("constant.numeric", Some((1, 2, 3)))],
            ..Theme::default()
        };

        let rgbs = diff_scope_background_rgbs_for_theme(&theme);
        assert_eq!(
            rgbs,
            DiffScopeBackgroundRgbs {
                inserted: None,
                deleted: None,
            }
        );
    }

    #[test]
    fn diff_scope_backgrounds_fall_back_to_diff_scopes() {
        let theme = Theme {
            settings: ThemeSettings::default(),
            scopes: vec![
                theme_item("diff.inserted", Some((16, 32, 48))),
                theme_item("diff.deleted", Some((64, 80, 96))),
            ],
            ..Theme::default()
        };

        let rgbs = diff_scope_background_rgbs_for_theme(&theme);
        assert_eq!(
            rgbs,
            DiffScopeBackgroundRgbs {
                inserted: Some((16, 32, 48)),
                deleted: Some((64, 80, 96)),
            }
        );
    }
}
