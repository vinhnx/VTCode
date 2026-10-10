//! Hypothesis-loop mismatch classification for the agent runloop.
//!
//! desirable loop: hypothesis → observation → mismatch → inspect evidence →
//! revise hypothesis. This module is the pure, testable core of that loop:
//! it names the mismatch, but never blocks, retries, or performs I/O. The
//! binary's deterministic failure diagnosis calls into it to frame the
//! model-facing `next_action`; the evaluator scores the behavior.
//!
//! Dimension key for [`MismatchEvidence`] (index → field → meaning):
//!
//! | field                 | meaning when set                                          |
//! | --------------------- | --------------------------------------------------------- |
//! | `exit_code`           | tool result carried this process exit status               |
//! | `empty_search_no_match` | grep-style exit 1 with no output inside the queried scope |
//! | `patch_mismatch`      | patch context/deletion lines do not match the current file |
//! | `verifier_failed`     | a standalone verifier reported a non-zero status           |
//! | `repeated_evidence`   | query returned only previously seen lines/ranges           |

/// Named mismatch between the agent's hypothesis and its observation.
///
/// Priority order in [`classify_mismatch`] is declaration order: patch
/// mismatch first, repeated evidence last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MismatchKind {
    /// Process reported a non-zero exit status.
    NonZeroExit,
    /// grep-style exit 1 with no output: evidence of absence, not a defect.
    EmptySearch,
    /// Patch context or deletion lines do not match the current file.
    PatchMismatch,
    /// A standalone verifier reported failure.
    VerifierFailed,
    /// Query returned only previously seen evidence.
    RepeatedEvidence,
}

/// Structured evidence for one mismatch classification.
///
/// Prefer this named struct over a bare tuple so the shape is explicit in
/// the type system. All fields default to unset via [`Default`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MismatchEvidence {
    /// Process exit status carried by the tool result, when present.
    pub exit_code: Option<i64>,
    /// grep-style exit 1 with fully visible empty output.
    pub empty_search_no_match: bool,
    /// Patch context or deletion lines mismatch the current file.
    pub patch_mismatch: bool,
    /// A standalone verifier reported a non-zero status.
    pub verifier_failed: bool,
    /// Query returned only previously seen lines or ranges.
    pub repeated_evidence: bool,
}

/// Classify mismatch evidence into its highest-priority kind.
///
/// Returns `None` when nothing is set (including `exit_code` of `Some(0)`),
/// so callers keep truly unknown failures short.
#[must_use]
pub fn classify_mismatch(evidence: &MismatchEvidence) -> Option<MismatchKind> {
    if evidence.patch_mismatch {
        return Some(MismatchKind::PatchMismatch);
    }
    if evidence.verifier_failed {
        return Some(MismatchKind::VerifierFailed);
    }
    if evidence.empty_search_no_match {
        return Some(MismatchKind::EmptySearch);
    }
    if evidence.exit_code.is_some_and(|code| code != 0) {
        return Some(MismatchKind::NonZeroExit);
    }
    if evidence.repeated_evidence {
        return Some(MismatchKind::RepeatedEvidence);
    }
    None
}

/// Model-facing revision guidance for a classified mismatch.
///
/// `None` (unknown context) yields the generic framing. Every variant names
/// inspection before retry; only [`MismatchKind::EmptySearch`] redirects to
/// a new scope instead of a re-read.
#[must_use]
pub fn revision_guidance(kind: Option<MismatchKind>) -> &'static str {
    match kind {
        Some(MismatchKind::EmptySearch) => {
            "Treat this as evidence of absence in the queried scope; revise the hypothesis to a new scope or question."
        }
        Some(MismatchKind::PatchMismatch) => {
            "Re-read the exact current lines, revise the patch hypothesis, then retry once with exact context."
        }
        Some(MismatchKind::VerifierFailed) => {
            "Inspect the verifier output, revise the fix hypothesis, then re-verify standalone."
        }
        Some(MismatchKind::RepeatedEvidence) => {
            "Stop repeating the same query; revise the hypothesis and change scope or approach."
        }
        Some(MismatchKind::NonZeroExit) => {
            "Inspect the bounded evidence, revise the hypothesis, then retry with corrected arguments."
        }
        None => "Revise the hypothesis from the bounded evidence before retrying.",
    }
}

