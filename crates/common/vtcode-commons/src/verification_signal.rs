//! Shape-only diagnostics for failed verification commands.
//!
//! Mirrors the checkpoint + fingerprint pattern: record *where* a failure
//! happened and a stable fingerprint used to group repeats, never the output
//! text itself. Every retained field is bounded and derived from a short
//! allowlist of well-known verifier markers, so user data appearing anywhere
//! else in command output cannot leak into prompts or handoffs. The `site` is the
//! bounded token found *at* the marker position; treat it as untrusted output
//! text (a localization hint), not as a verified identifier.

use crate::utils::calculate_sha256;

/// Maximum characters retained for a localized site.
const MAX_SITE_CHARS: usize = 120;
/// Characters of the hex digest retained as the grouping fingerprint.
const FINGERPRINT_CHARS: usize = 16;
/// Maximum window (in bytes) scanned after a marker while looking for a test
/// name. Sized to hold the longest retained site in the widest UTF-8 encoding
/// (120 chars x 4 bytes) plus the ` ... FAILED` suffix, so a name that will
/// be truncated still classifies instead of falling through to `Other`.
const MARKER_WINDOW_BYTES: usize = 512;
/// Maximum repeated markers examined in one output.
const MAX_MARKER_ATTEMPTS: usize = 64;

/// The recognizable category of a verification failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VerificationFailureKind {
    /// A Rust/`std`-style `panicked at` line.
    Panic,
    /// An individual test reported `FAILED`.
    TestFailed,
    /// A language compiler emitted a structured error.
    CompileError,
    /// The verifier reported a timeout.
    Timeout,
    /// No allowlisted marker matched. This is the default.
    #[default]
    Other,
}

impl VerificationFailureKind {
    /// Stable lowercase label used in diagnostics and as fingerprint input.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Panic => "panic",
            Self::TestFailed => "test",
            Self::CompileError => "compile",
            Self::Timeout => "timeout",
            Self::Other => "other",
        }
    }
}

/// Bounded, content-free shape of one failed verification command.
///
/// The default is the unrecognized shape (`Other`, no site, no fingerprint).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerificationFailureShape {
    /// Recognized category of the failure.
    pub kind: VerificationFailureKind,
    /// Localized `file:line` or test name, when an allowlisted marker supplied one.
    pub site: Option<String>,
    /// Stable fingerprint of `(kind, site)`, used to group repeated failures.
    /// `None` when no allowlisted marker matched: unrecognized failures have
    /// no identity to deduplicate against.
    pub fingerprint: Option<String>,
}

impl VerificationFailureShape {
    /// One-line, content-free description for prompts and handoffs.
    ///
    /// # Ensures
    ///
    /// The result contains only the kind label, the bounded site, and a
    /// fingerprint when one exists. Beyond the site token, it never reproduces
    /// command output.
    #[must_use]
    pub fn describe(&self) -> String {
        let base = match self.site.as_deref() {
            Some(site) => format!("{} at {site}", self.kind.as_str()),
            None => self.kind.as_str().to_string(),
        };
        match self.fingerprint.as_deref() {
            Some(fingerprint) => format!("{base} (fingerprint {fingerprint})"),
            None => base,
        }
    }
}

/// Anti-thrash coaching appended once the same failure shape has recurred.
///
/// Shared by every loop that tracks a fingerprint streak so the wording stays
/// identical across the interactive runloop and the headless runner. Returns an
/// empty string for streaks below 2 (a single sighting is evidence, not thrash).
///
/// # Requires
///
/// None.
///
/// # Ensures
///
/// The result is empty for `streak < 2`, otherwise a bounded sentence naming
/// the streak count. It never reproduces verifier output.
///
/// # Termination
///
/// Straight-line formatting.
#[must_use]
pub fn repeated_shape_note(streak: u8) -> String {
    if streak >= 2 {
        format!(
            " This failure shape has repeated {streak} times consecutively; change your approach instead of \
             retrying the same fix."
        )
    } else {
        String::new()
    }
}

