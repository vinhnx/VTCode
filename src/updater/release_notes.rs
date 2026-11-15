//! Release notes: parsing the highlights bullets and deciding which release
//! still owes the user its notes.
//!
//! Parsing turns a GitHub release body into bullet items; the policy half
//! answers "has this release been announced yet?" from the shared update
//! cache, so the CLI and the desktop app announce a release exactly once.

use std::sync::LazyLock;

use regex::Regex;
use semver::Version;
use tracing::debug;

use super::cache;
use super::preflight;
use super::types::UpdateInfo;

/// Parsed highlight items extracted from a GitHub release body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ParsedReleaseHighlights {
    /// The version string, e.g. "0.125.3"
    pub version: String,
    /// Bullet items extracted from the `### Highlights` section.
    pub items: Vec<String>,
}

/// Extract highlight bullet items from a GitHub release body markdown.
///
/// Looks for a `### Highlights` section and collects all `- ` prefixed lines
/// (including those under `####` sub-headers) until the next `##` header or
/// end of input. Trailing commit hash and author annotations like
/// `(071c9e64) (@username)` are stripped.
pub(crate) fn parse_highlights(version: &Version, body: &str) -> ParsedReleaseHighlights {
    let items = extract_highlight_items(body);

    ParsedReleaseHighlights { version: version.to_string(), items }
}

fn extract_highlight_items(body: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut in_highlights = false;

    for line in body.lines() {
        let trimmed = line.trim();

        // Detect the start of the Highlights section
        if trimmed.eq_ignore_ascii_case("### Highlights") {
            in_highlights = true;
            continue;
        }

        if !in_highlights {
            continue;
        }

        // Stop at the next major section: ## headers or ### headers that
        // are NOT #### sub-headers (e.g. "### Other Changes").
        if trimmed.starts_with("## ") || (trimmed.starts_with("### ") && !trimmed.starts_with("#### ")) {
            break;
        }

        // Skip #### sub-headers (Features, Bug Fixes, etc.) but keep collecting
        if trimmed.starts_with("#### ") {
            continue;
        }

        // Collect bullet items
        if let Some(content) = trimmed.strip_prefix("- ") {
            let cleaned = strip_commit_metadata(content);
            let cleaned = cleaned.trim();
            if !cleaned.is_empty() {
                items.push(cleaned.to_string());
            }
        }
    }

    items
}

/// Compiled regex for stripping trailing `(commit_hash) (@author)` patterns.
static COMMIT_METADATA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*\([0-9a-fA-F]+\)\s*(\(@[^)]+\))?\s*$").expect("valid regex pattern"));

/// Strip trailing `(commit_hash) (@author)` patterns from a highlight line.
fn strip_commit_metadata(text: &str) -> String {
    COMMIT_METADATA_RE.replace(text, "").to_string()
}

/// Check whether release notes should be shown for the current version.
///
/// Returns `true` if the current version has not been recorded as "seen" in the
/// update cache. A cache read failure stays quiet (`false`): the notes are a
/// courtesy, never worth a spurious banner.
///
/// This is the single source of truth for "does this release still owe the
/// user its notes?" — the TUI transcript banner and the desktop inline banner
/// both derive from it, so one release is announced once across surfaces.
pub(crate) fn should_show_release_notes_for_current_version() -> bool {
    let Ok(current) = Version::parse(env!("CARGO_PKG_VERSION")) else {
        return false;
    };
    !current_version_notes_seen_for(&current)
}

/// Whether the running version's release notes were already announced by any
/// VT Code surface (TUI transcript or desktop banner).
///
/// The marker lives in the shared update cache, so the CLI and the desktop app
/// cannot both announce the same release. A cache read failure reports "seen":
/// announcements are a courtesy, so an unreadable cache must stay quiet rather
/// than risk repeating the notes on every launch.
pub(crate) fn current_version_notes_seen() -> bool {
    let Ok(current) = Version::parse(env!("CARGO_PKG_VERSION")) else {
        return true;
    };
    current_version_notes_seen_for(&current)
}

