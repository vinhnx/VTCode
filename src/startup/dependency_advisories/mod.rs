mod notice;
mod storage;

use anyhow::Result;
use vtcode_core::utils::ansi::AnsiRenderer;

pub(crate) use notice::OptionalSearchToolsNotice as SearchToolsBundleNotice;
pub(crate) use storage::take_optional_search_tools_notice as take_search_tools_bundle_notice;

pub(crate) async fn render_optional_search_tools_notice(renderer: &mut AnsiRenderer) -> Result<()> {
    if let Some(notice) = take_search_tools_bundle_notice().await {
        notice.render(renderer)?;
    }
    Ok(())
}
