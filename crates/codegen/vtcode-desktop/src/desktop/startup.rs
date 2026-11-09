//! Startup handoff: `--session <id> --workspace <path>` argv and `vtcode://`
//! URLs (sent by CLI `/desktop`).
//!
//! Both producers yield the protocol [`HandoffLink`]: argv is parsed once in
//! `main`, URLs arrive through `on_open_urls` into a [`UrlInbox`] (events can
//! precede app init on a cold launch, so early arrivals queue instead of drop).
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use vtcode_desktop_protocol::handoff_url::{HandoffLink, parse_handoff_url};

/// Parse argv (without argv\[0\]) for `--session <id>` + `--workspace <path>`.
///
/// Both `--flag value` and `--flag=value` forms are accepted, in any order.
/// Everything else — including the `-psn_*` tokens macOS Finder injects — is
/// ignored so a GUI launch never fails argument parsing. Returns `None`
/// unless both flags carry non-empty values.
pub(crate) fn parse_startup_link(args: &[String]) -> Option<HandoffLink> {
    let mut session_id = None;
    let mut workspace = None;
    let mut pending: Option<&str> = None;
    for arg in args {
        if let Some(flag) = pending.take() {
            match flag {
                "session" => session_id = Some(arg.clone()),
                "workspace" => workspace = Some(PathBuf::from(arg)),
                _ => {}
            }
            continue;
        }
        if let Some(value) = arg.strip_prefix("--session=") {
            session_id = Some(value.to_owned());
        } else if let Some(value) = arg.strip_prefix("--workspace=") {
            workspace = Some(PathBuf::from(value));
        } else if arg == "--session" {
            pending = Some("session");
        } else if arg == "--workspace" {
            pending = Some("workspace");
        }
    }
    let session_id = session_id.map(|id| id.trim().to_owned()).filter(|id| !id.is_empty())?;
    // The workspace is never trimmed: leading/trailing spaces are legal in
    // directory names, and canonicalization rejects what does not exist.
    let workspace = workspace.filter(|path| !path.as_os_str().is_empty())?;
    Some(HandoffLink { session_id, workspace })
}

/// Queues handoff URLs until the app context exists to dispatch them.
/// Main-thread only (`Rc`): both the URL callback and the run closure run
/// there, and neither touches the `AppCell` except through a live `App`.
#[derive(Default)]
pub(crate) struct UrlInbox {
    app: Option<gpui_kit::AsyncApp>,
    pending: Vec<HandoffLink>,
}

pub(crate) type SharedUrlInbox = Rc<RefCell<UrlInbox>>;

impl UrlInbox {
    /// Parse one raw URL; anything but a handoff link is ignored.
    pub(crate) fn push_url(&mut self, raw: &str) {
        if let Some(link) = parse_handoff_url(raw) {
            self.pending.push(link);
        }
    }

    pub(crate) fn set_app(&mut self, app: gpui_kit::AsyncApp) {
        self.app = Some(app);
    }

    /// Take queued links with the app handle to dispatch them on. Returns
    /// `None` until the app exists and at least one link is queued.
    pub(crate) fn take_ready(&mut self) -> Option<(gpui_kit::AsyncApp, Vec<HandoffLink>)> {
        let app = self.app.clone()?;
        if self.pending.is_empty() {
            return None;
        }
        Some((app, std::mem::take(&mut self.pending)))
    }

    /// Take queued links for dispatch with an already-borrowed `App`.
    pub(crate) fn take_pending(&mut self) -> Vec<HandoffLink> {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn parses_session_and_workspace_in_either_order() {
        for argv in [
            args(&["--session", "thread-1", "--workspace", "/repo"]),
            args(&["--workspace", "/repo", "--session", "thread-1"]),
            args(&["--session=thread-1", "--workspace=/repo"]),
            args(&["--workspace=/repo", "--session", "thread-1"]),
        ] {
            assert_eq!(
                parse_startup_link(&argv),
                Some(HandoffLink {
                    session_id: "thread-1".to_string(),
                    workspace: PathBuf::from("/repo")
                }),
                "{argv:?}"
            );
        }
    }

    #[test]
    fn ignores_finder_and_unknown_tokens() {
        let argv = args(&[
            "-psn_0_123456",
            "--session",
            "thread-1",
            "--unknown",
            "--workspace",
            "/repo",
        ]);
        assert_eq!(
            parse_startup_link(&argv),
            Some(HandoffLink {
                session_id: "thread-1".to_string(),
                workspace: PathBuf::from("/repo")
            })
        );
        assert_eq!(parse_startup_link(&args(&["-psn_0_123456"])), None);
        assert_eq!(parse_startup_link(&[]), None);
    }

    #[test]
    fn rejects_missing_or_empty_values() {
        assert_eq!(parse_startup_link(&args(&["--session", "thread-1"])), None);
        assert_eq!(parse_startup_link(&args(&["--workspace", "/repo"])), None);
        assert_eq!(parse_startup_link(&args(&["--session"])), None);
        assert_eq!(parse_startup_link(&args(&["--session", "  ", "--workspace", "/repo"])), None);
        assert_eq!(parse_startup_link(&args(&["--session=", "--workspace=/repo"])), None);
    }

    #[test]
    fn inbox_queues_until_the_app_exists() {
        let mut inbox = UrlInbox::default();
        assert!(inbox.take_ready().is_none());
        inbox.push_url("vtcode://session/thread-1?workspace=%2Frepo");
        inbox.push_url("https://example.com/not-a-handoff");
        assert!(inbox.take_ready().is_none(), "no app yet: links stay queued");
        assert_eq!(inbox.take_pending().len(), 1, "only the handoff link is kept");
    }
}