/// Extract a shape-only diagnostic from verifier output.
///
/// # Requires
///
/// None: the function is total over `&str`, including empty and hostile input.
///
/// # Ensures
///
/// - `site` is at most 120 characters and comes only from an allowlisted marker.
/// - `fingerprint` is `None` when no allowlisted marker matched, otherwise 16
///   lowercase hexadecimal characters.
/// - No field reproduces arbitrary content from `output` beyond the bounded
///   token at the matched marker position.
/// - Total: a bounded number of substring scans over `output`.
///
/// # Termination
///
/// Bounded: at most 64 marker attempts per scan loop over 512-byte windows,
/// plus a constant number of linear substring scans.
#[must_use]
pub fn extract_verification_failure_shape(output: &str) -> VerificationFailureShape {
    let (kind, site) = classify(output);
    let fingerprint = fingerprint(kind, site.as_deref());
    VerificationFailureShape { kind, site, fingerprint }
}

fn classify(output: &str) -> (VerificationFailureKind, Option<String>) {
    if let Some(site) = location_after(output, "panicked at ") {
        return (VerificationFailureKind::Panic, Some(site));
    }
    if let Some(name) = failed_test_name(output) {
        return (VerificationFailureKind::TestFailed, Some(name));
    }
    if output.contains("test result: FAILED") {
        return (VerificationFailureKind::TestFailed, None);
    }
    if output.contains("error[E") || output.contains("error: could not compile") {
        return (VerificationFailureKind::CompileError, location_after(output, "--> "));
    }
    let lowered = output.to_ascii_lowercase();
    if is_timeout(&lowered) {
        return (VerificationFailureKind::Timeout, None);
    }
    // A panic marker without a parseable location still identifies the kind;
    // it sorts below localized shapes so a named failing test keeps its site.
    if output.contains("panicked at ") {
        return (VerificationFailureKind::Panic, None);
    }
    (VerificationFailureKind::Other, None)
}

/// True when the lowered output reports a timeout rather than merely
/// mentioning one. A bare `timeout` needs failure context so config echoes
/// like `timeout=30s` do not group unrelated failures under one fingerprint.
fn is_timeout(lowered: &str) -> bool {
    if lowered.contains("timed out") {
        return true;
    }
    const FAILURE_CONTEXT: [&str; 7] = ["error", "fail", "exceed", "expire", "killed", "abort", "deadline"];
    lowered.contains("timeout") && FAILURE_CONTEXT.iter().any(|word| lowered.contains(word))
}

/// Find an individual failed test name, tolerating JSON-escaped newlines.
fn failed_test_name(output: &str) -> Option<String> {
    let mut rest = output;
    for _ in 0..MAX_MARKER_ATTEMPTS {
        let Some((_, after)) = rest.split_once("test ") else {
            break;
        };
        // `trim_start` keeps `strip_prefix(name)` aligned when the runner pads
        // the name with extra whitespace (`test  foo ... FAILED`).
        let window = bounded_window(after).trim_start();
        let Some(name) = window.split_whitespace().next() else {
            rest = after;
            continue;
        };
        // Same rejection set as the `---- <name> stdout ----` fallback below:
        // quotes and backslashes mark JSON-escaped or spliced text, never a
        // test name.
        if !name.is_empty()
            && !name.contains(['\n', '"', '\\'])
            && window.strip_prefix(name).is_some_and(|tail| tail.starts_with(" ... FAILED"))
        {
            return Some(bound_site(name));
        }
        rest = after;
    }

    // Rust also emits `---- <name> stdout ----` above captured test output;
    // that header survives when the trailing `FAILED` line was cut from a tail.
    // `break` above (not `?`) is what keeps this fallback reachable.
    let mut rest = output;
    for _ in 0..MAX_MARKER_ATTEMPTS {
        let Some((_, after)) = rest.split_once("---- ") else {
            break;
        };
        let window = bounded_window(after);
        if let Some((name, _)) = window.split_once(" stdout ----")
            && !name.is_empty()
            && !name.contains(['\n', '"', '\\'])
        {
            return Some(bound_site(name));
        }
        rest = after;
    }
    None
}

