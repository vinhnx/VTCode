//! `/goal` slash command: thread goal loop controls.
//!
//! Pure command application ([`apply_goal_command`], fully unit-tested below)
//! plus the thin async handler ([`handle_goal_command`]) that locks the shared
//! tracker, persists, emits `goal.updated`, and renders status lines.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use vtcode_desktop_protocol::slash_command::{GOAL_SLASH_HELP, GoalSlashCommand};
use vtcode_memory::{GoalPauseReason, GoalTracker};
use vtcode_tui_runtime::utils::ansi::MessageStyle;

use super::{SlashCommandContext, SlashCommandControl};

/// Shareable handle to the session's loop-lived goal tracker.
pub(crate) type SharedGoalTracker = Arc<Mutex<GoalTracker>>;

/// Completion contract taught to the model in every goal (re)start prompt:
/// the `GOAL ACHIEVED:` first-line sentinel the turn-boundary gate accepts.
/// Shared by `Set`, `Resume`, and the auto-continue follow-up so all three
/// teach one vocabulary.
pub(crate) const GOAL_COMPLETION_CONTRACT: &str = "If the goal is fully achieved and verified against current evidence, end your turn without further tool calls and with a final message whose first line is exactly \"GOAL ACHIEVED:\" followed by a one-line summary.";

/// Stable opening of the machine-queued goal auto-continue follow-up.
/// Shared with `is_internal_harness_follow_up` so goal turns preserve
/// recovery budgets instead of reading as fresh user requests. Deliberately
/// distinct from the user-initiated Resume prompt (`Continue working toward
/// the goal: ...`), which stays a user boundary.
pub(crate) const GOAL_AUTO_CONTINUE_PREFIX: &str = "Continue working toward the active thread goal: ";

/// Seed for a freshly set goal (id + creation timestamp).
pub(crate) struct GoalSeed<'a> {
    /// Stable id for the new goal.
    pub id: &'a str,
    /// RFC3339 creation timestamp.
    pub created_at: &'a str,
}

/// Render lines plus whether the tracker mutated (persist + emit when true).
pub(crate) struct GoalCommandEffect {
    /// Lines to render, in order.
    pub lines: Vec<(MessageStyle, String)>,
    /// Whether the tracker changed and must be persisted and emitted.
    pub mutated: bool,
    /// Turn prompt to submit after rendering (`Set` starts work on the
    /// objective; successful `Resume` continues it; otherwise `None`).
    pub submit: Option<String>,
}

/// Apply a parsed `/goal` command to the tracker. Pure: no I/O, no clock.
pub(crate) fn apply_goal_command(
    tracker: &mut GoalTracker,
    command: &GoalSlashCommand,
    session_tokens: i64,
    seed: &GoalSeed,
) -> GoalCommandEffect {
    match command {
        GoalSlashCommand::Status => effect(status_lines(tracker), false),
        GoalSlashCommand::Set { objective } => {
            let replaced = tracker.has_goal();
            tracker.create_goal(
                seed.id.to_string(),
                objective.clone(),
                None,
                session_tokens,
                seed.created_at.to_string(),
                None,
            );
            let mut lines = if replaced {
                vec![(MessageStyle::Info, "Replaced previous goal.".to_string())]
            } else {
                Vec::new()
            };
            lines.push((MessageStyle::Info, format!("Goal set: {objective}")));
            lines.extend(status_lines(tracker));
            GoalCommandEffect {
                lines,
                mutated: true,
                submit: Some(format!("{objective}\n\n{GOAL_COMPLETION_CONTRACT}")),
            }
        }
        GoalSlashCommand::Pause => {
            if tracker.pause(GoalPauseReason::User) {
                let mut lines = vec![(MessageStyle::Info, "Goal paused.".to_string())];
                lines.extend(status_lines(tracker));
                effect(lines, true)
            } else if !tracker.has_goal() {
                effect(no_goal_lines(), false)
            } else {
                let status = tracker.status().map(|s| s.display_name()).unwrap_or("unknown");
                effect(vec![(MessageStyle::Info, format!("Goal is already {status}."))], false)
            }
        }
        GoalSlashCommand::Resume => {
            if tracker.resume() {
                let mut lines = vec![(MessageStyle::Info, "Goal resumed.".to_string())];
                lines.extend(status_lines(tracker));
                let objective = tracker.objective().unwrap_or_default();
                let submit = format!("Continue working toward the goal: \"{objective}\". {GOAL_COMPLETION_CONTRACT}");
                GoalCommandEffect { lines, mutated: true, submit: Some(submit) }
            } else if !tracker.has_goal() {
                effect(no_goal_lines(), false)
            } else {
                match tracker.status().map(|s| s.display_name()) {
                    Some("Active") => effect(vec![(MessageStyle::Info, "Goal is already active.".to_string())], false),
                    Some("Complete") => {
                        effect(vec![(MessageStyle::Info, "Goal is already complete.".to_string())], false)
                    }
                    Some("Budget limited") => effect(
                        vec![(MessageStyle::Info, "Goal hit its budget; clear it or set a new goal.".to_string())],
                        false,
                    ),
                    _ => effect(vec![(MessageStyle::Info, "Goal cannot resume.".to_string())], false),
                }
            }
        }
        GoalSlashCommand::Clear => {
            if !tracker.has_goal() {
                return effect(no_goal_lines(), false);
            }
            tracker.clear();
            effect(vec![(MessageStyle::Info, "Goal cleared.".to_string())], true)
        }
        GoalSlashCommand::Budget { limit } => {
            if !tracker.has_goal() {
                return effect(no_goal_lines(), false);
            }
            match limit {
                None => effect(budget_lines(tracker), false),
                Some(0) => {
                    tracker.set_token_budget(None);
                    effect(vec![(MessageStyle::Info, "Budget cleared.".to_string())], true)
                }
                Some(cap) => {
                    tracker.set_token_budget(Some(*cap));
                    let mut lines = vec![(MessageStyle::Info, format!("Budget set: {cap} tokens."))];
                    lines.extend(status_lines(tracker));
                    effect(lines, true)
                }
            }
        }
        GoalSlashCommand::Edit => match tracker.objective() {
            Some(objective) => effect(
                vec![
                    (MessageStyle::Info, format!("Current objective: {objective}")),
                    (MessageStyle::Info, "Use \"/goal <new objective>\" to replace it.".to_string()),
                ],
                false,
            ),
            None => effect(no_goal_lines(), false),
        },
    }
}

