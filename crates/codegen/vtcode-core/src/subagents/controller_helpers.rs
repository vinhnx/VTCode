#![allow(unused_imports)]
use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use futures::future::select_all;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Notify, RwLock};

use crate::config::VTCodeConfig;
use crate::config::types::ReasoningEffortLevel;
use crate::core::agent::runner::{AgentRunner, RunnerSettings};
use crate::core::agent::task::Task;
use crate::core::threads::{ThreadBootstrap, ThreadId, ThreadRuntimeHandle, ThreadSnapshot};
use crate::hooks::{LifecycleHookEngine, SessionStartTrigger};
use crate::llm::provider::Message;
use crate::tools::exec_session::ExecSessionManager;
use crate::tools::pty::{PtyManager, PtySize};
use crate::utils::session_archive::{SessionArchive, find_session_by_identifier};
use vtcode_config::SubagentSpec;
use vtcode_config::auth::OpenAIChatGptAuthHandle;

use self::background::*;
use self::config::*;
use self::constants::*;
use self::discovery::discover_controller_subagents;
use self::model::*;
use vtcode_config::subagents::SUBAGENT_HARD_CONCURRENCY_LIMIT;

#[allow(unused_imports)]
use super::*;

pub(super) fn sanitize_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

pub(super) async fn load_session_listing(
    path: &std::path::Path,
) -> Result<crate::utils::session_archive::SessionListing> {
    use anyhow::Context;
    let raw = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read session archive {}", path.display()))?;
    let snapshot: crate::utils::session_archive::SessionSnapshot =
        serde_json::from_str(&raw).with_context(|| format!("Failed to parse session archive {}", path.display()))?;
    Ok(crate::utils::session_archive::SessionListing { path: path.to_path_buf(), snapshot })
}

pub(super) async fn checkpoint_subagent_archive_start(archive: &SessionArchive, messages: &[Message]) -> Result<()> {
    use crate::utils::session_archive::SessionMessage;
    let recent_messages: Vec<SessionMessage> = messages.iter().map(SessionMessage::from).collect::<Vec<_>>();
    archive
        .persist_progress_async(crate::utils::session_archive::SessionProgressArgs {
            total_messages: recent_messages.len(),
            distinct_tools: Vec::new(),
            messages: recent_messages.clone(),
            recent_messages,
            turn_number: 1,
            token_usage: None,
            max_context_tokens: None,
            loaded_skills: Some(Vec::new()),
        })
        .await?;
    Ok(())
}

pub(super) async fn persist_child_archive(
    archive: &SessionArchive,
    messages: &[Message],
    agent_name: &str,
) -> Result<Option<PathBuf>> {
    use crate::utils::session_archive::SessionMessage;
    let transcript = messages
        .iter()
        .filter_map(transcript_line_from_message)
        .take(SUBAGENT_TRANSCRIPT_LINE_LIMIT)
        .collect::<Vec<_>>();
    let stored_messages = messages.iter().map(SessionMessage::from).collect::<Vec<_>>();
    let path = archive.finalize(transcript, stored_messages.len(), vec![agent_name.to_string()], stored_messages)?;
    Ok(Some(path))
}

pub(super) fn transcript_line_from_message(message: &Message) -> Option<String> {
    let role = message.role.to_string();
    let content = message.content.trim();
    if content.is_empty() {
        return None;
    }
    Some(format!("{role}: {content}"))
}

/// Extract issue descriptions from a verifier sub-agent's summary text.
///
/// Looks for lines starting with common issue markers (e.g. "- ISSUE:",
/// "- REJECT:", numbered items) and collects them. Returns an empty vec
/// if no structured issues are found.
pub(super) fn extract_issues_from_summary(summary: &str) -> Vec<String> {
    let mut issues = Vec::new();
    for line in summary.lines() {
        let trimmed = line.trim();
        // Match patterns like "- ISSUE: ...", "- REJECT: ...", "1. ISSUE: ..."
        let lower = trimmed.to_ascii_lowercase();
        if lower.contains("issue:")
            || lower.contains("reject:")
            || lower.contains("problem:")
            || lower.contains("error:")
            || lower.contains("violation:")
        {
            // Strip leading list markers ("- ", "1. ", "* ", etc.)
            let cleaned = trimmed
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-' || c == '*' || c == ' ')
                .trim();
            if !cleaned.is_empty() {
                issues.push(cleaned.to_string());
            }
        }
    }
    issues
}
