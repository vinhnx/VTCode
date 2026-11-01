//! Running-turn switch confirmation: stop-then-proceed for session changes.
//!
//! The service refuses every session transition while a turn runs, so New,
//! Switch, and Close used to go dead mid-turn. Those actions now open this
//! modal instead: confirming stops the turn and arms the intent, which drains
//! once the turn finishes (see `drain_armed_running_switch`). Declining
//! leaves all state untouched. Structured after `full_access_confirm.rs`.
use super::*;

/// Session change deferred until the running turn stops.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RunningSwitchIntent {
    NewThread,
    LoadSession {
        /// Session to load once the turn stops.
        target: String,
        /// Tabs to untab once the load is sent (tab-close flows that had to
        /// switch sessions first). Empty for direct switches.
        untab: Vec<String>,
    },
    CloseSession,
}

/// A stop-then-proceed request: the intent plus the workspace it was opened
/// from, so a workspace switch in between cannot misroute the deferred send.
/// Shared by the open modal (`running_confirm`) and the armed wait
/// (`armed_running_switch`); at most one of them holds a value at a time,
/// and while either does the funnel stays shut (see
/// [`DesktopView::running_switch_pending`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RunningSwitch {
    intent: RunningSwitchIntent,
    workspace: Option<PathBuf>,
}

impl DesktopView {
    pub(super) fn open_running_confirm(
        &mut self,
        intent: RunningSwitchIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.running_switch_pending() {
            // The funnel is owned: never swap intents, just hand focus back.
            // Unreachable from gated UI entries; slash and tests route here.
            self.running_confirm_focus.focus(window, cx);
            cx.notify();
            return;
        }
        self.disarm_double_escape();
        self.running_confirm = Some(RunningSwitch { intent, workspace: self.workspace.clone() });
        self.running_confirm_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn cancel_running_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.disarm_double_escape();
        self.running_confirm = None;
        // Route-aware: the modal can stand over the browser or settings,
        // where the composer is unmounted.
        self.focus_route(self.current_route(), window, cx);
        cx.notify();
    }

