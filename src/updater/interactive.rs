use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Notify;
use vtcode_core::tools::terminal_app::{TerminalAppLauncher, TerminalCommandStrategy};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::{
    InlineHandle, InlineHeaderContext, InlineHeaderHighlight, InlineListItem, InlineListSelection, InlineMessageKind,
    InlineSegment, InlineSession, InlineTextStyle, ListOverlayRequest, TransientRequest, TransientSubmission,
};

use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::main_helpers::{RelaunchPreference, queue_runtime_relaunch};

use super::{InstallOutcome, StartupUpdateNotice, UpdateExecutionStrategy, UpdateProgress, Updater};

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
            "Run /update install".to_string(),
            Updater::release_url(&notice.latest_version),
        ],
    }
}

pub(crate) fn append_notice_highlight(highlights: &mut Vec<InlineHeaderHighlight>, notice: &StartupUpdateNotice) {
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
        format!("Update available! {} -> {}", notice.current_version, notice.latest_version),
        "Run /update install, or `vtcode update` from the CLI, to update.".to_string(),
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

pub(crate) fn display_release_notes(handle: &InlineHandle, version: &semver::Version, highlights: &[String]) {
    let text = format_release_notes_text(version, highlights);
    handle.append_pasted_message(InlineMessageKind::Info, text.clone(), line_count(&text));
    handle.force_redraw();
}

fn format_release_notes_text(version: &semver::Version, highlights: &[String]) -> String {
    if highlights.is_empty() {
        return format!("VT Code v{}\n\nSee full release notes:\n{}", version, Updater::release_url(version));
    }

    let mut lines = vec![format!("VT Code v{}", version)];
    lines.push(String::new());
    for item in highlights {
        lines.push(format!(" {item}"));
    }
    lines.join("\n")
}

fn build_update_prompt_request(notice: &StartupUpdateNotice) -> TransientRequest {
    TransientRequest::List(ListOverlayRequest {
        title: "Update available".to_string(),
        lines: vec![
            format!("VT Code {} -> {}", notice.current_version, notice.latest_version),
            format!("Release notes: {}", Updater::release_url(&notice.latest_version)),
        ],
        footer_hint: Some(
            "Choose update and restart, stay on the current version, or run `vtcode update` from the CLI.".to_string(),
        ),
        items: vec![
            InlineListItem {
                title: "Update and restart".to_string(),
                subtitle: Some("Run the documented install command and relaunch VT Code.".to_string()),
                badge: Some("Recommended".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(UPDATE_AND_RESTART_ACTION.to_string())),
                search_value: None,
                ..Default::default()
            },
            InlineListItem {
                title: "Stay on current version".to_string(),
                subtitle: Some("Dismiss for now. Run `vtcode update` when ready.".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(STAY_CURRENT_ACTION.to_string())),
                search_value: None,
                ..Default::default()
            },
        ],
        selected: Some(InlineListSelection::ConfigAction(UPDATE_AND_RESTART_ACTION.to_string())),
        search: None,
        status: None,
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
        TransientSubmission::Selection(InlineListSelection::ConfigAction(action)) if action == STAY_CURRENT_ACTION => {
            Some(UpdatePromptChoice::StayCurrent)
        }
        TransientSubmission::Selection(_) => Some(UpdatePromptChoice::StayCurrent),
        _ => None,
    }
}

fn should_dismiss_update_prompt(outcome: &OverlayWaitOutcome<UpdatePromptChoice>) -> bool {
    matches!(
        outcome,
        OverlayWaitOutcome::Submitted(UpdatePromptChoice::StayCurrent)
            | OverlayWaitOutcome::Cancelled
            | OverlayWaitOutcome::Interrupted
            | OverlayWaitOutcome::Exit
    )
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

    if should_dismiss_update_prompt(&outcome) {
        let _ = super::cache::record_dismissed_version(&notice.latest_version);
    }

    match outcome {
        OverlayWaitOutcome::Submitted(UpdatePromptChoice::UpdateAndRestart) => {
            execute_inline_update(renderer, handle, workspace_root, notice).await
        }
        OverlayWaitOutcome::Submitted(UpdatePromptChoice::StayCurrent) => {
            renderer.line(MessageStyle::Info, "Staying on the current version for this session.")?;
            Ok(InlineUpdateOutcome::Continue)
        }
        OverlayWaitOutcome::Cancelled | OverlayWaitOutcome::Interrupted | OverlayWaitOutcome::Exit => {
            Ok(InlineUpdateOutcome::Continue)
        }
        OverlayWaitOutcome::Deferred => Ok(InlineUpdateOutcome::Continue),
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    match bytes {
        b if b >= MB => format!("{:.1} MB", b as f64 / MB as f64),
        b if b >= KB => format!("{:.1} KB", b as f64 / KB as f64),
        b => format!("{} B", b),
    }
}

fn progress_bar(percent: u8, width: usize) -> String {
    let filled = (percent as usize * width / 100).min(width);
    let empty = width - filled;
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

fn format_download_progress(downloaded: u64, total: Option<u64>) -> String {
    match total {
        Some(t) if t > 0 => {
            let percent = ((downloaded * 100) / t).min(100) as u8;
            let bar = progress_bar(percent, 20);
            format!("Downloading update  {bar}  {percent}%  {}/{}", format_bytes(downloaded), format_bytes(t))
        }
        _ => format!("Downloading update  {}", format_bytes(downloaded)),
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
        &format!("Updating VT Code {} -> {} ...", notice.current_version, notice.latest_version),
    )?;

    let updater = Updater::new(&notice.current_version.to_string())?;
    let progress_style = Arc::new(InlineTextStyle::default());
    let mut download_progress_emitted = false;
    // Drive real-time TUI feedback through the update pipeline. Download byte
    // progress updates the last transcript line in place (via `replace_last`)
    // so the scrollback is not flooded; phase transitions append new lines.
    // `show_progress` stays false so raw `\r` byte counters do not leak into
    // the alternate screen — this callback is the sole progress channel.
    let on_progress = move |event: UpdateProgress| match event {
        UpdateProgress::Downloading { downloaded, total } => {
            let text = format_download_progress(downloaded, total);
            let segment = InlineSegment { text, style: progress_style.clone() };
            if download_progress_emitted {
                handle.replace_last(1, InlineMessageKind::Info, vec![vec![segment]]);
            } else {
                handle.append_line(InlineMessageKind::Info, vec![segment]);
                download_progress_emitted = true;
            }
        }
        UpdateProgress::VerifyingChecksum => {
            handle.append_pasted_message(InlineMessageKind::Info, "Verifying checksum...".to_string(), 1);
            download_progress_emitted = false;
        }
        UpdateProgress::Extracting => {
            handle.append_pasted_message(InlineMessageKind::Info, "Extracting archive...".to_string(), 1);
            download_progress_emitted = false;
        }
        UpdateProgress::ReplacingBinary => {
            handle.append_pasted_message(InlineMessageKind::Info, "Installing new binary...".to_string(), 1);
            download_progress_emitted = false;
        }
    };
    match updater.install_update_reported(false, false, on_progress).await {
        Ok(InstallOutcome::Updated(version)) => {
            let _ = super::cache::clear_dismissed_version();
            queue_runtime_relaunch(relaunch_preference(notice));
            renderer.line(MessageStyle::Info, &format!("Update installed (v{version}). Restarting VT Code..."))?;
            Ok(InlineUpdateOutcome::RestartRequested)
        }
        Ok(InstallOutcome::UpToDate(version)) => {
            renderer.line(MessageStyle::Info, &format!("Already on the latest version (v{version})."))?;
            Ok(InlineUpdateOutcome::Continue)
        }
        Err(err) => {
            renderer.line(MessageStyle::Error, &format!("Failed to update: {err}"))?;
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
    renderer.line(MessageStyle::Info, &format!("Running update command: {}", notice.guidance.command()))?;

    let launcher = TerminalAppLauncher::new(workspace_root.to_path_buf());
    handle.suspend_event_loop();
    let result = launcher
        .run_command_with_strategy(notice.guidance.command(), terminal_strategy(notice.guidance.action.execution));
    handle.resume_event_loop();
    handle.force_redraw();

    match result {
        Ok(command_result) if command_result.success => {
            queue_runtime_relaunch(relaunch_preference(notice));
            renderer.line(MessageStyle::Info, "Update installed. Restarting VT Code...")?;
            Ok(InlineUpdateOutcome::RestartRequested)
        }
        Ok(command_result) => {
            renderer.line(
                MessageStyle::Error,
                &format!("Update command exited with status {}.", command_result.exit_code),
            )?;
            Ok(InlineUpdateOutcome::Continue)
        }
        Err(err) => {
            renderer.line(MessageStyle::Error, &format!("Failed to run update command: {err}"))?;
            Ok(InlineUpdateOutcome::Continue)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::overlay_prompt::OverlayWaitOutcome;
    use semver::Version;
    use tokio::sync::mpsc;
    use vtcode_ui::tui::app::{InlineCommand, InlineHandle};

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
        assert!(banner.contains("https://github.com/vinhnx/vtcode/releases/tag/0.113.0"));
        assert!(banner.contains("0.111.0 -> 0.113.0"));
        assert!(banner.contains("vtcode update"));
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
        assert!(matches!(second, InlineCommand::AppendPastedMessage { kind: InlineMessageKind::Info, .. }));
    }

    #[test]
    fn apply_notice_only_adds_one_highlight_per_version() {
        let notice = sample_notice();
        let mut header_context = InlineHeaderContext::default();
        append_notice_highlight(&mut header_context.highlights, &notice);
        append_notice_highlight(&mut header_context.highlights, &notice);
        assert_eq!(header_context.highlights.len(), 1);
    }

    #[test]
    fn closing_update_prompt_dismisses_the_current_release() {
        assert!(should_dismiss_update_prompt(&OverlayWaitOutcome::Submitted(UpdatePromptChoice::StayCurrent,)));
        assert!(should_dismiss_update_prompt(&OverlayWaitOutcome::Cancelled));
        assert!(should_dismiss_update_prompt(&OverlayWaitOutcome::Interrupted));
        assert!(should_dismiss_update_prompt(&OverlayWaitOutcome::Exit));
        assert!(!should_dismiss_update_prompt(&OverlayWaitOutcome::Submitted(UpdatePromptChoice::UpdateAndRestart,)));
    }

    #[test]
    fn format_bytes_uses_appropriate_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 27 + 1024 * 100), "27.1 MB");
    }

    #[test]
    fn progress_bar_fills_proportionally() {
        assert_eq!(progress_bar(0, 20), "░░░░░░░░░░░░░░░░░░░░");
        assert_eq!(progress_bar(50, 20), "██████████░░░░░░░░░░");
        assert_eq!(progress_bar(100, 20), "████████████████████");
    }

    #[test]
    fn format_download_progress_shows_bar_and_bytes() {
        let total = 1024 * 1024 * 27; // 27 MB
        let text = format_download_progress(total / 2, Some(total));
        assert!(text.contains("50%"));
        assert!(text.contains("13.5 MB"));
        assert!(text.contains("27.0 MB"));
        assert!(text.contains("Downloading update"));

        // No Content-Length: byte count only, no bar/percent.
        let text = format_download_progress(1024 * 10, None);
        assert!(text.contains("10.0 KB"));
        assert!(!text.contains("%"));
    }

    #[test]
    fn download_progress_callback_replaces_last_line_after_first_event() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let progress_style = Arc::new(InlineTextStyle::default());
        let mut emitted = false;
        let mut on_progress = move |event: UpdateProgress| match event {
            UpdateProgress::Downloading { downloaded, total } => {
                let text = format_download_progress(downloaded, total);
                let segment = InlineSegment { text, style: progress_style.clone() };
                if emitted {
                    handle.replace_last(1, InlineMessageKind::Info, vec![vec![segment]]);
                } else {
                    handle.append_line(InlineMessageKind::Info, vec![segment]);
                    emitted = true;
                }
            }
            UpdateProgress::VerifyingChecksum | UpdateProgress::Extracting | UpdateProgress::ReplacingBinary => {
                handle.append_pasted_message(InlineMessageKind::Info, "phase".to_string(), 1);
                emitted = false;
            }
        };

        // First download event appends a new transcript line.
        on_progress(UpdateProgress::Downloading { downloaded: 0, total: Some(100) });
        assert!(matches!(rx.try_recv().expect("first command"), InlineCommand::AppendLine { .. }));

        // Second download event replaces the last line in place.
        on_progress(UpdateProgress::Downloading { downloaded: 50, total: Some(100) });
        assert!(matches!(rx.try_recv().expect("second command"), InlineCommand::ReplaceLast { .. }));

        // A phase transition appends a new line and resets the flag.
        on_progress(UpdateProgress::VerifyingChecksum);
        assert!(matches!(rx.try_recv().expect("third command"), InlineCommand::AppendPastedMessage { .. }));

        // After reset, the next download event appends again (not replaces).
        on_progress(UpdateProgress::Downloading { downloaded: 10, total: Some(100) });
        assert!(matches!(rx.try_recv().expect("fourth command"), InlineCommand::AppendLine { .. }));
    }
}
