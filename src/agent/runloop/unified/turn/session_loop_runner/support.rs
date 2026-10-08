use super::archive::workspace_archive_label;
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::git::{DirtyWorktreeStatus, git_dirty_worktree_entries, workspace_relative_display};
use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Notify;
use vtcode_config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider::{AssistantPhase, MessageRole};
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::utils::session_archive;
use vtcode_core::utils::session_archive::{SessionMessage, SessionProgressArgs};
use vtcode_ui::tui::app::{
    InlineHandle, InlineListItem, InlineListSelection, InlineSession, ListOverlayRequest, TransientRequest,
    TransientSubmission,
};

const STARTUP_PLANNING_WORKFLOW_ENTER_ACTION: &str = "planning_active:start_enter";
const STARTUP_PLANNING_WORKFLOW_STAY_ACTION: &str = "planning_active:start_stay";

/// Boundary captured before a turn starts so a provider refusal can roll the
/// refused turn out of model-visible history.
///
/// A refused request is terminal: resending it, or keeping it where the next
/// request replays it, is refused again. Rolling back truncates history to its
/// state before the refused user message, so the surviving prefix is exactly
/// what the provider accepted and stays append-only. The refusal notice is
/// shown in the transcript and the harness event stream only.
#[derive(Debug, Clone)]
pub(super) struct RefusedTurnRollback {
    /// First index removed by the rollback: the turn's prompt message, or the
    /// pre-turn history length when the turn has no prompt message.
    index: usize,
    /// The prompt message at `index`, used to locate the boundary again if an
    /// in-turn rewrite (for example compaction) shifted it.
    prompt: Option<vtcode_core::llm::provider::Message>,
}

impl RefusedTurnRollback {
    /// Capture the boundary before any per-turn notes are appended.
    ///
    /// `prompt_message_index` is the index the interaction loop (or the
    /// approved-plan handoff) pushed the prompt at. Queued follow-ups report
    /// no index but append the prompt as the last user message.
    pub(super) fn capture(
        history: &[vtcode_core::llm::provider::Message],
        prompt_message_index: Option<usize>,
        prompt_text: &str,
    ) -> Self {
        let index = prompt_message_index.filter(|index| *index < history.len()).or_else(|| {
            history
                .last()
                .filter(|message| {
                    message.role == MessageRole::User && message.content.as_text().trim() == prompt_text.trim()
                })
                .map(|_| history.len() - 1)
        });
        match index {
            Some(index) => Self { index, prompt: history.get(index).cloned() },
            None => Self { index: history.len(), prompt: None },
        }
    }

    /// Truncate `history` to its state before the refused turn. Returns
    /// whether the boundary was found; when it was not, history is left
    /// untouched rather than truncated at a guessed position.
    pub(super) fn apply(&self, history: &mut Vec<vtcode_core::llm::provider::Message>) -> bool {
        let boundary = match &self.prompt {
            Some(prompt) if history.get(self.index) == Some(prompt) => Some(self.index),
            Some(prompt) => history.iter().rposition(|message| message == prompt),
            None => (self.index <= history.len()).then_some(self.index),
        };
        match boundary {
            Some(boundary) => {
                history.truncate(boundary);
                true
            }
            None => {
                tracing::warn!(
                    index = self.index,
                    history_len = history.len(),
                    "Refused turn boundary not found; history left unchanged"
                );
                false
            }
        }
    }
}

#[cfg(test)]
#[derive(Clone)]
pub(super) struct TurnHistoryCheckpoint {
    baseline_len: usize,
    #[cfg(debug_assertions)]
    prefix_fingerprint: u64,
}

#[cfg(test)]
impl TurnHistoryCheckpoint {
    pub(super) fn capture(history: &[vtcode_core::llm::provider::Message]) -> Self {
        Self {
            baseline_len: history.len(),
            #[cfg(debug_assertions)]
            prefix_fingerprint: Self::prefix_fingerprint(history),
        }
    }

    pub(super) fn rollback(&self, history: &mut Vec<vtcode_core::llm::provider::Message>) {
        #[cfg(debug_assertions)]
        self.assert_append_only(history);
        history.truncate(self.baseline_len);
    }

    #[cfg(debug_assertions)]
    fn assert_append_only(&self, history: &[vtcode_core::llm::provider::Message]) {
        debug_assert!(
            history.len() >= self.baseline_len,
            "turn history rollback requires append-only growth after checkpoint"
        );
        debug_assert_eq!(
            Self::prefix_fingerprint(&history[..self.baseline_len]),
            self.prefix_fingerprint,
            "turn history rollback requires the pre-checkpoint prefix to remain unchanged"
        );
    }

    #[cfg(debug_assertions)]
    fn prefix_fingerprint(history: &[vtcode_core::llm::provider::Message]) -> u64 {
        use std::hash::{Hash, Hasher};

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(history).unwrap_or_default().hash(&mut hasher);
        hasher.finish()
    }
}

pub(super) fn remove_transient_system_notes(history: &mut Vec<vtcode_core::llm::provider::Message>, notes: &[String]) {
    for note in notes.iter().rev() {
        if let Some(index) = history
            .iter()
            .rposition(|message| message.role == MessageRole::System && message.content.as_text() == note.as_str())
        {
            let _ = history.remove(index);
        }
    }
}

