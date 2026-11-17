//! Chat-width settings row: the layout picker that caps the conversation column.

use super::*;

impl DesktopView {
    pub(super) fn set_chat_width(&mut self, width: preferences::ChatWidth, cx: &mut Context<Self>) {
        if self.preferences.chat_width() == width {
            return;
        }
        self.preferences.set_chat_width(width);
        // The column width changes wrapped-line measurement, so invalidate the
        // cached transcript layout just like a font/contrast change does.
        self.transcript.remeasure();
        self.save_preferences();
        cx.notify();
    }

    pub(super) fn build_chat_width_select(
        preferences: &preferences::Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<preferences::ChatWidthSelectState>, Subscription) {
        let select = cx.new(|cx| preferences::ChatWidthOption::select_state(preferences.chat_width(), window, cx));
        let subscription = cx.subscribe_in(&select, window, |view, _, event, _, cx| {
            if let SelectEvent::Confirm(Some(width)) = event {
                view.set_chat_width(*width, cx);
            }
        });
        (select, subscription)
    }
}

/// The "Chat width" row for the Appearance → Layout group.
pub(super) fn render_chat_width_row(
    select: &Entity<preferences::ChatWidthSelectState>,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> impl IntoElement {
    settings_row(
        rust_i18n::t!("app.settings.appearance.chat_width").to_string(),
        rust_i18n::t!("app.settings.appearance.chat_width_description").to_string(),
        Select::new(select)
            .id("chat-width-select")
            .accessibility_label(rust_i18n::t!("app.settings.appearance.chat_width").to_string())
            .menu_max_h(px(320.))
            .w(px(240.)),
        colors,
    )
}
