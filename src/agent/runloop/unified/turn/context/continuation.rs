use vtcode_core::llm::provider as uni;

/// Pushed when an interim/status reply (or a recap while tracker steps remain)
/// would otherwise end the turn. Also covers the tracker override path, so it
/// must not claim the reply was specifically a progress update.
pub(super) const AUTONOMOUS_CONTINUE_DIRECTIVE: &str = "The last reply did not finish the task, so the turn continues. \
     Take the next concrete action, then report the result or what is blocking it.";

/// Maximum number of consecutive relaxed continuation decisions before the turn
/// is forced to end. This prevents infinite loops where the model keeps producing
/// continuation-worthy text without making actual progress.
pub(super) const MAX_CONSECUTIVE_RELAXED_CONTINUATIONS: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct InterimTextContinuationDecision {
    pub(super) should_continue: bool,
    pub(super) reason: &'static str,
    pub(super) is_interim_progress: bool,
    pub(super) last_user_follow_up: bool,
    pub(super) recent_tool_activity: bool,
    pub(super) last_user_requested_progressive_work: bool,
    /// True if this continuation decision came from the relaxed path
    /// (recent_tool_activity_relaxed or progressive_relaxed).
    pub(super) is_relaxed_continuation: bool,
}

impl InterimTextContinuationDecision {
    fn with(
        should_continue: bool,
        reason: &'static str,
        is_interim_progress: bool,
        last_user_follow_up: bool,
        recent_tool_activity: bool,
        last_user_requested_progressive_work: bool,
    ) -> Self {
        Self {
            should_continue,
            reason,
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
            is_relaxed_continuation: false,
        }
    }

    fn with_relaxed(
        should_continue: bool,
        reason: &'static str,
        is_interim_progress: bool,
        last_user_follow_up: bool,
        recent_tool_activity: bool,
        last_user_requested_progressive_work: bool,
    ) -> Self {
        Self {
            should_continue,
            reason,
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
            is_relaxed_continuation: true,
        }
    }
}

pub(super) fn evaluate_interim_text_continuation(
    full_auto: bool,
    planning_active: bool,
    history: &[uni::Message],
    text: &str,
    consecutive_relaxed_continuations: u32,
) -> InterimTextContinuationDecision {
    let is_interim_progress = is_interim_progress_update(text);
    let lower = text.to_ascii_lowercase();
    let last_user_follow_up = last_user_message_is_follow_up(history);
    let recent_tool_activity = has_recent_tool_activity(history);
    let last_user_requested_progressive_work = last_user_requested_progressive_work(history);
    let read_only_request = last_user_requested_read_only_answer(history);

    let d = |should_continue: bool, reason: &'static str| {
        InterimTextContinuationDecision::with(
            should_continue,
            reason,
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
        )
    };

    let d_relaxed = |should_continue: bool, reason: &'static str| {
        InterimTextContinuationDecision::with_relaxed(
            should_continue,
            reason,
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
        )
    };

    if planning_active {
        return d(false, "planning_active");
    }

    // Full-auto sessions must not mistake an ordinary progress update for a
    // completed turn. The model often emits a short, non-interim sentence
    // before it makes its next tool call; returning here would hand control
    // back to the TUI input prompt and require a manual `continue`.
    //
    // Keep explicit terminal signals ahead of this autonomous fallback. A
    // response cap and the existing loop guards remain the authoritative
    // bounds when the model keeps producing non-conclusive text.
    if full_auto {
        let asks_user_input = text.contains('?') || contains_user_input_request(&lower);
        if text.trim().is_empty() {
            return d(false, "full_auto_empty_response");
        }
        if asks_user_input {
            return d(false, "full_auto_user_input_handoff");
        }
        // Verification-gate / safety recaps are terminal even in full-auto:
        // autonomous continuation must not race past the anti-blind checkpoint.
        if lower.contains("verification is still pending")
            || lower.contains("unverified assistant responses")
            || vtcode_core::core::agent::completion::tracker_final_text_is_safety_handoff(text)
        {
            return d(false, "full_auto_safety_handoff");
        }
        if has_explicit_blocker(&lower) {
            return d(false, "full_auto_blocked_handoff");
        }
        // Full-auto is an execution policy, not a reason to keep generating
        // after an informational request has been answered. The old fallback
        // below treated every non-keyword response as unfinished work, so a
        // read-only request such as "explore the codebase and summarize" could
        // trigger a second provider call after the answer was already shown.
        // If that follow-up was interrupted, the stream had visible content
        // but no confirmed final response and the turn was reported as a
        // recovery fallback. Decide this from the user's request, not the
        // answer's formatting or vocabulary.
        if read_only_request {
            return d(false, "full_auto_read_only_answer");
        }
        if full_auto_response_is_conclusive(&lower) {
            return d(false, "full_auto_final_completion");
        }
    }

    if !is_interim_progress {
        let not_conclusive = !last_clause_contains_conclusive_marker(&lower);
        let has_relaxed_continuation_intent = has_relaxed_continuation_intent(&lower);
        // Relaxed: if the model just ran tools, or the user asked for progressive work,
        // and the text still contains a continuation-intent clause, treat long
        // analysis text as continuation-worthy even if it exceeded the strict
        // interim-progress shape.
        // However, if the text contains a user-directed question, it's asking for
        // input and should NOT be treated as continuation-worthy. This prevents
        // infinite loops where the model explains blockers and asks the user how
        // to proceed, but the system keeps injecting continue directives.
        let asks_user_question = text.contains('?') || contains_user_input_request(&lower);
        // Also cap relaxed continuations to prevent infinite loops where the model
        // keeps producing continuation-worthy text without progress.
        let relaxed_budget_exhausted = consecutive_relaxed_continuations >= MAX_CONSECUTIVE_RELAXED_CONTINUATIONS;
        if !asks_user_question
            && !relaxed_budget_exhausted
            && not_conclusive
            && has_relaxed_continuation_intent
            && recent_tool_activity
        {
            return d_relaxed(true, "recent_tool_activity_relaxed");
        }
        if !asks_user_question
            && !relaxed_budget_exhausted
            && not_conclusive
            && has_relaxed_continuation_intent
            && last_user_requested_progressive_work
        {
            return d_relaxed(true, "progressive_relaxed");
        }
        if full_auto {
            if relaxed_budget_exhausted {
                return d(false, "full_auto_relaxed_cap");
            }
            return d_relaxed(true, "full_auto_continuation");
        }
        return d(false, "non_interim_text");
    }

    if last_user_follow_up {
        return d(true, "follow_up_prompt");
    }

    if recent_tool_activity {
        return d(true, "recent_tool_activity");
    }

    if last_user_requested_progressive_work {
        return d(true, "progressive_request");
    }

    if full_auto {
        return d(true, "full_auto_continuation");
    }

    d(false, "interactive_mode")
}

/// Budget/recovery phrasing that must not end a tracker-incomplete turn,
/// including tool-call / tool-loop budget vocabulary the model uses in status
/// recaps ("blocked by turn tool budget", "tool loop budget", "read cap").
fn recoverable_tracker_budget_phrasing(lower: &str) -> bool {
    vtcode_core::core::agent::completion::recoverable_status_recap_phrasing(lower)
}

