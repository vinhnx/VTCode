//! OSC 7577 resume advertisement for interactive sessions.
//!
//! Tells supporting terminals how to bring this session back after a restart.
//! The terminal verifies the report and decides whether to honor it; this
//! module only describes the resume target. Headless `exec` runs never report:
//! re-running an agent prompt has side effects, so it is not safe to repeat.

use std::path::Path;

use vtcode_commons::program_resume::ProgramResumeUpdate;
use vtcode_core::utils::session_archive::SessionArchive;
use vtcode_ui::tui::app::InlineHandle;

/// Build the resume update for a session archive identifier, if reportable.
///
/// The resume command re-enters this exact session through its archive
/// identifier with the workspace pinned, so a terminal restart lands back here
/// without workspace prompts. Returns `None` when the workspace is not an
/// absolute UTF-8 path or the identifier is unusable on the protocol wire.
pub(crate) fn resume_update_for_identifier(identifier: &str, workspace: &Path) -> Option<ProgramResumeUpdate> {
    // Same rule as the archive lookup (`is_valid_session_identifier`): only
    // identifiers the lookup accepts are worth advertising.
    if identifier.is_empty()
        || identifier.len() > 256
        || !identifier
            .chars()
            .all(|part| part.is_ascii_alphanumeric() || part == '-' || part == '_')
    {
        return None;
    }
    if !workspace.is_absolute() {
        return None;
    }
    let workspace_text = workspace.to_str()?;
    if workspace_text.chars().any(char::is_control) {
        return None;
    }
    Some(ProgramResumeUpdate::SetTarget {
        argv: vec![
            "vtcode".to_string(),
            "--workspace".to_string(),
            workspace_text.to_string(),
            "--resume".to_string(),
            identifier.to_string(),
        ],
        cwd: Some(workspace_text.to_string()),
    })
}

/// Advertise the live session archive to the terminal. Best effort: history
/// disabled (no archive) or an unreportable target simply sends nothing.
pub(crate) fn advertise_program_resume(handle: &InlineHandle, archive: Option<&SessionArchive>, workspace: &Path) {
    let Some(archive) = archive else {
        return;
    };
    let Some(identifier) = archive.path().file_stem().and_then(|stem| stem.to_str()) else {
        return;
    };
    if let Some(update) = resume_update_for_identifier(identifier, workspace) {
        handle.program_resume(update);
    }
}

/// Withdraw the resume record on deliberate user exit. Best effort.
///
/// Call only when the user ends the session. Never call on SIGHUP, SIGTERM,
/// terminal loss, or crashes: that is when the record is needed.
pub(crate) fn clear_program_resume(handle: &InlineHandle) {
    handle.program_resume(ProgramResumeUpdate::Clear);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_argv_pins_workspace_and_identifier() {
        let update = resume_update_for_identifier("session-ws1-20260101T000000Z_000001-12345", Path::new("/tmp/ws"))
            .expect("valid target must build");
        let ProgramResumeUpdate::SetTarget { argv, cwd } = update else {
            panic!("expected a resume target");
        };
        assert_eq!(
            argv,
            [
                "vtcode",
                "--workspace",
                "/tmp/ws",
                "--resume",
                "session-ws1-20260101T000000Z_000001-12345"
            ]
        );
        assert_eq!(cwd.as_deref(), Some("/tmp/ws"));
    }

    #[test]
    fn unreportable_targets_build_nothing() {
        assert_eq!(resume_update_for_identifier("", Path::new("/tmp/ws")), None);
        assert_eq!(resume_update_for_identifier("has space", Path::new("/tmp/ws")), None);
        assert_eq!(resume_update_for_identifier("has/slash", Path::new("/tmp/ws")), None);
        assert_eq!(resume_update_for_identifier("session-1", Path::new("relative/ws")), None);
        assert_eq!(resume_update_for_identifier("session-1", Path::new("")), None);
        // Identifier length matches the archive lookup on both sides of 256.
        assert!(resume_update_for_identifier(&"s".repeat(256), Path::new("/tmp/ws")).is_some());
        assert_eq!(resume_update_for_identifier(&"s".repeat(257), Path::new("/tmp/ws")), None);
    }
}
