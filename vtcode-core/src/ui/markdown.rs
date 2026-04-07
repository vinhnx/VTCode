use crate::config::loader::SyntaxHighlightingConfig;
use crate::ui::theme::{self, ThemeStyles};
use anstyle::Style;

// When TUI is enabled, use vtcode-tui's richer types (with internal methods).
#[cfg(feature = "tui")]
pub use vtcode_tui::ui::markdown::{
    HighlightedSegment, MarkdownLine, MarkdownSegment, RenderMarkdownOptions,
    highlight_code_to_ansi, highlight_code_to_segments, highlight_line_for_diff,
};

// When headless, use the plain data types from commons.
#[cfg(not(feature = "tui"))]
pub use vtcode_commons::ui_protocol::{
    HighlightedSegment, MarkdownLine, MarkdownSegment, RenderMarkdownOptions,
};

#[cfg(not(feature = "tui"))]
pub fn highlight_code_to_ansi(code: &str, _language: Option<&str>, _theme: &str) -> String {
    code.to_string()
}

#[cfg(not(feature = "tui"))]
pub fn highlight_code_to_segments(
    code: &str,
    _language: Option<&str>,
    _theme: &str,
) -> Vec<HighlightedSegment> {
    vec![HighlightedSegment {
        style: Style::default(),
        text: code.to_string(),
    }]
}

#[cfg(not(feature = "tui"))]
pub fn highlight_line_for_diff(
    line: &str,
    _language: Option<&str>,
) -> Option<Vec<(Style, String)>> {
    Some(vec![(Style::default(), line.to_string())])
}

// ── Markdown rendering ──────────────────────────────────────────────────────

pub fn render_markdown_to_lines(
    source: &str,
    base_style: Style,
    theme_styles: &ThemeStyles,
    highlight_config: Option<&SyntaxHighlightingConfig>,
) -> Vec<MarkdownLine> {
    render_markdown_to_lines_with_options(
        source,
        base_style,
        theme_styles,
        highlight_config,
        RenderMarkdownOptions::default(),
    )
}

#[cfg(feature = "tui")]
pub fn render_markdown_to_lines_with_options(
    source: &str,
    base_style: Style,
    theme_styles: &ThemeStyles,
    highlight_config: Option<&SyntaxHighlightingConfig>,
    render_options: RenderMarkdownOptions,
) -> Vec<MarkdownLine> {
    let tui_theme_styles = crate::ui::tui_compat::tui_theme_styles_from_core(theme_styles);
    let tui_highlight_cfg = highlight_config.map(|cfg| vtcode_tui::TuiSyntaxHighlightingConfig {
        enabled: cfg.enabled,
        theme: cfg.theme.clone(),
        cache_themes: cfg.cache_themes,
        max_file_size_mb: cfg.max_file_size_mb,
        enabled_languages: cfg.enabled_languages.clone(),
        highlight_timeout_ms: cfg.highlight_timeout_ms,
    });
    vtcode_tui::ui::markdown::render_markdown_to_lines_with_options(
        source,
        base_style,
        &tui_theme_styles,
        tui_highlight_cfg.as_ref(),
        render_options,
    )
}

#[cfg(not(feature = "tui"))]
pub fn render_markdown_to_lines_with_options(
    source: &str,
    base_style: Style,
    _theme_styles: &ThemeStyles,
    _highlight_config: Option<&SyntaxHighlightingConfig>,
    _render_options: RenderMarkdownOptions,
) -> Vec<MarkdownLine> {
    let mut lines: Vec<MarkdownLine> = source
        .lines()
        .map(|line| MarkdownLine {
            segments: if line.is_empty() {
                Vec::new()
            } else {
                vec![MarkdownSegment {
                    style: base_style,
                    text: line.to_string(),
                    link_target: None,
                }]
            },
        })
        .collect();
    if lines.is_empty() {
        lines.push(MarkdownLine::default());
    }
    lines
}

pub fn render_markdown(source: &str) -> Vec<MarkdownLine> {
    let styles = theme::active_styles();
    render_markdown_to_lines(source, Style::default(), &styles, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_renders_markdown() {
        let lines = render_markdown("# Heading");
        assert!(!lines.is_empty());
    }
}