/// True-handoff detector used when `task_tracker` still has incomplete steps.
///
/// Mid-text `?` in status sections and optional-offer closers (`happy to`,
/// `let me know if`, `if you want me to`) are **not** handoffs — only a
/// trailing clarifying question or strong interview/permission phrases end the
/// turn while TODO work remains. Broad mid-text fragments like "can you" /
/// "need your" are omitted so status recaps that mention code or config are
/// not treated as user asks.
fn tracker_incomplete_text_is_user_handoff(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.ends_with('?') {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    const STRONG_HANDOFF: &[&str] = &[
        "please provide",
        "please confirm",
        "please approve",
        "need your approval",
        "need your permission",
        "need your decision",
        "need you to choose",
        "need you to confirm",
        "waiting for your",
        "awaiting your",
        "your choice",
        "your decision",
        "need approval",
        "requires approval",
        "require approval",
        "approval is required",
        "need permission",
        "requires permission",
        "require permission",
        "permission is required",
        "grant permission",
        "authorize this",
        "need a decision",
        "need clarification",
        "waiting for input",
        "awaiting input",
        "waiting on you",
        "how should i proceed",
        "what should i do",
        "what would you like",
        "how would you like",
        "do you want me to",
    ];
    // Clause-start interview asks (not bare mid-text "can you" in status prose).
    const CLAUSE_START_HANDOFF: &[&str] = &["could you ", "can you ", "shall i ", "should i "];
    if STRONG_HANDOFF.iter().any(|pattern| lower.contains(pattern)) {
        return true;
    }
    CLAUSE_START_HANDOFF
        .iter()
        .any(|pattern| lower.trim_start().starts_with(pattern) || lower.contains(&format!("\n{pattern}")))
}

/// Tracker-aware override: when `task_tracker` still has incomplete steps,
/// status-only responses (including "blocked by budget" / "next step on
/// resume" recaps) must not end the turn and nudge the user. Force a
/// non-relaxed continuation unless the text is a true user handoff.
pub(super) fn apply_tracker_continuation_override(
    mut decision: InterimTextContinuationDecision,
    tracker_incomplete: bool,
    planning_active: bool,
    text: &str,
) -> InterimTextContinuationDecision {
    if !tracker_incomplete || planning_active {
        return decision;
    }
    // True safety/permission/credential/verification-gate handoffs always end
    // the turn, even if an earlier full-auto/relaxed path already chose to
    // continue. In-turn tracker continuation must not race past the anti-blind
    // gate the outer loop will not auto-queue.
    if vtcode_core::core::agent::completion::tracker_final_text_is_safety_handoff(text)
        || text.to_ascii_lowercase().contains("verification is still pending")
        || text.to_ascii_lowercase().contains("unverified assistant responses")
    {
        decision.should_continue = false;
        decision.reason = "tracker_safety_handoff";
        decision.is_relaxed_continuation = false;
        return decision;
    }
    if decision.should_continue {
        return decision;
    }
    if tracker_incomplete_text_is_user_handoff(text) {
        return decision;
    }
    let lower = text.to_ascii_lowercase();
    let explicit_safety_handoff = lower.contains("safety fuse")
        || lower.contains("permission denied")
        || lower.contains("access denied")
        || lower.contains("requires manual intervention")
        || lower.contains("missing credentials")
        || lower.contains("credentials are missing")
        || lower.contains("policy block")
        || lower.contains("blocked by policy")
        || lower.contains("denied by policy")
        || lower.contains("denied by tool policy")
        || lower.contains("denied by workspace tool policy");
    if explicit_safety_handoff {
        decision.should_continue = false;
        decision.reason = "tracker_safety_handoff";
        decision.is_relaxed_continuation = false;
        return decision;
    }
    // Recoverable budget/recovery phrasing continues even when the recap also
    // contains blocker tokens like "blocked by".
    if has_explicit_blocker(&lower) && !recoverable_tracker_budget_phrasing(&lower) {
        return decision;
    }
    decision.should_continue = true;
    decision.reason = "tracker_incomplete_continuation";
    decision.is_relaxed_continuation = false;
    decision
}

/// Classify the outcome emitted with the text-response telemetry record.
///
/// The canonical turn events still describe completed and blocked turns. This
/// finer-grained metric makes the continuation decision visible before the
/// outer turn loop finalizes, including the intentional input handoff and the
/// bounded full-auto continuation path.
pub(super) fn continuation_telemetry_outcome(
    full_auto: bool,
    decision: &InterimTextContinuationDecision,
) -> &'static str {
    if decision.should_continue {
        return if full_auto {
            "full_auto_continuation"
        } else {
            "continuation"
        };
    }

    match decision.reason {
        "full_auto_user_input_handoff" => "user_input_handoff",
        "full_auto_relaxed_cap" => "safety_cap_handoff",
        "full_auto_safety_handoff" => "safety_handoff",
        "full_auto_blocked_handoff" | "full_auto_empty_response" => "blocked_handoff",
        _ => "final_completion",
    }
}

pub(super) fn push_system_directive_once(history: &mut Vec<uni::Message>, directive: &str) {
    let current_turn_start = history
        .iter()
        .rposition(|message| message.role == uni::MessageRole::User)
        .unwrap_or(0);
    let already_present = history
        .iter()
        .skip(current_turn_start)
        .any(|message| message.role == uni::MessageRole::System && message.content.as_text().trim() == directive);
    if !already_present {
        history.push(uni::Message::system(directive.to_string()));
    }
}

/// Returns true when the **last** clause (after the final sentence boundary) contains
/// a conclusive marker like "completed", "done", "fixed", "summary", etc.
/// A middle clause with "completed" followed by "Now let me ..." does NOT match —
/// only the final clause determines conclusiveness.
fn last_clause_contains_conclusive_marker(lower: &str) -> bool {
    let conclusive_markers = [
        "completed",
        "done",
        "fixed",
        "resolved",
        "summary",
        "final review",
        "final blocker",
        "next action",
        "what changed",
        "validation",
        "passed",
        "passes",
        "cannot proceed",
        "can't proceed",
        "blocked by",
        "all set",
    ];
    let last_clause = lower
        .char_indices()
        .rfind(|(_, ch)| ['.', '!', '\n', '—', '…'].contains(ch))
        .map(|(idx, ch)| lower[idx + ch.len_utf8()..].trim_start())
        .unwrap_or(lower);
    conclusive_markers.iter().any(|marker| last_clause.contains(marker))
}

/// Full-auto responses need to recognize a conclusive marker even when the
/// final sentence ends with punctuation. The legacy helper intentionally keeps
/// its existing clause behavior for interactive relaxed continuation; this
/// companion handles the terminal full-auto classification without broadening
/// the interactive path.
fn full_auto_response_is_conclusive(lower: &str) -> bool {
    let conclusive_markers = [
        "completed",
        "complete",
        "done",
        "fixed",
        "resolved",
        "summary",
        "result summary",
        "final review",
        "final blocker",
        "next action",
        "what changed",
        "validation",
        "validated",
        "passed",
        "passes",
        "finished",
        "successful",
        "successfully",
        "no issues",
        "no errors",
        "nothing to fix",
        "all set",
        "that's all",
        "that’s all",
    ];
    // Strip terminal sentence/Markdown punctuation before looking for the
    // last clause. This keeps an inline-code `!` from becoming the apparent
    // final clause in text such as ``printing `Hello!`.``.
    let terminal_text =
        lower.trim_end_matches(|ch| ['.', '!', '\n', '—', '…', '`', '*', '_', ')', ']', '}'].contains(&ch));
    let last_non_empty_clause = terminal_text
        .split(|ch| ['.', '!', '\n', '—', '…'].contains(&ch))
        .rfind(|clause| !clause.trim().is_empty())
        .unwrap_or(terminal_text)
        .trim();
    // A future action can mention a successful outcome as part of its plan,
    // e.g. "I'll run the linter again to verify the warnings are resolved."
    // Treat only a final clause that is not itself continuation intent as a
    // completed result.
    let last_clause_has_continuation_intent =
        has_interim_intent_clause(last_non_empty_clause) || has_relaxed_continuation_intent(last_non_empty_clause);
    if !last_clause_has_continuation_intent
        && (last_clause_contains_conclusive_marker(terminal_text)
            || conclusive_markers.iter().any(|marker| last_non_empty_clause.contains(marker)))
    {
        return true;
    }

    let has_summary_heading = lower.lines().any(|line| {
        let line = line.trim().trim_start_matches(['#', '*', '-', '>', ' ']);
        ["summary:", "result:", "results:", "what changed:", "final answer:"]
            .iter()
            .any(|marker| line.starts_with(marker))
    });
    has_summary_heading && !has_interim_intent_clause(lower)
}

fn has_explicit_blocker(lower: &str) -> bool {
    [
        "i'm blocked",
        "im blocked",
        "i’m blocked",
        "i am blocked",
        "is blocked",
        "blocked by",
        "cannot proceed",
        "can't proceed",
        "can’t proceed",
        "unable to proceed",
        "cannot continue",
        "can't continue",
        "can’t continue",
        "unable to continue",
        "cannot complete",
        "can't complete",
        "can’t complete",
        "unable to complete",
        "no access to",
        "permission denied",
        "access denied",
        "missing credentials",
        "credentials are missing",
        "not possible to proceed",
        "requires manual intervention",
        "safety fuse",
        "tool-call safety fuse",
        "policy block",
        "verification is still pending",
        "unverified assistant responses",
        "anti-blind",
        "verification gate",
        "turn blocked after",
    ]
    .iter()
    .any(|pattern| lower.contains(pattern))
}