fn effect(lines: Vec<(MessageStyle, String)>, mutated: bool) -> GoalCommandEffect {
    GoalCommandEffect { lines, mutated, submit: None }
}

/// One-line TUI indicator for the goal loop, printed once per turn boundary
/// while a goal is active or paused. Terminal states return `None`: their
/// transitions already printed.
#[must_use]
pub(crate) fn goal_indicator_line(tracker: &GoalTracker) -> Option<String> {
    let snapshot = tracker.goal_snapshot()?;
    use vtcode_core::exec::events::goal::GoalState;
    let label = match snapshot.state {
        GoalState::Active => "Pursuing goal",
        GoalState::Paused | GoalState::Unknown => "Paused goal",
        GoalState::Complete | GoalState::Blocked | GoalState::BudgetLimited => return None,
    };
    Some(format!("[i] {label}: {} ({})", snapshot.objective, snapshot.elapsed_label()))
}

fn no_goal_lines() -> Vec<(MessageStyle, String)> {
    vec![
        (MessageStyle::Info, "No thread goal.".to_string()),
        (MessageStyle::Info, GOAL_SLASH_HELP.to_string()),
    ]
}

fn budget_lines(tracker: &GoalTracker) -> Vec<(MessageStyle, String)> {
    match tracker.goal_snapshot() {
        Some(snapshot) => match snapshot.token_budget {
            Some(_) => {
                vec![(MessageStyle::Info, format!("Budget: {} used.", snapshot.tokens_label()))]
            }
            None => {
                vec![(MessageStyle::Info, format!("No budget set ({} tokens used).", snapshot.tokens_used))]
            }
        },
        None => no_goal_lines(),
    }
}

fn status_lines(tracker: &GoalTracker) -> Vec<(MessageStyle, String)> {
    let (Some(status), Some(snapshot)) = (tracker.status(), tracker.goal_snapshot()) else {
        return no_goal_lines();
    };
    let mut lines = vec![(MessageStyle::Info, format!("Goal [{}]: {}", status.display_name(), snapshot.objective))];
    lines.push((MessageStyle::Info, format!("Elapsed {} · {}", snapshot.elapsed_label(), snapshot.tokens_label())));
    if let Some(message) = &snapshot.pause_message {
        lines.push((MessageStyle::Info, format!("Note: {message}")));
    }
    lines
}

