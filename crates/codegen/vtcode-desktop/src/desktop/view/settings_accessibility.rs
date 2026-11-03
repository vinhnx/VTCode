use super::*;

impl DesktopView {
    pub(super) fn render_accessibility_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("accessibility-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_6()
            .child(
                div()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(rust_i18n::t!("app.settings.accessibility.description").to_string()),
            )
            .child(accessible_group_box(
                "accessibility-display-group",
                rust_i18n::t!("app.settings.accessibility.display").to_string(),
                settings_group_box(rust_i18n::t!("app.settings.accessibility.display").to_string(), &cx.theme().colors)
                    .child(
                        gpui_kit::component::switch::Switch::new("show-status-bar")
                            .label(rust_i18n::t!("app.settings.accessibility.show_status_bar").to_string())
                            .checked(self.preferences.show_status_bar)
                            .on_click(cx.listener(|view, _, _, cx| view.toggle_status_bar(cx))),
                    )
                    .child(self.render_reduced_motion(cx)),
            ))
            .into_any_element()
    }
}