    pub(super) fn confirm_running_switch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(switch) = self.running_confirm.take() else {
            return;
        };
        self.disarm_double_escape();
        self.focus_route(self.current_route(), window, cx);
        // Unreachable in practice: workspace flows reset (clearing the modal)
        // and every service-loss arm interrupts it. Loud if that changes.
        debug_assert!(
            self.workspace == switch.workspace && self.backend.is_some(),
            "running confirm cleared on workspace change and service loss"
        );
        if self.workspace != switch.workspace || self.backend.is_none() {
            cx.notify();
            return;
        }
        if !self.running {
            // The turn finished while the modal stood open: proceed directly.
            self.execute_running_switch(switch, cx);
            return;
        }
        self.armed_running_switch = Some(switch);
        self.cancel_turn_for_running_switch(cx);
    }

    /// Run a deferred session change now that the turn has stopped. Consumes
    /// the armed wait; failures surface the switch-failed error so the user
    /// can retry instead of wondering where the thread went.
    pub(super) fn drain_armed_running_switch(&mut self, cx: &mut Context<Self>) {
        let Some(switch) = self.armed_running_switch.take() else {
            return;
        };
        // Contract: the only caller is the running-to-idle transition in the
        // thread-event arm, so the service-side turn guard must be clear.
        debug_assert!(!self.running, "armed switch drains only once the turn finishes");
        self.execute_running_switch(switch, cx);
    }

    fn execute_running_switch(&mut self, switch: RunningSwitch, cx: &mut Context<Self>) {
        if self.workspace != switch.workspace || self.backend.is_none() || self.interaction.is_some() {
            self.error = Some(rust_i18n::t!("app.session.running_switch_failed").to_string());
            cx.notify();
            return;
        }
        let sent = match switch.intent {
            RunningSwitchIntent::NewThread => self.begin_new_thread(cx),
            RunningSwitchIntent::LoadSession { target, untab } => {
                let sent = self.load_session(target, cx);
                if sent {
                    for tab in untab {
                        self.remove_tab(&tab);
                    }
                }
                sent
            }
            RunningSwitchIntent::CloseSession => {
                if self.selected_session_id.is_none() {
                    true
                } else {
                    self.close_session(cx);
                    self.session_request == Some(protocol::method::SESSION_CLOSE)
                }
            }
        };
        if !sent && self.error.is_none() {
            // The primitives report specific causes (unsent draft, full
            // queue) themselves; only fill in when they stayed silent.
            self.error = Some(rust_i18n::t!("app.session.running_switch_failed").to_string());
        }
        cx.notify();
    }

    /// Drop the modal and any armed wait: the service is gone or the session
    /// was reset, so the deferred send has no valid target anymore.
    pub(super) fn interrupt_armed_running_switch(&mut self) {
        self.running_confirm = None;
        self.armed_running_switch = None;
    }

    /// Drop a stop-then-proceed wait whose turn died before lifecycle events
    /// (rejected submit, invalid ack): the deferred send has no valid moment
    /// anymore, and the arm's own error already explains why. The open modal
    /// (pre-confirm) is untouched: answering it still proceeds immediately.
    pub(super) fn drop_armed_running_switch(&mut self) {
        self.armed_running_switch = None;
    }

    fn display_title_for(&self, session_id: &str) -> String {
        let title = self
            .sessions
            .iter()
            .find(|session| session.session_id == session_id)
            .and_then(|session| session.title.clone());
        session_display_title(session_id, title.as_deref())
    }

    /// Centered consent card over the shared modal scrim. The Stop action is
    /// destructive-styled: it ends the running turn and discards queued
    /// prompts, which the warning line calls out while the queue is non-empty.
    pub(super) fn render_running_confirm(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let Some(switch) = self.running_confirm.as_ref() else {
            return div().into_any_element();
        };
        let running_title = self
            .selected_session_id
            .as_deref()
            .map(|session_id| self.display_title_for(session_id))
            .unwrap_or_default();
        let target_title = match &switch.intent {
            RunningSwitchIntent::LoadSession { target, .. } => Some(self.display_title_for(target)),
            RunningSwitchIntent::NewThread | RunningSwitchIntent::CloseSession => None,
        };
        let body = running_switch_body_text(&switch.intent, &running_title, target_title.as_deref());
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .bg(modal_scrim(&colors))
            .child(
                div()
                    .id("running-confirm-dialog")
                    .test_support()
                    .track_focus(&self.running_confirm_focus)
                    .role(gpui_kit::Role::Dialog)
                    .accessibility_id("running-confirm-dialog")
                    .aria_label(rust_i18n::t!("app.session.running_switch_title").to_string())
                    .w(px(480.))
                    .max_w_full()
                    .max_h_full()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .rounded_md()
                    .bg(colors.popover)
                    .border_1()
                    .border_color(colors.border)
                    .shadow_md()
                    .p_5()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(Icon::new(IconName::TriangleAlert).text_color(colors.warning))
                            .child(
                                div()
                                    .font_semibold()
                                    .text_lg()
                                    .text_color(colors.foreground)
                                    .child(rust_i18n::t!("app.session.running_switch_title").to_string()),
                            ),
                    )
                    .child(
                        div()
                            .id("running-confirm-body")
                            .test_support()
                            .text_sm()
                            .text_color(colors.muted_foreground)
                            .child(body),
                    )
                    .when(!self.queued_submissions.is_empty(), |card| {
                        card.child(
                            div()
                                .id("running-confirm-queue-warning")
                                .test_support()
                                .text_sm()
                                .text_color(colors.warning)
                                .child(rust_i18n::t!("app.session.running_switch_queue_warning").to_string()),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .child(
                                desktop_button("running-confirm-cancel")
                                    .label(rust_i18n::t!(i18n::CONFIGURATION_CANCEL).to_string())
                                    .ghost()
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.cancel_running_confirm(window, cx);
                                    })),
                            )
                            .child(
                                desktop_button("running-confirm-stop")
                                    .label(rust_i18n::t!("app.session.running_switch_stop").to_string())
                                    .danger()
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.confirm_running_switch(window, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}

/// Localized modal body for one intent. Pure so unit tests pin the
/// running/target interpolation without rendering.
fn running_switch_body_text(intent: &RunningSwitchIntent, running: &str, target: Option<&str>) -> String {
    match intent {
        RunningSwitchIntent::NewThread => {
            rust_i18n::t!("app.session.running_switch_new_body", thread = running).to_string()
        }
        RunningSwitchIntent::LoadSession { .. } => {
            rust_i18n::t!("app.session.running_switch_load_body", target = target.unwrap_or_default(), thread = running)
                .to_string()
        }
        RunningSwitchIntent::CloseSession => {
            rust_i18n::t!("app.session.running_switch_close_body", thread = running).to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_bodies_name_the_running_and_target_threads() {
        crate::desktop::localization::initialize(crate::desktop::localization::UiLanguage::English);
        assert_eq!(
            running_switch_body_text(&RunningSwitchIntent::NewThread, "Live", None),
            "Starting a new thread stops the turn running in \"Live\"."
        );
        // Asymmetric titles catch a running/target swap.
        let load = RunningSwitchIntent::LoadSession { target: "goal".into(), untab: Vec::new() };
        assert_eq!(
            running_switch_body_text(&load, "Live", Some("Goal")),
            "Switching to \"Goal\" stops the turn running in \"Live\"."
        );
        assert_eq!(
            running_switch_body_text(&RunningSwitchIntent::CloseSession, "Live", None),
            "Closing \"Live\" stops the running turn."
        );
    }
}
