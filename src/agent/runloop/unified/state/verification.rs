//! Bounded continuation budgets and verification-recovery episodes.
//!
//! Extracted from state.rs beside the pre-existing preflight_recovery,
//! request_envelope, and ctrl_c submodules. No behavior change: verifier,
//! tracker-continuation, and plan-continuation budgets plus the
//! verification failure record are verbatim, and VerificationFailureSummary
//! is re-exported from the parent.

use super::SessionStats;

/// A verifier failure caused by a missing executable rather than a failing
/// assertion.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MissingExecutable {
    /// The output names the missing binary (e.g. `npx`).
    Named(String),
    /// The output shows a missing-executable marker but no usable name
    /// (e.g. a truncated traceback tail).
    Unnamed,
}

/// Environment guidance shared by the in-turn recovery coaching and the
/// blocked handoff. A missing binary on `PATH` can never be fixed by
/// re-running the same command, so both surfaces say so explicitly instead of
/// repeating the generic "repair the reported failure" recipe.
const MISSING_EXECUTABLE_GUIDANCE: &str = "was not found on PATH: this is an environment problem, not a failing \
    assertion, so re-running the same command cannot fix it. Install the toolchain and ensure `exec_command` \
    inherits it on PATH, or correct the tracker's `verify` metadata if the assertion itself is wrong.";

/// Detect a missing-executable verifier failure from tool-output text.
///
/// Returns environment guidance naming the binary when the output identifies
/// one, or `None` when the output looks like a genuine assertion failure.
/// Matching stays narrow on purpose: a bare `No such file or directory` can
/// name a missing input file, so only the quoted executable form and the
/// shell/spawn markers count.
pub(crate) fn missing_executable_hint(output: &str) -> Option<String> {
    let subject = match detect_missing_executable(output)? {
        MissingExecutable::Named(name) => format!("`{name}`"),
        MissingExecutable::Unnamed => "A required executable".to_string(),
    };
    Some(format!("{subject} {MISSING_EXECUTABLE_GUIDANCE}"))
}

fn detect_missing_executable(output: &str) -> Option<MissingExecutable> {
    let mut unnamed = false;
    for line in output.lines().map(str::trim) {
        if let Some(name) = quoted_executable_name(line) {
            return Some(MissingExecutable::Named(name));
        }
        if let Some(name) = shell_not_found_name(line) {
            return Some(MissingExecutable::Named(name));
        }
        if let Some(name) = spawn_enoent_name(line) {
            return Some(MissingExecutable::Named(name));
        }
        unnamed |= has_unnamed_missing_executable_marker(line);
    }
    unnamed.then_some(MissingExecutable::Unnamed)
}

/// Python `FileNotFoundError: [Errno 2] No such file or directory: 'npx'`,
/// Go `exec: "bun": executable file not found in $PATH`, and the
/// `'<binary>' was not found on PATH` toolchain error quote the binary.
fn quoted_executable_name(line: &str) -> Option<String> {
    let quoted = if line.contains("No such file or directory: '") || line.contains("not found on PATH") {
        first_single_quoted_token(line)
    } else if line.contains("executable file not found in") {
        first_double_quoted_token(line)
    } else {
        None
    }?;
    is_plausible_binary_name(quoted).then(|| quoted.to_string())
}

/// bash/zsh `npx: command not found` and dash `sh: 1: npx: not found`. The
/// dash form requires an explicit shell prefix so `error: foo: not found`
/// (a missing item, not a missing binary) does not match.
fn shell_not_found_name(line: &str) -> Option<String> {
    if let Some(prefix) = line.strip_suffix("command not found") {
        let name = prefix
            .trim_end()
            .trim_end_matches(':')
            .split_whitespace()
            .next_back()
            .unwrap_or("");
        let name = name.trim_end_matches(':');
        return is_plausible_binary_name(name).then(|| name.to_string());
    }
    if let Some(prefix) = line.strip_suffix(": not found") {
        let mut tokens = prefix.split_whitespace();
        if !matches!(tokens.next().unwrap_or(""), "sh:" | "bash:" | "zsh:" | "dash:" | "fish:") {
            return None;
        }
        let name = tokens.last().unwrap_or("").trim_end_matches(':');
        return is_plausible_binary_name(name).then(|| name.to_string());
    }
    None
}

