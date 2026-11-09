use super::settings::switch_action;
use super::*;

/// Cloned render inputs for the notifications pane. Kit item closures run
/// without access to the rendering view, so everything the content
/// builder reads is cloned here per frame; writes go through the view
/// handle in event handlers instead.
#[derive(Clone)]
pub(super) struct NotificationSettingsProps {
    pub(super) notifications: preferences::NotificationPreferences,
    pub(super) turn_completion_select: Entity<preferences::TurnCompletionModeSelectState>,
}

impl NotificationSettingsProps {
    /// Snapshot the view fields the notifications builder reads. Both the
    /// legacy shell wrapper and the Kit adapter build props here.
    pub(super) fn capture(view: &DesktopView) -> Self {
        Self {
            notifications: view.preferences.notifications.clone(),
            turn_completion_select: view.turn_completion_select.clone(),
        }
    }
}

impl DesktopView {
    /// App-context content builder, hostable inside a Kit `SettingItem`
    /// closure. Render-time reads come from cloned props; event-time
    /// writes go through the view handle (events run outside render, so
    /// there is no double-borrow of the rendering view).
    pub(super) fn render_notification_settings_content(
        props: NotificationSettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let notifications = &props.notifications;
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
                            switch_action(&view, Self::set_notification_enabled),
                        ),
                        &colors,
                    ))
                    .child(settings_row(
                        rust_i18n::t!("app.settings.notifications.turn_completion.title").to_string(),
                        rust_i18n::t!("app.settings.notifications.turn_completion.description").to_string(),
                        Select::new(&props.turn_completion_select)
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
                            switch_action(&view, Self::set_notification_permission),
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
                            switch_action(&view, Self::set_notification_questions),
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
                            switch_action(&view, Self::set_notification_updates),
                        ),
                        &colors,
                    )),
            ))
            .into_any_element()
    }

    pub(super) fn render_notification_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        Self::render_notification_settings_content(NotificationSettingsProps::capture(self), cx.entity(), cx)
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
