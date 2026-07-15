//! Canonical plan-mode enter/exit phrase sets and matchers.
//!
//! Plan-mode intent detection lives in two consumers — the runloop
//! (`detect_planning_intent`) and the Codex app-server bridge
//! (`normalize_planning_input`). Both must agree on which phrases enter,
//! exit, or stay in plan mode, so the phrase literals live here as the single
//! source of truth. Each consumer keeps its own matching strategy (the runloop
//! matches natural-language substrings; the Codex bridge matches bare tokens
//! exactly) but draws the literals from this module so they cannot drift apart.

/// Short imperative commands that exit plan mode and start implementing.
/// Matched exactly (whole-trimmed) by the runloop, and recognized by the Codex
/// bridge as implementation aliases.
pub const EXIT_DIRECT_COMMANDS: &[&str] = &[
    "implement",
    "yes",
    "go",
    "start",
    "approve",
    "approved",
    "implement now",
    "start implementing",
    "start implementation",
    "execute plan",
    "execute the plan",
    "execute this plan",
    "switch to agent mode",
    "exit planning workflow",
    "exit planning workflow and implement",
];

/// Whole-word approval tokens (so "disapprove" does not match "approve").
pub const APPROVAL_WORDS: &[&str] = &["approve", "approved", "lgtm", "accepted", "accept"];

/// Multi-word approval phrases.
pub const APPROVAL_PHRASES: &[&str] = &[
    "approve the plan",
    "approve this plan",
    "approve the proposed plan",
    "looks good",
    "ship it",
    "accept the plan",
    "accept this plan",
];

/// Longer exit trigger phrases (matched as substrings).
pub const EXIT_TRIGGER_PHRASES: &[&str] = &[
    "start implement",
    "start implementation",
    "start implementing",
    "implement now",
    "implement the plan",
    "implement this plan",
    "begin implement",
    "begin implementation",
    "begin coding",
    "proceed to implement",
    "proceed with implementation",
    "proceed to coding",
    "proceed with coding",
    "execute the plan",
    "execute this plan",
    "let s implement",
    "lets implement",
    "go ahead and implement",
    "go ahead and code",
    "ready to implement",
    "start coding",
    "start building",
    "switch to agent mode",
    "exit planning workflow",
    "exit planning workflow and implement",
];

/// Phrases that explicitly keep the user in plan mode (highest priority).
pub const STAY_PHRASES: &[&str] = &[
    "stay in planning workflow",
    "keep in planning workflow",
    "continue planning",
    "keep planning",
    "do not implement",
    "don t implement",
    "not ready to implement",
    "don t exit planning workflow",
    "do not exit planning workflow",
];

/// Phrases that enter plan mode (does not include the `/plan` slash command,
/// which callers check separately).
pub const ENTER_PHRASES: &[&str] = &[
    "make a plan",
    "create a plan",
    "write a plan",
    "come up with a plan",
    "plan this",
    "stay in planning workflow",
    "keep planning",
    "continue planning",
    "before you implement make a plan",
    "before implementing make a plan",
    "outline the implementation plan",
];

/// Assistant cues that indicate a recent implementation prompt.
pub const IMPLEMENTATION_CUES: &[&str] = &[
    "implement this plan",
    "implement the plan",
    "ready to implement",
    "exit planning workflow",
    "execute the plan",
    "switch out of planning workflow",
    "start implementation",
    "start implementing",
    "start coding",
];

/// Aliases that clear the planning-active flag AND switch to execution mode.
/// Used by the Codex bridge's bare-token exact matching. Includes the
/// implementation intents that the runloop recognizes so the two paths cannot
/// drift (previously the bridge silently missed `approve`/`lgtm`/`ship it`).
pub const EXECUTION_MODE_ALIASES: &[&str] = &[
    "implement",
    "continue",
    "go",
    "start",
    "yes",
    "approve",
    "approved",
    "lgtm",
    "accepted",
    "accept",
    "ship it",
    "implement now",
    "start implementing",
    "start implementation",
    "execute plan",
    "execute the plan",
    "execute this plan",
    "switch to agent mode",
    "exit planning workflow and implement",
];

/// Aliases that clear the planning-active flag but are NOT implementation
/// intents — they pass through verbatim rather than being rewritten to the
/// execution prompt.
pub const FLAG_CLEARING_ONLY_ALIASES: &[&str] = &["exit planning workflow"];

/// Normalize user text for plan-mode intent matching: lowercase and replace
/// non-alphanumeric characters with spaces for flexible substring matching.
pub fn normalize_plan_intent(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect()
}

/// Whether the normalized text expresses an exit-and-implement intent.
///
/// Mirrors the runloop's detection priority: direct commands (exact), approval
/// words (whole-word), approval phrases (substring), then trigger phrases
/// (substring). The caller must check [`matches_stay_intent`] first so that
/// stay phrases win.
pub fn matches_exit_intent(normalized: &str) -> bool {
    let trimmed = normalized.trim();
    EXIT_DIRECT_COMMANDS.contains(&trimmed)
        || APPROVAL_WORDS
            .iter()
            .any(|word| normalized.split_whitespace().any(|tok| tok == *word))
        || APPROVAL_PHRASES
            .iter()
            .any(|phrase| normalized.contains(phrase))
        || EXIT_TRIGGER_PHRASES
            .iter()
            .any(|phrase| normalized.contains(phrase))
}

/// Whether the normalized text keeps the user in plan mode (highest priority).
pub fn matches_stay_intent(normalized: &str) -> bool {
    STAY_PHRASES
        .iter()
        .any(|phrase| normalized.contains(phrase))
}

/// Whether the normalized text is an explicit request to enter plan mode
/// (excluding the `/plan` slash command, which callers check separately).
pub fn matches_enter_intent(normalized: &str) -> bool {
    ENTER_PHRASES
        .iter()
        .any(|phrase| normalized.contains(phrase))
}

/// Whether the normalized assistant text contains an implementation prompt cue.
pub fn contains_implementation_cue(normalized: &str) -> bool {
    IMPLEMENTATION_CUES
        .iter()
        .any(|cue| normalized.contains(cue))
}

/// Codex-bridge matcher: bare-token exact match against the execution/flag
/// alias sets (preserves the bridge's verbatim-input contract).
pub fn is_execution_mode_alias(input: &str) -> bool {
    let normalized = input.trim().to_ascii_lowercase();
    EXECUTION_MODE_ALIASES
        .iter()
        .chain(FLAG_CLEARING_ONLY_ALIASES.iter())
        .any(|alias| *alias == normalized)
}

/// Whether a Codex-bridge alias is an implementation intent (rewrite to the
/// execution prompt) vs. a flag-clearing-only alias (pass through verbatim).
pub fn is_implementation_alias(input: &str) -> bool {
    let normalized = input.trim().to_ascii_lowercase();
    EXECUTION_MODE_ALIASES
        .iter()
        .any(|alias| *alias == normalized)
}