/// Node `Error: spawn npx ENOENT`.
fn spawn_enoent_name(line: &str) -> Option<String> {
    let rest = line.split_once("spawn ")?.1;
    if !rest.contains("ENOENT") {
        return None;
    }
    let name = rest.split_whitespace().next().unwrap_or("");
    (is_plausible_binary_name(name) && name != "ENOENT").then(|| name.to_string())
}

/// Truncated tails may keep only the marker without the binary name.
fn has_unnamed_missing_executable_marker(line: &str) -> bool {
    line.contains("FileNotFoundError")
        || line.contains("command not found")
        || line.contains("not found on PATH")
        || line.contains("executable file not found in")
        || (line.contains("spawn") && line.contains("ENOENT"))
}

/// Reject empty names, paths (a missing file, not a missing binary), and
/// anything too long or too odd to be an executable name.
fn is_plausible_binary_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 128 && !name.contains(['/', '\\']) && !name.chars().any(char::is_whitespace)
}

fn first_single_quoted_token(line: &str) -> Option<&str> {
    line.split_once('\'')?.1.split('\'').next()
}

fn first_double_quoted_token(line: &str) -> Option<&str> {
    line.split_once('"')?.1.split('"').next()
}

/// Bounded record of a failed harness auto-verification, kept for handoff
/// enrichment so the eventual manual blocker names the failing command and
/// shows its output tail instead of repeating the generic "run a verifier"
/// recipe.
#[derive(Debug, Clone, Default)]
pub(crate) struct VerificationFailureSummary {
    /// Verifier command that failed (e.g. `cargo check --locked`).
    pub command: String,
    /// Observed terminal status, retained separately from the bounded tail.
    pub exit_code: Option<i64>,
    /// Tail excerpt of the tool output, bounded to
    /// `VERIFICATION_FAILURE_EXCERPT_CHARS` chars at record time.
    pub excerpt_tail: String,
    /// Consecutive failure count including this failure.
    pub consecutive_failures: u8,
}

impl SessionStats {
    /// Preserve the legacy snapshot shape; the retired edit allowance is zero.
    pub(crate) fn verification_snapshot(&self) -> (bool, u8) {
        (self.verification_pending, 0)
    }

    pub(crate) fn set_verification_snapshot(&mut self, snapshot: (bool, u8)) {
        self.verification_pending = snapshot.0;
    }

    /// Config-aware cross-turn recovery recorder; prefer it at call sites with
    /// workspace config access so `[agent.harness.verification].cross_turn_turns`
    /// is honored. Returns `true` when budget remained (the caller should
    /// queue a recovery turn instead of writing a blocked handoff); `false`
    /// once exhausted and manual `continue` is required.
    pub(crate) fn record_verification_auto_recovery_turn_with_limit(&mut self, max_turns: u8) -> bool {
        if self.verification_auto_recovery_turns >= max_turns {
            return false;
        }
        self.verification_auto_recovery_turns = self.verification_auto_recovery_turns.saturating_add(1);
        true
    }

    pub(crate) fn verification_auto_recovery_turns(&self) -> u8 {
        self.verification_auto_recovery_turns
    }

    /// Record one tracker auto-continue turn. Returns `true` when budget
    /// remained (`max_turns`); `false` once exhausted.
    pub(crate) fn record_tracker_continuation_turn_with_limit(&mut self, max_turns: u8) -> bool {
        if self.tracker_continuation_turns >= max_turns {
            return false;
        }
        self.tracker_continuation_turns = self.tracker_continuation_turns.saturating_add(1);
        true
    }

    pub(crate) fn tracker_continuation_turns(&self) -> u8 {
        self.tracker_continuation_turns
    }

