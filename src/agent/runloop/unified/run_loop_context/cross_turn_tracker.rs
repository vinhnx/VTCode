//! Cross-turn stuck-pattern tracker (zero-mutation, identical shell failures).

use super::*;

pub(crate) struct CrossTurnTracker {
    /// Rolling window of per-turn action fingerprints.
    turn_fingerprints: VecDeque<u64>,
    /// Maximum window size for cross-turn loop detection.
    window_size: usize,
    /// Consecutive turns with no workspace mutation or command execution.
    zero_mutation_turns: usize,
    /// Failure key of the previous turn's shell execution, if it failed.
    last_failed_shell_key: Option<String>,
    /// Consecutive turns repeating the same failed shell key.
    consecutive_same_failed_shell: usize,
}

/// Number of consecutive zero-mutation turns before a HARD STOP fires.
const STUCK_ZERO_MUTATION_THRESHOLD: usize = 3;

/// Consecutive turns repeating an identical shell failure before a warning
/// fires. Three turns (two repeats) confirm the pattern while tolerating a
/// single fix-then-reverify cycle.
const IDENTICAL_SHELL_FAILURE_TURNS_THRESHOLD: usize = 3;

impl CrossTurnTracker {
    pub(crate) fn new() -> Self {
        Self {
            turn_fingerprints: VecDeque::with_capacity(8),
            window_size: 8,
            zero_mutation_turns: 0,
            last_failed_shell_key: None,
            consecutive_same_failed_shell: 0,
        }
    }

    /// Seal the current turn: compute a fingerprint from the provided tool
    /// signatures, check for cross-turn loops and stuck states.
    ///
    /// - `read_only_signatures`: signatures of read-only tool calls this turn.
    /// - `written_files`: paths of files written this turn.
    /// - `shell_command`: last shell command signature, if any.
    /// - `failed_shell_key`: `signature::err::error-signature` of this turn's
    ///   failed shell execution, if any. Identical keys across consecutive
    ///   turns mean the same command fails with an unchanged error — retries
    ///   after a genuine fix change the error or succeed, so they never
    ///   extend the streak.
    /// - `planning_active`: whether the planning workflow is currently active.
    ///
    /// Returns a warning string if a loop or stuck pattern is detected.
    #[allow(
        dead_code,
        reason = "Compatibility wrapper retained for callers using the original tracker API."
    )]
    pub(crate) fn seal_turn(
        &mut self,
        read_only_signatures: &[String],
        written_files: &HashSet<String>,
        shell_command: Option<&str>,
        planning_active: bool,
    ) -> Option<String> {
        self.seal_turn_with_progress(read_only_signatures, written_files, shell_command, None, false, planning_active)
    }

    /// Seal a turn while accounting for productive provider-native tool work
    /// that is not represented by a normal tool-result message.
    pub(crate) fn seal_turn_with_progress(
        &mut self,
        read_only_signatures: &[String],
        written_files: &HashSet<String>,
        shell_command: Option<&str>,
        failed_shell_key: Option<&str>,
        out_of_band_tool_progress: bool,
        planning_active: bool,
    ) -> Option<String> {
        let mut signatures: Vec<String> = read_only_signatures.to_vec();
        for path in written_files {
            signatures.push(format!("write::{path}"));
        }
        if let Some(cmd) = shell_command {
            signatures.push(cmd.to_string());
        }

        let had_execution_progress = !written_files.is_empty() || shell_command.is_some() || out_of_band_tool_progress;

        // Compute fingerprint from sorted signatures so order doesn't matter.
        let fingerprint = if signatures.is_empty() {
            0
        } else {
            let mut sorted: Vec<&str> = signatures.iter().map(String::as_str).collect();
            sorted.sort_unstable();
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for sig in &sorted {
                sig.hash(&mut hasher);
            }
            hasher.finish()
        };

        // Check cross-turn loop before pushing this turn's fingerprint.
        let loop_warning = if fingerprint != 0 && self.turn_fingerprints.contains(&fingerprint) {
            Some(
                "Cross-turn loop detected: the same set of tool actions has repeated across \
                 consecutive turns. Break the pattern by trying a different approach or \
                 synthesizing a final answer from existing context."
                    .to_string(),
            )
        } else {
            None
        };

        if fingerprint != 0 {
            if self.turn_fingerprints.len() >= self.window_size {
                self.turn_fingerprints.pop_front();
            }
            self.turn_fingerprints.push_back(fingerprint);
        }

        // Track zero-mutation turns for stuck detection.
        if had_execution_progress {
            self.zero_mutation_turns = 0;
        } else if !signatures.is_empty() && !planning_active {
            self.zero_mutation_turns = self.zero_mutation_turns.saturating_add(1);
        }

        // Track identical shell failures across turns. Unlike the
        // fingerprint set above, this fires regardless of surrounding
        // variation (different reads between retries must not mask the
        // loop), and only when the error itself is unchanged.
        let identical_failure_warning = match failed_shell_key {
            Some(key) if self.last_failed_shell_key.as_deref() == Some(key) => {
                self.consecutive_same_failed_shell = self.consecutive_same_failed_shell.saturating_add(1);
                (self.consecutive_same_failed_shell >= IDENTICAL_SHELL_FAILURE_TURNS_THRESHOLD).then(|| {
                    format!(
                        "Identical shell failure in {} consecutive turns ({key}). The command fails with an unchanged error, so rerunning it cannot make progress. \
                         Inspect the underlying state the error names (file existence, manifest validity, toolchain availability), fix the root cause, then verify once. \
                         If the error already changed, ignore this warning and continue.",
                        self.consecutive_same_failed_shell,
                    )
                })
            }
            Some(key) => {
                self.last_failed_shell_key = Some(key.to_string());
                self.consecutive_same_failed_shell = 1;
                None
            }
            None => {
                self.last_failed_shell_key = None;
                self.consecutive_same_failed_shell = 0;
                None
            }
        };

        // Return loop warning first (higher priority), then stuck warning.
        if loop_warning.is_some() {
            return loop_warning;
        }

        if identical_failure_warning.is_some() {
            return identical_failure_warning;
        }

        if !planning_active && self.zero_mutation_turns >= STUCK_ZERO_MUTATION_THRESHOLD {
            return Some(format!(
                "No progress detected for {} consecutive turns (all read-only tool calls, \
                 no file mutations or command executions). Synthesize a final answer from \
                 existing context or ask the user for guidance.",
                self.zero_mutation_turns
            ));
        }

        None
    }

    /// Check if the tracker has detected a stuck pattern (for diagnostics).
    #[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
    pub(crate) fn zero_mutation_turns(&self) -> usize {
        self.zero_mutation_turns
    }
}
