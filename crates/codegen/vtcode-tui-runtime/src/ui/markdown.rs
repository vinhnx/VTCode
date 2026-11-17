use crate::ui::theme::{self, ThemeStyles};
use anstyle::Style;
use vtcode_core::config::loader::SyntaxHighlightingConfig;

pub use vtcode_commons::ui_protocol::CodeBlockWeight;

pub use vtcode_ui::tui::ui::markdown::{
    HighlightedSegment, MarkdownLine, MarkdownSegment, RenderMarkdownOptions, highlight_code_to_ansi,
    highlight_code_to_segments, highlight_line_for_diff,
};

// ── Markdown rendering ──────────────────────────────────────────────────────

/// Render markdown source into styled lines using default rendering options.
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

/// Render markdown source into styled lines with custom rendering options.
pub fn render_markdown_to_lines_with_options(
    source: &str,
    base_style: Style,
    theme_styles: &ThemeStyles,
    highlight_config: Option<&SyntaxHighlightingConfig>,
    render_options: RenderMarkdownOptions,
) -> Vec<MarkdownLine> {
    let tui_theme_styles = crate::ui::tui_compat::tui_theme_styles_from_core(theme_styles);
    vtcode_ui::tui::ui::markdown::render_markdown_to_lines_with_options(
        source,
        base_style,
        &tui_theme_styles,
        highlight_config,
        render_options,
    )
}

/// Render markdown source using the currently active theme styles.
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
