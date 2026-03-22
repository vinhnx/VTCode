use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Notify;
use vtcode_core::tools::terminal_app::{TerminalAppLauncher, TerminalCommandStrategy};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::app::{
    InlineHandle, InlineHeaderContext, InlineHeaderHighlight, InlineListItem, InlineListSelection,
    InlineMessageKind, InlineSession, ListOverlayRequest, TransientRequest, TransientSubmission,
};

use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::main_helpers::{RelaunchPreference, queue_runtime_relaunch};

use super::{InstallOutcome, StartupUpdateNotice, UpdateExecutionStrategy, Updater};

const UPDATE_AND_RESTART_ACTION: &str = "update:install_and_restart";
const STAY_CURRENT_ACTION: &str = "update:stay_current";
const UPDATE_HIGHLIGHT_TITLE: &str = "Update";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdatePromptChoice {
    UpdateAndRestart,
    StayCurrent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InlineUpdateOutcome {
    Continue,
    RestartRequested,
}

fn line_count(text: &str) -> usize {
    text.lines().count().max(1)
}

fn update_highlight(notice: &StartupUpdateNotice) -> InlineHeaderHighlight {
    InlineHeaderHighlight {
        title: UPDATE_HIGHLIGHT_TITLE.to_string(),
        lines: vec![
            format!("v{} -> v{}", notice.current_version, notice.latest_version),
            format!("Run {}", notice.guidance.command()),
            Updater::release_url(&notice.latest_version),
        ],
    }
}

pub(crate) fn append_notice_highlight(
    highlights: &mut Vec<InlineHeaderHighlight>,
    notice: &StartupUpdateNotice,
) {
    let highlight = update_highlight(notice);
    if highlights
        .iter()
        .any(|existing| existing.title == highlight.title && existing.lines == highlight.lines)
    {
        return;
    }
    highlights.push(highlight);
}

fn format_update_banner(notice: &StartupUpdateNotice, _use_unicode: bool) -> String {
    let lines = [
        format!(
            "Update available! {} -> {}",
            notice.current_version, notice.latest_version
        ),
        format!("Run {} to update.", notice.guidance.command()),
        String::new(),
        "See full release notes:".to_string(),
        Updater::release_url(&notice.latest_version),
    ];

    lines.join("\n")
}

pub(crate) fn display_update_notice(
    handle: &InlineHandle,
    header_context: &mut InlineHeaderContext,
    use_unicode: bool,
    notice: &StartupUpdateNotice,
) {
    append_notice_highlight(&mut header_context.highlights, notice);
    handle.set_header_context(header_context.clone());

    let banner = format_update_banner(notice, use_unicode);
    handle.append_pasted_message(InlineMessageKind::Info, banner.clone(), line_count(&banner));
    handle.force_redraw();
}

fn build_update_prompt_request(notice: &StartupUpdateNotice) -> TransientRequest {
    TransientRequest::List(ListOverlayRequest {
        title: "Update available".to_string(),
        lines: vec![
            format!(
                "VT Code {} -> {}",
                notice.current_version, notice.latest_version
            ),
            format!("Install source: {}", notice.guidance.action.source_label),
            format!("Command: {}", notice.guidance.command()),
            format!(
                "Release notes: {}",
                Updater::release_url(&notice.latest_version)
            ),
        ],
        footer_hint: Some("Choose update and restart, or stay on the current version.".to_string()),
        items: vec![
            InlineListItem {
                title: "Update and restart".to_string(),
                subtitle: Some(
                    "Run the documented install command and relaunch VT Code.".to_string(),
                ),
                badge: Some("Recommended".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    UPDATE_AND_RESTART_ACTION.to_string(),
                )),
                search_value: None,
            },
            InlineListItem {
                title: "Stay on current version".to_string(),
                subtitle: Some("Dismiss this prompt for the rest of this launch.".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    STAY_CURRENT_ACTION.to_string(),
                )),
                search_value: None,
            },
        ],
        selected: Some(InlineListSelection::ConfigAction(
            UPDATE_AND_RESTART_ACTION.to_string(),
        )),
        search: None,
        hotkeys: Vec::new(),
    })
}

fn terminal_strategy(strategy: UpdateExecutionStrategy) -> TerminalCommandStrategy {
    match strategy {
        UpdateExecutionStrategy::Shell => TerminalCommandStrategy::Shell,
        UpdateExecutionStrategy::PowerShell => TerminalCommandStrategy::PowerShell,
    }
}

fn relaunch_preference(notice: &StartupUpdateNotice) -> RelaunchPreference {
    if notice.guidance.action.prefer_path_relaunch {
        RelaunchPreference::PreferPathCommand
    } else {
        RelaunchPreference::PreferOriginalExecutable
    }
}

fn map_update_prompt_submission(submission: TransientSubmission) -> Option<UpdatePromptChoice> {
    match submission {
        TransientSubmission::Selection(InlineListSelection::ConfigAction(action))
            if action == UPDATE_AND_RESTART_ACTION =>
        {
            Some(UpdatePromptChoice::UpdateAndRestart)
        }
        TransientSubmission::Selection(InlineListSelection::ConfigAction(action))
            if action == STAY_CURRENT_ACTION =>
        {
            Some(UpdatePromptChoice::StayCurrent)
        }
        TransientSubmission::Selection(_) => Some(UpdatePromptChoice::StayCurrent),
        _ => None,
    }
}

