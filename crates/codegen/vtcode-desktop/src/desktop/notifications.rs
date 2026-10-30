use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::{Anchor, App, Window};

use super::strings::app;

struct ClipboardCopyNotification;
struct BackgroundAgentActivityNotification;
struct UpdateAvailableNotification;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BackgroundAgentActivity {
    Completed,
    NeedsAttention,
    PermissionRequired,
    QuestionWaiting,
    ServiceDisconnected,
}

pub(super) fn initialize(cx: &mut App) {
    cx.set_app_identity("com.vinhnx.vtcode.desktop", app::NAME);
}

pub(super) fn copied_to_clipboard(window: &mut Window, cx: &mut App) {
    window.push_notification(
        Notification::success(rust_i18n::t!("app.notifications.copied").to_string())
            .id::<ClipboardCopyNotification>()
            .placement(Anchor::BottomRight),
        cx,
    );
}

/// Pure delivery decision for agent-activity notifications, separated from
/// event mapping so the preference matrix is unit-testable.
pub(super) fn activity_allowed(
    activity: BackgroundAgentActivity,
    notifications: &super::preferences::NotificationPreferences,
    window_active: bool,
) -> bool {
    if !notifications.enabled() {
        return false;
    }
    match activity {
        BackgroundAgentActivity::Completed | BackgroundAgentActivity::NeedsAttention => {
            match notifications.turn_completion() {
                super::preferences::TurnCompletionMode::Always => true,
                super::preferences::TurnCompletionMode::Unfocused => !window_active,
                super::preferences::TurnCompletionMode::Never => false,
            }
        }
        BackgroundAgentActivity::PermissionRequired => notifications.permission_prompts(),
        BackgroundAgentActivity::QuestionWaiting => notifications.questions(),
        // Disconnects are rare and infrastructure-critical: the master
        // toggle alone governs them, regardless of window focus.
        BackgroundAgentActivity::ServiceDisconnected => true,
    }
}

/// Map an interaction to its notification activity. Approvals gate on the
/// permission toggle; every other interaction kind asks for input.
pub(super) fn activity_for_interaction_kind(kind: vtcode_desktop_protocol::InteractionKind) -> BackgroundAgentActivity {
    match kind {
        vtcode_desktop_protocol::InteractionKind::Approval | vtcode_desktop_protocol::InteractionKind::PlanApproval => {
            BackgroundAgentActivity::PermissionRequired
        }
        vtcode_desktop_protocol::InteractionKind::Question
        | vtcode_desktop_protocol::InteractionKind::Choice
        | vtcode_desktop_protocol::InteractionKind::Other => BackgroundAgentActivity::QuestionWaiting,
    }
}

pub(super) fn background_agent_activity(window: &mut Window, activity: BackgroundAgentActivity, cx: &mut App) {
    let message = match activity {
        BackgroundAgentActivity::Completed => rust_i18n::t!("app.notifications.agent_done").to_string(),
        BackgroundAgentActivity::NeedsAttention => rust_i18n::t!("app.notifications.agent_attention").to_string(),
        BackgroundAgentActivity::PermissionRequired => rust_i18n::t!("app.notifications.approval_waiting").to_string(),
        BackgroundAgentActivity::QuestionWaiting => rust_i18n::t!("app.notifications.input_waiting").to_string(),
        BackgroundAgentActivity::ServiceDisconnected => {
            rust_i18n::t!("app.notifications.service_disconnected").to_string()
        }
    };
    window.push_notification(
        Notification::info(message)
            .title(app::NAME)
            .id::<BackgroundAgentActivityNotification>()
            .system(),
        cx,
    );
}

/// Announce a newer VT Code release. Focused windows get an in-app toast so
/// the startup check is never silently dropped; backgrounded windows (or an
/// explicit always-notify preference) get a system banner instead.
pub(super) fn update_available(window: &mut Window, latest: &str, system: bool, cx: &mut App) {
    let message = rust_i18n::t!("app.notifications.update_available", latest = latest).to_string();
    let notification = Notification::info(message).title(app::NAME).id::<UpdateAvailableNotification>();
    if system {
        window.push_notification(notification.system(), cx);
    } else {
        window.push_notification(notification.placement(Anchor::BottomRight), cx);
    }
}
