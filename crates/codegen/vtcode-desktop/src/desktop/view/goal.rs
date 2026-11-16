//! Composer goal bar: live `/goal` loop status above the draft input.
//!
//! The bar renders the latest `goal.updated` snapshot (service-pushed on
//! every mutation and every active turn). Its buttons execute through the
//! quiet `goal/execute` RPC — never as turn text, which would wedge waiting
//! for a terminal turn event that a non-turn command never emits.

use super::*;
use serde_json::{Value, json};
use vtcode_exec_events::goal::{GoalSnapshot, GoalState};

/// Max objective chars shown before the bar truncates with `…`.
const GOAL_OBJECTIVE_PREVIEW_CHARS: usize = 60;

/// Goal timer cadence: the bar re-renders each second while Active.
pub(super) const GOAL_TICK_INTERVAL: Duration = Duration::from_secs(1);

/// Live elapsed label for the bar and Utilities pane: wall-clock past the
/// snapshot while Active (snapshots only arrive on turn boundaries and
/// mutations), frozen otherwise. A missing arrival stamp falls back to the
/// snapshot so direct assignments never panic.
pub(super) fn goal_live_elapsed_label(snapshot: &GoalSnapshot, arrived_at: Option<Instant>) -> String {
    let elapsed_ms = match (snapshot.state, arrived_at) {
        (GoalState::Active, Some(at)) => snapshot
            .elapsed_ms
            .saturating_add(u64::try_from(at.elapsed().as_millis()).unwrap_or(u64::MAX)),
        _ => snapshot.elapsed_ms,
    };
    GoalSnapshot::elapsed_label_for(elapsed_ms)
}

