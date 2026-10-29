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
        Notification::success("Copied to clipboard.")
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
        BackgroundAgentActivity::Completed => "Your agent run has finished.",
        BackgroundAgentActivity::NeedsAttention => "Your agent run needs attention.",
        BackgroundAgentActivity::PermissionRequired => "An agent run is waiting for your approval.",
        BackgroundAgentActivity::QuestionWaiting => "An agent run is waiting for your input.",
        BackgroundAgentActivity::ServiceDisconnected => "The local VT Code service disconnected.",
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
    let message = format!("VT Code {latest} is available. Check Settings for details.");
    let notification = Notification::info(message).title(app::NAME).id::<UpdateAvailableNotification>();
    if system {
        window.push_notification(notification.system(), cx);
    } else {
        window.push_notification(notification.placement(Anchor::BottomRight), cx);
    }
}
