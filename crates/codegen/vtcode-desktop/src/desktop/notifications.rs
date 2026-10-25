use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::{Anchor, App, Window};

struct ClipboardCopyNotification;
struct BackgroundAgentActivityNotification;

#[derive(Clone, Copy)]
pub(super) enum BackgroundAgentActivity {
    Completed,
    NeedsAttention,
    WaitingForDecision,
}

pub(super) fn initialize(cx: &mut App) {
    cx.set_app_identity("com.vinhnx.vtcode.desktop", "VT Code");
}

pub(super) fn copied_to_clipboard(window: &mut Window, cx: &mut App) {
    window.push_notification(
        Notification::success("Copied to clipboard.")
            .id::<ClipboardCopyNotification>()
            .placement(Anchor::BottomRight),
        cx,
    );
}

pub(super) fn background_agent_activity(window: &mut Window, activity: BackgroundAgentActivity, cx: &mut App) {
    let message = match activity {
        BackgroundAgentActivity::Completed => "Your agent run has finished.",
        BackgroundAgentActivity::NeedsAttention => "Your agent run needs attention.",
        BackgroundAgentActivity::WaitingForDecision => "An agent run is waiting for your input.",
    };
    window.push_notification(
        Notification::info(message)
            .title("VT Code")
            .id::<BackgroundAgentActivityNotification>()
            .system(),
        cx,
    );
}