/// Distinctive goal identity mark: a bullseye tinted by goal state, so goal
/// surfaces scan apart from every other UI. The state label text always
/// names the state; the tint is supplemental, never the sole meaning.
pub(super) fn goal_state_icon(
    id: impl Into<gpui_kit::ElementId>,
    state: GoalState,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> AnyElement {
    let tint = match state {
        GoalState::Active => colors.primary,
        GoalState::Paused | GoalState::Unknown => colors.muted_foreground,
        GoalState::Complete => colors.success,
        GoalState::Blocked => colors.danger,
        GoalState::BudgetLimited => colors.warning,
    };
    div()
        .id(id)
        .test_support()
        .flex_shrink_0()
        .child(Icon::new(IconName::Target).size_4().text_color(tint))
        .into_any_element()
}

/// Localized state label shared by the goal bar, transition notices, and
/// the Utilities pane.
pub(super) fn goal_state_label(state: GoalState) -> String {
    match state {
        GoalState::Active => rust_i18n::t!("app.composer.goal_pursuing").to_string(),
        GoalState::Paused | GoalState::Unknown => rust_i18n::t!("app.composer.goal_paused").to_string(),
        GoalState::Complete => rust_i18n::t!("app.composer.goal_complete").to_string(),
        GoalState::Blocked => rust_i18n::t!("app.composer.goal_blocked").to_string(),
        GoalState::BudgetLimited => rust_i18n::t!("app.composer.goal_budget").to_string(),
    }
}

impl DesktopView {
    /// Start the always-on 1s ticker that re-renders only while a goal is
    /// Active, so the bar ticks between snapshots without any per-event task
    /// lifecycle. Idempotent, mirroring the event drain starter.
    pub(super) fn start_goal_ticker(&mut self, cx: &mut Context<Self>) {
        if self.goal_tick_task.is_some() {
            return;
        }
        self.goal_tick_task = Some(cx.spawn(async move |weak_view, cx| {
            loop {
                let alive = weak_view
                    .update(cx, |view, cx| {
                        if view
                            .goal_snapshot
                            .as_ref()
                            .is_some_and(|snapshot| snapshot.state == GoalState::Active)
                        {
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
                cx.background_executor().timer(GOAL_TICK_INTERVAL).await;
            }
        }));
    }

    /// Store a goal snapshot and stamp its arrival for the live timer.
    /// Sole production writer so the stamp can never drift; tests assign the
    /// field directly to cover the unstamped fallback.
    pub(super) fn set_goal_snapshot(&mut self, snapshot: Option<GoalSnapshot>) {
        self.goal_snapshot_at = snapshot.as_ref().map(|_| Instant::now());
        self.goal_snapshot = snapshot;
    }

    /// Seed the goal bar from the newest `goal.updated` checkpoint in the
    /// restored timeline, mirroring the tracker snapshot restore.
    pub(super) fn restore_goal_snapshot(&mut self, events: &[vtcode_exec_events::VersionedThreadEvent]) {
        let snapshot = events
            .iter()
            .rev()
            .find_map(|event| match event.event() {
                vtcode_exec_events::ThreadEvent::GoalUpdated(event) => Some(event.goal.clone()),
                _ => None,
            })
            .flatten();
        self.set_goal_snapshot(snapshot);
        self.goal_notified = self.goal_snapshot.as_ref().map(|snapshot| snapshot.state);
    }

    /// Push a transcript notice when a live `goal.updated` event changes the
    /// goal state. Repeated snapshots (per-turn refreshes) stay quiet, and
    /// RPC responses never notice: the event is the single source, so each
    /// transition surfaces exactly once.
    pub(super) fn note_goal_transition(&mut self, goal: Option<&GoalSnapshot>) {
        let state = goal.map(|snapshot| snapshot.state);
        if state == self.goal_notified {
            return;
        }
        self.goal_notified = state;
        let body = match goal {
            Some(snapshot) => {
                format!("{}: {} ({})", goal_state_label(snapshot.state), snapshot.objective, snapshot.elapsed_label())
            }
            None => rust_i18n::t!("app.composer.goal_cleared").to_string(),
        };
        self.transcript.push(TranscriptItem::notice(
            format!("goal-{}", uuid_like_id()),
            rust_i18n::t!("app.composer.goal_notice_title").to_string().into(),
            body,
            NoticeSeverity::Info,
        ));
    }

    /// Execute a `/goal` command quietly against the selected session.
    pub(super) fn execute_goal_command(&mut self, args: String, cx: &mut Context<Self>) -> bool {
        let Some(session_id) = self.selected_session_id.clone() else {
            return false;
        };
        if !self.request(protocol::method::GOAL_EXECUTE, json!({ "sessionId": session_id, "args": args })) {
            self.error = Some(rust_i18n::t!(i18n::QUEUE_FULL).to_string());
            cx.notify();
            return false;
        }
        true
    }

    /// Apply a `goal/execute` response: refresh the bar, submit follow-up
    /// turns normally, and surface read-only output as a transcript notice.
    pub(super) fn apply_goal_execute_response(&mut self, result: Value, cx: &mut Context<Self>) {
        let Ok(response) = serde_json::from_value::<protocol::GoalExecuteResult>(result) else {
            self.error = Some(rust_i18n::t!("app.service.invalid_ack_detail").to_string());
            cx.notify();
            return;
        };
        if self.selected_session_id.as_deref() != Some(response.session_id.as_str()) {
            return;
        }
        self.set_goal_snapshot(response.goal);
        if let Some(prompt) = response.submit {
            let draft = SubmissionDraft {
                prompt,
                attachments: Vec::new(),
                echo_id: format!("desktop-user-{}", uuid_like_id()),
                delivery_uncertain: false,
            };
            let retained = if self.running {
                self.enqueue_queued_submission(draft, false, cx).err()
            } else {
                self.enqueue_submission(draft, cx).err()
            };
            if let Some(draft) = retained {
                self.retained_submission = Some(draft);
                self.status = rust_i18n::t!(i18n::COMPOSER_MESSAGE_NOT_SUBMITTED).to_string();
            }
            cx.notify();
            return;
        }
        if !response.mutated && !response.lines.is_empty() {
            self.transcript.push(TranscriptItem::notice(
                format!("goal-{}", uuid_like_id()),
                rust_i18n::t!("app.composer.goal_notice_title").to_string().into(),
                response.lines.join("\n"),
                NoticeSeverity::Info,
            ));
        }
        cx.notify();
    }

    /// Toggle between the truncated and full objective in the goal bar.
    pub(super) fn toggle_goal_expanded(&mut self, cx: &mut Context<Self>) {
        self.goal_expanded = !self.goal_expanded;
        cx.notify();
    }

    /// Goal status bar above the draft input, or `None` without a goal.
    pub(super) fn render_goal_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let snapshot = self.goal_snapshot.as_ref()?;
        let colors = cx.theme().colors;
        let label = goal_state_label(snapshot.state);
        let objective = if self.goal_expanded {
            snapshot.objective.clone()
        } else {
            snapshot.short_objective(GOAL_OBJECTIVE_PREVIEW_CHARS)
        };
        let elapsed = goal_live_elapsed_label(snapshot, self.goal_snapshot_at);
        // The accessible summary stays on the snapshot cadence: a ticking
        // live region would announce every second while Active.
        let summary = format!("{label}: {objective}, {}", snapshot.elapsed_label());
        let mut row = div()
            .id("goal-bar")
            .test_support()
            .role(gpui_kit::Role::Status)
            .aria_label(summary)
            .w_full()
            .rounded_md()
            .border_1()
            .border_color(themes::hairline(colors.popover, colors.foreground))
            .bg(colors.popover)
            .px_3()
            .py_2()
            .flex()
            .items_center()
            .gap_2()
            .child(goal_state_icon("goal-state-icon", snapshot.state, &colors))
            .child(div().text_sm().text_color(colors.foreground).child(label))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child(objective),
            )
            .child(div().text_sm().text_color(colors.muted_foreground).child(elapsed));
        row = row.child(
            div().flex().items_center().gap_1().child(
                desktop_button("goal-clear")
                    .icon(IconName::Trash)
                    .accessibility_label(rust_i18n::t!("app.composer.goal_clear").to_string())
                    .tooltip(rust_i18n::t!("app.composer.goal_clear").to_string())
                    .ghost()
                    .xsmall()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let _ = this.execute_goal_command("clear".to_string(), cx);
                    })),
            ),
        );
        if matches!(snapshot.state, GoalState::Active) {
            row = row.child(
                desktop_button("goal-pause")
                    .icon(IconName::Pause)
                    .accessibility_label(rust_i18n::t!("app.composer.goal_pause").to_string())
                    .tooltip(rust_i18n::t!("app.composer.goal_pause").to_string())
                    .ghost()
                    .xsmall()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let _ = this.execute_goal_command("pause".to_string(), cx);
                    })),
            );
        } else if matches!(snapshot.state, GoalState::Paused | GoalState::Unknown) {
            row = row.child(
                desktop_button("goal-resume")
                    .icon(IconName::Play)
                    .accessibility_label(rust_i18n::t!("app.composer.goal_resume").to_string())
                    .tooltip(rust_i18n::t!("app.composer.goal_resume").to_string())
                    .ghost()
                    .xsmall()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let _ = this.execute_goal_command("resume".to_string(), cx);
                    })),
            );
        }
        let (expand_label, expand_icon) = if self.goal_expanded {
            (rust_i18n::t!("app.composer.goal_collapse").to_string(), IconName::Minimize2)
        } else {
            (rust_i18n::t!("app.composer.goal_expand").to_string(), IconName::Maximize2)
        };
        row = row.child(
            desktop_button("goal-expand")
                .icon(expand_icon)
                .accessibility_label(expand_label.clone())
                .tooltip(expand_label)
                .ghost()
                .xsmall()
                .on_click(cx.listener(|this, _, _, cx| this.toggle_goal_expanded(cx))),
        );
        Some(row.into_any_element())
    }
}