pub(crate) async fn run_inline_update_prompt(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    session: &mut InlineSession,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    workspace_root: &Path,
    notice: &StartupUpdateNotice,
) -> Result<InlineUpdateOutcome> {
    let outcome = show_overlay_and_wait(
        handle,
        session,
        build_update_prompt_request(notice),
        ctrl_c_state,
        ctrl_c_notify,
        map_update_prompt_submission,
    )
    .await?;

    match outcome {
        OverlayWaitOutcome::Submitted(UpdatePromptChoice::UpdateAndRestart) => {
            execute_inline_update(renderer, handle, workspace_root, notice).await
        }
        OverlayWaitOutcome::Submitted(UpdatePromptChoice::StayCurrent) => {
            renderer.line(
                MessageStyle::Info,
                "Staying on the current version for this session.",
            )?;
            Ok(InlineUpdateOutcome::Continue)
        }
        OverlayWaitOutcome::Cancelled
        | OverlayWaitOutcome::Interrupted
        | OverlayWaitOutcome::Exit => Ok(InlineUpdateOutcome::Continue),
    }
}

pub(crate) async fn execute_inline_update(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    workspace_root: &Path,
    notice: &StartupUpdateNotice,
) -> Result<InlineUpdateOutcome> {
    if notice.guidance.source.is_managed() {
        return execute_managed_update(renderer, handle, workspace_root, notice);
    }

    renderer.line(
        MessageStyle::Info,
        &format!(
            "Updating VT Code {} -> {} ...",
            notice.current_version, notice.latest_version
        ),
    )?;

    let updater = Updater::new(&notice.current_version.to_string())?;
    match updater.install_update(false).await {
        Ok(InstallOutcome::Updated(version)) => {
            queue_runtime_relaunch(relaunch_preference(notice));
            renderer.line(
                MessageStyle::Info,
                &format!("Update installed (v{}). Restarting VT Code...", version),
            )?;
            Ok(InlineUpdateOutcome::RestartRequested)
        }
        Ok(InstallOutcome::UpToDate(version)) => {
            renderer.line(
                MessageStyle::Info,
                &format!("Already on the latest version (v{}).", version),
            )?;
            Ok(InlineUpdateOutcome::Continue)
        }
        Err(err) => {
            renderer.line(MessageStyle::Error, &format!("Failed to update: {}", err))?;
            Ok(InlineUpdateOutcome::Continue)
        }
    }
}

fn execute_managed_update(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    workspace_root: &Path,
    notice: &StartupUpdateNotice,
) -> Result<InlineUpdateOutcome> {
    renderer.line(
        MessageStyle::Info,
        &format!("Running update command: {}", notice.guidance.command()),
    )?;

    let launcher = TerminalAppLauncher::new(workspace_root.to_path_buf());
    handle.suspend_event_loop();
    let result = launcher.run_command_with_strategy(
        notice.guidance.command(),
        terminal_strategy(notice.guidance.action.execution),
    );
    handle.resume_event_loop();
    handle.force_redraw();

    match result {
        Ok(command_result) if command_result.success => {
            queue_runtime_relaunch(relaunch_preference(notice));
            renderer.line(
                MessageStyle::Info,
                "Update installed. Restarting VT Code...",
            )?;
            Ok(InlineUpdateOutcome::RestartRequested)
        }
        Ok(command_result) => {
            renderer.line(
                MessageStyle::Error,
                &format!(
                    "Update command exited with status {}.",
                    command_result.exit_code
                ),
            )?;
            Ok(InlineUpdateOutcome::Continue)
        }
        Err(err) => {
            renderer.line(
                MessageStyle::Error,
                &format!("Failed to run update command: {}", err),
            )?;
            Ok(InlineUpdateOutcome::Continue)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;
    use tokio::sync::mpsc;
    use vtcode_tui::app::{InlineCommand, InlineHandle};

    fn sample_notice() -> StartupUpdateNotice {
        let updater = Updater::new("0.111.0").expect("updater");
        StartupUpdateNotice {
            current_version: Version::parse("0.111.0").expect("current"),
            latest_version: Version::parse("0.113.0").expect("latest"),
            guidance: updater.update_guidance(),
        }
    }

    #[test]
    fn banner_uses_release_specific_url() {
        let banner = format_update_banner(&sample_notice(), true);
        assert!(banner.contains("https://github.com/vinhnx/vtcode/releases/tag/v0.113.0"));
        assert!(banner.contains("0.111.0 -> 0.113.0"));
    }

    #[test]
    fn display_notice_updates_header_and_transcript() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut header_context = InlineHeaderContext::default();
        display_update_notice(&handle, &mut header_context, true, &sample_notice());

        let first = rx.blocking_recv().expect("header command");
        let second = rx.blocking_recv().expect("transcript command");
        assert!(matches!(first, InlineCommand::SetHeaderContext { .. }));
        assert!(matches!(
            second,
            InlineCommand::AppendPastedMessage {
                kind: InlineMessageKind::Info,
                ..
            }
        ));
    }

    #[test]
    fn apply_notice_only_adds_one_highlight_per_version() {
        let notice = sample_notice();
        let mut header_context = InlineHeaderContext::default();
        append_notice_highlight(&mut header_context.highlights, &notice);
        append_notice_highlight(&mut header_context.highlights, &notice);
        assert_eq!(header_context.highlights.len(), 1);
    }
}