/// User-facing notice when a prompt checkpoint cannot be taken.
///
/// Pure policy: maps the checkpoint error text to a short `Info` notice.
/// The full error stays in the tracing log; the UI stays concise. The busy
/// branch names the safety reason (checkpoint corruption) and both options
/// (wait + retry, separate worktree) so parallel-session users can act.
pub(super) fn checkpoint_unavailable_notice(err_text: &str) -> &'static str {
    if err_text.contains("Another turn or rewind is using this workspace") {
        "Prompt kept — workspace busy (safety lock prevents checkpoint corruption). Options: wait then retry, or use a separate worktree."
    } else if err_text.contains("Interrupted rewind") {
        "Prompt kept — rewind interrupted. Run /rewind again to auto-recover, then retry."
    } else {
        "Prompt kept — checkpoint unavailable. Wait a moment and retry."
    }
}

pub(super) fn build_tracked_file_freshness_note(
    workspace: &std::path::Path,
    stale_paths: &[std::path::PathBuf],
) -> Option<String> {
    if stale_paths.is_empty() {
        return None;
    }

    let display_paths = stale_paths
        .iter()
        .map(|path| format!("- {}", workspace_relative_display(workspace, path)))
        .collect::<Vec<_>>()
        .join("\n");

    Some(format!(
        "Freshness note: the following files changed on disk after VT Code last read them:\n{display_paths}\nRe-read these files before relying on earlier content because disk content is newer than the agent's prior read snapshot."
    ))
}

/// Upper bound on paths listed in [`build_withdrawn_turn_changes_note`].
const MAX_WITHDRAWN_TURN_PATHS: usize = 20;

/// Model-visible note for file changes made by a refused turn.
///
/// Rolling back a refused turn removes its tool calls and results from
/// history, but the edits those calls made stay on disk. Without this note the
/// next turn reasons from file contents that no longer match. The note names
/// only paths, never content from the refused turn.
pub(super) fn build_withdrawn_turn_changes_note(
    workspace: &std::path::Path,
    modified_paths: &std::collections::BTreeSet<std::path::PathBuf>,
) -> Option<String> {
    if modified_paths.is_empty() {
        return None;
    }

    let mut display_paths = modified_paths
        .iter()
        .take(MAX_WITHDRAWN_TURN_PATHS)
        .map(|path| format!("- {}", workspace_relative_display(workspace, path)))
        .collect::<Vec<_>>();
    let omitted = modified_paths.len().saturating_sub(MAX_WITHDRAWN_TURN_PATHS);
    if omitted > 0 {
        display_paths.push(format!("- and {omitted} more"));
    }

    Some(format!(
        "The previous request was declined and removed from this conversation, but before that it modified these files:\n{}\nTheir contents may differ from what earlier messages show; read them again before relying on or editing them.",
        display_paths.join("\n")
    ))
}

pub(super) fn format_workspace_relative_paths<I>(workspace: &std::path::Path, paths: I) -> String
where
    I: IntoIterator,
    I::Item: AsRef<std::path::Path>,
{
    let display_paths = paths
        .into_iter()
        .map(|path| workspace_relative_display(workspace, path.as_ref()))
        .collect::<Vec<_>>();
    if display_paths.is_empty() {
        return "none recorded".to_string();
    }

    display_paths.join(", ")
}

pub(super) fn build_unrelated_dirty_worktree_note(
    workspace: &std::path::Path,
    agent_touched_paths: &std::collections::BTreeSet<std::path::PathBuf>,
) -> Result<Option<String>> {
    let Some(entries) = git_dirty_worktree_entries(workspace)? else {
        return Ok(None);
    };

    let display_paths = entries
        .into_iter()
        .filter(|entry| entry.status == DirtyWorktreeStatus::Modified && !agent_touched_paths.contains(&entry.path))
        .map(|entry| format!("- {}", workspace_relative_display(workspace, &entry.path)))
        .collect::<Vec<_>>();

    if display_paths.is_empty() {
        return Ok(None);
    }

    Ok(Some(format!(
        "Workspace note: the following files already have unrelated user modifications before this turn:\n{}\nTreat these files as user-owned changes. Do not edit, format, revert, or overwrite them unless the user explicitly asks to work on those files.",
        display_paths.join("\n")
    )))
}

/// Assemble transient system notes without mutating model history.
/// The caller publishes them only after its cancellable preparation wait succeeds.
///
/// `unrelated_dirty_note` is pre-fetched by the caller via `spawn_blocking`
/// (see `orchestration.rs`) because `build_unrelated_dirty_worktree_note`
/// spawns blocking `git` subprocesses — see the `# Blocking` docs in `git.rs`.
pub(super) async fn prepare_transient_turn_notes(
    workspace: &std::path::Path,
    tool_registry: &ToolRegistry,
    unrelated_dirty_note: Option<String>,
    background_completion_note: Option<String>,
) -> Vec<String> {
    let mut transient_system_notes = Vec::with_capacity(4);

    if let Some(note) = {
        let stale_paths = tool_registry.edited_file_monitor_ref().stale_tracked_paths();
        build_tracked_file_freshness_note(workspace, &stale_paths)
    } {
        transient_system_notes.push(note);
    }

    if let Some(note) = unrelated_dirty_note {
        transient_system_notes.push(note);
    }

    // Cross-turn exec-session resume: when the previous turn ended with a
    // long command still running, hand the model the settled identity and a
    // pre-filled wait call so it needs zero reconstruction. This also covers
    // session restore — both paths flow through the same turn loop.
    if let Some(note) = build_exec_session_resume_note(tool_registry).await {
        transient_system_notes.push(note);
    }

    if let Some(note) = background_completion_note {
        transient_system_notes.push(note);
    }

    transient_system_notes
}

