use super::*;
use vtcode_commons::history_scope::HistoryScope;
use vtcode_core::utils::session_archive::{list_recent_sessions, list_recent_sessions_in_workspace};

use crate::agent::runloop::unified::turn::clean_archived_prompts;

/// Number of recent archives scanned for history search.
const RECENT_PROMPT_SESSIONS: usize = 50;

pub(super) async fn handle_recent_prompts_request(
    context: &ServiceContext,
    request: &Request,
) -> Result<(Value, bool)> {
    let params: protocol::RecentPromptsParams = parse_params(&request.params)?;
    // Session scope is served from the client's live thread; answer honestly
    // instead of erroring if a client asks anyway.
    let listings = match params.scope {
        HistoryScope::Project => list_recent_sessions_in_workspace(RECENT_PROMPT_SESSIONS, &context.workspace).await?,
        HistoryScope::Everywhere => list_recent_sessions(RECENT_PROMPT_SESSIONS).await?,
        HistoryScope::Session => Vec::new(),
    };
    // Same recovery logic as the TUI history injection: genuine user prompts
    // only, listing and message order preserved, bounded.
    let prompts: Vec<protocol::RecentPrompt> =
        clean_archived_prompts(&listings, &context.workspace, protocol::MAX_RECENT_PROMPTS)
            .into_iter()
            .filter(|prompt| params.scope != HistoryScope::Project || prompt.in_workspace)
            .map(|prompt| protocol::RecentPrompt {
                content: prompt.content,
                session_id: prompt.session_id,
            })
            .collect();
    let result = protocol::RecentPromptsResult { scope: params.scope, prompts };
    Ok((serde_json::to_value(result)?, false))
}
