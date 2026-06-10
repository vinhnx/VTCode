use super::archive::workspace_archive_label;
use super::*;
use crate::agent::runloop::git::{
    DirtyWorktreeStatus, git_dirty_worktree_entries, workspace_relative_display,
};
use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use crate::agent::runloop::welcome::SessionBootstrap;
use std::sync::Arc;
use vtcode_core::llm::provider::MessageRole;
use vtcode_core::utils::session_archive;
use vtcode_ui::tui::app::{
    InlineHandle, InlineListItem, InlineListSelection, InlineSession, ListOverlayRequest,
    TransientRequest, TransientSubmission,
};

const STARTUP_PLANNING_WORKFLOW_ENTER_ACTION: &str = "planning_active:start_enter";
const STARTUP_PLANNING_WORKFLOW_STAY_ACTION: &str = "planning_active:start_stay";

#[derive(Clone)]
pub(super) struct TurnHistoryCheckpoint {
    pub(super) baseline_len: usize,
    #[cfg(debug_assertions)]
    prefix_fingerprint: u64,
}

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
        serde_json::to_string(history)
            .unwrap_or_default()
            .hash(&mut hasher);
        hasher.finish()
    }
}

#[derive(Clone)]
pub(super) struct PendingTimeoutRecovery {
    pub(super) reason: String,
    pub(super) mode: RecoveryMode,
}

