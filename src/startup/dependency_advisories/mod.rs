mod notice;
mod storage;

use anyhow::Result;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::InlineHeaderHighlight;

use storage::take_optional_search_tools_notice;

pub(crate) async fn append_optional_search_tools_highlight(
    highlights: &mut Vec<InlineHeaderHighlight>,
) {
    if let Some(notice) = take_optional_search_tools_notice().await {
        highlights.push(notice.to_highlight());
    }
}

pub(crate) async fn render_optional_search_tools_notice(renderer: &mut AnsiRenderer) -> Result<()> {
    if let Some(notice) = take_optional_search_tools_notice().await {
        notice.render(renderer)?;
    }
    Ok(())
}
