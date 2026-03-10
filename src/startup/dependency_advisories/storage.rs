use vtcode_core::tools::{AstGrepStatus, RipgrepStatus};
use vtcode_core::utils::dot_config::{DotConfig, load_user_config, save_user_config};

use super::notice::OptionalSearchToolsNotice;

pub(super) async fn take_optional_search_tools_notice() -> Option<OptionalSearchToolsNotice> {
    let ripgrep_status = RipgrepStatus::check();
    let ast_grep_status = AstGrepStatus::check();
    let (mut config, persist_notice) = load_notice_config().await;
    let notice =
        OptionalSearchToolsNotice::from_snapshot(&config, ripgrep_status, ast_grep_status)?;

    if persist_notice {
        notice.apply_to_config(&mut config);
        let _ = save_user_config(&config).await;
    }

    Some(notice)
}

async fn load_notice_config() -> (DotConfig, bool) {
    match load_user_config().await {
        Ok(config) => (config, true),
        Err(_) => (DotConfig::default(), false),
    }
}
