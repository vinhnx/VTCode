use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::{Anchor, App, ClickEvent, Window};

use super::strings::app;

struct ClipboardCopyNotification;
struct BackgroundAgentActivityNotification;
struct UpdateAvailableNotification;
/// A thread handed off from the CLI into the desktop app.
struct HandoffNotification;
/// Replace marker for turn-undo outcome toasts, so rapid undos replace
/// rather than stack. Shared with the turn-undo notifier.
pub(super) struct TurnUndoNotification;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BackgroundAgentActivity {
    Completed,
    NeedsAttention,
    PermissionRequired,
    QuestionWaiting,
    ServiceDisconnected,
}

impl BackgroundAgentActivity {
    /// Truthful Kit severity: terminal success stays green, anything needing
    /// a decision warns, and disconnects error. Kit pairs each severity with
    /// a distinct icon, so severity is never carried by color alone.
    pub(super) fn severity(self) -> gpui_kit::component::notification::NotificationType {
        use gpui_kit::component::notification::NotificationType::{Error, Success, Warning};
        match self {
            Self::Completed => Success,
            Self::NeedsAttention | Self::PermissionRequired | Self::QuestionWaiting => Warning,
            Self::ServiceDisconnected => Error,
        }
    }

    /// Localized banner body. Every activity carries its own message so the
    /// delivery split (system vs in-app) never changes what is said.
    pub(super) fn message(self) -> String {
        match self {
            Self::Completed => rust_i18n::t!("app.notifications.agent_done").to_string(),
            Self::NeedsAttention => rust_i18n::t!("app.notifications.agent_attention").to_string(),
            Self::PermissionRequired => rust_i18n::t!("app.notifications.approval_waiting").to_string(),
            Self::QuestionWaiting => rust_i18n::t!("app.notifications.input_waiting").to_string(),
            Self::ServiceDisconnected => rust_i18n::t!("app.notifications.service_disconnected").to_string(),
        }
    }
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

pub(super) fn background_agent_activity(
    window: &mut Window,
    activity: BackgroundAgentActivity,
    system: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut App,
) {
    let notification = Notification::new()
        .message(activity.message())
        .with_type(activity.severity())
        .title(app::NAME)
        .id::<BackgroundAgentActivityNotification>()
        .on_click(on_click);
    // Focused windows toast in-app where the click target lives; backgrounded
    // windows banner through the OS center. System-banner clicks activate the
    // window and fire this same callback, so one handler covers both paths.
    if system {
        window.push_notification(notification.system(), cx);
    } else {
        window.push_notification(notification.placement(Anchor::BottomRight), cx);
    }
}

/// Announce a thread handed off from the CLI. Focused windows get an in-app
/// toast so the handoff is never silently dropped; backgrounded windows get
/// a system banner instead. Clicking either opens the handed-off thread.
///
/// The body names no thread: the handoff archive carries a fresh identifier
/// that the sidebar does not know yet, so any title resolved here would be
/// either missing or an opaque id. The click target carries the identity.
pub(super) fn handoff_received(
    window: &mut Window,
    system: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut App,
) {
    let message = rust_i18n::t!("app.notifications.handoff_received").to_string();
    let notification = Notification::info(message)
        .title(app::NAME)
        .id::<HandoffNotification>()
        .on_click(on_click);
    if system {
        window.push_notification(notification.system(), cx);
    } else {
        window.push_notification(notification.placement(Anchor::BottomRight), cx);
    }
}

/// Pure delivery decision for handoff notices, separated from the emitter so
/// the preference matrix stays unit-testable like [`activity_allowed`].
///
/// A handoff is user-initiated, so focus never suppresses it: the master
/// toggle alone governs it.
pub(super) fn handoff_allowed(notifications: &super::preferences::NotificationPreferences) -> bool {
    notifications.enabled()
}

/// Announce a newer VT Code release. Focused windows get an in-app toast so
/// the startup check is never silently dropped; backgrounded windows (or an
/// explicit always-notify preference) get a system banner instead.
pub(super) fn update_available(
    window: &mut Window,
    latest: &str,
    system: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut App,
) {
    let message = rust_i18n::t!("app.notifications.update_available", latest = latest).to_string();
    let notification = Notification::info(message)
        .title(app::NAME)
        .id::<UpdateAvailableNotification>()
        .on_click(on_click);
    if system {
        window.push_notification(notification.system(), cx);
    } else {
        window.push_notification(notification.placement(Anchor::BottomRight), cx);
    }
}
