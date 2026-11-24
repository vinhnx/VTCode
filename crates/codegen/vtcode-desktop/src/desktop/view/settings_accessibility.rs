use super::*;

/// Cloned render inputs for the accessibility pane (see
/// `NotificationSettingsProps` for the Kit-hosting rationale).
#[derive(Clone)]
pub(super) struct AccessibilitySettingsProps {
    pub(super) show_status_bar: bool,
    pub(super) reduced_motion: preferences::ReducedMotionPreference,
    pub(super) motion_choice_focus: FocusHandle,
    pub(super) contrast_slider: Entity<gpui_kit::component::slider::SliderState>,
    pub(super) contrast_focus: FocusHandle,
    pub(super) contrast: u8,
    pub(super) high_contrast: bool,
}

impl AccessibilitySettingsProps {
    /// Snapshot the view fields the accessibility builder reads. Both the
    /// legacy shell wrapper and the Kit adapter build props here.
    pub(super) fn capture(view: &DesktopView) -> Self {
        Self {
            show_status_bar: view.preferences.show_status_bar,
            reduced_motion: view.preferences.reduced_motion,
            motion_choice_focus: view.motion_choice_focus.clone(),
            contrast_slider: view.contrast_slider.clone(),
            contrast_focus: view.contrast_focus.clone(),
            contrast: view.preferences.contrast(),
            high_contrast: view.preferences.high_contrast,
        }
    }
}

impl DesktopView {
    pub(super) fn render_contrast_settings_content(
        props: &AccessibilitySettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let label = rust_i18n::t!("app.settings.accessibility.contrast").to_string();
        let slider = props.contrast_slider.clone();
        let focus = props.contrast_focus.clone();
        // Arrow keys call `SliderState::set_value`, which notifies but emits no
        // `SliderEvent::Release`. Persist here so a discrete keyboard change is
        // never lost (the drag path saves once on Release instead).
        let save_view = view.clone();
        let drag_view = view.clone();
        let drag_view_up = view.clone();
        let drag_view_out = view.clone();
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
                            // This wrapper is the single focus stop; the Kit
                            // slider keeps the value/actions but is not focused.
                            // Name the focused node so it can be announced.
                            .role(gpui_kit::Role::Group)
                            .aria_label(label.clone())
                            .track_focus(&props.contrast_focus)
                            .focus_visible(move |style| style.border_1().border_color(ring))
                            .capture_any_mouse_down(move |event, window, cx| {
                                if event.button == MouseButton::Left {
                                    focus.focus(window, cx);
                                    drag_view.update(cx, |view, _| view.contrast_drag_active = true);
                                }
                            })
                            // Safety net: a press that starts on the wrapper but
                            // not the slider track emits no `Release`, so clear
                            // the drag flag on any pointer-up too.
                            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                                drag_view_up.update(cx, |view, _| view.contrast_drag_active = false);
                            })
                            .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
                                drag_view_out.update(cx, |view, _| view.contrast_drag_active = false);
                            })
                            .on_key_down(move |event, window, cx| {
                                if event.keystroke.modifiers.control
                                    || event.keystroke.modifiers.alt
                                    || event.keystroke.modifiers.platform
                                    || event.keystroke.modifiers.function
                                {
                                    return;
                                }
                                let delta = match key::Named::parse(event.keystroke.key.as_str()) {
                                    Some(key::Named::Left | key::Named::Down) => -1.0,
                                    Some(key::Named::Right | key::Named::Up) => 1.0,
                                    _ => return,
                                };
                                cx.stop_propagation();
                                let next = (slider.read(cx).value().start() + delta).clamp(0.0, 100.0);
                                slider.update(cx, |state, cx| state.set_value(next, window, cx));
                                // `set_value` notifies the SliderState asynchronously,
                                // so its `observe_in` callback (which applies the
                                // palette) would run after this handler. Apply and
                                // persist synchronously here; the observer later
                                // sees the same value and short-circuits.
                                save_view.update(cx, |view, cx| {
                                    view.set_contrast(next, window, cx);
                                    view.save_preferences();
                                });
                            })
                            .child(
                                gpui_kit::component::slider::Slider::new(&props.contrast_slider)
                                    .accessibility_label(label)
                                    // The wrapper owns the single Tab stop and the
                                    // arrow-key handler that persists the value.
                                    // Leaving Kit's default stop would add a second
                                    // stop whose keys change the value without saving.
                                    .tab_stop(false)
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
                    .child(
                        gpui_kit::component::switch::Switch::new("high-contrast")
                            .label(rust_i18n::t!("app.settings.accessibility.high_contrast").to_string())
                            .checked(props.high_contrast)
                            .on_click({
                                let view = view.clone();
                                move |checked: &bool, window: &mut Window, cx: &mut App| {
                                    view.update(cx, |view, cx| {
                                        view.set_high_contrast(*checked, window, cx);
                                    });
                                }
                            }),
                    )
                    .child(Self::render_reduced_motion_content(
                        props.reduced_motion,
                        props.motion_choice_focus.clone(),
                        view.clone(),
                        cx,
                    )),
            ))
            .child(Self::render_contrast_settings_content(&props, view, cx))
            .into_any_element()
    }

    pub(super) fn render_accessibility_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        Self::render_accessibility_settings_content(AccessibilitySettingsProps::capture(self), cx.entity(), cx)
    }
}