/// Append revision guidance to a deterministic `next_action`.
///
/// An empty base yields the guidance alone so callers never emit a leading
/// separator without content.
#[must_use]
pub fn append_revision_guidance(base: &str, kind: Option<MismatchKind>) -> String {
    let base = base.trim_end();
    if base.is_empty() {
        return revision_guidance(kind).to_string();
    }
    format!("{base} {}", revision_guidance(kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_alone_distinguishes_zero_from_nonzero() {
        let nonzero = MismatchEvidence { exit_code: Some(1), ..Default::default() };
        assert_eq!(classify_mismatch(&nonzero), Some(MismatchKind::NonZeroExit));
        let zero = MismatchEvidence { exit_code: Some(0), ..Default::default() };
        assert_eq!(classify_mismatch(&zero), None);
        assert_eq!(classify_mismatch(&MismatchEvidence::default()), None);
    }

    #[test]
    fn empty_search_outranks_exit_code() {
        let both = MismatchEvidence {
            exit_code: Some(1),
            empty_search_no_match: true,
            ..Default::default()
        };
        assert_eq!(classify_mismatch(&both), Some(MismatchKind::EmptySearch));
        let exit_only = MismatchEvidence { exit_code: Some(1), ..Default::default() };
        assert_eq!(classify_mismatch(&exit_only), Some(MismatchKind::NonZeroExit));
    }

    #[test]
    fn patch_mismatch_is_top_priority() {
        let all = MismatchEvidence {
            exit_code: Some(1),
            empty_search_no_match: true,
            patch_mismatch: true,
            verifier_failed: true,
            repeated_evidence: true,
        };
        assert_eq!(classify_mismatch(&all), Some(MismatchKind::PatchMismatch));
        let verifier_and_exit = MismatchEvidence {
            exit_code: Some(2),
            verifier_failed: true,
            repeated_evidence: true,
            ..Default::default()
        };
        assert_eq!(classify_mismatch(&verifier_and_exit), Some(MismatchKind::VerifierFailed));
    }

    #[test]
    fn repeated_evidence_is_lowest_priority() {
        let repeated_only = MismatchEvidence { repeated_evidence: true, ..Default::default() };
        assert_eq!(classify_mismatch(&repeated_only), Some(MismatchKind::RepeatedEvidence));
        let repeated_and_exit = MismatchEvidence {
            exit_code: Some(1),
            repeated_evidence: true,
            ..Default::default()
        };
        assert_eq!(classify_mismatch(&repeated_and_exit), Some(MismatchKind::NonZeroExit));
    }

    #[test]
    fn guidance_names_revision_and_scopes_empty_search() {
        for kind in [
            Some(MismatchKind::NonZeroExit),
            Some(MismatchKind::EmptySearch),
            Some(MismatchKind::PatchMismatch),
            Some(MismatchKind::VerifierFailed),
            Some(MismatchKind::RepeatedEvidence),
            None,
        ] {
            let guidance = revision_guidance(kind);
            assert!(!guidance.is_empty(), "{kind:?} guidance must not be empty");
            assert!(guidance.contains("hypothesis"), "{kind:?} guidance must name the hypothesis: {guidance}");
            assert!(
                guidance.to_ascii_lowercase().contains("revis"),
                "{kind:?} guidance must name revision: {guidance}"
            );
        }
        let empty = revision_guidance(Some(MismatchKind::EmptySearch));
        assert!(empty.contains("new scope"), "empty search must redirect scope: {empty}");
        let generic = revision_guidance(None);
        assert_ne!(generic, revision_guidance(Some(MismatchKind::NonZeroExit)));
        for shout in ["MUST", "NEVER", "ALWAYS", "CRITICAL", "IMPORTANT"] {
            assert!(!generic.contains(shout), "unexpected shouting: {shout}");
        }
    }

    #[test]
    fn append_keeps_base_and_handles_empty_base() {
        let appended = append_revision_guidance("Retry once.", Some(MismatchKind::NonZeroExit));
        assert!(appended.starts_with("Retry once."));
        assert!(appended.contains("hypothesis"));
        assert!(appended.to_ascii_lowercase().contains("revis"));
        let bare = append_revision_guidance("   ", None);
        assert_eq!(bare, revision_guidance(None));
    }
}