/// Execute `/goal`: lock the shared tracker, apply, persist, emit, render.
pub(crate) async fn handle_goal_command(
    ctx: SlashCommandContext<'_>,
    command: GoalSlashCommand,
) -> Result<SlashCommandControl> {
    let session_tokens = ctx.session_stats.total_tokens_i64();
    let now = chrono::Utc::now();
    let id = format!("goal-{}", now.timestamp_millis());
    let created_at = now.to_rfc3339();
    let seed = GoalSeed { id: &id, created_at: &created_at };

    let mut tracker = ctx.goal_tracker.lock().await;
    let effect = apply_goal_command(&mut tracker, &command, session_tokens, &seed);
    let mut extra = Vec::new();
    if effect.mutated {
        if let Err(err) = tracker.persist() {
            extra.push((MessageStyle::Error, format!("Could not persist goal: {err:#}")));
        }
        if let Some(emitter) = ctx.harness_emitter
            && let Err(err) = emitter.emit(tracker.to_goal_updated_event())
        {
            extra.push((MessageStyle::Error, format!("Could not emit goal event: {err:#}")));
        }
    }
    drop(tracker);
    for (style, line) in effect.lines.into_iter().chain(extra) {
        ctx.renderer.line(style, &line)?;
    }
    if let Some(prompt) = effect.submit {
        return Ok(SlashCommandControl::SubmitPrompt(prompt));
    }
    Ok(SlashCommandControl::Continue)
}

/// Pause message left on the goal when its driving session ends.
const SESSION_END_PAUSE_MESSAGE: &str = "Session ended; resume to continue.";

