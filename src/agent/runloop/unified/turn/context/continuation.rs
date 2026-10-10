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
/// incomplete steps from another session. Adoption requires a successful
/// tracker create/update/add in the current request, explicit continuation,
/// or an approved-plan handoff. Ordinary tool use and work-intent words do
/// not establish that a workspace checklist belongs to this task.
pub(crate) fn tracker_continuation_adoption_allowed(history: &[uni::Message]) -> bool {
    let Some((request_start, request_message)) = history.iter().enumerate().rev().find(|(_, message)| {
        message.role == uni::MessageRole::User
            && !crate::agent::runloop::unified::turn::is_internal_harness_follow_up(message.content.as_text().as_ref())
    }) else {
        return false;
    };
    let request = request_message.content.as_text();
    if tracker_request_explicitly_adopts(request.as_ref()) {
        return true;
    }
    vtcode_core::core::agent::completion::tracker_was_adopted(history.get(request_start..).unwrap_or_default())
}

pub(crate) fn tracker_request_explicitly_adopts(request: &str) -> bool {
    crate::agent::runloop::unified::state::is_follow_up_prompt_like(request)
        || request.trim() == vtcode_core::prompts::system::PLANNING_WORKFLOW_IMPLEMENTATION_PROMPT
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
mod tests;
