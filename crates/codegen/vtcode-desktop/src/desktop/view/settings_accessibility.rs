use super::*;

/// Cloned render inputs for the accessibility pane (see
/// `NotificationSettingsProps` for the Kit-hosting rationale).
#[derive(Clone)]
pub(super) struct AccessibilitySettingsProps {
    pub(super) show_status_bar: bool,
    pub(super) reduced_motion: preferences::ReducedMotionPreference,
    pub(super) contrast_slider: Entity<gpui_kit::component::slider::SliderState>,
    pub(super) contrast_focus: FocusHandle,
    pub(super) contrast: u8,
}

impl AccessibilitySettingsProps {
    /// Snapshot the view fields the accessibility builder reads. Both the
    /// legacy shell wrapper and the Kit adapter build props here.
    pub(super) fn capture(view: &DesktopView) -> Self {
        Self {
            show_status_bar: view.preferences.show_status_bar,
            reduced_motion: view.preferences.reduced_motion,
            contrast_slider: view.contrast_slider.clone(),
            contrast_focus: view.contrast_focus.clone(),
            contrast: view.preferences.contrast,
        }
    }
}

impl DesktopView {
    pub(super) fn render_contrast_settings_content(
        props: &AccessibilitySettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let _ = &view;
        let label = rust_i18n::t!("app.settings.accessibility.contrast").to_string();
        let slider = props.contrast_slider.clone();
        let focus = props.contrast_focus.clone();
        let ring = cx.theme().colors.ring;
        accessible_group_box(
            "accessibility-contrast-group",
            label.clone(),
            settings_group_box(label.clone(), &cx.theme().colors).child(settings_row(
                label.clone(),
                rust_i18n::t!("app.settings.accessibility.contrast_description").to_string(),
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("contrast-slider-control")
                            .test_support()
                            .track_focus(&props.contrast_focus)
                            .focus_visible(move |style| style.border_1().border_color(ring))
                            .capture_any_mouse_down(move |event, window, cx| {
                                if event.button == MouseButton::Left {
                                    focus.focus(window, cx);
                                }
                            })
                            .on_key_down(move |event, window, cx| {
                                if event.keystroke.modifiers.control
                                    || event.keystroke.modifiers.alt
                                    || event.keystroke.modifiers.platform
                                    || event.keystroke.modifiers.function
                                {
                                    return;
                                }
                                let delta = match event.keystroke.key.as_str() {
                                    "left" | "down" => -1.0,
                                    "right" | "up" => 1.0,
                                    _ => return,
                                };
                                cx.stop_propagation();
                                slider.update(cx, |state, cx| {
                                    let value = (state.value().start() + delta).clamp(0.0, 100.0);
                                    state.set_value(value, window, cx);
                                });
                            })
                            .child(
                                gpui_kit::component::slider::Slider::new(&props.contrast_slider)
                                    .accessibility_label(label)
                                    .w(px(180.)),
                            ),
                    )
                    .child(props.contrast.to_string()),
                &cx.theme().colors,
            )),
        )
    }

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
                    .child(Self::render_reduced_motion_content(props.reduced_motion, view.clone(), cx)),
            ))
            .child(Self::render_contrast_settings_content(&props, view, cx))
            .into_any_element()
    }

    pub(super) fn render_accessibility_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        Self::render_accessibility_settings_content(AccessibilitySettingsProps::capture(self), cx.entity(), cx)
    }
}