/// Pause an Active goal at session teardown so neither the persisted
/// snapshot nor the live UI claims a dead session is still pursuing it.
/// Non-Active goals and lock contention fail closed (`false`): restore
/// demotes any leftover Active goal to paused anyway.
pub(crate) fn pause_goal_at_session_end(tracker: &SharedGoalTracker, emitter: Option<&HarnessEventEmitter>) -> bool {
    let Ok(mut guard) = tracker.try_lock() else {
        return false;
    };
    if !guard.pause_with_message(GoalPauseReason::SessionEnd, SESSION_END_PAUSE_MESSAGE.to_string()) {
        return false;
    }
    if let Err(err) = guard.persist() {
        tracing::warn!("goal snapshot persist failed: {err:#}");
    }
    if let Some(emitter) = emitter
        && let Err(err) = emitter.emit(guard.to_goal_updated_event())
    {
        tracing::warn!("goal event emit failed: {err:#}");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tracker() -> (tempfile::TempDir, GoalTracker) {
        let dir = tempfile::tempdir().unwrap();
        let tracker = GoalTracker::new(dir.path().join("session-1"));
        (dir, tracker)
    }

    fn seed() -> GoalSeed<'static> {
        GoalSeed { id: "goal-1", created_at: "2026-01-01T00:00:00Z" }
    }

    fn texts(effect: &GoalCommandEffect) -> Vec<&str> {
        effect.lines.iter().map(|(_, line)| line.as_str()).collect()
    }

    #[test]
    fn status_without_goal_shows_help_and_does_not_mutate() {
        let (_dir, mut tracker) = test_tracker();
        let effect = apply_goal_command(&mut tracker, &GoalSlashCommand::Status, 0, &seed());
        assert!(!effect.mutated);
        let lines = texts(&effect);
        assert!(lines.iter().any(|l| l.contains("No thread goal")), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("/goal")), "{lines:?}");
    }

    #[test]
    fn set_creates_goal_and_second_set_reports_replacement() {
        let (_dir, mut tracker) = test_tracker();
        let set = GoalSlashCommand::Set { objective: "Ship it".into() };
        let first = apply_goal_command(&mut tracker, &set, 100, &seed());
        assert!(first.mutated);
        assert!(texts(&first).iter().any(|l| l.contains("Goal set: Ship it")));
        assert!(!texts(&first).iter().any(|l| l.contains("Replaced")));
        let submit = first.submit.as_deref().expect("Set must submit a turn");
        assert!(submit.starts_with("Ship it"), "submit must lead with the objective: {submit}");
        assert!(
            submit.contains("GOAL ACHIEVED:"),
            "submit must teach the completion contract from the first turn: {submit}"
        );

        let second = apply_goal_command(&mut tracker, &set, 200, &seed());
        assert!(second.mutated);
        assert!(texts(&second).iter().any(|l| l.contains("Replaced previous goal")));
    }

    #[test]
    fn pause_and_resume_round_trip_with_precise_repeat_messages() {
        let (_dir, mut tracker) = test_tracker();
        let set = GoalSlashCommand::Set { objective: "Ship it".into() };
        apply_goal_command(&mut tracker, &set, 0, &seed());

        let paused = apply_goal_command(&mut tracker, &GoalSlashCommand::Pause, 0, &seed());
        assert!(paused.mutated);
        assert!(texts(&paused).iter().any(|l| l.contains("Goal paused")));

        let repeat = apply_goal_command(&mut tracker, &GoalSlashCommand::Pause, 0, &seed());
        assert!(!repeat.mutated);
        assert!(texts(&repeat).iter().any(|l| l.contains("already Paused")), "{:?}", texts(&repeat));

        let resumed = apply_goal_command(&mut tracker, &GoalSlashCommand::Resume, 0, &seed());
        assert!(resumed.mutated);
        let submit = resumed
            .submit
            .as_deref()
            .expect("successful Resume must submit a continue prompt");
        assert!(submit.contains("Ship it"), "resume prompt must name the objective: {submit}");
        assert!(submit.contains("GOAL ACHIEVED:"), "resume prompt must re-teach the completion contract: {submit}");

        let repeat = apply_goal_command(&mut tracker, &GoalSlashCommand::Resume, 0, &seed());
        assert!(!repeat.mutated);
        assert!(texts(&repeat).iter().any(|l| l.contains("already active")));
        assert!(repeat.submit.is_none(), "failed Resume must not submit");
    }

    #[test]
    fn goal_indicator_line_covers_active_paused_and_terminal() {
        let (_dir, mut tracker) = test_tracker();
        assert_eq!(goal_indicator_line(&tracker), None);

        tracker.create_goal("goal-1".into(), "Ship it".into(), None, 0, "2026-01-01T00:00:00Z".into(), None);
        let active = goal_indicator_line(&tracker).expect("active goal shows");
        assert!(active.starts_with("[i] Pursuing goal: Ship it ("), "{active}");
        assert!(active.ends_with(")"), "{active}");

        assert!(tracker.pause(GoalPauseReason::User));
        let paused = goal_indicator_line(&tracker).expect("paused goal shows");
        assert!(paused.starts_with("[i] Paused goal: Ship it ("), "{paused}");

        assert!(tracker.resume());
        assert!(tracker.complete());
        assert_eq!(goal_indicator_line(&tracker), None, "terminal states stay quiet");
    }

    #[test]
    fn only_set_and_successful_resume_submit_a_turn() {
        let (_dir, mut tracker) = test_tracker();
        let set = GoalSlashCommand::Set { objective: "Ship it".into() };
        apply_goal_command(&mut tracker, &set, 0, &seed());

        for command in [
            GoalSlashCommand::Status,
            GoalSlashCommand::Pause,
            GoalSlashCommand::Clear,
            GoalSlashCommand::Edit,
        ] {
            let effect = apply_goal_command(&mut tracker, &command, 0, &seed());
            assert!(effect.submit.is_none(), "{command:?} must not submit a turn");
        }
    }

    #[test]
    fn resume_terminal_states_explain_instead_of_resuming() {
        let (_dir, mut tracker) = test_tracker();
        let set = GoalSlashCommand::Set { objective: "Ship it".into() };
        apply_goal_command(&mut tracker, &set, 0, &seed());
        assert!(tracker.budget_limit());

        let effect = apply_goal_command(&mut tracker, &GoalSlashCommand::Resume, 0, &seed());
        assert!(!effect.mutated);
        assert!(texts(&effect).iter().any(|l| l.contains("budget")), "{:?}", texts(&effect));
    }

    #[test]
    fn clear_removes_goal_and_second_clear_is_a_noop() {
        let (_dir, mut tracker) = test_tracker();
        let set = GoalSlashCommand::Set { objective: "Ship it".into() };
        apply_goal_command(&mut tracker, &set, 0, &seed());

        let cleared = apply_goal_command(&mut tracker, &GoalSlashCommand::Clear, 0, &seed());
        assert!(cleared.mutated);
        assert!(!tracker.has_goal());

        let repeat = apply_goal_command(&mut tracker, &GoalSlashCommand::Clear, 0, &seed());
        assert!(!repeat.mutated);
        assert!(texts(&repeat).iter().any(|l| l.contains("No thread goal")));
    }

    #[test]
    fn budget_shows_sets_and_clears() {
        use vtcode_desktop_protocol::slash_command::GoalSlashCommand;

        let (_dir, mut tracker) = test_tracker();
        let show = GoalSlashCommand::Budget { limit: None };
        let effect = apply_goal_command(&mut tracker, &show, 0, &seed());
        assert!(!effect.mutated);
        assert!(texts(&effect).iter().any(|l| l.contains("No thread goal")));

        let set = GoalSlashCommand::Set { objective: "Ship it".into() };
        apply_goal_command(&mut tracker, &set, 0, &seed());
        let effect = apply_goal_command(&mut tracker, &show, 0, &seed());
        assert!(!effect.mutated);
        assert!(texts(&effect).iter().any(|l| l.contains("No budget set")));

        let set_budget = GoalSlashCommand::Budget { limit: Some(100_000) };
        let effect = apply_goal_command(&mut tracker, &set_budget, 0, &seed());
        assert!(effect.mutated);
        assert!(texts(&effect).iter().any(|l| l.contains("Budget set: 100000 tokens")));
        assert_eq!(tracker.token_budget(), Some(100_000));

        let clear = GoalSlashCommand::Budget { limit: Some(0) };
        let effect = apply_goal_command(&mut tracker, &clear, 0, &seed());
        assert!(effect.mutated);
        assert!(texts(&effect).iter().any(|l| l.contains("Budget cleared")));
        assert_eq!(tracker.token_budget(), None);
    }

    #[test]
    fn edit_shows_objective_or_no_goal() {
        let (_dir, mut tracker) = test_tracker();
        let empty = apply_goal_command(&mut tracker, &GoalSlashCommand::Edit, 0, &seed());
        assert!(!empty.mutated);
        assert!(texts(&empty).iter().any(|l| l.contains("No thread goal")));

        let set = GoalSlashCommand::Set { objective: "Ship it".into() };
        apply_goal_command(&mut tracker, &set, 0, &seed());
        let shown = apply_goal_command(&mut tracker, &GoalSlashCommand::Edit, 0, &seed());
        assert!(!shown.mutated);
        assert!(texts(&shown).iter().any(|l| l.contains("Current objective: Ship it")));
    }

    #[tokio::test]
    async fn session_end_pauses_active_goal_persists_and_emits() {
        let dir = tempfile::tempdir().unwrap();
        let tracker: SharedGoalTracker = Arc::new(Mutex::new(GoalTracker::new(dir.path().join("session-1"))));
        tracker.lock().await.create_goal(
            "goal-1".into(),
            "Ship it".into(),
            None,
            0,
            "2026-01-01T00:00:00Z".into(),
            None,
        );
        let events_path = dir.path().join("events.jsonl");
        let emitter = HarnessEventEmitter::new(events_path.clone()).unwrap();

        assert!(pause_goal_at_session_end(&tracker, Some(&emitter)));
        assert_eq!(tracker.lock().await.status(), Some(vtcode_memory::GoalStatus::UserPaused));
        assert_eq!(tracker.lock().await.pause_message(), Some("Session ended; resume to continue."));
        assert!(dir.path().join("session-1").join("goal").join("goal.json").exists(), "teardown pause must persist");
        let log = std::fs::read_to_string(&events_path).unwrap();
        assert!(log.contains("\"goal.updated\""), "teardown pause must emit: {log}");
        assert!(log.contains("\"paused\""), "emit must carry the paused state: {log}");

        assert!(!pause_goal_at_session_end(&tracker, Some(&emitter)), "repeat pause must no-op");
    }

    #[tokio::test]
    async fn session_end_leaves_settled_missing_and_locked_goals_alone() {
        let dir = tempfile::tempdir().unwrap();
        let tracker: SharedGoalTracker = Arc::new(Mutex::new(GoalTracker::new(dir.path().join("session-1"))));
        assert!(!pause_goal_at_session_end(&tracker, None), "no goal must no-op");

        tracker.lock().await.create_goal(
            "goal-1".into(),
            "Ship it".into(),
            None,
            0,
            "2026-01-01T00:00:00Z".into(),
            None,
        );
        {
            let _held = tracker.lock().await;
            assert!(!pause_goal_at_session_end(&tracker, None), "contended lock must fail closed");
        }

        assert!(tracker.lock().await.complete());
        assert!(!pause_goal_at_session_end(&tracker, None), "complete goal must no-op");
    }

    #[test]
    fn status_lines_show_budget_and_pause_note_only_when_present() {
        let (_dir, mut tracker) = test_tracker();
        let set = GoalSlashCommand::Set { objective: "Ship it".into() };
        let effect = apply_goal_command(&mut tracker, &set, 0, &seed());
        assert!(texts(&effect).iter().any(|l| l.contains("Goal [Active]: Ship it")));
        assert!(texts(&effect).iter().any(|l| l.contains("0 tokens") && !l.contains('/')));

        tracker.pause_with_message(GoalPauseReason::User, "taking a break".into());
        let effect = apply_goal_command(&mut tracker, &GoalSlashCommand::Status, 0, &seed());
        assert!(texts(&effect).iter().any(|l| l.contains("Goal [Paused]: Ship it")));
        assert!(texts(&effect).iter().any(|l| l.contains("Note: taking a break")));
    }
}
