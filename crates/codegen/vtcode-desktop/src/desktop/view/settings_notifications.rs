use super::*;

impl DesktopView {
    pub(super) fn render_notification_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let notifications = &self.preferences.notifications;
        let group_title = rust_i18n::t!("app.settings.notifications.group").to_string();
        div()
            .id("notification-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_6()
            .child(accessible_group_box(
                "notifications-delivery-group",
                group_title.clone(),
                settings_group_box(group_title, &colors)
                    .child(settings_row(
                        rust_i18n::t!("app.settings.notifications.enable").to_string(),
                        rust_i18n::t!("app.settings.notifications.enable_description").to_string(),
                        settings_switch(
                            "notifications-enabled",
                            rust_i18n::t!("app.settings.notifications.enable").to_string(),
                            notifications.enabled(),
                            cx.listener(|view, checked, _, cx| {
                                view.set_notification_enabled(*checked, cx);
                            }),
                        ),
                        &colors,
                    ))
                    .child(settings_row(
                        rust_i18n::t!("app.settings.notifications.turn_completion.title").to_string(),
                        rust_i18n::t!("app.settings.notifications.turn_completion.description").to_string(),
                        Select::new(&self.turn_completion_select)
                            .id("turn-completion-select")
                            .accessibility_label(
                                rust_i18n::t!("app.settings.notifications.turn_completion.title").to_string(),
                            )
                            .menu_max_h(px(320.))
                            .w(px(240.)),
                        &colors,
                    ))
                    .child(settings_row(
                        rust_i18n::t!("app.settings.notifications.permission").to_string(),
                        rust_i18n::t!("app.settings.notifications.permission_description").to_string(),
                        settings_switch(
                            "notifications-permission",
                            rust_i18n::t!("app.settings.notifications.permission").to_string(),
                            notifications.permission_prompts(),
                            cx.listener(|view, checked, _, cx| {
                                view.set_notification_permission(*checked, cx);
                            }),
                        ),
                        &colors,
                    ))
                    .child(settings_row(
                        rust_i18n::t!("app.settings.notifications.questions").to_string(),
                        rust_i18n::t!("app.settings.notifications.questions_description").to_string(),
                        settings_switch(
                            "notifications-questions",
                            rust_i18n::t!("app.settings.notifications.questions").to_string(),
                            notifications.questions(),
                            cx.listener(|view, checked, _, cx| {
                                view.set_notification_questions(*checked, cx);
                            }),
                        ),
                        &colors,
                    ))
                    .child(settings_row(
                        rust_i18n::t!("app.settings.notifications.updates").to_string(),
                        rust_i18n::t!("app.settings.notifications.updates_description").to_string(),
                        settings_switch(
                            "notifications-updates",
                            rust_i18n::t!("app.settings.notifications.updates").to_string(),
                            notifications.updates(),
                            cx.listener(|view, checked, _, cx| {
                                view.set_notification_updates(*checked, cx);
                            }),
                        ),
                        &colors,
                    ))
                    .child(settings_row(
                        rust_i18n::t!("app.settings.notifications.updates_status").to_string(),
                        self.update_status_description(),
                        desktop_button("check-for-updates")
                            .label(rust_i18n::t!("app.settings.notifications.check_for_updates").to_string())
                            .ghost()
                            .small()
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.request_update_status();
                                cx.notify();
                            })),
                        &colors,
                    )),
            ))
            .into_any_element()
    }

    fn update_status_description(&self) -> String {
        let Some(status) = self.update_status.as_ref() else {
            return rust_i18n::t!("app.settings.notifications.status_unavailable").to_string();
        };
        match (status.update_available, status.latest_version.as_deref()) {
            (true, Some(latest)) => {
                format!("{} {latest}", rust_i18n::t!("app.settings.notifications.update_available"))
            }
            (true, None) => rust_i18n::t!("app.settings.notifications.status_unavailable").to_string(),
            (false, _) => {
                format!("{} ({})", rust_i18n::t!("app.settings.notifications.up_to_date"), status.current_version)
            }
        }
    }

    fn set_notification_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.preferences.notifications.enabled() == enabled {
            return;
        }
        self.preferences.notifications.enabled = Some(enabled);
        self.save_preferences();
        cx.notify();
    }

    fn set_notification_updates(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.preferences.notifications.updates() == enabled {
            return;
        }
        self.preferences.notifications.updates = Some(enabled);
        self.save_preferences();
        cx.notify();
    }

    fn set_notification_permission(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.preferences.notifications.permission_prompts() == enabled {
            return;
        }
        self.preferences.notifications.permission_prompts = Some(enabled);
        self.save_preferences();
        cx.notify();
    }

    fn set_notification_questions(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.preferences.notifications.questions() == enabled {
            return;
        }
        self.preferences.notifications.questions = Some(enabled);
        self.save_preferences();
        cx.notify();
    }

    pub(super) fn set_turn_completion_mode(&mut self, mode: preferences::TurnCompletionMode, cx: &mut Context<Self>) {
        if self.preferences.notifications.turn_completion() == mode {
            return;
        }
        self.preferences.notifications.set_turn_completion(mode);
        self.save_preferences();
        cx.notify();
    }
}