/// Cap on exec sessions surfaced in one cross-turn resume hint.
const EXEC_SESSION_RESUME_HINT_CAP: usize = 4;

/// Per-session command display cap in the resume hint. Keeps the injected
/// message bounded even for a command with a long inlined script body.
const EXEC_SESSION_RESUME_COMMAND_MAX_BYTES: usize = 160;

/// Build the bounded cross-turn exec-session resume note, or `None` when no
/// exec session is still running.
///
/// The note carries the session id, command display, and a pre-filled
/// `next_wait_args` shape so the model can settle the session without
/// reconstructing anything. Bounded to a few hundred bytes.
pub(super) async fn build_exec_session_resume_note(tool_registry: &ToolRegistry) -> Option<String> {
    // Background sessions are intentionally retained across turns; surfacing
    // them as mandatory resume work would make the next model turn wait on
    // the very sessions the caller asked to keep running asynchronously.
    let sessions = tool_registry
        .in_progress_foreground_exec_sessions(EXEC_SESSION_RESUME_HINT_CAP)
        .await;
    if sessions.is_empty() {
        return None;
    }

    let lines = sessions
        .iter()
        .map(|session| {
            let raw_command = if session.args.is_empty() {
                session.command.clone()
            } else {
                format!("{} {}", session.command, session.args.join(" "))
            };
            let command_display = vtcode_commons::formatting::truncate_byte_budget(
                &raw_command,
                EXEC_SESSION_RESUME_COMMAND_MAX_BYTES,
                "…",
            );
            let elapsed = session
                .started_at
                .map(|started| {
                    let secs = chrono::Utc::now().signed_duration_since(started).num_seconds().max(0);
                    format!(", running {secs}s")
                })
                .unwrap_or_default();
            format!("- {} (`{}`{})", session.id.as_str(), command_display, elapsed)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let first_session_id = sessions[0].id.as_str().to_string();
    let count = sessions.len();
    Some(format!(
        "Exec session resume: {count} foreground command session(s) are still running:\n\
         {lines}\n\
         (showing newest {count}, capped at 4)\n\
         Settle them before starting new work: call `write_stdin` with \
         {{\"session_id\": \"{first_session_id}\", \"action\": \"wait\", \
         \"wait_timeout_seconds\": 600}} verbatim (copy the session_id exactly; never guess the id) — \
         `wait` blocks until exit or deadline while `poll` returns immediately; wait/inspect calls are exempt from the per-turn \
         tool-call budget. A deadline-expired wait returns an in-progress session that can be \
         waited on again with the same session_id. With several sessions, repeat this wait per id newest-first; \
         use `inspect` only for a bounded snapshot, not to wait."
    ))
}

pub(super) fn latest_assistant_result_text(messages: &[vtcode_core::llm::provider::Message]) -> Option<String> {
    messages.iter().rev().find_map(|message| {
        if message.role != MessageRole::Assistant
            || message.tool_calls.is_some()
            || message.phase == Some(AssistantPhase::Commentary)
        {
            return None;
        }

        let text = message.content.as_text();
        let trimmed = text.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExecutionSummaryStatus {
    Completed,
    Blocked,
    Failed,
}

impl ExecutionSummaryStatus {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExecutionSummary {
    pub(super) status: ExecutionSummaryStatus,
    pub(super) blocker: Option<String>,
}

pub(super) fn classify_execution_summary(
    result: &TurnLoopResult,
    final_response_was_fallback: bool,
    checklist: Option<&serde_json::Value>,
    changed_files: bool,
) -> ExecutionSummaryStatus {
    match result {
        TurnLoopResult::Completed { .. } => {
            if final_response_was_fallback {
                return ExecutionSummaryStatus::Blocked;
            }
            let Some(checklist) = checklist else {
                return ExecutionSummaryStatus::Blocked;
            };
            let total = checklist.get("total").and_then(serde_json::Value::as_u64).unwrap_or_default();
            let completed = checklist
                .get("completed")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            let pending = checklist.get("pending").and_then(serde_json::Value::as_u64).unwrap_or_default();
            let in_progress = checklist
                .get("in_progress")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            let blocked = checklist.get("blocked").and_then(serde_json::Value::as_u64).unwrap_or_default();
            if total == 0 || completed < total || pending != 0 || in_progress != 0 || blocked != 0 {
                return ExecutionSummaryStatus::Blocked;
            }
            // Read-only review plans complete without file mutations:
            // session-20260923 was marked Blocked despite a fully completed
            // review checklist because no files changed. Implementation plans
            // still require evidence of mutation.
            if !changed_files && !checklist_is_review_only(checklist) {
                return ExecutionSummaryStatus::Blocked;
            }
            ExecutionSummaryStatus::Completed
        }
        TurnLoopResult::Blocked { .. } => ExecutionSummaryStatus::Blocked,
        TurnLoopResult::Aborted | TurnLoopResult::Cancelled | TurnLoopResult::Exit => ExecutionSummaryStatus::Failed,
    }
}

/// Whether an approved-plan checklist describes read-only review work with no
/// file mutations expected. All item descriptions must match review verbs and
/// none may match mutation verbs; empty or malformed checklists fail closed
/// as implementation work so the file-change requirement is preserved.
fn checklist_is_review_only(checklist: &serde_json::Value) -> bool {
    let Some(items) = checklist.get("items").and_then(serde_json::Value::as_array) else {
        return false;
    };
    if items.is_empty() {
        return false;
    }
    items.iter().all(|item| {
        let description = item
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if description.trim().is_empty() {
            return false;
        }
        let is_review = description.contains("review")
            || description.contains("inspect")
            || description.contains("assess")
            || description.contains("audit")
            || description.contains("investigat")
            || description.contains("analy");
        if !is_review {
            return false;
        }
        !(description.contains("implement")
            || description.contains("fix")
            || description.contains("edit")
            || description.contains("create")
            || description.contains("update")
            || description.contains("delete")
            || description.contains("refactor")
            || description.contains("migrat"))
    })
}

fn pending_checklist_items(checklist: &serde_json::Value) -> Vec<String> {
    checklist
        .get("items")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("status").and_then(serde_json::Value::as_str) != Some("completed"))
        .filter_map(|item| item.get("description").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .take(4)
        .collect()
}

fn execution_summary_blocker(
    result: &TurnLoopResult,
    final_response_was_fallback: bool,
    checklist: Option<&serde_json::Value>,
    changed_files: bool,
) -> Option<String> {
    if final_response_was_fallback {
        return Some("recovery ended with a deterministic fallback and did not confirm the requested work".to_string());
    }
    if !changed_files
        && matches!(result, TurnLoopResult::Completed { .. })
        && !checklist.is_some_and(checklist_is_review_only)
    {
        return Some(
            "the approved-plan turn produced no file changes, so implementation completion was not confirmed"
                .to_string(),
        );
    }
    if matches!(result, TurnLoopResult::Aborted | TurnLoopResult::Cancelled | TurnLoopResult::Exit) {
        return Some("the execution turn did not finish successfully".to_string());
    }
    if let TurnLoopResult::Blocked { reason } = result {
        return Some(reason.clone().unwrap_or_else(|| "the execution turn was blocked".to_string()));
    }
    let Some(checklist) = checklist else {
        return Some("the approved-plan task checklist was not available".to_string());
    };
    let pending = pending_checklist_items(checklist);
    if !pending.is_empty() {
        return Some(format!("pending checklist items: {}", pending.join(", ")));
    }
    Some("the approved-plan checklist is not fully completed".to_string())
}

pub(super) async fn approved_plan_execution_summary(
    tool_registry: &ToolRegistry,
    result: &TurnLoopResult,
    final_response_was_fallback: bool,
    changed_files: bool,
) -> ExecutionSummary {
    let checklist_result = match tool_registry.get_tool(vtcode_core::config::constants::tools::TASK_TRACKER) {
        Some(tool) => match tool.execute(serde_json::json!({"action": "list"})).await {
            Ok(value) => Some(value),
            Err(err) => {
                tracing::warn!(error = %err, "Failed to read approved-plan task tracker for execution summary");
                None
            }
        },
        None => None,
    };
    let checklist = checklist_result.as_ref().and_then(|value| value.get("checklist"));
    let status = classify_execution_summary(result, final_response_was_fallback, checklist, changed_files);
    let blocker = (status != ExecutionSummaryStatus::Completed)
        .then(|| execution_summary_blocker(result, final_response_was_fallback, checklist, changed_files))
        .flatten();
    ExecutionSummary { status, blocker }
}

pub(super) fn take_pending_resumed_user_prompt(
    history: &mut Vec<vtcode_core::llm::provider::Message>,
) -> Option<String> {
    let user_index = history.iter().rposition(|message| message.role == MessageRole::User)?;
    if history
        .iter()
        .skip(user_index + 1)
        .any(|message| message.role != MessageRole::System)
    {
        return None;
    }

    let prompt = history[user_index].content.as_text().trim().to_string();
    if prompt.is_empty() {
        return None;
    }

    let _ = history.remove(user_index);
    Some(prompt)
}

pub(super) fn prepare_resume_bootstrap_without_archive(
    resume: &ResumeSession,
    mut metadata: session_archive::SessionArchiveMetadata,
    reserved_archive_id: Option<String>,
) -> (vtcode_core::core::threads::ThreadBootstrap, String) {
    let source_metadata = &resume.snapshot().metadata;
    let is_compatible = metadata.workspace_path == source_metadata.workspace_path
        && metadata.provider == source_metadata.provider
        && metadata.model == source_metadata.model;
    if is_compatible && let Some(lineage_id) = source_metadata.prompt_cache_lineage_id.as_ref() {
        metadata.prompt_cache_lineage_id = Some(lineage_id.clone());
    }
    metadata.continuation_metadata = source_metadata.continuation_metadata.clone();
    if resume.is_fork() {
        metadata.parent_session_id = Some(resume.identifier());
        metadata.fork_mode = Some(if resume.summarize_fork() {
            session_archive::SessionForkMode::Summarized
        } else {
            session_archive::SessionForkMode::FullCopy
        });
    }

    let mut bootstrap = resume.bootstrap().clone();
    bootstrap.metadata = Some(metadata);
    if resume.is_fork() {
        bootstrap.archive_listing = None;
    }

    let thread_id = match resume.intent() {
        vtcode_core::core::threads::ArchivedSessionIntent::ResumeInPlace => resume.identifier(),
        vtcode_core::core::threads::ArchivedSessionIntent::ForkNewArchive { .. } => {
            reserved_archive_id.unwrap_or_else(|| {
                session_archive::generate_session_archive_identifier(
                    &workspace_archive_label(std::path::Path::new(&resume.snapshot().metadata.workspace_path)),
                    resume.custom_suffix().map(str::to_owned),
                )
            })
        }
    };

    (bootstrap, thread_id)
}

pub(super) async fn checkpoint_session_archive_start(
    archive: &session_archive::SessionArchive,
    thread_handle: &vtcode_core::core::threads::ThreadRuntimeHandle,
) -> Result<()> {
    let snapshot = thread_handle.snapshot();
    let messages: Vec<SessionMessage> = snapshot.messages.iter().map(SessionMessage::from).collect();
    archive
        .persist_progress_async(SessionProgressArgs {
            total_messages: snapshot.messages.len(),
            distinct_tools: Vec::new(),
            messages: messages.clone(),
            recent_messages: messages,
            turn_number: 1,
            token_usage: None,
            max_context_tokens: None,
            loaded_skills: Some(snapshot.loaded_skills),
            turn_diagnostics: None,
        })
        .await?;
    Ok(())
}

pub(super) async fn force_reload_workspace_config_for_execution(
    workspace: &std::path::Path,
    runtime_cfg: &CoreAgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
    tool_registry: &mut ToolRegistry,
    async_mcp_manager: Option<&crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager>,
) -> Result<()> {
    crate::agent::runloop::unified::turn::workspace::refresh_vt_config(workspace, runtime_cfg, vt_cfg).await?;

    if let Some(cfg) = vt_cfg.as_ref() {
        crate::agent::runloop::unified::turn::workspace::apply_workspace_config_to_registry(tool_registry, cfg)?;

        if let Some(mcp_manager) = async_mcp_manager {
            let desired_policy =
                crate::agent::runloop::unified::async_mcp_manager::approval_policy_from_human_in_the_loop(
                    cfg.security.human_in_the_loop,
                );
            if mcp_manager.approval_policy() != desired_policy {
                mcp_manager.set_approval_policy(desired_policy);
            }
        }
    }

    Ok(())
}

pub(super) async fn prompt_startup_planning_workflow(
    handle: &InlineHandle,
    session: &mut InlineSession,
    ctrl_c_state: &Arc<crate::agent::runloop::unified::state::CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<bool> {
    let overlay = TransientRequest::List(ListOverlayRequest {
        title: "Start planning workflow?".to_string(),
        lines: vec![
            "Your configuration starts new sessions in the planning workflow.".to_string(),
            "The planning workflow keeps mutating tools blocked until execution is approved.".to_string(),
        ],
        footer_hint: Some("You can start or finish planning later with `/plan`.".to_string()),
        items: vec![
            InlineListItem {
                title: "Start planning".to_string(),
                subtitle: Some("Use the planning workflow before execution.".to_string()),
                badge: Some("Recommended".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(STARTUP_PLANNING_WORKFLOW_ENTER_ACTION.to_string())),
                search_value: None,
                ..Default::default()
            },
            InlineListItem {
                title: "Start normally".to_string(),
                subtitle: Some("Use the selected primary agent without planning first.".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(STARTUP_PLANNING_WORKFLOW_STAY_ACTION.to_string())),
                search_value: None,
                ..Default::default()
            },
        ],
        selected: Some(InlineListSelection::ConfigAction(STARTUP_PLANNING_WORKFLOW_ENTER_ACTION.to_string())),
        search: None,
        status: None,
        hotkeys: Vec::new(),
    });

    let outcome =
        show_overlay_and_wait(handle, session, overlay, ctrl_c_state, ctrl_c_notify, |submission| match submission {
            TransientSubmission::Selection(InlineListSelection::ConfigAction(action))
                if action == STARTUP_PLANNING_WORKFLOW_ENTER_ACTION =>
            {
                Some(true)
            }
            TransientSubmission::Selection(InlineListSelection::ConfigAction(action))
                if action == STARTUP_PLANNING_WORKFLOW_STAY_ACTION =>
            {
                Some(false)
            }
            TransientSubmission::Selection(_) => Some(false),
            _ => None,
        })
        .await?;

    Ok(matches!(outcome, OverlayWaitOutcome::Submitted(true)))
}

#[cfg(test)]
mod tests {
    use super::{
        ExecutionSummaryStatus, RefusedTurnRollback, checkpoint_unavailable_notice, classify_execution_summary,
    };
    use crate::agent::runloop::unified::turn::context::TurnLoopResult;
    use serde_json::json;

    #[test]
    fn checkpoint_unavailable_notice_names_safety_lock_and_options_for_busy_workspace() {
        let notice =
            checkpoint_unavailable_notice("Another turn or rewind is using this workspace: operation would block");
        assert!(notice.starts_with("Prompt kept"), "notice must confirm the prompt is kept: {notice}");
        assert!(notice.contains("safety lock"), "notice must name the safety reason: {notice}");
        assert!(notice.contains("wait then retry"), "notice must offer wait+retry: {notice}");
        assert!(notice.contains("separate worktree"), "notice must offer worktree option: {notice}");
    }

    #[test]
    fn checkpoint_unavailable_notice_routes_interrupted_rewind_to_recover() {
        let notice = checkpoint_unavailable_notice("Interrupted rewind; run again before continuing");
        assert_eq!(notice, "Prompt kept — rewind interrupted. Run /rewind again to auto-recover, then retry.");
    }

    #[test]
    fn checkpoint_unavailable_notice_falls_back_to_concise_retry() {
        let notice = checkpoint_unavailable_notice("Checkpoint skipped paths; cannot safely start turn");
        assert_eq!(notice, "Prompt kept — checkpoint unavailable. Wait a moment and retry.");
    }

    #[tokio::test]
    async fn exec_session_resume_note_is_none_without_sessions() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = vtcode_core::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;

        let note = super::build_exec_session_resume_note(&registry).await;
        assert!(note.is_none(), "no sessions must produce no hint");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn exec_session_resume_note_carries_identity_and_wait_args() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = vtcode_core::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;

        // Start a long-running command through the public exec surface so the
        // session lands in the live registry.
        let run = registry
            .execute_public_tool_ref(
                vtcode_core::config::constants::tools::EXEC_COMMAND,
                &json!({"cmd": "sleep 5", "yield_time_ms": 100}),
            )
            .await
            .expect("run should start");
        let session_id = run["session_id"].as_str().expect("session id present").to_string();

        let note = super::build_exec_session_resume_note(&registry).await.expect("hint present");
        assert!(note.contains(&session_id), "hint must carry the session id: {note}");
        assert!(note.contains("sleep 5"), "hint must carry the command: {note}");
        assert!(note.contains("\"action\": \"wait\""), "hint must pre-fill the wait action: {note}");
        assert!(note.contains("wait_timeout_seconds"), "hint must pre-fill the deadline: {note}");
        assert!(note.len() < 1_024, "hint must stay bounded: {} bytes", note.len());

        registry.close_harness_exec_session(&session_id).await.expect("close session");
        let note_after = super::build_exec_session_resume_note(&registry).await;
        assert!(note_after.is_none(), "closed session must clear the hint");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn exec_session_resume_note_bounds_a_long_command() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = vtcode_core::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;

        // A command body large enough to blow any per-line bound if inlined
        // whole; the note must still stay within its few-hundred-byte budget.
        let long_command = format!("sleep 5 # {}", "x".repeat(4_000));
        let run = registry
            .execute_public_tool_ref(
                vtcode_core::config::constants::tools::EXEC_COMMAND,
                &json!({"cmd": long_command, "yield_time_ms": 100}),
            )
            .await
            .expect("run should start");
        let session_id = run["session_id"].as_str().expect("session id present").to_string();

        let note = super::build_exec_session_resume_note(&registry).await.expect("hint present");
        assert!(note.len() < 1_024, "a long command must not inflate the hint: {} bytes", note.len());
        assert!(!note.contains(&"x".repeat(4_000)), "the command body must be truncated");
        assert!(note.contains(&session_id));

        registry.close_harness_exec_session(&session_id).await.expect("close session");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn exec_session_resume_note_shape_is_stable_contract() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = vtcode_core::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;

        let run = registry
            .execute_public_tool_ref(
                vtcode_core::config::constants::tools::EXEC_COMMAND,
                &json!({"cmd": "sleep 5", "yield_time_ms": 100}),
            )
            .await
            .expect("run should start");
        let session_id = run["session_id"].as_str().expect("session id present").to_string();

        let note = super::build_exec_session_resume_note(&registry).await.expect("hint present");
        assert!(note.starts_with("Exec session resume:"), "stable prefix: {note}");
        assert!(note.contains("are still running:"), "stable running header: {note}");
        assert!(
            note.contains(&format!("- {session_id} (`")),
            "stable session line without guessing shell prefix or elapsed secs: {note}"
        );
        assert!(note.contains("sleep 5"), "stable command body: {note}");
        assert!(note.contains("Settle them before starting new work"), "stable settle directive: {note}");
        assert!(
            note.contains(&format!(
                "{{\"session_id\": \"{session_id}\", \"action\": \"wait\", \"wait_timeout_seconds\": 600}}"
            )),
            "stable pre-filled wait shape: {note}"
        );
        assert!(note.contains("exempt from the per-turn"), "stable budget exemption: {note}");
        assert!(note.contains("deadline-expired wait"), "stable deadline language: {note}");
        assert!(note.len() < 1_024, "contract stays bounded: {} bytes", note.len());

        registry.close_harness_exec_session(&session_id).await.expect("close session");
    }

    #[tokio::test]
    async fn cancelled_note_preparation_does_not_publish_transient_history() {
        use super::{Notify, ToolRegistry};
        use crate::agent::runloop::unified::state::CtrlCState;
        use crate::agent::runloop::unified::stop_requests::await_with_stop;
        use vtcode_core::llm::provider::Message;
        let temp = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
        let state = CtrlCState::new();
        let notify = Notify::new();
        let mut history = vec![Message::user("preserved request".to_owned())];
        let preparation = async {
            let notes = super::prepare_transient_turn_notes(
                temp.path(),
                &registry,
                Some("dirty workspace evidence".to_owned()),
                Some("completed background job".to_owned()),
            )
            .await;
            assert_eq!(notes, ["dirty workspace evidence", "completed background job"]);
            // Exercise the harder boundary: preparation finished, but stop is
            // accepted before its result is admitted to the request history.
            state.request_local_cancel();
            notes
        };
        if let Some(notes) = await_with_stop(&state, &notify, preparation).await {
            history.extend(notes.into_iter().map(Message::system));
        }
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content.as_text(), "preserved request");
    }

    #[tokio::test]
    async fn prepare_transient_turn_notes_omits_resume_hint_without_sessions() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = vtcode_core::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;
        let transient = super::prepare_transient_turn_notes(temp.path(), &registry, None, None).await;
        assert!(!transient.iter().any(|note| note.starts_with("Exec session resume:")));
        assert!(transient.is_empty(), "no sessions must produce no notes");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn prepare_transient_turn_notes_injects_bounded_resume_hint() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = vtcode_core::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;
        let run = registry
            .execute_public_tool_ref(
                vtcode_core::config::constants::tools::EXEC_COMMAND,
                &json!({"cmd": "sleep 5", "yield_time_ms": 100}),
            )
            .await
            .expect("run should start");
        let session_id = run["session_id"].as_str().expect("session id present").to_string();

        let transient = super::prepare_transient_turn_notes(temp.path(), &registry, None, None).await;

        let hint = transient
            .iter()
            .find(|note| note.starts_with("Exec session resume:"))
            .expect("transient list must carry the resume hint");
        assert!(hint.contains(&session_id));
        assert!(hint.len() < 1_024);
        assert_eq!(transient.len(), 1, "exactly one resume note");

        // Hint suggests the wait; it must not settle the session on its own.
        assert!(
            !registry.in_progress_exec_sessions(4).await.is_empty(),
            "injection must not auto-wait the live session"
        );
        registry.close_harness_exec_session(&session_id).await.expect("close session");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn prepare_transient_turn_notes_omits_retained_background_sessions() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = vtcode_core::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;
        let run = registry
            .execute_public_tool_ref(
                vtcode_core::config::constants::tools::EXEC_COMMAND,
                &json!({"cmd": "sleep 5", "background": true, "yield_time_ms": 100}),
            )
            .await
            .expect("background run should start");
        let session_id = run["session_id"].as_str().expect("session id present").to_string();

        let transient = super::prepare_transient_turn_notes(temp.path(), &registry, None, None).await;

        assert!(!transient.iter().any(|note| note.starts_with("Exec session resume:")));
        assert!(transient.is_empty(), "retained background work must not force a resume wait");
        registry.close_harness_exec_session(&session_id).await.expect("close session");
    }

    #[test]
    fn pending_approved_plan_checklist_cannot_be_completed() {
        let checklist = json!({
            "total": 3,
            "completed": 1,
            "pending": 1,
            "in_progress": 1,
            "blocked": 0
        });
        let result = TurnLoopResult::Completed { plan_approved_execution_pending: false };

        assert_eq!(classify_execution_summary(&result, false, Some(&checklist), true), ExecutionSummaryStatus::Blocked);
    }

    #[test]
    fn completed_approved_plan_requires_all_checklist_items() {
        let checklist = json!({
            "total": 2,
            "completed": 2,
            "pending": 0,
            "in_progress": 0,
            "blocked": 0
        });
        let result = TurnLoopResult::Completed { plan_approved_execution_pending: false };

        assert_eq!(
            classify_execution_summary(&result, false, Some(&checklist), true),
            ExecutionSummaryStatus::Completed
        );
    }

    #[test]
    fn recovery_fallback_is_blocked_even_with_a_complete_tracker() {
        let checklist = json!({
            "total": 1,
            "completed": 1,
            "pending": 0,
            "in_progress": 0,
            "blocked": 0
        });
        let result = TurnLoopResult::Completed { plan_approved_execution_pending: false };

        assert_eq!(classify_execution_summary(&result, true, Some(&checklist), true), ExecutionSummaryStatus::Blocked);
    }

    #[test]
    fn completed_plan_without_file_changes_is_blocked() {
        let checklist = json!({
            "total": 1,
            "completed": 1,
            "pending": 0,
            "in_progress": 0,
            "blocked": 0,
            "items": [{"description": "Implement feature", "status": "completed"}]
        });
        let result = TurnLoopResult::Completed { plan_approved_execution_pending: false };

        assert_eq!(
            classify_execution_summary(&result, false, Some(&checklist), false),
            ExecutionSummaryStatus::Blocked
        );
    }

    #[test]
    fn completed_review_only_plan_without_file_changes_is_completed() {
        let checklist = json!({
            "total": 3,
            "completed": 3,
            "pending": 0,
            "in_progress": 0,
            "blocked": 0,
            "items": [
                {"description": "Review background completion monitoring", "status": "completed"},
                {"description": "Review bounded completion-event handling", "status": "completed"},
                {"description": "Inspect additional runtime diffs", "status": "completed"}
            ]
        });
        let result = TurnLoopResult::Completed { plan_approved_execution_pending: false };

        assert_eq!(
            classify_execution_summary(&result, false, Some(&checklist), false),
            ExecutionSummaryStatus::Completed
        );
        assert!(super::checklist_is_review_only(&checklist));
    }

    #[test]
    fn review_only_checklist_rejects_mutation_descriptions() {
        let mixed = json!({
            "items": [
                {"description": "Review background completion", "status": "completed"},
                {"description": "Implement fix", "status": "completed"}
            ]
        });
        assert!(!super::checklist_is_review_only(&mixed));

        let empty_items = json!({"items": []});
        assert!(!super::checklist_is_review_only(&empty_items));

        let missing_items = json!({"total": 1});
        assert!(!super::checklist_is_review_only(&missing_items));

        // Asymmetric boundary: colon/suffix forms still count as mutation,
        // and `prefix` fail-closes toward implementation (safe direction:
        // preserves the file-change requirement rather than waiving it).
        let fix_colon = json!({"items": [{"description": "Review then Fix: race", "status": "completed"}]});
        assert!(!super::checklist_is_review_only(&fix_colon));

        let prefix_mention = json!({"items": [{"description": "Review prefix handling", "status": "completed"}]});
        assert!(!super::checklist_is_review_only(&prefix_mention));
    }

    fn pre_turn_history() -> Vec<vtcode_core::llm::provider::Message> {
        use vtcode_core::llm::provider::Message;
        vec![
            Message::system("system prompt".to_string()),
            Message::user("first request".to_string()),
            Message::assistant("first answer".to_string()),
        ]
    }

    fn run_refused_turn(history: &mut Vec<vtcode_core::llm::provider::Message>) {
        use vtcode_core::llm::provider::Message;
        // Transient note, tool round-trip, and a partial assistant response
        // accumulated before the provider refused.
        history.push(Message::system("Freshness note: transient".to_string()));
        history.push(Message::assistant("partial answer".to_string()));
        history.push(Message::system("recovery directive".to_string()));
    }

    #[test]
    fn refused_turn_rollback_restores_history_before_the_turn() {
        use vtcode_core::llm::provider::Message;
        let before = pre_turn_history();
        let mut history = before.clone();
        let prompt_index = history.len();
        history.push(Message::user("refused request".to_string()));

        let rollback = RefusedTurnRollback::capture(&history, Some(prompt_index), "refused request");
        run_refused_turn(&mut history);

        assert!(rollback.apply(&mut history));
        assert_eq!(history, before);
    }

    #[test]
    fn refused_turn_rollback_finds_queued_follow_up_prompt_without_index() {
        use vtcode_core::llm::provider::Message;
        let before = pre_turn_history();
        let mut history = before.clone();
        history.push(Message::user("queued follow-up".to_string()));

        let rollback = RefusedTurnRollback::capture(&history, None, "queued follow-up");
        run_refused_turn(&mut history);

        assert!(rollback.apply(&mut history));
        assert_eq!(history, before);
    }

    #[test]
    fn refused_turn_rollback_relocates_shifted_prompt() {
        use vtcode_core::llm::provider::Message;
        let mut history = pre_turn_history();
        let prompt_index = history.len();
        history.push(Message::user("refused request".to_string()));
        let rollback = RefusedTurnRollback::capture(&history, Some(prompt_index), "refused request");

        // An in-turn rewrite removed an earlier message, shifting the prompt.
        history.remove(0);
        let rewritten_prefix = history[..prompt_index - 1].to_vec();
        run_refused_turn(&mut history);

        assert!(rollback.apply(&mut history));
        assert_eq!(history, rewritten_prefix);
    }

    #[test]
    fn refused_turn_rollback_leaves_history_when_boundary_is_lost() {
        use vtcode_core::llm::provider::Message;
        let mut history = pre_turn_history();
        let prompt_index = history.len();
        history.push(Message::user("refused request".to_string()));
        let rollback = RefusedTurnRollback::capture(&history, Some(prompt_index), "refused request");

        history.pop();
        history.push(Message::user("summarized request".to_string()));
        let unchanged = history.clone();

        assert!(!rollback.apply(&mut history));
        assert_eq!(history, unchanged);
    }

    #[test]
    fn withdrawn_turn_changes_note_lists_bounded_relative_paths() {
        let workspace = std::path::Path::new("/workspace");
        assert!(super::build_withdrawn_turn_changes_note(workspace, &Default::default()).is_none());

        let paths: std::collections::BTreeSet<std::path::PathBuf> = (0..super::MAX_WITHDRAWN_TURN_PATHS + 3)
            .map(|i| workspace.join(format!("src/f{i:02}.rs")))
            .collect();
        let note = super::build_withdrawn_turn_changes_note(workspace, &paths).expect("note");
        assert!(note.contains("- src/f00.rs\n"), "{note}");
        assert!(!note.contains("/workspace/"), "paths must be workspace-relative: {note}");
        assert_eq!(note.matches("\n- src/").count(), super::MAX_WITHDRAWN_TURN_PATHS);
        assert!(note.contains("- and 3 more\n"), "{note}");
        assert!(note.contains("read them again"), "{note}");
    }

    #[test]
    fn refused_turn_rollback_without_prompt_restores_pre_turn_length() {
        let before = pre_turn_history();
        let mut history = before.clone();
        let rollback = RefusedTurnRollback::capture(&history, None, "prompt that was never appended");
        run_refused_turn(&mut history);

        assert!(rollback.apply(&mut history));
        assert_eq!(history, before);
    }
}
