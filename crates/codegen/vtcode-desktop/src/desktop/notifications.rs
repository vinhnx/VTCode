use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::{Anchor, App, Window};

struct ClipboardCopyNotification;

pub(super) fn copied_to_clipboard(window: &mut Window, cx: &mut App) {
    window.push_notification(
        Notification::success("Copied to clipboard.")
            .id::<ClipboardCopyNotification>()
            .placement(Anchor::BottomRight),
        cx,
    );
}