    pub(crate) fn reset_tracker_continuation_budget(&mut self) {
        self.tracker_continuation_turns = 0;
    }

    /// Apply a live tracker probe to the incomplete-items cache.
    ///
    /// Successful `Complete` probes **clear** the cache so auto-continue stops
    /// after the tracker finishes. `Unavailable` keeps the last value.
    pub(crate) fn apply_tracker_probe(
        &mut self,
        probe: crate::agent::runloop::unified::turn::tool_outcomes::helpers::TrackerProbeOutcome,
    ) -> Option<&[String]> {
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::apply_tracker_probe_to_cache(
            &mut self.last_incomplete_tracker_items,
            probe,
        )
    }

    /// Only new completion progress restores the auto-continue episode.
    /// Recreating a checklist or toggling statuses must not refresh retries.
    pub(crate) fn note_tracker_completed_count(&mut self, completed: u32) -> bool {
        let progressed = completed > self.tracker_completed_count_high_water;
        if progressed {
            self.tracker_completed_count_high_water = completed;
        }
        progressed
    }

    /// Record one plan-mode auto-continue turn against its own budget.
    pub(crate) fn record_plan_continuation_turn_with_limit(&mut self, max_turns: u8) -> bool {
        if self.plan_continuation_turns >= max_turns {
            return false;
        }
        self.plan_continuation_turns = self.plan_continuation_turns.saturating_add(1);
        true
    }

    pub(crate) fn plan_continuation_turns(&self) -> u8 {
        self.plan_continuation_turns
    }

    pub(crate) fn reset_plan_continuation_budget(&mut self) {
        self.plan_continuation_turns = 0;
    }

    /// Record a failed harness auto-verification of `command`, keeping a
    /// bounded tail of its output for the escalated handoff. Returns the new
    /// consecutive-failure count so callers can compare against the
    /// configured escalation threshold without a second accessor call.
    pub(crate) fn record_verification_auto_failure(&mut self, command: String, excerpt: &str) -> u8 {
        const BOUND: usize =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::VERIFICATION_FAILURE_EXCERPT_CHARS;
        // Keep the last BOUND chars (the tail usually holds the error
        // summary) without materializing the full char vector.
        let start = excerpt
            .char_indices()
            .rev()
            .nth(BOUND.saturating_sub(1))
            .map_or(0, |(idx, _)| idx);
        let tail = excerpt[start..].to_string();
        self.verification_consecutive_failures = self.verification_consecutive_failures.saturating_add(1);
        let consecutive_failures = self.verification_consecutive_failures;
        self.last_verification_failure = Some(VerificationFailureSummary {
            command,
            exit_code: serde_json::from_str::<serde_json::Value>(excerpt)
                .ok()
                .and_then(|value| value.get("exit_code").and_then(serde_json::Value::as_i64)),
            excerpt_tail: tail,
            consecutive_failures,
        });
        consecutive_failures
    }

    /// Record a successful harness auto-verification: the suite is green, so
    /// the consecutive-failure count and any stored failure tail are dropped.
    pub(crate) fn record_verification_auto_success(&mut self) {
        self.verification_consecutive_failures = 0;
        self.last_verification_failure = None;
    }

    pub(crate) fn verification_consecutive_failures(&self) -> u8 {
        self.verification_consecutive_failures
    }

    pub(crate) fn last_verification_failure(&self) -> Option<&VerificationFailureSummary> {
        self.last_verification_failure.as_ref()
    }

    /// Clear terminal diagnostics without replenishing request-scoped recovery.
    /// Only genuine fresh input restores the corrective-pass allowance.
    pub(crate) fn reset_verification_recovery_episode(&mut self) {
        self.verification_consecutive_failures = 0;
        self.last_verification_failure = None;
        // Tracker/plan continuation budgets are independent episode counters;
        // do not wipe them here (Completed-turn verification resets must not
        // silently restore full tracker auto-continue budgets).
    }
}
