use super::*;

/// Cloned render inputs for the accessibility pane (see
/// `NotificationSettingsProps` for the Kit-hosting rationale).
#[derive(Clone)]
pub(super) struct AccessibilitySettingsProps {
    pub(super) show_status_bar: bool,
    pub(super) reduced_motion: preferences::ReducedMotionPreference,
}

impl AccessibilitySettingsProps {
    /// Snapshot the view fields the accessibility builder reads. Both the
    /// legacy shell wrapper and the Kit adapter build props here.
    pub(super) fn capture(view: &DesktopView) -> Self {
        Self {
            show_status_bar: view.preferences.show_status_bar,
            reduced_motion: view.preferences.reduced_motion,
        }
    }
}

impl DesktopView {
    /// App-context content builder, hostable inside a Kit `SettingItem`
    /// closure.
    pub(super) fn render_accessibility_settings_content(
        props: AccessibilitySettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        div()
            .id("accessibility-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_6()
            .child(row_description(
                rust_i18n::t!("app.settings.accessibility.description").to_string(),
                &cx.theme().colors,
            ))
            .child(accessible_group_box(
                "accessibility-display-group",
                rust_i18n::t!("app.settings.accessibility.display").to_string(),
                settings_group_box(rust_i18n::t!("app.settings.accessibility.display").to_string(), &cx.theme().colors)
                    .child(
                        gpui_kit::component::switch::Switch::new("show-status-bar")
                            .label(rust_i18n::t!("app.settings.accessibility.show_status_bar").to_string())
                            .checked(props.show_status_bar)
                            .on_click({
                                let view = view.clone();
                                move |_, _: &mut Window, cx: &mut App| {
                                    view.update(cx, |view, cx| view.toggle_status_bar(cx));
                                }
                            }),
                    )
                    .child(Self::render_reduced_motion_content(props.reduced_motion, view, cx)),
            ))
            .into_any_element()
    }

    pub(super) fn render_accessibility_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        Self::render_accessibility_settings_content(AccessibilitySettingsProps::capture(self), cx.entity(), cx)
    }
}
