pub(super) use vtcode_core::ui::InlineHandle;
pub(super) use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

pub(super) use anstyle::AnsiColor;
pub(super) use vtcode_commons::diff_theme::{DiffColorLevel, DiffTheme};
pub(super) use vtcode_diff::{DiffDisplayKind, DiffDisplayLine, SideBySideRow};

pub(super) use crate::agent::runloop::tool_output::collect_inline_output;

pub(super) use super::super::styles::{GitStyles, LsStyles};
pub(super) use super::{
    EXEC_SESSION_OUTPUT_MAX_LINES, HiddenLinesNoticeKind, MAX_LINE_LENGTH, collect_run_command_preview,
    diff_language_hint_from_content, format_diff_line_with_gutter_and_syntax,
    format_diff_line_with_gutter_and_syntax_to_width, format_side_by_side_row_ansi, hidden_lines_notice,
    hidden_lines_notice_with, highlight_diff_body_with_syntax, highlight_diff_content, is_exec_session_tool,
    language_hint_for_display_line, render_diff_content_block, render_preview_line, render_stream_section,
    select_render_line_style, should_show_diff_gutter, strip_ansi_codes, syntax_segments_for_diff_body, trim_to_tail,
};
pub(super) use smallvec::SmallVec;
pub(super) use vtcode_core::config::ToolOutputMode;
pub(super) use vtcode_core::config::constants::tools as tool_names;

/// One style key per rendered body line, covering every segment on that line
/// (indent prefix plus text). Text alone cannot prove styling, and per-line
/// coloring shows up on the indent prefix even when the text stays uncolored.
async fn collect_inline_line_styles(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<vtcode_core::ui::InlineCommand>,
) -> Vec<String> {
    let mut lines = Vec::new();
    while let Ok(command) = receiver.try_recv() {
        if let vtcode_core::ui::InlineCommand::AppendLine { segments, .. } = command {
            let key = segments
                .iter()
                .filter(|segment| !segment.text.is_empty())
                .map(|segment| style_key(segment.style.as_ref()))
                .collect::<Vec<_>>()
                .join("|");
            if !key.is_empty() {
                lines.push(key);
            }
        }
    }
    lines
}

/// The same content rendered through the styled diff path, proving per-line
/// coloring is observable through this seam when it is applied.
async fn collect_diff_body_styles(content: &str) -> Vec<String> {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    render_stream_section(
        &mut renderer,
        "",
        content,
        ToolOutputMode::Compact,
        30,
        Some(vtcode_core::config::constants::tools::APPLY_PATCH),
        &GitStyles::new(),
        &LsStyles::from_env(),
        MessageStyle::ToolDetail,
        false,
        true,
        None,
    )
    .await
    .expect("control diff body should render");
    collect_inline_line_styles(&mut receiver).await
}

fn style_key(style: &vtcode_commons::ui_protocol::InlineTextStyle) -> String {
    format!("{:?}/{:?}", style.color, style.effects)
}

fn test_diff_line(kind: DiffDisplayKind, old_line: Option<u32>, new_line: Option<u32>, text: &str) -> DiffDisplayLine {
    DiffDisplayLine {
        kind,
        old_line,
        new_line,
        text: text.to_string(),
        changed: Vec::new(),
    }
}

mod diff_format;
mod diff_rows;
mod exec_body;
mod preview_trim;
mod side_by_side;
