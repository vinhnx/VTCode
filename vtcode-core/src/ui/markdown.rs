//! Compatibility facade for markdown rendering.
//!
//! `vtcode-tui` owns the markdown rendering implementation. `vtcode-core`
//! re-exports the public API and adapts core config/theme types to the TUI
//! equivalents.

use crate::config::loader::SyntaxHighlightingConfig;
use crate::ui::theme::{self, ThemeStyles};
use anstyle::Style;

pub use vtcode_tui::ui::markdown::{
    HighlightedSegment, MarkdownLine, MarkdownSegment, RenderMarkdownOptions,
    highlight_code_to_ansi, highlight_code_to_segments, highlight_line_for_diff,
};

fn to_tui_theme_styles(styles: &ThemeStyles) -> vtcode_tui::ui::theme::ThemeStyles {
    vtcode_tui::ui::theme::ThemeStyles {
        info: styles.info,
        error: styles.error,
        output: styles.output,
        response: styles.response,
        reasoning: styles.reasoning,
        tool: styles.tool,
        tool_detail: styles.tool_detail,
        tool_output: styles.tool_output,
        pty_output: styles.pty_output,
        status: styles.status,
        mcp: styles.mcp,
        user: styles.user,
        primary: styles.primary,
        secondary: styles.secondary,
        background: styles.background,
        foreground: styles.foreground,
    }
}

fn to_tui_highlight_config(
    cfg: &SyntaxHighlightingConfig,
) -> vtcode_tui::TuiSyntaxHighlightingConfig {
    vtcode_tui::TuiSyntaxHighlightingConfig {
        enabled: cfg.enabled,
        theme: cfg.theme.clone(),
        cache_themes: cfg.cache_themes,
        max_file_size_mb: cfg.max_file_size_mb,
        enabled_languages: cfg.enabled_languages.clone(),
        highlight_timeout_ms: cfg.highlight_timeout_ms,
    }
}

/// Render markdown text to styled lines that can be written to the terminal renderer.
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

pub fn render_markdown_to_lines_with_options(
    source: &str,
    base_style: Style,
    theme_styles: &ThemeStyles,
    highlight_config: Option<&SyntaxHighlightingConfig>,
    render_options: RenderMarkdownOptions,
) -> Vec<MarkdownLine> {
    let tui_theme_styles = to_tui_theme_styles(theme_styles);
    let tui_highlight_cfg = highlight_config.map(to_tui_highlight_config);
    vtcode_tui::ui::markdown::render_markdown_to_lines_with_options(
        source,
        base_style,
        &tui_theme_styles,
        tui_highlight_cfg.as_ref(),
        render_options,
    )
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
