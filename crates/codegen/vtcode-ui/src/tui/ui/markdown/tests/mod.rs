pub(super) use super::code_blocks::{
    normalize_code_indentation, normalize_diff_lines, render_diff_code_block_with_context, render_diff_content_segments,
};
pub(super) use super::links::{
    COLON_LOCATION_SUFFIX_RE, HASH_LOCATION_SUFFIX_RE, label_has_location_suffix, label_segments_have_location_suffix,
    normalize_hash_location,
};
pub(super) use super::*;
pub(super) use crate::tui::ui::syntax_highlight::DiffScopeBackgroundRgbs;
pub(super) use crate::tui::utils::diff_styles::DiffColorPalette;
pub(super) use crate::tui::utils::diff_styles::{
    DiffColorLevel, DiffLineType, DiffTheme, diff_add_fg, diff_del_fg, diff_render_style_context_for,
    style_content_ansi, style_sign_ansi,
};

fn lines_to_text(lines: &[MarkdownLine]) -> Vec<String> {
    lines
        .iter()
        .map(|line| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>())
        .collect()
}

fn render_markdown_with_table_width(markdown: &str, table_max_width: usize) -> Vec<MarkdownLine> {
    render_markdown_to_lines_with_options(
        markdown,
        Style::default(),
        &theme::active_styles(),
        None,
        RenderMarkdownOptions {
            preserve_code_indentation: false,
            disable_code_block_table_reparse: false,
            table_max_width: Some(table_max_width),
            code_block_weight: CodeBlockWeight::Primary,
        },
    )
}

fn render_markdown_highlighted(markdown: &str) -> Vec<MarkdownLine> {
    render_markdown_to_lines_with_options(
        markdown,
        Style::default(),
        &theme::active_styles(),
        Some(&SyntaxHighlightingConfig::default()),
        RenderMarkdownOptions::default(),
    )
}

fn diff_backgrounds_enabled() -> bool {
    DiffColorLevel::detect() != DiffColorLevel::Ansi16
}

/// Foreground colors of the code body of the first code line (gutter excluded).
fn code_foreground_colors(lines: &[MarkdownLine]) -> Vec<anstyle::Color> {
    lines
        .iter()
        .filter(|line| {
            line.segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<String>()
                .contains("fn main()")
        })
        .flat_map(|line| line.segments.iter().skip(1))
        .filter_map(|segment| segment.style.get_fg_color())
        .collect()
}

mod blocks;
mod code_blocks;
mod diff;
mod lists_links;
mod tables;
