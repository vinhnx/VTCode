//! `/goal` slash command: thread goal loop controls.
//!
//! Pure command application ([`apply_goal_command`], fully unit-tested below)
//! plus the thin async handler ([`handle_goal_command`]) that locks the shared
//! tracker, persists, emits `goal.updated`, and renders status lines.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_desktop_protocol::slash_command::{GOAL_SLASH_HELP, GoalSlashCommand};
use vtcode_memory::{GoalPauseReason, GoalTracker};

use super::{SlashCommandContext, SlashCommandControl};

/// Shareable handle to the session's loop-lived goal tracker.
pub(crate) type SharedGoalTracker = Arc<Mutex<GoalTracker>>;

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
                submit: Some(objective.clone()),
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
                let submit = format!("Continue working toward the goal: \"{objective}\".");
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

fn no_goal_lines() -> Vec<(MessageStyle, String)> {
    vec![
        (MessageStyle::Info, "No thread goal.".to_string()),
        (MessageStyle::Info, GOAL_SLASH_HELP.to_string()),
    ]
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
    if effect.submit.is_some() {
        ctx.session_stats.reset_goal_continuation_budget();
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
        assert_eq!(first.submit.as_deref(), Some("Ship it"), "Set must submit the objective as a turn");

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
        assert!(
            resumed.submit.as_deref().is_some_and(|p| p.contains("Ship it")),
            "successful Resume must submit a continue prompt"
        );

        let repeat = apply_goal_command(&mut tracker, &GoalSlashCommand::Resume, 0, &seed());
        assert!(!repeat.mutated);
        assert!(texts(&repeat).iter().any(|l| l.contains("already active")));
        assert!(repeat.submit.is_none(), "failed Resume must not submit");
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