fn current_version_notes_seen_for(version: &Version) -> bool {
    match cache::read_snapshot() {
        Ok(snapshot) => snapshot.last_seen_version.as_ref() == Some(version),
        Err(err) => {
            debug!("Failed to read update cache for release notes check: {err}");
            true
        }
    }
}

pub(crate) fn cached_current_release_highlights() -> Option<(Version, Vec<String>)> {
    let current = Version::parse(env!("CARGO_PKG_VERSION")).ok()?;
    parse_cached_highlights(cache::current_release_info(&current))
        // The spawned preflight fetch may have landed after the banner
        // decision read the cache (first launch after an update): serve the
        // running version's notes from that fetch instead of deferring them
        // to a second launch. A newer release never leaks in here.
        .or_else(|| parse_cached_highlights(current_notes_from_preflight(&current)))
}

/// The preflight fetch's notes when they describe the running version.
fn current_notes_from_preflight(current: &Version) -> Option<(Version, String)> {
    let (version, release_notes) = preflight::get_preflight_latest_release()?;
    (version == *current).then_some((version, release_notes))
}

/// The newest release the update cache knows about, with the Highlights
/// bullets parsed from its cached notes. Served to the desktop
/// `update/status` handler as the reader-facing changelog source, so the
/// viewer works whether or not the release is still an offered update.
///
/// An empty item list means the cached notes carry no Highlights section:
/// callers render a link-only state. `None` means the cache holds no notes at
/// all, which is the only "nothing to read" case.
pub(crate) fn cached_latest_release() -> Option<(Version, Vec<String>)> {
    let (version, release_notes) = cache::latest_release_info()?;
    Some((version.clone(), parse_highlights(&version, &release_notes).items))
}

/// Resolve the release notes to announce on this launch.
///
/// Prefers the highlights captured during session hydration, but re-reads the
/// live cache when hydration found nothing: the preflight fetch often lands
/// between hydration and the transcript banner (first launch after an external
/// update), and serving it now beats deferring the notes to a second launch.
pub(crate) fn resolve_release_notes_for_display(
    bootstrap: Option<(Version, Vec<String>)>,
) -> Option<(Version, Vec<String>)> {
    if bootstrap.is_some() {
        return bootstrap;
    }
    if !should_show_release_notes_for_current_version() {
        return None;
    }
    cached_current_release_highlights()
}

fn parse_cached_highlights(info: Option<(Version, String)>) -> Option<(Version, Vec<String>)> {
    let (version, release_notes) = info?;
    let parsed = parse_highlights(&version, &release_notes);
    (!parsed.items.is_empty()).then_some((version, parsed.items))
}

/// Record a release an explicit check just fetched from GitHub.
///
/// Mirrors the cache write the background refresh performs so every check —
/// explicit or background — leaves the same on-disk state for the next launch
/// to read.
pub(super) fn record_fetched_release(current: &Version, info: &UpdateInfo) {
    let latest_is_newer = info.version > *current;
    let _ = cache::record_successful_check_with_notes(Some(&info.version), latest_is_newer, Some(&info.release_notes));
}

/// Seed the update cache with the release notes for a newly installed version.
///
/// Called immediately after a successful install (standalone `self_replace`
/// or a managed package-manager command) so the next `vtcode` launch finds
/// `latest_version == current` with notes available and shows highlights on
/// its first start. Best effort: cache failures are ignored.
pub(crate) fn seed_release_notes_for_installed_version(version: &Version, release_notes: &str) {
    let _ = cache::record_successful_check_with_notes(Some(version), false, Some(release_notes));
    // Keep the install record apart from the check slot: a later check that
    // reports an even newer release overwrites `latest_version`/`release_notes`
    // and would otherwise hide the notes this version still owes the user.
    let _ = cache::record_installed_release(version, release_notes);
    let _ = cache::clear_dismissed_version();
}

