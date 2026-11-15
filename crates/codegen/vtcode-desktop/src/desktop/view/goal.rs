//! Composer goal bar: live `/goal` loop status above the draft input.
//!
//! The bar renders the latest `goal.updated` snapshot (service-pushed on
//! every mutation and every active turn). Its buttons execute through the
//! quiet `goal/execute` RPC — never as turn text, which would wedge waiting
//! for a terminal turn event that a non-turn command never emits.

use super::*;
use serde_json::{Value, json};

/// Max objective chars shown before the bar truncates with `…`.
const GOAL_OBJECTIVE_PREVIEW_CHARS: usize = 60;

impl DesktopView {
    /// Seed the goal bar from the newest `goal.updated` checkpoint in the
    /// restored timeline, mirroring the tracker snapshot restore.
    pub(super) fn restore_goal_snapshot(&mut self, events: &[vtcode_exec_events::VersionedThreadEvent]) {
        self.goal_snapshot = events
            .iter()
            .rev()
            .find_map(|event| match event.event() {
                vtcode_exec_events::ThreadEvent::GoalUpdated(event) => Some(event.goal.clone()),
                _ => None,
            })
            .flatten();
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
        self.goal_snapshot = response.goal;
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
        use vtcode_exec_events::goal::GoalState;

        let snapshot = self.goal_snapshot.as_ref()?;
        let colors = cx.theme().colors;
        let label = match snapshot.state {
            GoalState::Active => rust_i18n::t!("app.composer.goal_pursuing").to_string(),
            GoalState::Paused | GoalState::Unknown => rust_i18n::t!("app.composer.goal_paused").to_string(),
            GoalState::Complete => rust_i18n::t!("app.composer.goal_complete").to_string(),
            GoalState::Blocked => rust_i18n::t!("app.composer.goal_blocked").to_string(),
            GoalState::BudgetLimited => rust_i18n::t!("app.composer.goal_budget").to_string(),
        };
        let objective = if self.goal_expanded {
            snapshot.objective.clone()
        } else {
            snapshot.short_objective(GOAL_OBJECTIVE_PREVIEW_CHARS)
        };
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
            .child(div().text_sm().text_color(colors.foreground).child(label))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child(objective),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child(snapshot.elapsed_label()),
            );
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
