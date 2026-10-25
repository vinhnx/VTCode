use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SettingsTab {
    Appearance,
    Accessibility,
    Shortcuts,
    VtCode,
    Connections,
}

impl SettingsTab {
    const ALL: [Self; 5] = [
        Self::Appearance,
        Self::Accessibility,
        Self::Shortcuts,
        Self::VtCode,
        Self::Connections,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Accessibility => "Accessibility",
            Self::Shortcuts => "Shortcuts",
            Self::VtCode => "VT Code",
            Self::Connections => "Connections",
        }
    }
}

impl DesktopView {
    pub(super) fn render_settings_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let selected_index = SettingsTab::ALL
            .iter()
            .position(|tab| *tab == self.settings_tab)
            .unwrap_or_default();
        let content = match self.settings_tab {
            SettingsTab::Appearance => self.render_appearance_settings(cx),
            SettingsTab::Accessibility => self.render_accessibility_settings(cx),
            SettingsTab::Shortcuts => div()
                .id("keyboard-settings")
                .test_support()
                .child(self.render_shortcuts(cx))
                .into_any_element(),
            SettingsTab::VtCode => self.render_vtcode_settings(cx),
            SettingsTab::Connections => self.render_connections_settings(cx),
        };

        div()
            .id("settings-panel")
            .lock_scroll_axis()
            .track_focus(&self.settings_focus)
            .test_support()
            .role(gpui_kit::Role::Dialog)
            .aria_label("Settings")
            .w(px(860.))
            .max_w_full()
            .h_full()
            .max_h_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                DialogContent::new()
                    .w_full()
                    .h_full()
                    .max_h_full()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_6()
                    .rounded_xl()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.background)
                    .shadow_lg()
                    .text_sm()
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap_4()
                            .child(
                                DialogHeader::new()
                                    .child(DialogTitle::new().text_2xl().child("Settings"))
                                    .child(DialogDescription::new().child("Customize VT Code Desktop")),
                            )
                            .child(
                                desktop_button("close-settings")
                                    .icon(IconName::X)
                                    .accessibility_label("Close settings")
                                    .ghost()
                                    .on_click(cx.listener(|view, _, window, cx| view.close_settings(window, cx))),
                            ),
                    )
                    .child(
                        div()
                            .id("settings-navigation")
                            .test_support()
                            .track_focus(&self.settings_navigation_focus)
                            .key_context("VTCodeSettings")
                            .role(gpui_kit::Role::Group)
                            .aria_label("Settings categories")
                            .aria_description("Use Left and Right arrows, Home, or End to choose a category.")
                            .border_1()
                            .border_color(colors.border.opacity(0.))
                            .focus_visible(|style| style.border_color(colors.ring))
                            .on_action(cx.listener(|view, _: &NextSettingsTab, window, cx| {
                                view.cycle_settings_tab(true, window, cx)
                            }))
                            .on_action(cx.listener(|view, _: &PreviousSettingsTab, window, cx| {
                                view.cycle_settings_tab(false, window, cx)
                            }))
                            .on_action(cx.listener(|view, _: &FirstSettingsTab, window, cx| {
                                view.select_settings_tab(0, window, cx)
                            }))
                            .on_action(cx.listener(|view, _: &LastSettingsTab, window, cx| {
                                view.select_settings_tab(SettingsTab::ALL.len().saturating_sub(1), window, cx)
                            }))
                            .child(
                                TabBar::new("settings-tabs")
                                    .underline()
                                    .small()
                                    .w_full()
                                    .selected_index(selected_index)
                                    .on_click(cx.listener(|view, index: &usize, window, cx| {
                                        view.select_settings_tab(*index, window, cx)
                                    }))
                                    .children(SettingsTab::ALL.map(|tab| Tab::new().label(tab.label()))),
                            ),
                    )
                    .child(
                        div()
                            .id("settings-content")
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .lock_scroll_axis()
                            .test_support()
                            .track_scroll(&self.settings_scroll)
                            .overflow_y_scroll()
                            .vertical_scrollbar(&self.settings_scroll)
                            .pr_2()
                            .child(content),
                    ),
            )
            .into_any_element()
    }

    fn render_appearance_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let swatches = themes::SYSTEM_SWATCHES
            .iter()
            .copied()
            .chain(themes::all_themes().iter().copied().map(themes::SwatchEntry::Palette));
        let swatch_rows = swatches.map(|entry| self.render_theme_swatch(entry, cx)).collect::<Vec<_>>();
        let theme = cx.theme();
        let colors = &theme.colors;
        let active_theme_profile = if self.theme_mode == ThemeMode::Light {
            "light"
        } else {
            "dark"
        };
        let mut content = div()
            .id("appearance-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_5()
            .child(accessible_group_box(
                "appearance-visual-style-group",
                "Visual style",
                GroupBox::new()
                    .flex_shrink_0()
                    .fill()
                    .title("Visual style")
                    .child(
                        gpui_kit::component::switch::Switch::new("separate-theme-profiles")
                            .label("Separate light and dark modes")
                            .checked(self.preferences.separate_theme_profiles)
                            .on_click(cx.listener(|view, _, _, cx| view.toggle_separate_theme_profiles(cx))),
                    )
                    .when(self.preferences.separate_theme_profiles, |group| {
                        group.child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .id("theme-profile-hint")
                                        .role(gpui_kit::Role::Label)
                                        .aria_label(format!("Editing the {active_theme_profile} appearance profile"))
                                        .text_sm()
                                        .child("Editing profile"),
                                )
                                .children([ThemeMode::Light, ThemeMode::Dark].into_iter().map(|mode| {
                                    let light = mode == ThemeMode::Light;
                                    let id = if light { "light" } else { "dark" };
                                    let label = if light { "Light" } else { "Dark" };
                                    let selected = self.theme_mode == mode;
                                    desktop_button(format!("edit-{id}-profile"))
                                        .label(label)
                                        .accessibility_label(format!(
                                            "Edit {id} appearance profile{}",
                                            if selected { " (selected)" } else { "" }
                                        ))
                                        .ghost()
                                        .when(selected, |button| {
                                            button.bg(colors.secondary).text_color(colors.foreground)
                                        })
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            if view.theme_mode != mode {
                                                view.apply_theme_mode(mode, window, cx);
                                            }
                                        }))
                                })),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(section_label("THEMES", colors.muted_foreground))
                            .child(div().flex().gap_1().children([false, true].into_iter().map(|forward| {
                                let scroll = self.theme_scroll.clone();
                                desktop_button(if forward { "next-themes" } else { "previous-themes" })
                                    .icon(if forward {
                                        IconName::ChevronRight
                                    } else {
                                        IconName::ChevronLeft
                                    })
                                    .accessibility_label(if forward { "Next themes" } else { "Previous themes" })
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        let mut offset = scroll.offset();
                                        offset.x += px(if forward { -336. } else { 336. });
                                        scroll.set_offset(offset);
                                        cx.notify();
                                    }))
                            }))),
                    )
                    .child(
                        div()
                            .id("theme-list")
                            .test_support()
                            .role(gpui_kit::Role::Group)
                            .aria_label("Visual themes")
                            .w_full()
                            .min_w_0()
                            .flex_shrink_0()
                            .h(px(112.))
                            .relative()
                            .child(
                                div()
                                    .id("theme-scroller")
                                    .lock_scroll_axis()
                                    .size_full()
                                    .overflow_x_scroll()
                                    .track_scroll(&self.theme_scroll)
                                    .flex()
                                    .items_start()
                                    .gap_2()
                                    .children(swatch_rows)
                                    .test_support(),
                            )
                            .child(ScrollableMask::vertical_wheel(&self.theme_scroll))
                            .child(Scrollbar::horizontal(&self.theme_scroll)),
                    ),
            ))
            .child(accessible_group_box(
                "appearance-typography-group",
                "Typography",
                GroupBox::new()
                    .flex_shrink_0()
                    .fill()
                    .title("Typography")
                    .child(self.render_font_family(FontTarget::Ui, cx))
                    .child(self.render_font_family(FontTarget::Code, cx))
                    .child(self.render_font_size(false, cx))
                    .child(self.render_font_size(true, cx))
                    .child(render_markdown(
                        "appearance-code-preview".into(),
                        "```rust\nfn main() { /* VT Code */ }\n```".into(),
                        cx,
                    )),
            ));

        content = content.child(desktop_button("reset-appearance").label("Reset appearance").ghost().on_click(
            cx.listener(|view, _, window, cx| {
                view.preferences.separate_theme_profiles = false;
                view.preferences.light_theme = None;
                view.preferences.dark_theme = None;
                view.preferences.light_typography = None;
                view.preferences.dark_typography = None;
                view.preferences.ui_font_family = None;
                view.preferences.code_font_family = None;
                view.preferences.ui_font_size = None;
                view.preferences.code_font_size = None;
                view.preferences.reduced_motion = preferences::ReducedMotionPreference::System;
                preferences::apply_reduced_motion(view.preferences.reduced_motion, view.system_motion_baseline, cx);
                view.apply_theme(themes::DEFAULT_THEME_ID.to_string(), window, cx);
            }),
        ));
        content.into_any_element()
    }

    fn render_accessibility_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("accessibility-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("Adjust motion and the amount of persistent interface detail."),
            )
            .child(accessible_group_box(
                "accessibility-display-group",
                "Display",
                GroupBox::new()
                    .flex_shrink_0()
                    .fill()
                    .title("Display")
                    .child(
                        gpui_kit::component::switch::Switch::new("show-status-bar")
                            .label("Show status bar")
                            .checked(self.preferences.show_status_bar)
                            .on_click(cx.listener(|view, _, _, cx| view.toggle_status_bar(cx))),
                    )
                    .child(self.render_reduced_motion(cx)),
            ))
            .into_any_element()
    }

    fn render_vtcode_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let content = div()
            .id("vtcode-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_4()
            .child(self.render_provider_settings(cx))
            .child(self.render_shared_preferences(cx))
            .child(self.render_cli_configuration(cx));
        content.into_any_element()
    }

    fn render_provider_settings(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let Some(configuration) = self.configuration.as_ref() else {
            return accessible_group_box(
                "provider-settings-group",
                "Provider settings",
                GroupBox::new().flex_shrink_0().fill().title("Provider").child(
                    div()
                        .text_color(theme.colors.muted_foreground)
                        .child("Connect to your workspace to view provider and model details."),
                ),
            );
        };

        let provider = if configuration.provider.is_empty() {
            "Not configured".to_string()
        } else {
            configuration.provider.clone()
        };
        let mut group = GroupBox::new().flex_shrink_0().fill().title("Provider").child(
            div()
                .id("provider-details")
                .test_support()
                .role(gpui_kit::Role::Group)
                .aria_label("Provider and model configuration")
                .child(
                    DescriptionList::horizontal()
                        .columns(1)
                        .label_width(px(128.))
                        .bordered(false)
                        .item("Provider", provider, 1)
                        .item("Model", configuration.model.clone(), 1),
                ),
        );
        if !configuration.credential_setup_command.is_empty() {
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
                            .font_family(theme.mono_font_family.clone())
                            .text_color(theme.colors.muted_foreground)
                            .truncate()
                            .child(command.clone()),
                    )
                    .child(desktop_button("copy-credential-command").label("Copy").ghost().on_click(
                        move |_, window, app| {
                            app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(command.to_string()));
                            super::super::notifications::copied_to_clipboard(window, app);
                        },
                    )),
            );
        }
        accessible_group_box("provider-settings-group", "Provider settings", group)
    }

    fn render_connections_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("connections-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_4()
            .child(self.render_webmcp_settings(cx))
            .when_some(self.service_info.as_ref(), |content, info| {
                content.child(accessible_group_box(
                    "local-service-settings-group",
                    "Local service",
                    GroupBox::new().flex_shrink_0().outline().title("Local service").child(
                        div()
                            .id("local-service-details")
                            .test_support()
                            .role(gpui_kit::Role::Group)
                            .aria_label("Local service connection details")
                            .child(
                                DescriptionList::horizontal()
                                    .columns(1)
                                    .label_width(px(128.))
                                    .bordered(false)
                                    .item("Runtime", "VT Code", 1)
                                    .item("Version", info.server_version.clone(), 1)
                                    .item("Protocol", format!("{}.{}", info.protocol.major, info.protocol.minor), 1),
                            ),
                    ),
                ))
            })
            .into_any_element()
    }

    pub(super) fn select_settings_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = SettingsTab::ALL.get(index) {
            self.settings_tab = *tab;
            self.settings_navigation_focus.focus(window, cx);
            cx.notify();
        }
    }

    pub(super) fn cycle_settings_tab(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let count = SettingsTab::ALL.len();
        let current = SettingsTab::ALL
            .iter()
            .position(|tab| *tab == self.settings_tab)
            .unwrap_or_default();
        let index = if forward {
            (current + 1) % count
        } else {
            (current + count - 1) % count
        };
        self.select_settings_tab(index, window, cx);
    }
}