/// Cap a scan window without cutting through a multi-byte character.
fn bounded_window(text: &str) -> &str {
    if text.len() <= MARKER_WINDOW_BYTES {
        return text;
    }
    let mut end = MARKER_WINDOW_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or(text)
}

/// Read a bounded `file:line` token immediately after a known marker.
fn location_after(output: &str, marker: &str) -> Option<String> {
    let (_, after) = output.split_once(marker)?;
    let token = after
        .split(|c: char| c.is_whitespace() || matches!(c, '"' | '\\'))
        .next()?
        .trim_end_matches(':');
    normalize_location(token)
}

fn normalize_location(token: &str) -> Option<String> {
    let mut segments = token.rsplitn(3, ':');
    let first = segments.next()?;
    let second = segments.next();
    let third = segments.next();
    let (line, path) = match (second, third) {
        (Some(line), Some(path)) if is_number(first) => (line, path),
        (Some(path), None) => (first, path),
        _ => return None,
    };
    if !is_number(line) {
        return None;
    }
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    Some(bound_site(&format!("{file}:{line}")))
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn bound_site(text: &str) -> String {
    text.chars().take(MAX_SITE_CHARS).collect()
}

fn fingerprint(kind: VerificationFailureKind, site: Option<&str>) -> Option<String> {
    if kind == VerificationFailureKind::Other {
        return None;
    }
    let input = format!("{}|{}", kind.as_str(), site.unwrap_or(""));
    let digest = calculate_sha256(input.as_bytes());
    debug_assert!(digest.len() >= FINGERPRINT_CHARS && digest.is_ascii(), "sha256 hex digest invariant violated");
    // The digest is lowercase ASCII hex, so the prefix is a char boundary.
    Some(digest.get(..FINGERPRINT_CHARS).unwrap_or_default().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_failure_localizes_the_individual_test_name() {
        let shape = extract_verification_failure_shape(
            "running 3 tests\ntest auth::expired_token ... FAILED\ntest cache::hit ... ok\n",
        );

        assert_eq!(shape.kind, VerificationFailureKind::TestFailed);
        assert_eq!(shape.site.as_deref(), Some("auth::expired_token"));
        assert!(shape.describe().contains("auth::expired_token"), "{}", shape.describe());
    }

    #[test]
    fn test_summary_without_an_individual_name_keeps_the_kind_but_no_site() {
        let shape = extract_verification_failure_shape("test result: FAILED. 1 failed, 2 passed");

        assert_eq!(shape.kind, VerificationFailureKind::TestFailed);
        assert_eq!(shape.site, None);
    }

    #[test]
    fn panic_localizes_the_trailing_file_and_line_not_the_full_path() {
        let shape = extract_verification_failure_shape(
            "thread 'main' panicked at /Users/someone/project/crates/foo/src/lib.rs:42:9:\nassertion failed",
        );

        assert_eq!(shape.kind, VerificationFailureKind::Panic);
        assert_eq!(shape.site.as_deref(), Some("lib.rs:42"));
        assert!(!shape.describe().contains("/Users/"), "site must not leak an absolute path");
    }

    #[test]
    fn compile_error_uses_the_arrow_location() {
        let shape = extract_verification_failure_shape("error[E0308]: mismatched types\n  --> src/main.rs:7:5\n");

        assert_eq!(shape.kind, VerificationFailureKind::CompileError);
        assert_eq!(shape.site.as_deref(), Some("main.rs:7"));
    }

    #[test]
    fn timeout_is_recognized_without_a_site() {
        let shape = extract_verification_failure_shape("Error: operation timed out after 30s");

        assert_eq!(shape.kind, VerificationFailureKind::Timeout);
        assert_eq!(shape.site, None);
    }

    #[test]
    fn json_escaped_output_is_recognized_like_plain_text() {
        let escaped = r#"{"exit_code":1,"output":"test auth::expired_token ... FAILED\nboom"}"#;
        let shape = extract_verification_failure_shape(escaped);

        assert_eq!(shape.kind, VerificationFailureKind::TestFailed);
        assert_eq!(shape.site.as_deref(), Some("auth::expired_token"));
    }

    #[test]
    fn equivalent_failures_share_a_fingerprint_but_distinct_sites_do_not() {
        let first = extract_verification_failure_shape("test auth::expired ... FAILED\nassertion a == b failed");
        let same_bug = extract_verification_failure_shape("test auth::expired ... FAILED\nleft == right differs");
        let other_bug = extract_verification_failure_shape("test cache::expired ... FAILED\nassertion a == b failed");

        let first_fp = first.fingerprint.as_deref().expect("fingerprint");
        assert_eq!(first_fp, same_bug.fingerprint.as_deref().expect("fingerprint"));
        assert_ne!(first_fp, other_bug.fingerprint.as_deref().expect("fingerprint"));
        assert_eq!(first_fp.len(), FINGERPRINT_CHARS);
        assert!(first_fp.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn oversized_test_names_are_bounded_on_both_sides_of_the_limit() {
        let under_limit = extract_verification_failure_shape(&format!("test {} ... FAILED", "a".repeat(119)));
        assert_eq!(under_limit.site.as_ref().expect("site").chars().count(), 119);

        let over_limit = extract_verification_failure_shape(&format!("test {} ... FAILED", "b".repeat(200)));
        assert_eq!(over_limit.site.as_ref().expect("site").chars().count(), MAX_SITE_CHARS);
        assert_eq!(over_limit.fingerprint.as_deref().expect("fingerprint").len(), FINGERPRINT_CHARS);
        assert_ne!(under_limit.fingerprint, over_limit.fingerprint);
    }

    #[test]
    fn unrecognized_output_has_no_grouping_fingerprint() {
        let first = extract_verification_failure_shape("something odd happened");
        let second = extract_verification_failure_shape("a completely different oddity");

        assert_eq!(first.kind, VerificationFailureKind::Other);
        assert_eq!(first.site, None);
        assert_eq!(first.fingerprint, None);
        assert_eq!(second.fingerprint, None);
        assert!(!first.describe().contains("fingerprint"), "{}", first.describe());
    }

    #[test]
    fn captured_output_header_localizes_even_when_the_failed_line_is_cut() {
        let shape =
            extract_verification_failure_shape("---- auth::expired_token stdout ----\nassertion failed: left == right");

        assert_eq!(shape.kind, VerificationFailureKind::TestFailed);
        assert_eq!(shape.site.as_deref(), Some("auth::expired_token"));
        assert!(shape.fingerprint.is_some());
    }

    #[test]
    fn arbitrary_output_content_never_appears_in_the_description() {
        let secret = "SECRET_USER_TOKEN_XYZ";
        let shape = extract_verification_failure_shape(&format!("leftover {secret} after the failure"));

        assert!(!shape.describe().contains(secret), "{}", shape.describe());
        assert_eq!(shape.kind, VerificationFailureKind::Other);
    }

    #[test]
    fn panic_without_a_parseable_location_keeps_the_kind_but_no_site() {
        let shape = extract_verification_failure_shape("thread 'main' panicked at 'assertion failed'");

        assert_eq!(shape.kind, VerificationFailureKind::Panic);
        assert_eq!(shape.site, None);
        assert!(shape.fingerprint.is_some(), "kind-only failures still group");
    }

    #[test]
    fn timeout_mention_without_failure_context_is_not_a_timeout() {
        for output in [
            "setting timeout=30s for cache\nall green",
            "request timeout: 5s (default)\nerror[E0308]: mismatched types",
        ] {
            let shape = extract_verification_failure_shape(output);
            assert_ne!(shape.kind, VerificationFailureKind::Timeout, "{output}");
        }
        // The second input must still classify by its stronger marker.
        let compile = extract_verification_failure_shape("request timeout: 5s\nerror[E0308]: boom");
        assert_eq!(compile.kind, VerificationFailureKind::CompileError);
    }

    #[test]
    fn timeout_with_failure_context_keeps_the_kind() {
        let mut fingerprints = Vec::new();
        for output in [
            "Error: connection timeout on upstream",
            "Timeout exceeded after 3 retries",
        ] {
            let shape = extract_verification_failure_shape(output);
            assert_eq!(shape.kind, VerificationFailureKind::Timeout, "{output}");
            assert_eq!(shape.site, None);
            fingerprints.push(shape.fingerprint.clone().expect("fingerprint"));
        }
        // Siteless kinds group by kind: consecutive timeouts are one streak
        // even when the underlying cause differs.
        assert_eq!(fingerprints[0], fingerprints[1]);
    }

    #[test]
    fn siteless_panics_share_one_kind_level_fingerprint() {
        let first = extract_verification_failure_shape("thread 'main' panicked at 'assertion failed'");
        let second = extract_verification_failure_shape("thread 'worker' panicked at 'index out of bounds'");

        assert_eq!(first.kind, VerificationFailureKind::Panic);
        assert_eq!(second.kind, VerificationFailureKind::Panic);
        assert_eq!(first.fingerprint, second.fingerprint);
        assert_ne!(
            first.fingerprint,
            extract_verification_failure_shape("Error: operation timed out").fingerprint,
            "kind levels must not collide across kinds"
        );
    }

    #[test]
    fn json_escaped_panic_keeps_the_kind_without_a_site() {
        // Tool results arrive JSON-escaped, so the `src\/lib.rs` backslashes
        // defeat the location token: the kind must survive anyway.
        let escaped = r#"{"exit_code":101,"output":"thread 'main' panicked at src\/lib.rs:42:9\nboom"}"#;
        let shape = extract_verification_failure_shape(escaped);

        assert_eq!(shape.kind, VerificationFailureKind::Panic);
        assert_eq!(shape.site, None);
        assert!(shape.fingerprint.is_some());
    }

    #[test]
    fn quoted_or_escaped_test_name_is_rejected() {
        let shape = extract_verification_failure_shape(r#"test injected\"name ... FAILED"#);

        // The token at the marker is spliced text, not a test name: no site,
        // and no fingerprint built from attacker-influenced bytes.
        assert_eq!(shape.kind, VerificationFailureKind::Other);
        assert_eq!(shape.site, None);
        assert_eq!(shape.fingerprint, None);
    }

    #[test]
    fn extra_whitespace_between_test_and_name_still_localizes() {
        let shape = extract_verification_failure_shape("test  foo ... FAILED");

        assert_eq!(shape.kind, VerificationFailureKind::TestFailed);
        assert_eq!(shape.site.as_deref(), Some("foo"));
    }

    #[test]
    fn multibyte_site_is_bounded_by_chars_not_bytes() {
        // 200 `é` = 400 bytes: the name alone exceeds the old 256-byte scan
        // window, so this pins both classification and char-based truncation.
        let shape = extract_verification_failure_shape(&format!("test {} ... FAILED", "é".repeat(200)));

        assert_eq!(shape.kind, VerificationFailureKind::TestFailed);
        let site = shape.site.as_deref().expect("site");
        assert_eq!(site.chars().count(), MAX_SITE_CHARS);
        assert!(site.is_char_boundary(site.len()));
    }

    #[test]
    fn repeated_shape_note_emits_only_from_the_second_sighting() {
        assert!(repeated_shape_note(0).is_empty());
        assert!(repeated_shape_note(1).is_empty());

        let note = repeated_shape_note(3);
        assert!(note.contains("3 times consecutively"), "{note}");
        assert!(note.contains("change your approach"), "{note}");
    }
}