pub(super) fn remove_transient_system_notes(
    history: &mut Vec<vtcode_core::llm::provider::Message>,
    notes: &[String],
) {
    for note in notes.iter().rev() {
        if let Some(index) = history.iter().rposition(|message| {
            message.role == MessageRole::System && message.content.as_text() == note.as_str()
        }) {
            let _ = history.remove(index);
        }
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

pub(super) fn build_unrelated_dirty_worktree_note(
    workspace: &std::path::Path,
    agent_touched_paths: &std::collections::BTreeSet<std::path::PathBuf>,
) -> Result<Option<String>> {
    let Some(entries) = git_dirty_worktree_entries(workspace)? else {
        return Ok(None);
    };

    let display_paths = entries
        .into_iter()
        .filter(|entry| {
            entry.status == DirtyWorktreeStatus::Modified
                && !agent_touched_paths.contains(&entry.path)
        })
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

pub(super) fn append_transient_turn_notes(
    history: &mut Vec<vtcode_core::llm::provider::Message>,
    workspace: &std::path::Path,
    tool_registry: &vtcode_core::tools::registry::ToolRegistry,
    agent_touched_paths: &std::collections::BTreeSet<std::path::PathBuf>,
) -> Vec<String> {
    let mut transient_system_notes = Vec::with_capacity(2);

    if let Some(note) = {
        let stale_paths = tool_registry
            .edited_file_monitor_ref()
            .stale_tracked_paths();
        build_tracked_file_freshness_note(workspace, &stale_paths)
    } {
        transient_system_notes.push(note.clone());
        history.push(vtcode_core::llm::provider::Message::system(note));
    }

    match build_unrelated_dirty_worktree_note(workspace, agent_touched_paths) {
        Ok(Some(note)) => {
            transient_system_notes.push(note.clone());
            history.push(vtcode_core::llm::provider::Message::system(note));
        }
        Ok(None) => {}
        Err(err) => {
            tracing::warn!(
                error = %err,
                "Failed to inspect unrelated dirty worktree entries before turn"
            );
        }
    }

    transient_system_notes
}

pub(super) fn latest_assistant_result_text(
    messages: &[vtcode_core::llm::provider::Message],
) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == MessageRole::Assistant)
        .map(|message| message.content.as_text().trim().to_string())
        .filter(|text| !text.is_empty())
}

pub(super) fn take_pending_resumed_user_prompt(
    history: &mut Vec<vtcode_core::llm::provider::Message>,
) -> Option<String> {
    let user_index = history
        .iter()
        .rposition(|message| message.role == MessageRole::User)?;
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

pub(super) fn live_reload_preserves_session_config(
    initial_vt_cfg: Option<&VTCodeConfig>,
    runtime_cfg: &CoreAgentConfig,
) -> bool {
    let Some(initial_vt_cfg) = initial_vt_cfg else {
        return true;
    };

    let mut reloaded_vt_cfg =
        vtcode_core::config::loader::ConfigManager::load_from_workspace(&runtime_cfg.workspace)
            .ok()
            .map(|manager| manager.config().clone());
    crate::agent::agents::apply_runtime_overrides(reloaded_vt_cfg.as_mut(), runtime_cfg);

    let Some(reloaded_vt_cfg) = reloaded_vt_cfg else {
        return false;
    };

    let Ok(initial_value) = serde_json::to_value(initial_vt_cfg) else {
        return false;
    };
    let Ok(reloaded_value) = serde_json::to_value(reloaded_vt_cfg) else {
        return false;
    };

    initial_value == reloaded_value
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
                    &workspace_archive_label(std::path::Path::new(
                        &resume.snapshot().metadata.workspace_path,
                    )),
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
    let recent_messages = snapshot.messages.iter().map(SessionMessage::from).collect();
    archive
        .persist_progress_async(SessionProgressArgs {
            total_messages: snapshot.messages.len(),
            distinct_tools: Vec::new(),
            recent_messages,
            turn_number: 1,
            token_usage: None,
            max_context_tokens: None,
            loaded_skills: Some(snapshot.loaded_skills),
        })
        .await?;
    Ok(())
}

pub(super) async fn force_reload_workspace_config_for_execution(
    workspace: &std::path::Path,
    runtime_cfg: &CoreAgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
    tool_registry: &mut vtcode_core::tools::registry::ToolRegistry,
    async_mcp_manager: Option<&crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager>,
) -> Result<()> {
    crate::agent::runloop::unified::turn::workspace::refresh_vt_config(
        workspace,
        runtime_cfg,
        vt_cfg,
    )
    .await?;

    if let Some(cfg) = vt_cfg.as_ref() {
        crate::agent::runloop::unified::turn::workspace::apply_workspace_config_to_registry(
            tool_registry,
            cfg,
        )?;

        if let Some(mcp_manager) = async_mcp_manager {
            let desired_policy = crate::agent::runloop::unified::async_mcp_manager::approval_policy_from_human_in_the_loop(
                cfg.security.human_in_the_loop,
            );
            if mcp_manager.approval_policy() != desired_policy {
                mcp_manager.set_approval_policy(desired_policy);
            }
        }
    }

    Ok(())
}

pub(super) struct ExitHeaderDisplay {
    pub(super) provider_label: String,
    pub(super) reasoning_label: String,
    pub(super) context_window_size: usize,
    pub(super) full_auto: bool,
    pub(super) primary_agent: Option<String>,
}

pub(super) fn build_exit_header_context_fast(
    config: &CoreAgentConfig,
    session_bootstrap: &SessionBootstrap,
    display: ExitHeaderDisplay,
) -> vtcode_ui::tui::app::InlineHeaderContext {
    use vtcode_core::config::constants::ui;

    let trust_label = match session_bootstrap.acp_workspace_trust {
        Some(vtcode_core::config::AgentClientProtocolZedWorkspaceTrustMode::FullAuto) => {
            "full_auto"
        }
        Some(vtcode_core::config::AgentClientProtocolZedWorkspaceTrustMode::ToolsPolicy) => {
            "tools_policy"
        }
        None if display.full_auto => "full auto",
        None => "tools policy",
    };

    vtcode_ui::tui::app::InlineHeaderContext {
        app_name: vtcode_core::config::constants::app::DISPLAY_NAME.to_string(),
        provider: format!("{}{}", ui::HEADER_PROVIDER_PREFIX, display.provider_label),
        model: format!("{}{}", ui::HEADER_MODEL_PREFIX, config.model),
        context_window_size: Some(display.context_window_size),
        version: env!("CARGO_PKG_VERSION").to_string(),
        search_tools: Some(crate::agent::runloop::ui::build_search_tools_badge(
            &config.workspace,
        )),
        persistent_memory: None,
        pr_review: None,
        editor_context: None,
        git: String::new(),
        reasoning: format!("{}{}", ui::HEADER_REASONING_PREFIX, display.reasoning_label),
        reasoning_stage: None,
        workspace_trust: format!("{}{}", ui::HEADER_TRUST_PREFIX, trust_label),
        tools: String::new(),
        mcp: format!(
            "{}{}",
            ui::HEADER_MCP_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        ),
        primary_agent: display.primary_agent,
        highlights: Vec::new(),
        subagent_badges: Vec::new(),
    }
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
            "The planning workflow keeps mutating tools blocked until execution is approved."
                .to_string(),
        ],
        footer_hint: Some("You can start or finish planning later with `/plan`.".to_string()),
        items: vec![
            InlineListItem {
                title: "Start planning".to_string(),
                subtitle: Some("Use the planning workflow before execution.".to_string()),
                badge: Some("Recommended".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    STARTUP_PLANNING_WORKFLOW_ENTER_ACTION.to_string(),
                )),
                search_value: None,
            },
            InlineListItem {
                title: "Start normally".to_string(),
                subtitle: Some(
                    "Use the selected primary agent without planning first.".to_string(),
                ),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    STARTUP_PLANNING_WORKFLOW_STAY_ACTION.to_string(),
                )),
                search_value: None,
            },
        ],
        selected: Some(InlineListSelection::ConfigAction(
            STARTUP_PLANNING_WORKFLOW_ENTER_ACTION.to_string(),
        )),
        search: None,
        hotkeys: Vec::new(),
    });

    let outcome = show_overlay_and_wait(
        handle,
        session,
        overlay,
        ctrl_c_state,
        ctrl_c_notify,
        |submission| match submission {
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
        },
    )
    .await?;

    Ok(matches!(outcome, OverlayWaitOutcome::Submitted(true)))
}