pub(super) fn is_interim_progress_update(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.len() > 800 {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();
    if !has_interim_intent_clause(&lower) {
        return false;
    }

    if trimmed.contains('?') || contains_user_input_request(&lower) {
        return false;
    }

    !last_clause_contains_conclusive_marker(&lower)
}

fn last_user_message_is_follow_up(history: &[uni::Message]) -> bool {
    history
        .iter()
        .rev()
        .find(|message| message.role == uni::MessageRole::User)
        .is_some_and(|message| {
            crate::agent::runloop::unified::state::is_follow_up_prompt_like(message.content.as_text().as_ref())
        })
}

/// Whether workspace tracker state may drive automatic continuation.
///
/// A fresh informational question (`what is vtcode`) must not be redirected
/// into unrelated edits just because `.vtcode/tasks/current_task.md` has
/// incomplete steps from another session. Require adoption: the current
/// request asked for progressive work, recent tool activity exists in this
/// session, or the last user message is a harness follow-up (`continue`
/// resume). Explicit `--resume` paths bypass this gate (explicit adoption).
pub(crate) fn tracker_continuation_adoption_allowed(history: &[uni::Message]) -> bool {
    last_user_message_is_follow_up(history)
        || has_recent_tool_activity(history)
        || last_user_requested_progressive_work(history)
}

fn has_recent_tool_activity(history: &[uni::Message]) -> bool {
    history.iter().rev().take(16).any(|message| {
        message.role == uni::MessageRole::Tool || message.tool_call_id.is_some() || message.tool_calls.is_some()
    })
}

fn last_user_requested_progressive_work(history: &[uni::Message]) -> bool {
    let Some(text) = last_user_message_text(history) else {
        return false;
    };
    [
        "explore",
        "inspect",
        "look into",
        "investigate",
        "debug",
        "trace",
        "check",
        "review",
        "analy",
        "walk through",
        "run ",
        "execute",
        "format",
        "cargo fmt",
        "cargo check",
        "cargo test",
        "fix",
        "edit",
        "update",
        "change",
        "modify",
        "scan",
        "search",
        "grep",
        "ast-grep",
        "find ",
        "use vt code",
        "semantic code understanding",
        "show me how",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

/// Returns true when the latest user request asks for information or a
/// summary, without also asking the agent to change or verify the workspace.
/// Such requests are terminal once the model has supplied the answer, even in
/// full-auto mode. Action-oriented requests deliberately remain on the
/// autonomous continuation path.
fn last_user_requested_read_only_answer(history: &[uni::Message]) -> bool {
    let Some(text) = last_user_message_text(history) else {
        return false;
    };

    let asks_for_information = [
        "summarize",
        "summary",
        "overview",
        "explain",
        "describe",
        "what is",
        "what's",
        "what are",
        "why ",
        "how does",
        "how do",
        "show me",
        "tell me",
        "compare",
        "explore",
        "walk me through",
    ]
    .iter()
    .any(|needle| text.contains(needle));
    if !asks_for_information {
        return false;
    }

    // Match imperative/action clauses rather than any occurrence of a verb.
    // For example, "How do I fix the parser?" is still an informational
    // question, while "Explore the parser and fix the regression" is an
    // execution request.
    let action_verbs = [
        "fix ",
        "edit ",
        "update ",
        "change ",
        "modify ",
        "create ",
        "implement ",
        "refactor ",
        "write ",
        "delete ",
        "remove ",
        "apply ",
        "run ",
        "execute ",
        "format ",
        "test ",
        "build ",
    ];
    let action_clause = action_verbs
        .iter()
        .any(|verb| text.starts_with(verb) || text.match_indices(verb).any(|(index, _)| is_clause_start(&text, index)));
    let chained_action = action_verbs.iter().any(|verb| {
        [" and ", " then ", "; ", ", "]
            .iter()
            .any(|connector| text.contains(&format!("{connector}{verb}")))
    });

    !(action_clause || chained_action)
}

fn has_interim_intent_clause(lower: &str) -> bool {
    if clause_has_continuation_intent(lower) {
        return true;
    }

    for (idx, ch) in lower.char_indices() {
        if matches!(ch, '.' | '!' | '?' | ':' | ';' | '\n' | '—' | '…') {
            let remainder = lower[idx + ch.len_utf8()..].trim_start();
            if !remainder.is_empty() && clause_has_continuation_intent(remainder) {
                return true;
            }
        }
    }

    false
}

fn has_relaxed_continuation_intent(lower: &str) -> bool {
    if has_interim_intent_clause(lower) {
        return true;
    }

    [
        " let me ",
        " i'll ",
        " i will ",
        " i need to ",
        " i want to ",
        " i'd like to ",
        " next step ",
        " next up:",
        " continuing ",
        " time to ",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// Returns true when the text asks for user input,
/// indicating the model is waiting for input rather than continuing autonomously.
/// This prevents infinite loops where the model explains blockers and asks the
/// user how to proceed, but the system keeps injecting continue directives.
fn contains_user_input_request(lower: &str) -> bool {
    let anywhere_patterns = [
        "please provide",
        "need your",
        "need you to",
        "please confirm",
        "please let me know",
        "waiting for your",
        "awaiting your",
        "your choice",
        "your decision",
        "need approval",
        "need your approval",
        "requires approval",
        "require approval",
        "approval is required",
        "please approve",
        "need permission",
        "need your permission",
        "requires permission",
        "require permission",
        "permission is required",
        "grant permission",
        "authorize this",
        "need a decision",
        "need clarification",
        "waiting for input",
        "awaiting input",
        "waiting on you",
        // Closing offers of optional follow-up work ("…say the word and I'll
        // do a larger pass"): the model has finished and is waiting on the
        // user, so the relaxed continuation paths must not re-prompt past the
        // final answer (checkpoint session-vtcode-20260912T083718Z: a verified
        // recap was continued twice and the turn blocked on the text cap).
        "say the word",
        "tell me and i'll",
        "let me know if you want",
        "if you want me to",
        "just ask",
        "happy to",
    ];
    if anywhere_patterns.iter().any(|pattern| lower.contains(pattern)) {
        return true;
    }

    let clause_start_patterns = [
        "could you",
        "can you",
        "how would you like",
        "what would you like",
        "which option",
        "would you like me to",
        "shall i",
        "do you want me to",
        "should i",
        "how should i proceed",
        "what should i do",
        "how do you want to proceed",
        "how would you like to proceed",
        "what would you prefer",
        "which approach",
        "any suggestions",
        "let me know how",
        "let me know if",
        "let me know what",
        "let me know which",
        "let me know whether",
        "let me know where",
        "let me know when",
        "let me know why",
        "tell me how",
        "tell me if",
        "tell me what",
        "tell me which",
        "tell me whether",
        "tell me where",
        "tell me when",
        "tell me why",
    ];
    clause_start_patterns
        .iter()
        .any(|pattern| contains_phrase_at_clause_start(lower, pattern))
}

fn contains_phrase_at_clause_start(lower: &str, phrase: &str) -> bool {
    lower.match_indices(phrase).any(|(idx, _)| is_clause_start(lower, idx))
}

fn is_clause_start(text: &str, idx: usize) -> bool {
    for ch in text[..idx].chars().rev() {
        if ch == '\n' {
            return true;
        }
        if ch.is_whitespace() {
            continue;
        }
        return matches!(ch, '.' | '!' | '?' | ':' | ';' | ',' | '—' | '…');
    }
    true
}

/// Returns true when a single clause expresses an intent to continue working.
///
/// Instead of matching an ever-growing list of specific prefixes, this
/// normalizes away clause-initial transition words ("now", "next", "then",
/// "first") and subjects ("i", "we"), then checks a compact set of
/// grammatical patterns that capture the underlying linguistic structure
/// of continuation intent.  This handles many more phrasings automatically
/// than explicitly listing every variant.
fn clause_has_continuation_intent(clause: &str) -> bool {
    let clause = clause.trim_start();
    if clause.is_empty() {
        return false;
    }

    let normalized = normalize_clause(clause);
    if normalized.is_empty() {
        return false;
    }

    core_intent_matches(normalized) || starts_with_present_progress_update(normalized)
}

/// Strip leading transition words and subjects to reach the core intent
/// expression.  Reduces hundreds of possible phrasings down to a handful
/// of grammatical patterns checked by [`core_intent_matches`].
///
/// Transitions: "now", "next" (only before i/we), "then", "first"
/// Possessives: "my" (for "my next step is ...")
/// Subjects: "i" (and optionally "am"), "we"
fn normalize_clause(s: &str) -> &str {
    let s = s.trim_start();
    let s = s.strip_prefix("now ").unwrap_or(s);
    let s = s.strip_prefix("then ").unwrap_or(s);
    let s = s.strip_prefix("first, ").unwrap_or(s);
    let s = s.strip_prefix("first ").unwrap_or(s);
    let s = s.strip_prefix("next, ").unwrap_or(s);
    // "next" before a subject is a transition; otherwise it may be
    // part of an intent expression ("next step", "next up").
    let s = match s.strip_prefix("next ") {
        Some(after) if after.starts_with("i ") || after.starts_with("we ") => after,
        _ => s,
    };
    let s = s.strip_prefix("my ").unwrap_or(s);
    let s = s.strip_prefix("i am ").unwrap_or(s);
    // Handle both "i " (uncontracted) and contractions (i'll, i'd, i'm, i've).
    // For contractions we strip only the "i", keeping the apostrophe so that
    // patterns like "'ll " and "'d like to " still match.
    let s = if let Some(after) = s.strip_prefix("i ") {
        after
    } else if let Some(after) = s.strip_prefix("i").filter(|after| after.starts_with('\'')) {
        after
    } else {
        s
    };
    let s = s.strip_prefix("we ").unwrap_or(s);
    s.trim_start()
}

/// Check the core grammatical patterns of continuation intent.
fn core_intent_matches(text: &str) -> bool {
    // "let {me|us|'s}" + action verb
    if text.starts_with("let ") {
        return true;
    }

    // <intent-verb> "to" <action>
    // Covers: need to, want to, going to, plan to, intend to,
    //         have to, 'm going to, 'd like to, hope to, etc.
    const TO_INTENTS: &[&str] = &[
        "need to ",
        "want to ",
        "going to ",
        "plan to ",
        "intend to ",
        "have to ",
        "'m going to ",
        "'d like to ",
        "hope to ",
    ];
    if TO_INTENTS.iter().any(|v| text.starts_with(v)) {
        return true;
    }

    // <modal> <action> — exclude conclusive follow-ups
    if let Some(rest) = text.strip_prefix("will ").or_else(|| text.strip_prefix("'ll ")) {
        return !rest.starts_with("be ") && !rest.starts_with("now be ");
    }

    // Standalone expressions that don't fit the verb patterns above
    if text.starts_with("time to ")
        || text.starts_with("next up:")
        || text.starts_with("next step ")
        || text.starts_with("continuing")
    {
        return true;
    }

    false
}

fn starts_with_present_progress_update(lower: &str) -> bool {
    let present_progress_prefixes = [
        "running ",
        "checking ",
        "formatting ",
        "scanning ",
        "inspecting ",
        "searching ",
        "reading ",
        "reviewing ",
        "tracing ",
        "debugging ",
    ];
    let forward_markers = [
        " now",
        " then ",
        " next ",
        " follow-up",
        " to confirm",
        " to check",
        " to verify",
        " to inspect",
    ];

    present_progress_prefixes.iter().any(|prefix| lower.starts_with(prefix))
        && forward_markers.iter().any(|marker| lower.contains(marker))
}

fn last_user_message_text(history: &[uni::Message]) -> Option<String> {
    history
        .iter()
        .rev()
        .find(|message| message.role == uni::MessageRole::User)
        .map(|message| message.content.as_text().to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::run_loop_context::RecoveryMode;
    use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult};
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

    #[test]
    fn system_directive_is_not_duplicated_after_intervening_messages() {
        let directive = "Synthesize a final response now.";
        let mut history = vec![uni::Message::system(directive.to_string())];
        for index in 0..4 {
            history.push(uni::Message::assistant(format!("intervening message {index}")));
        }

        push_system_directive_once(&mut history, directive);

        assert_eq!(
            history
                .iter()
                .filter(|message| message.role == uni::MessageRole::System && message.content.as_text() == directive)
                .count(),
            1
        );
    }

    #[test]
    fn system_directive_is_reissued_for_a_new_user_turn() {
        let directive = "Synthesize a final response now.";
        let mut history = vec![
            uni::Message::system(directive.to_string()),
            uni::Message::user("first request".to_string()),
        ];

        push_system_directive_once(&mut history, directive);

        assert_eq!(
            history
                .iter()
                .filter(|message| message.role == uni::MessageRole::System && message.content.as_text() == directive)
                .count(),
            2
        );
    }

    #[test]
    fn follow_up_prompt_detection_accepts_continue_variants() {
        assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("continue"));
        assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("continue."));
        assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("go on"));
        assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("please continue"));
        assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like(
            "Continue autonomously from the last stalled turn. Stall reason: x."
        ));
        assert!(!crate::agent::runloop::unified::state::is_follow_up_prompt_like("run cargo clippy and fix"));
    }

    #[test]
    fn tracker_adoption_requires_progressive_work_or_activity() {
        // Fresh informational question with no tool activity must not adopt
        // unrelated workspace tracker work (session-vtcode-20261008T094713Z:
        // `what is vtcode` redirected into README edits).
        let fresh_info = vec![uni::Message::user("what is vtcode".to_string())];
        assert!(!tracker_continuation_adoption_allowed(&fresh_info));
        // Progressive-work request adopts.
        let progressive = vec![uni::Message::user("please fix the README table".to_string())];
        assert!(tracker_continuation_adoption_allowed(&progressive));
        // Recent tool activity in this session adopts.
        let with_tools = vec![
            uni::Message::user("what is vtcode".to_string()),
            uni::Message::assistant_with_tools(
                String::new(),
                vec![vtcode_core::llm::provider::ToolCall::function(
                    "call_1".to_string(),
                    "exec_command".to_string(),
                    "{}".to_string(),
                )],
            ),
        ];
        assert!(tracker_continuation_adoption_allowed(&with_tools));
        // Explicit follow-up adopts.
        let follow_up = vec![uni::Message::user("continue".to_string())];
        assert!(tracker_continuation_adoption_allowed(&follow_up));
    }

    #[test]
    fn interim_progress_detection_requires_non_conclusive_intent_text() {
        assert!(is_interim_progress_update("Let me fix the second collapsible if statement:"));
        assert!(is_interim_progress_update(
            "Let me fix the second collapsible if statement in the Anthropic provider:"
        ));
        assert!(is_interim_progress_update(
            "Now I need to update the function body to use settings.reasoning_effort and settings.verbosity:"
        ));
        assert!(is_interim_progress_update("I'll continue with the next fix."));
        assert!(is_interim_progress_update(
            "Running formatter now, then I'll do a quick follow-up check (`cargo check`) to confirm nothing regressed."
        ));
        assert!(is_interim_progress_update(
            "The structural search keeps returning empty results. Let me verify the indexer is working and try with a simpler known pattern:"
        ));
        assert!(!is_interim_progress_update("I need you to choose which option to apply."));
        assert!(!is_interim_progress_update("Let me know what you'd like to dig into next."));
        assert!(!is_interim_progress_update("Running cargo fmt uses rustfmt to rewrite the source files."));
        assert!(!is_interim_progress_update("Completed. All requested fixes are done."));
        assert!(!is_interim_progress_update("Final review: two blockers remain with next action."));
    }

    #[test]
    fn autonomous_continue_triggers_for_follow_up_and_interim_text() {
        let history = vec![uni::Message::user("continue".to_string())];
        assert!(
            evaluate_interim_text_continuation(true, false, &history, "Let me fix the next issue.", 0).should_continue
        );
        assert!(
            !evaluate_interim_text_continuation(true, true, &history, "Let me fix the next issue.", 0).should_continue
        );
        assert!(
            evaluate_interim_text_continuation(false, false, &history, "Let me fix the next issue.", 0).should_continue
        );
    }

    #[test]
    fn autonomous_continue_triggers_for_interim_text_after_tool_activity() {
        let history = vec![
            uni::Message::user("run cargo clippy and fix".to_string()),
            uni::Message::assistant("I will run cargo clippy now.".to_string()).with_tool_calls(vec![
                uni::ToolCall::function("call_1".to_string(), "command_session".to_string(), "{}".to_string()),
            ]),
            uni::Message::tool_response("call_1".to_string(), "warning: ...".to_string()),
        ];

        assert!(
            evaluate_interim_text_continuation(
                true,
                false,
                &history,
                "Now I need to update the function body to use settings.reasoning_effort and settings.verbosity:",
                0
            )
            .should_continue
        );
    }

    #[test]
    fn autonomous_continue_triggers_for_execution_request_without_prior_tool_activity() {
        let history = vec![
            uni::Message::user("run cargo clippy and fix".to_string()),
            uni::Message::assistant("I will start now.".to_string()),
        ];

        assert!(
            evaluate_interim_text_continuation(
                true,
                false,
                &history,
                "Now I need to update the function body to use settings.reasoning_effort and settings.verbosity:",
                0
            )
            .should_continue
        );
    }

    #[test]
    fn full_auto_continues_after_non_conclusive_text_without_tool_activity() {
        let history = vec![uni::Message::user("work through the requested task".to_string())];
        let decision = evaluate_interim_text_continuation(
            true,
            false,
            &history,
            "I reviewed the request and have more work to do.",
            0,
        );

        assert!(decision.should_continue);
        assert_eq!(decision.reason, "full_auto_continuation");
        assert!(decision.is_relaxed_continuation);
    }

    #[test]
    fn full_auto_continues_after_inspect_fix_and_check_updates() {
        let history = vec![uni::Message::user("complete the requested work".to_string())];
        for text in [
            "I'll inspect the remaining files now.",
            "I'll fix the parser branch next.",
            "I'll check the targeted test output before summarizing.",
        ] {
            let decision = evaluate_interim_text_continuation(true, false, &history, text, 0);
            assert!(decision.should_continue, "expected continuation for {text:?}");
        }
    }

    #[test]
    fn full_auto_stops_after_a_read_only_summary_request_without_keyword_markers() {
        let history = vec![uni::Message::user(
            "Explore the codebase and summarize what makes this project special.".to_string(),
        )];
        let answer = "Here's a consolidated table of the project's differentiators:\n\n| Area | Meaning |\n|---|---|\n| Runtime | The harness coordinates tools and context. |\n| Safety | Commands are checked before execution. |";

        let decision = evaluate_interim_text_continuation(true, false, &history, answer, 0);

        assert!(!decision.should_continue);
        assert_eq!(decision.reason, "full_auto_read_only_answer");
    }

    #[test]
    fn full_auto_keeps_action_requests_on_the_autonomous_path() {
        let history = vec![uni::Message::user(
            "Explore the parser, summarize the issue, and fix the regression.".to_string(),
        )];

        let decision = evaluate_interim_text_continuation(true, false, &history, "I reviewed the request.", 0);

        assert!(decision.should_continue);
        assert_eq!(decision.reason, "full_auto_continuation");
    }

    #[test]
    fn full_auto_stops_for_an_informational_how_to_fix_question() {
        let history = vec![uni::Message::user("How do I fix the parser regression?".to_string())];
        let decision = evaluate_interim_text_continuation(
            true,
            false,
            &history,
            "The parser loses the trailing token when the input ends with a delimiter.",
            0,
        );

        assert!(!decision.should_continue);
        assert_eq!(decision.reason, "full_auto_read_only_answer");
    }

    #[tokio::test]
    async fn read_only_summary_is_published_as_final_without_a_follow_up_directive() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.full_auto = true;
        ctx.working_history.push(uni::Message::user(
            "Explore the codebase and summarize what makes this project special.".to_string(),
        ));

        let outcome = ctx
            .handle_text_response(
                "Here's a consolidated table of the project's differentiators:\n\n| Area | Meaning |\n|---|---|\n| Runtime | The harness coordinates tools and context. |\n| Safety | Commands are checked before execution. |"
                    .to_string(),
                Vec::new(),
                None,
                None,
                true,
            )
            .await
            .expect("read-only summary should complete");

        assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
        assert!(!ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().contains(AUTONOMOUS_CONTINUE_DIRECTIVE)
        }));
        assert_eq!(
            ctx.working_history
                .iter()
                .rev()
                .find(|message| message.role == uni::MessageRole::Assistant)
                .and_then(|message| message.phase),
            Some(uni::AssistantPhase::FinalAnswer)
        );
    }

    #[test]
    fn full_auto_stops_on_completion_summaries() {
        let history = vec![uni::Message::user("finish the requested task".to_string())];
        for text in [
            "Completed the requested changes. All targeted tests passed.",
            "Summary:\n- Updated the parser and verified the targeted test.",
            "The result is complete.",
        ] {
            let decision = evaluate_interim_text_continuation(true, false, &history, text, 0);
            assert!(!decision.should_continue, "expected terminal completion for {text:?}");
            assert_eq!(decision.reason, "full_auto_final_completion");
        }
    }

    #[test]
    fn full_auto_stops_on_questions_approval_requests_and_blockers() {
        let history = vec![uni::Message::user("complete the requested task".to_string())];
        let cases = [
            ("Should I continue with the migration?", "full_auto_user_input_handoff"),
            ("I need your approval before I proceed.", "full_auto_user_input_handoff"),
            // Credentials are a true safety handoff (S2A): the shared
            // `tracker_final_text_is_safety_handoff` classifier wins over the
            // generic blocker bucket, so the turn ends as a safety handoff
            // rather than a plain blocked handoff.
            ("I’m blocked by missing credentials.", "full_auto_safety_handoff"),
        ];

        for (text, reason) in cases {
            let decision = evaluate_interim_text_continuation(true, false, &history, text, 0);
            assert!(!decision.should_continue, "expected terminal handoff for {text:?}");
            assert_eq!(decision.reason, reason);
        }
    }

    #[test]
    fn interactive_mode_keeps_non_conclusive_text_terminal() {
        let history = vec![uni::Message::user("work through the requested task".to_string())];
        let decision = evaluate_interim_text_continuation(
            false,
            false,
            &history,
            "I reviewed the request and have more work to do.",
            0,
        );

        assert!(!decision.should_continue);
        assert_eq!(decision.reason, "non_interim_text");
    }

    #[test]
    fn continuation_telemetry_distinguishes_terminal_paths() {
        let history = vec![uni::Message::user("complete the requested task".to_string())];
        let continuing = evaluate_interim_text_continuation(true, false, &history, "I reviewed the request.", 0);
        assert_eq!(continuation_telemetry_outcome(true, &continuing), "full_auto_continuation");

        let completed = evaluate_interim_text_continuation(true, false, &history, "The result is complete.", 0);
        assert_eq!(continuation_telemetry_outcome(true, &completed), "final_completion");

        let input = evaluate_interim_text_continuation(true, false, &history, "Should I continue?", 0);
        assert_eq!(continuation_telemetry_outcome(true, &input), "user_input_handoff");

        let capped = evaluate_interim_text_continuation(true, false, &history, "I have more work to do.", 3);
        assert_eq!(capped.reason, "full_auto_relaxed_cap");
        assert_eq!(continuation_telemetry_outcome(true, &capped), "safety_cap_handoff");

        let blocked = evaluate_interim_text_continuation(true, false, &history, "I cannot proceed without access.", 0);
        assert_eq!(continuation_telemetry_outcome(true, &blocked), "blocked_handoff");

        // Safety/verification handoffs must not be counted as completions.
        let safety = evaluate_interim_text_continuation(
            true,
            false,
            &history,
            "Verification is still pending; run cargo check next.",
            0,
        );
        assert_eq!(safety.reason, "full_auto_safety_handoff");
        assert_eq!(continuation_telemetry_outcome(true, &safety), "safety_handoff");
    }

    #[test]
    fn autonomous_continue_triggers_for_exploration_request_without_full_auto() {
        let history = vec![
            uni::Message::user("explore about vtcode core agent loop".to_string()),
            uni::Message::assistant("I can help.".to_string()),
        ];

        assert!(evaluate_interim_text_continuation(
            false,
            false,
            &history,
            "I'll quickly inspect the actual vtcode-core runloop files and then summarize the core agent loop concretely from code.",
            0
        )
        .should_continue);
    }

    #[test]
    fn autonomous_continue_does_not_trigger_for_explanatory_request_without_full_auto() {
        let history = vec![
            uni::Message::user("tell me about core agent loop".to_string()),
            uni::Message::assistant("I can help.".to_string()),
        ];

        assert!(!evaluate_interim_text_continuation(
            false,
            false,
            &history,
            "I'll quickly inspect the actual vtcode-core runloop files and then summarize the core agent loop concretely from code.",
            0
        )
        .should_continue);
    }

    #[tokio::test]
    async fn recovery_pass_progress_only_text_completes_turn() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let outcome = ctx
            .handle_text_response("Let me try a narrower search next.".to_string(), Vec::new(), None, None, false)
            .await
            .expect("recovery response should be handled");

        assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
        assert!(!ctx.is_recovery_active());
    }

    #[tokio::test]
    async fn recovery_pass_diagnostic_then_next_step_text_completes_turn() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("turn balancer");
        assert!(ctx.consume_recovery_pass());

        let outcome = ctx
            .handle_text_response(
                "The structural search keeps returning empty results. Let me verify the indexer is working and try with a simpler known pattern:".to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("recovery response should be handled");

        assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
        assert!(!ctx.is_recovery_active());
    }

    #[tokio::test]
    async fn tool_free_recovery_continuation_intent_text_is_terminal() {
        // Regression guard for the post-tool follow-up infinite loop.
        // During tool-free recovery, even when the text expresses continuation
        // intent AND there is recent tool activity (the exact combination that
        // previously produced a non-relaxed `Continue` via "recent_tool_activity",
        // resetting `consecutive_relaxed_continuations` to 0 and re-enabling
        // tools after `finish_recovery_pass()`), the turn must end. The recovery
        // text IS the final answer; allowing continuation re-enables tools and
        // re-triggers recovery when the follow-up fails again — an infinite
        // cycle no existing bound catches.
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.working_history
            .push(uni::Message::user("run cargo nextest and summarize".to_string()));
        ctx.working_history
            .push(uni::Message::assistant(String::new()).with_tool_calls(vec![uni::ToolCall::function(
                "call_1".to_string(),
                "command_session".to_string(),
                "{}".to_string(),
            )]));
        ctx.working_history
            .push(uni::Message::tool_response("call_1".to_string(), "test result: ok".to_string()));
        ctx.activate_recovery("post-tool follow-up failure");
        assert!(ctx.consume_recovery_pass());
        assert!(ctx.recovery_is_tool_free());

        // Sanity: without recovery this exact text+history would continue.
        // "Let me continue analyzing the results." is interim progress
        // (<=800 chars, has "let me" intent clause, no question, no
        // conclusive marker) and recent_tool_activity is true → the raw
        // evaluator returns should_continue=true, is_relaxed_continuation=false.
        assert!(
            evaluate_interim_text_continuation(
                true,
                false,
                ctx.working_history,
                "Let me continue analyzing the results.",
                0,
            )
            .should_continue
        );

        // Under tool-free recovery, the turn loop must override to terminal.
        let outcome = ctx
            .handle_text_response("Let me continue analyzing the results.".to_string(), Vec::new(), None, None, false)
            .await
            .expect("recovery response should be handled");

        assert!(
            matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })),
            "tool-free recovery text with continuation intent must end the turn, \
             not re-enable tools and loop"
        );
        // Recovery is finished (not active), and the turn did not continue.
        assert!(!ctx.is_recovery_active());
        assert!(!ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().trim() == AUTONOMOUS_CONTINUE_DIRECTIVE
        }));
    }

    #[test]
    fn detects_new_intent_prefixes_as_interim() {
        assert!(is_interim_progress_update("I'd like to check the next file before proceeding."));
        assert!(is_interim_progress_update("I want to verify the output of the previous step."));
        assert!(is_interim_progress_update("My next step is to run the full test suite."));
        assert!(is_interim_progress_update("Time to fix the remaining lint warnings."));
        assert!(is_interim_progress_update("Let me now inspect the second module for regressions."));
    }

    #[test]
    fn em_dash_boundary_detected_as_interim() {
        assert!(is_interim_progress_update("First check passed—now let me verify the second component."));
        assert!(has_interim_intent_clause("done with the first task—now i'll move to the next"));
    }

    #[test]
    fn ellipsis_boundary_detected_as_interim() {
        assert!(has_interim_intent_clause("checking results…now i need to update the config"));
        assert!(is_interim_progress_update("Scanning the logs…now let me check for error patterns."));
    }

    #[test]
    fn long_text_with_progressive_request_continues_via_relaxed_path() {
        // User asked for progressive work ("fix"), model responds with long analysis
        // (> 800 chars, non-interim pattern), no tool activity.
        // Should continue via "progressive_relaxed" path.
        let long_analysis_base = "The root cause of this issue is a race condition in the connection \
            pool initialization. When multiple threads attempt to acquire connections \
            simultaneously, the pool's internal counter can overflow. This happens because \
            the increment operation is not atomic. The fix should wrap the counter update \
            in a mutex lock. Additionally, we should consider using atomic operations for \
            better performance. ";
        // Pad to exceed 800 chars
        let padding = "x".repeat(820usize.saturating_sub(long_analysis_base.len()));
        let long_text = format!(
            "{long_analysis_base}{padding} Let me now implement the mutex-based fix in the connection pool module."
        );
        assert!(long_text.len() > 800, "test text must exceed 800-char limit to trigger relaxed path");

        let history = vec![
            uni::Message::user("fix the race condition in connection pool".to_string()),
            uni::Message::assistant("I'll look into it.".to_string()),
        ];

        // Without tool activity, the text is > 800 chars → not interim → hits relaxed path
        // last_user_requested_progressive_work is true → should continue
        assert!(evaluate_interim_text_continuation(false, false, &history, &long_text, 0).should_continue);
    }

    #[test]
    fn long_text_with_tool_activity_and_not_conclusive_continues() {
        let long_analysis = "I've reviewed the output from the linter. There are several \
            warnings in the networking module. The main issues are unused imports and \
            a potential memory leak in the connection handler. The fixes are \
            straightforward: remove the unused imports and add proper cleanup in the \
            deinit method. Let me apply these changes to the affected files now. \
            Starting with the networking module, I'll remove the unused imports and \
            then fix the memory leak in the connection handler. After that, I'll \
            run the linter again to verify the warnings are resolved.";
        assert!(long_analysis.len() > 280, "test text must exceed original 280-char limit");

        let history =
            vec![
                uni::Message::user("run cargo clippy and fix warnings".to_string()),
                uni::Message::assistant("Running clippy now.".to_string()).with_tool_calls(vec![
                    uni::ToolCall::function("call_1".to_string(), "command_session".to_string(), "{}".to_string()),
                ]),
                uni::Message::tool_response("call_1".to_string(), "warning: ...".to_string()),
            ];

        assert!(evaluate_interim_text_continuation(true, false, &history, long_analysis, 0).should_continue);
    }

    #[test]
    fn short_completion_after_tool_activity_does_not_continue_via_relaxed_path() {
        let history = vec![
            uni::Message::user("create a simple rust hello world program".to_string()),
            uni::Message::assistant("Let me compile and run it to confirm it works:".to_string()).with_tool_calls(
                vec![uni::ToolCall::function(
                    "call_1".to_string(),
                    "command_session".to_string(),
                    "{}".to_string(),
                )],
            ),
            uni::Message::tool_response("call_1".to_string(), "Hello, World!".to_string()),
        ];

        assert!(
            !evaluate_interim_text_continuation(
                true,
                false,
                &history,
                "It works! The program compiled and ran successfully, printing `Hello, World!`.",
                0
            )
            .should_continue
        );
    }

    #[test]
    fn short_completion_after_progressive_request_does_not_continue_via_relaxed_path() {
        let history = vec![
            uni::Message::user("fix the parser regression".to_string()),
            uni::Message::assistant("I'll inspect the parser.".to_string()),
        ];

        assert!(
            !evaluate_interim_text_continuation(
                false,
                false,
                &history,
                "I updated the parser logic and the targeted regression test now passes.",
                0
            )
            .should_continue
        );
    }

    #[tokio::test]
    async fn tool_enabled_recovery_pass_can_continue_after_interim_progress() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.working_history
            .push(uni::Message::user("run cargo fmt and follow up".to_string()));
        ctx.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);
        assert!(ctx.consume_recovery_pass());

        let outcome = ctx
            .handle_text_response(
                "Running formatter now, then I'll do a quick follow-up check (`cargo check`) to confirm nothing regressed."
                    .to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("tool-enabled recovery response should be handled");

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active());
        assert!(ctx.recovery_pass_used());
    }

    #[tokio::test]
    async fn continuing_text_response_is_recorded_as_commentary_phase() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.working_history
            .push(uni::Message::user("fix the parser regression".to_string()));

        let outcome = ctx
            .handle_text_response(
                "Now I need to update the parser branch and rerun the targeted test.".to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("continuing text response should be handled");

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        let last_assistant = ctx
            .working_history
            .iter()
            .rev()
            .find(|message| message.role == uni::MessageRole::Assistant)
            .expect("assistant message should be recorded");
        assert_eq!(last_assistant.phase, Some(uni::AssistantPhase::Commentary));
    }

    #[tokio::test]
    async fn completed_text_response_is_recorded_as_final_answer_phase() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.working_history
            .push(uni::Message::user("explain the core loop".to_string()));

        let outcome = ctx
            .handle_text_response(
                "The core loop requests model output, dispatches tool calls, and ends once a final textual answer is produced."
                    .to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("completed text response should be handled");

        assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
        let last_assistant = ctx
            .working_history
            .iter()
            .rev()
            .find(|message| message.role == uni::MessageRole::Assistant)
            .expect("assistant message should be recorded");
        assert_eq!(last_assistant.phase, Some(uni::AssistantPhase::FinalAnswer));
    }

    #[test]
    fn relaxed_continuation_does_not_trigger_for_user_directed_questions() {
        // This is the exact scenario from the infinite loop bug:
        // Agent tries to fetch a URL, both tools fail, agent explains blockers
        // and asks "How would you like to proceed?" - this should NOT trigger
        // the relaxed continuation path.
        let blocker_explanation = "I don't have a direct web-fetch tool available in this session. \
            My available tools are scoped to local file/code operations, and `exec_command` \
            requires explicit safety approval \
            for outbound network requests. A few options: 1. Approve the network call 2. Use a \
            subagent 3. Paste the content. How would you like to proceed? If you want me to fetch \
            it, please confirm and I'll retry with the appropriate sandbox permission.";
        let history = vec![
            uni::Message::user("can you fetch https://www.google.com.vn/".to_string()),
            uni::Message::assistant("I'll try to fetch it.".to_string()).with_tool_calls(vec![
                uni::ToolCall::function("call_1".to_string(), "exec_command".to_string(), "{}".to_string()),
            ]),
            uni::Message::tool_response("call_1".to_string(), "Tool preflight validation failed".to_string()),
        ];

        // Should NOT continue - the model is asking the user a question
        assert!(
            !evaluate_interim_text_continuation(true, false, &history, blocker_explanation, 0).should_continue,
            "User-directed questions should not trigger relaxed continuation"
        );
    }

    #[test]
    fn relaxed_continuation_does_not_trigger_for_first_turn_handoff_offer() {
        let repo_overview = checkpoint_shaped_repo_overview();
        let history = vec![
            uni::Message::user("what's in this repo?".to_string()),
            uni::Message::assistant(String::new()).with_tool_calls(vec![uni::ToolCall::function(
                "call_1".to_string(),
                "exec_command".to_string(),
                "{}".to_string(),
            )]),
            uni::Message::tool_response("call_1".to_string(), "README summary".to_string()),
        ];

        let decision = evaluate_interim_text_continuation(true, false, &history, repo_overview, 0);

        assert!(
            !decision.should_continue,
            "handoff offers after answering a first-turn question should end the turn"
        );
    }

    #[tokio::test]
    async fn first_turn_handoff_offer_completes_without_continue_directive() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.push(uni::Message::user("what's in this repo?".to_string()));
        ctx.working_history
            .push(uni::Message::assistant(String::new()).with_tool_calls(vec![uni::ToolCall::function(
                "call_1".to_string(),
                "file_operation".to_string(),
                "{}".to_string(),
            )]));
        ctx.working_history
            .push(uni::Message::tool_response("call_1".to_string(), "README summary".to_string()));

        let outcome = ctx
            .handle_text_response(checkpoint_shaped_repo_overview().to_string(), Vec::new(), None, None, false)
            .await
            .expect("repo overview response should be handled");

        assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
        let last_assistant = ctx
            .working_history
            .iter()
            .rev()
            .find(|message| message.role == uni::MessageRole::Assistant)
            .expect("assistant message should be recorded");
        assert_eq!(last_assistant.phase, Some(uni::AssistantPhase::FinalAnswer));
        assert!(!ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().trim() == AUTONOMOUS_CONTINUE_DIRECTIVE
        }));
    }

    fn checkpoint_shaped_repo_overview() -> &'static str {
        "Here's a quick overview of the repo:\n\n\
        **VT Code** — a Rust coding agent for long-running autonomous workflows, with OS-native sandboxing, multi-provider LLM support, open protocols, and extensible Skills.\n\n\
        **Layout**\n\
        - Root crate `vtcode` (binary) + workspace of ~30 member crates under `vtcode-*` (e.g. `vtcode-core`, `vtcode-ui`, `vtcode-llm`, `vtcode-mcp`, `vtcode-safety`, `vtcode-exec-events`, `vtcode-indexer`, `vtcode-skills`, `vtcode-config`, `vtcode-a2a`, `vtcode-acp`, etc.)\n\
        - Rust stable, MSRV 1.98.1, edition 2024; CI runs `RUSTFLAGS=\"-D warnings\"` with `--locked`\n\
        - `default-members` = root, `vtcode-core`, `vtcode-ui`\n\n\
        **Capabilities**\n\
        - Agent runtime: interactive TUI, slash commands, streaming, `ask`/`exec` CLI, session resume\n\
        - Coding tools: safe file ops, ripgrep search, fuzzy discovery, code intelligence, project indexing, terminal execution\n\
        - Extensibility: Agent Skills, MCP client/server, lifecycle hooks, subagents, custom providers, Zed ACP, Claude Code\n\
        - Model providers: 21+ LLMs (Anthropic, OpenAI, Gemini, OpenRouter, Ollama, LM Studio, etc.)\n\
        - Safety: restricted shell sandbox, tool guardrails, subprocess isolation, full audit logging\n\
        - Protocols: Open Responses, Agent2Agent (A2A), ATIF, Anthropic Messages API\n\n\
        **Default model**: MiMo V2.6 Pro (Xiaomi), 1M-token context (`mimo-v2.6-pro`).\n\n\
        **Other top-level dirs**: `docs/`, `plans/`, `rules/`, `examples/`, `tests/`, `evals/`, `fuzz/`, `scripts/`, `crates/codegen/xtask/`, `homebrew/`.\n\n\
        Quick start:\n\
        ```shell\n\
        curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash\n\
        vtcode init\n\
        vtcode\n\
        ```\n\n\
        Let me know what you'd like to dig into next — a specific crate, the agent loop, the TUI, sandboxing, or something else."
    }

    #[test]
    fn relaxed_continuation_still_works_for_genuine_interim_progress() {
        // Legitimate interim progress should still trigger continuation
        let interim_text = "I've reviewed the linter output. There are several warnings. \
            Let me apply these changes to the affected files now. Starting with the networking \
            module, I'll remove the unused imports and then fix the memory leak.";
        let history =
            vec![
                uni::Message::user("run cargo clippy and fix warnings".to_string()),
                uni::Message::assistant("Running clippy now.".to_string()).with_tool_calls(vec![
                    uni::ToolCall::function("call_1".to_string(), "command_session".to_string(), "{}".to_string()),
                ]),
                uni::Message::tool_response("call_1".to_string(), "warning: ...".to_string()),
            ];

        // Should continue - this is genuine interim progress with no user question
        assert!(
            evaluate_interim_text_continuation(true, false, &history, interim_text, 0).should_continue,
            "Genuine interim progress should still trigger continuation"
        );
    }

    #[test]
    fn relaxed_continuation_stops_after_budget_exhausted() {
        // After MAX_CONSECUTIVE_RELAXED_CONTINuations, relaxed path should stop.
        // Use a long text (> 800 chars) to trigger the relaxed path (not interim progress).
        let long_analysis = "I've reviewed the output from the linter. There are several \
            warnings in the networking module. The main issues are unused imports and \
            a potential memory leak in the connection handler. The fixes are \
            straightforward: remove the unused imports and add proper cleanup in the \
            deinit method. Let me apply these changes to the affected files now. \
            Starting with the networking module, I'll remove the unused imports and \
            then fix the memory leak in the connection handler. After that, I'll \
            run the linter again to verify the warnings are resolved. The key changes \
            involve updating the connection pool initialization and adding proper \
            resource cleanup in the deinitialization path. I will also review the \
            authentication module for similar issues and ensure all error paths are \
            properly handled with appropriate cleanup routines to prevent resource leaks.";
        assert!(long_analysis.len() > 800, "test text must exceed 800-char limit to trigger relaxed path");
        let history =
            vec![
                uni::Message::user("run cargo clippy and fix warnings".to_string()),
                uni::Message::assistant("Running clippy now.".to_string()).with_tool_calls(vec![
                    uni::ToolCall::function("call_1".to_string(), "command_session".to_string(), "{}".to_string()),
                ]),
                uni::Message::tool_response("call_1".to_string(), "warning: ...".to_string()),
            ];

        // Should continue with budget = 0
        assert!(evaluate_interim_text_continuation(true, false, &history, long_analysis, 0).should_continue);

        // Should continue with budget = 1 (still under limit)
        assert!(evaluate_interim_text_continuation(true, false, &history, long_analysis, 1).should_continue);

        // Should continue with budget = 2 (still under limit)
        assert!(evaluate_interim_text_continuation(true, false, &history, long_analysis, 2).should_continue);

        // Should NOT continue with budget = 3 (at limit)
        assert!(
            !evaluate_interim_text_continuation(true, false, &history, long_analysis, 3).should_continue,
            "Relaxed continuation should stop after MAX_CONSECUTIVE_RELAXED_CONTINuations"
        );

        // Should NOT continue with budget = 4 (over limit)
        assert!(!evaluate_interim_text_continuation(true, false, &history, long_analysis, 4).should_continue);
    }

    #[test]
    fn contains_user_input_request_detects_various_patterns() {
        assert!(contains_user_input_request("how would you like to proceed?"));
        assert!(contains_user_input_request("what would you like me to do?"));
        assert!(contains_user_input_request("which option should i choose?"));
        assert!(contains_user_input_request("shall i continue?"));
        assert!(contains_user_input_request("do you want me to retry?"));
        assert!(contains_user_input_request("please confirm and i'll retry"));
        assert!(contains_user_input_request("let me know how to proceed"));
        assert!(contains_user_input_request("let me know what you'd like to dig into next"));
        assert!(contains_user_input_request("tell me which area you want next"));
        assert!(contains_user_input_request("waiting for your approval"));
        assert!(!contains_user_input_request(
            "The compiler errors tell me what to fix next. Let me update the parser branch now."
        ));
        assert!(!contains_user_input_request("Let me check whether this should initialize the cache before use."));
        assert!(!contains_user_input_request("let me fix the next issue"));
        assert!(!contains_user_input_request("i'll apply these changes now"));
    }

    #[test]
    fn contains_user_input_request_detects_closing_offers() {
        assert!(contains_user_input_request(
            "verified and done. if you want a deeper pass, say the word and i'll do a larger pass."
        ));
        assert!(contains_user_input_request("all checks pass; happy to iterate further."));
        assert!(contains_user_input_request("the docs are updated — just ask if you want the full diff."));
    }

    #[test]
    fn verified_recap_with_trailing_offer_does_not_continue() {
        // Regression guard for session-vtcode-20260912T083718Z: a conclusive
        // verification recap that closes by offering optional follow-up work
        // ("say the word and I'll do a larger pass") was relaxed-continued,
        // re-prompted into a second recap, and the turn ended Blocked on the
        // text cap after the verification gate had already cleared.
        let mut recap = String::from(
            "Verification passed: `cargo fmt --all -- --check && cargo check --locked -p vtcode` → exit 0.\n\n\
             **Recap**\n\n\
             **Changed (README.md):** tightened Overview prose, sharpened the pillar table, \
             streamlined Quick start phrasing, condensed the Documentation table, compacted the \
             Providers paragraph, and fixed a hyphen→em-dash in the TUI tip. Contributor and \
             sponsor HTML blocks preserved verbatim.\n\n\
             **Verified:** all referenced doc paths exist; the fmt and check commands exit 0.\n\n",
        );
        while recap.len() <= 800 {
            recap.push_str("Extra verified detail line that keeps the recap above the interim length cap.\n");
        }
        recap.push_str(
            "**Remaining risk:** none functional. If you want a deeper revamp (restructured \
             sections, feature highlights, refreshed screenshots), say the word and I'll do a \
             larger pass.",
        );
        assert!(recap.len() > 800, "recap must exceed the interim length cap to reach the relaxed path");

        let history = vec![
            uni::Message::user("polish the README intro".to_string()),
            uni::Message::assistant(String::new()).with_tool_calls(vec![uni::ToolCall::function(
                "call_1".to_string(),
                "exec_command".to_string(),
                "{}".to_string(),
            )]),
            uni::Message::tool_response("call_1".to_string(), "Finished `dev` profile".to_string()),
        ];

        assert!(
            !evaluate_interim_text_continuation(true, false, &history, &recap, 0).should_continue,
            "a conclusive recap closing with an optional-work offer must end the turn"
        );
    }

    #[test]
    fn tracker_incomplete_override_continues_status_recaps() {
        use super::apply_tracker_continuation_override;
        let history: Vec<uni::Message> = Vec::new();
        let recap = "## Status\nBlocked by turn budget. Next step on resume: read design/diff.rs.";
        let base = evaluate_interim_text_continuation(true, false, &history, recap, 0);
        assert!(!base.should_continue);
        let overridden = apply_tracker_continuation_override(base, true, false, recap);
        assert!(overridden.should_continue);
        assert_eq!(overridden.reason, "tracker_incomplete_continuation");
        assert!(!overridden.is_relaxed_continuation);

        let question = "Should I proceed with the remaining steps?";
        let q_base = evaluate_interim_text_continuation(true, false, &history, question, 0);
        let q_over = apply_tracker_continuation_override(q_base, true, false, question);
        assert!(!q_over.should_continue);

        let plan_over = apply_tracker_continuation_override(
            evaluate_interim_text_continuation(true, true, &history, recap, 0),
            true,
            true,
            recap,
        );
        assert!(!plan_over.should_continue);

        let complete_tracker = apply_tracker_continuation_override(base, false, false, recap);
        assert!(!complete_tracker.should_continue);

        // Permission/safety handoffs stay terminal for the user.
        for blocker in [
            "Permission denied for exec_command. Next step: retry after access is granted.",
            "Access denied to the sandbox; policy block.",
            "I hit the tool-call safety fuse mid-verification; policy block.",
            "Requires manual intervention from the workspace owner.",
            "Missing credentials for the provider route.",
        ] {
            let b_base = evaluate_interim_text_continuation(true, false, &history, blocker, 0);
            let b_over = apply_tracker_continuation_override(b_base, true, false, blocker);
            assert!(!b_over.should_continue, "must not continue on blocker: {blocker}");
        }

        // Budget-like "blocked by …" still continues when tracker work remains.
        let budget = "## Status\nBlocked by turn budget. Next step: read design/diff.rs.";
        let budget_over = apply_tracker_continuation_override(
            evaluate_interim_text_continuation(true, false, &history, budget, 0),
            true,
            false,
            budget,
        );
        assert!(budget_over.should_continue);
        assert_eq!(budget_over.reason, "tracker_incomplete_continuation");

        // Tool-loop / tool-budget recaps continue even with "blocked by".
        for recap in [
            "## Status\nBlocked by turn tool budget. Next step: patch FileChangeItem.",
            "## Status\nTool loop budget exhausted this turn. Next: run cargo check.",
            "Task 3 mid-flight — hit the per-file read cap before editing. Next: one targeted read then patch.",
        ] {
            let over = apply_tracker_continuation_override(
                evaluate_interim_text_continuation(true, false, &history, recap, 0),
                true,
                false,
                recap,
            );
            assert!(over.should_continue, "must continue on recoverable recap: {recap}");
        }

        // Mid-text `?` in a status section is not a user handoff when tracker incomplete.
        let status_question =
            "## Status\nTask 2 done — is the next step clear? Continuing with task 3 now using tools.";
        let s_over = apply_tracker_continuation_override(
            evaluate_interim_text_continuation(true, false, &history, status_question, 0),
            true,
            false,
            status_question,
        );
        assert!(s_over.should_continue, "mid-text question in status recap must continue");

        // Broad mid-text "can you" / "need your" in status prose is not a handoff.
        let status_prose =
            "## Status\nNext we need your workspace-relative path in CONFIG; can you see it in the dump? Patching now.";
        let prose_over = apply_tracker_continuation_override(
            evaluate_interim_text_continuation(true, false, &history, status_prose, 0),
            true,
            false,
            status_prose,
        );
        assert!(prose_over.should_continue, "status prose mentioning need-your/can-you must continue");

        // Clause-start interview ask without a trailing `?` still ends the turn.
        let clause_ask = "Can you confirm which migration path to take before I edit the schema.";
        let c_over = apply_tracker_continuation_override(
            evaluate_interim_text_continuation(true, false, &history, clause_ask, 0),
            true,
            false,
            clause_ask,
        );
        assert!(!c_over.should_continue);

        // Trailing `?` stays terminal.
        let trailing_q = "Should I proceed with the remaining steps?";
        let t_over = apply_tracker_continuation_override(
            evaluate_interim_text_continuation(true, false, &history, trailing_q, 0),
            true,
            false,
            trailing_q,
        );
        assert!(!t_over.should_continue);

        // Verification-pending recaps are true handoffs — outer verification
        // recovery owns that path; in-turn tracker continuation must not race
        // past the anti-blind gate, even when full-auto already continued.
        for recap in [
            "Turn blocked after repeated unverified assistant responses; verification is still pending.",
            "## Status\nBlocked by turn budget. Verification is still pending; run cargo check --locked.",
            "Anti-blind checkpoint: verification is still pending before further edits.",
        ] {
            let base = evaluate_interim_text_continuation(true, false, &history, recap, 0);
            let over = apply_tracker_continuation_override(base, true, false, recap);
            assert!(!over.should_continue, "must not continue past verification gate: {recap}");
        }

        // Session evidence: budget/status recaps without a gate still continue.
        // Full-auto may already continue some of these; the tracker override
        // must not reverse that. Assert `tracker_incomplete_continuation` only
        // when the base classifier would have stopped.
        for recap in [
            "## Status\nTask 7 blocked by the turn's preview budget before I could read docs.",
            "## Status\nTool budget ran out after evidence gathering; next step is the patch.",
            "Task 3 mid-flight — hit the per-file read cap before editing.",
            "Fix 1 partially applied, needs verification; continuing with the remaining checks now.",
        ] {
            let base = evaluate_interim_text_continuation(true, false, &history, recap, 0);
            let over = apply_tracker_continuation_override(base, true, false, recap);
            assert!(over.should_continue, "must continue on session budget recap: {recap}");
            if !base.should_continue {
                assert_eq!(over.reason, "tracker_incomplete_continuation");
            }
        }

        // When the base classifier would end the turn, tracker override forces
        // non-relaxed continuation with the tracker reason.
        let status_only =
            "## Status\nBlocked by the turn's preview budget. Next step on resume: read docs/development/README.md.";
        let s_base = evaluate_interim_text_continuation(false, false, &history, status_only, 0);
        assert!(!s_base.should_continue, "interactive status recap must not auto-continue without tracker");
        let s_over = apply_tracker_continuation_override(s_base, true, false, status_only);
        assert!(s_over.should_continue);
        assert_eq!(s_over.reason, "tracker_incomplete_continuation");
        assert!(!s_over.is_relaxed_continuation);
    }

    #[test]
    fn recoverable_status_recap_vocabulary_matches_outer_classifier() {
        use vtcode_core::core::agent::completion::recoverable_status_recap_phrasing;
        for phrase in [
            "blocked by the turn's preview budget",
            "tool budget ran out",
            "per-file read cap",
            "preview budget exhausted",
            "tool loop budget exhausted",
            "recovery fallback",
            "reached the safety cap",
        ] {
            let lower = phrase.to_ascii_lowercase();
            assert!(recoverable_status_recap_phrasing(&lower), "shared vocab must cover {phrase}");
            assert!(
                crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_auto_continue_is_recoverable_block(
                    Some(phrase)
                ),
                "outer classifier must cover {phrase}"
            );
        }
        for phrase in [
            "Turn blocked after repeated unverified assistant responses; verification is still pending.",
            "exec_command is denied by permission policy",
            "request_user_input is required",
        ] {
            assert!(
                !crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_auto_continue_is_recoverable_block(
                    Some(phrase)
                ),
                "outer classifier must deny {phrase}"
            );
        }
    }
}