/// Record that a version's release notes have been shown.
///
/// Called by every surface that announced the notes: the TUI transcript
/// banner and the desktop banner via `update/acknowledge`. `vtcode update`
/// prints the installed release's notes as immediate feedback but leaves the
/// marker alone, so the first app launch still announces them.
pub(crate) fn record_version_notes_seen(version: &Version) {
    let _ = cache::record_seen_version(version);
}

/// Record that the current version's release notes have been shown.
pub(crate) fn record_current_version_seen() {
    if let Ok(current) = Version::parse(env!("CARGO_PKG_VERSION")) {
        record_version_notes_seen(&current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn empty_body_returns_empty_items() {
        let result = parse_highlights(&v("0.1.0"), "");
        assert!(result.items.is_empty());
        assert_eq!(result.version, "0.1.0");
    }

    #[test]
    fn no_highlights_section_returns_empty() {
        let body = "## 0.1.0\n### Features\n- Something\n";
        let result = parse_highlights(&v("0.1.0"), body);
        assert!(result.items.is_empty());
    }

    #[test]
    fn extracts_basic_highlights() {
        let body = "\
## 0.125.0 - 2026-06-10

### Highlights
#### Features

- Add vtcode-ui to publish sequence (bb7228ed)
- Implement new parser (a1b2c3d4) (@vinhnx)

### Other Changes
#### Other

- Some other change
";
        let result = parse_highlights(&v("0.125.0"), body);
        assert_eq!(result.items.len(), 2);
        assert_eq!(result.items[0], "Add vtcode-ui to publish sequence");
        assert_eq!(result.items[1], "Implement new parser");
    }

    #[test]
    fn handles_subsections() {
        let body = "\
### Highlights
#### Bug Fixes

- Fix parameter handling (dac7afa0)
- Apply PR review fixes (d530f6c7) (@kernitus)

#### Features

- Add new model support (d4ed7872)

### Other Changes
- Not a highlight
";
        let result = parse_highlights(&v("0.124.0"), body);
        assert_eq!(result.items.len(), 3);
        assert_eq!(result.items[0], "Fix parameter handling");
        assert_eq!(result.items[1], "Apply PR review fixes");
        assert_eq!(result.items[2], "Add new model support");
    }

    #[test]
    fn strips_commit_hash_only() {
        let body = "\
### Highlights
- Simple fix (abc1234)
";
        let result = parse_highlights(&v("0.1.0"), body);
        assert_eq!(result.items, vec!["Simple fix"]);
    }

    #[test]
    fn strips_commit_hash_and_author() {
        let body = "\
### Highlights
- Feature name (deadbeef) (@someone)
";
        let result = parse_highlights(&v("0.1.0"), body);
        assert_eq!(result.items, vec!["Feature name"]);
    }

    #[test]
    fn stops_at_next_major_section() {
        let body = "\
### Highlights
- Item one (abc1234)

### Other Changes
- Should not appear (def5678)

### More Stuff
- Also excluded
";
        let result = parse_highlights(&v("0.1.0"), body);
        assert_eq!(result.items, vec!["Item one"]);
    }

    #[test]
    fn skips_empty_bullet_lines() {
        let body = "\
### Highlights
- First item (abc1234)

- Second item (def5678)
";
        let result = parse_highlights(&v("0.1.0"), body);
        assert_eq!(result.items, vec!["First item", "Second item"]);
    }

    #[test]
    fn highlights_with_no_subsections() {
        let body = "\
### Highlights
- Direct item one (abc1234)
- Direct item two (def5678) (@author)
";
        let result = parse_highlights(&v("0.1.0"), body);
        assert_eq!(result.items, vec!["Direct item one", "Direct item two"]);
    }

    #[test]
    fn case_insensitive_header_match() {
        let body = "### highlights\n- Item (abc1234)\n";
        let result = parse_highlights(&v("0.1.0"), body);
        assert_eq!(result.items, vec!["Item"]);
    }
}
