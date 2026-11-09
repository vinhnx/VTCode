//! `vtcode://` handoff links: open the desktop app at a thread from the CLI.
//!
//! Contract shared by the CLI (producer) and the desktop app (consumer):
//! `vtcode://session/<archive-id>?workspace=<url-encoded-absolute-path>`.
//! Parsing is fail-closed: anything else yields `None` and the URL is
//! ignored. Paths decode lossy on purpose: a mangled path fails workspace
//! canonicalization later and surfaces invalid-folder instead of linking
//! somewhere unexpected.

use std::path::PathBuf;

/// URL scheme claimed by the desktop bundle (`CFBundleURLTypes`).
pub const HANDOFF_SCHEME: &str = "vtcode";

/// A parsed handoff link: load this thread in this workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoffLink {
    /// Archive identifier of the thread (== CLI thread id).
    pub session_id: String,
    /// Workspace the thread belongs to (canonicalized by the consumer).
    pub workspace: PathBuf,
}

/// Build the canonical handoff URL for a link.
#[must_use]
pub fn handoff_url(link: &HandoffLink) -> String {
    let session: String = url::form_urlencoded::byte_serialize(link.session_id.as_bytes()).collect();
    let workspace: String =
        url::form_urlencoded::byte_serialize(link.workspace.as_os_str().as_encoded_bytes()).collect();
    format!("{HANDOFF_SCHEME}://session/{session}?workspace={workspace}")
}

/// Parse a handoff URL. Returns `None` for any other shape.
#[must_use]
pub fn parse_handoff_url(raw: &str) -> Option<HandoffLink> {
    let url = url::Url::parse(raw).ok()?;
    if url.scheme() != HANDOFF_SCHEME || url.host_str() != Some("session") {
        return None;
    }
    let mut segments = url.path_segments()?;
    let session_id = segments.next().filter(|id| !id.trim().is_empty())?;
    if segments.next().is_some() {
        return None;
    }
    let workspace = url
        .query_pairs()
        .find_map(|(key, value)| (key == "workspace").then_some(value))?;
    if workspace.trim().is_empty() {
        return None;
    }
    Some(HandoffLink {
        session_id: session_id.trim().to_owned(),
        workspace: PathBuf::from(workspace.into_owned()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(session_id: &str, workspace: &str) -> HandoffLink {
        HandoffLink {
            session_id: session_id.to_string(),
            workspace: PathBuf::from(workspace),
        }
    }

    #[test]
    fn canonical_form_is_stable() {
        assert_eq!(
            handoff_url(&link("thread-1", "/repo/workspace")),
            "vtcode://session/thread-1?workspace=%2Frepo%2Fworkspace"
        );
    }

    #[test]
    fn build_parse_round_trips_hostile_paths() {
        for original in [
            link("thread-1", "/repo/workspace"),
            link("session-label-9", "/sp ace/üñî +?&=#%"),
            link("a", "/"),
        ] {
            let parsed = parse_handoff_url(&handoff_url(&original)).expect("round trip");
            assert_eq!(parsed, original);
        }
    }

    #[test]
    fn rejects_non_handoff_urls() {
        for raw in [
            "",
            "not a url",
            "https://session/thread-1?workspace=/repo",
            "vtcode://other/thread-1?workspace=/repo",
            "vtcode:session/thread-1?workspace=/repo",
            "vtcode://session/?workspace=/repo",
            "vtcode://session/a/b?workspace=/repo",
            "vtcode://session/thread-1",
            "vtcode://session/thread-1?workspace=",
            "vtcode://session/thread-1?workspace=%20%20",
            "vtcode://session/thread-1?other=/repo",
        ] {
            assert_eq!(parse_handoff_url(raw), None, "{raw}");
        }
    }
}
