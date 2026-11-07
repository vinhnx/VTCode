use super::*;

/// Provider group id shared by the empty and loaded states so the group
/// keeps stable identity across configuration changes.
const PROVIDER_SETTINGS_GROUP_ID: &str = "provider-settings-group";

impl DesktopView {
    pub(super) fn render_vtcode_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let content = div()
            .id("vtcode-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_6()
            .child(self.render_provider_settings(cx))
            .child(self.render_shared_preferences(cx))
            .child(self.render_cli_configuration(cx))
            .child(self.render_licenses_settings(cx))
            .child(self.render_menu_bar_settings(cx))
            .child(self.render_login_items_settings(cx));
        content.into_any_element()
    }

    fn render_provider_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let Some(configuration) = self.configuration.as_ref() else {
            return accessible_group_box(
                PROVIDER_SETTINGS_GROUP_ID,
                rust_i18n::t!("app.settings.provider.group").to_string(),
                settings_group_box(rust_i18n::t!("app.settings.provider.title").to_string(), &theme.colors).child(
                    div()
                        .text_color(theme.colors.muted_foreground)
                        .child(rust_i18n::t!("app.settings.provider.connect_hint").to_string()),
                ),
            );
        };

        let provider = if configuration.provider.is_empty() {
            rust_i18n::t!("app.settings.provider.not_configured").to_string()
        } else {
            configuration.provider.clone()
        };
        let mut group = settings_group_box(rust_i18n::t!("app.settings.provider.title").to_string(), &theme.colors)
            .child(
                div()
                    .id("provider-details")
                    .test_support()
                    .role(gpui_kit::Role::Group)
                    .aria_label(rust_i18n::t!("app.settings.provider.accessibility_label").to_string())
                    .child(
                        DescriptionList::horizontal()
                            .columns(1)
                            .label_width(px(128.))
                            .bordered(false)
                            .item(rust_i18n::t!("app.settings.provider.provider").to_string(), provider, 1)
                            .item(
                                rust_i18n::t!("app.settings.provider.model").to_string(),
                                configuration.model.clone(),
                                1,
                            ),
                    ),
            );
        // Owned copies for the Terminal fallback row so the secret form can
        // borrow `cx` mutably below.
        let mono_font_family = theme.mono_font_family.clone();
        let muted_foreground = theme.colors.muted_foreground;
        if let Some(secret) = configuration.secret_store.as_ref() {
            group = group.child(self.render_secret_store_form(secret, cx));
        }
        if configuration.secret_store.is_none() && !configuration.credential_setup_command.is_empty() {
            let command = configuration.credential_setup_command.clone();
            group = group.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .font_family(mono_font_family)
                            .text_color(muted_foreground)
                            .truncate()
                            .child(command.clone()),
                    )
                    .child(
                        desktop_button("copy-credential-command")
                            .label(rust_i18n::t!("gpui_component.Input.Copy").to_string())
                            .ghost()
                            .on_click(move |_, window, app| {
                                app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(command.to_string()));
                                super::super::notifications::copied_to_clipboard(window, app);
                            }),
                    ),
            );
        }
        accessible_group_box(
            PROVIDER_SETTINGS_GROUP_ID,
            rust_i18n::t!("app.settings.provider.group").to_string(),
            group,
        )
    }
}
