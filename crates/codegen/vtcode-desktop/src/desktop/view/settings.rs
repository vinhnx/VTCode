use super::*;
use crate::desktop::localization::UiLanguage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

    fn label(self) -> String {
        match self {
            Self::Appearance => rust_i18n::t!("app.settings.tab.appearance"),
            Self::Accessibility => rust_i18n::t!("app.settings.tab.accessibility"),
            Self::Shortcuts => rust_i18n::t!("app.settings.tab.shortcuts"),
            Self::VtCode => rust_i18n::t!("app.settings.tab.vtcode"),
            Self::Connections => rust_i18n::t!("app.settings.tab.connections"),
        }
        .to_string()
    }

    fn search_keywords(self) -> &'static [&'static str] {
        match self {
            Self::Appearance => &[
                "appearance",
                "theme",
                "themes",
                "color",
                "font",
                "typography",
                "visual style",
                "language",
                "navigation",
                "sidebar",
                "layout",
            ],
            Self::Accessibility => &[
                "accessibility",
                "contrast",
                "reduce motion",
                "motion",
                "display",
                "status bar",
                "trợ năng",
            ],
            Self::Shortcuts => &[
                "shortcuts",
                "shortcut",
                "keyboard",
                "keybinding",
                "bindings",
                "phím tắt",
            ],
            Self::VtCode => &[
                "vt code",
                "config",
                "configuration",
                "provider",
                "model",
                "agent",
                "cli",
                "permissions",
            ],
            Self::Connections => &["connections", "mcp", "webmcp", "server", "bridge"],
        }
    }
}

impl DesktopView {
    pub(super) fn open_settings_search_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() && self.palette_kind == PaletteKind::SettingsSearch {
            self.close_palette(window, cx);
            return;
        }

        let settings = SettingsTab::ALL
            .into_iter()
            .map(|tab| {
                let mut entry = command(tab.label(), PaletteAction::SelectSettingsTab(tab));
                entry.keywords = tab.search_keywords().iter().map(|keyword| (*keyword).to_owned()).collect();
                entry
            })
            .collect();
        let mut groups = vec![(rust_i18n::t!("app.settings.search.group_settings").to_string(), settings)];
        if let Some(snapshot) = self.cli_config_settings.as_ref() {
            let config_group = rust_i18n::t!("app.settings.search.group_cli_config").to_string();
            groups.extend(
                configuration::config_palette_groups(snapshot)
                    .into_iter()
                    .map(|(section, entries)| (format!("{config_group} · {section}"), entries)),
            );
        }
        self.show_palette(PaletteKind::SettingsSearch, groups, window, cx);
    }

    pub(super) fn render_settings_tabs(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        // The sheet title row scales with the active UI font (40px at 16px).
        // Leave 8px beneath it so only settings-content scrolls in the sheet.
        let sheet_header_height = cx.theme().font_size * 2.5;
        let panel_height =
            (window.viewport_size().height - cx.theme().sheet.margin_top - sheet_header_height - px(8.)).max(px(0.));
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
                .child(self.render_shortcuts(window, cx))
                .into_any_element(),
            SettingsTab::VtCode => self.render_vtcode_settings(cx),
            SettingsTab::Connections => self.render_connections_settings(cx),
        };

        let mut panel = div()
            .id("settings-panel")
            .lock_scroll_axis()
            .track_focus(&self.settings_focus)
            .test_support()
            .role(gpui_kit::Role::Dialog)
            .aria_label(rust_i18n::t!("app.settings.title").to_string())
            .relative()
            .w_full()
            .h(panel_height)
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .text_sm()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("settings-navigation")
                    .test_support()
                    .track_focus(&self.settings_navigation_focus)
                    .key_context("VTCodeSettings")
                    .role(gpui_kit::Role::Group)
                    .aria_label(rust_i18n::t!("app.settings.categories").to_string())
                    .aria_description("Use Left and Right arrows, Home, or End to choose a category.")
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .border_1()
                    .border_color(colors.border.opacity(0.))
                    .focus_visible(|style| style.border_color(colors.ring))
                    .on_action(
                        cx.listener(|view, _: &NextSettingsTab, window, cx| view.cycle_settings_tab(true, window, cx)),
                    )
                    .on_action(cx.listener(|view, _: &PreviousSettingsTab, window, cx| {
                        view.cycle_settings_tab(false, window, cx)
                    }))
                    .on_action(cx.listener(|view, _: &OpenSettingsSearch, window, cx| {
                        view.open_settings_search_palette(window, cx)
                    }))
                    .on_action(cx.listener(|view, _: &FirstSettingsTab, window, cx| {
                        view.select_settings_tab_from_keyboard(0, window, cx)
                    }))
                    .on_action(cx.listener(|view, _: &LastSettingsTab, window, cx| {
                        view.select_settings_tab_from_keyboard(SettingsTab::ALL.len().saturating_sub(1), window, cx)
                    }))
                    .child(
                        desktop_button("settings-filter-search")
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(Icon::new(IconName::Search))
                                    .child(rust_i18n::t!("app.settings.search.button").to_string()),
                            )
                            .accessibility_label(rust_i18n::t!("app.settings.search.button").to_string())
                            .tooltip_with_action(
                                "Search settings and CLI config",
                                &OpenSettingsSearch,
                                Some("VTCodeDesktop"),
                            )
                            .ghost()
                            .small()
                            .w_full()
                            .justify_start()
                            .bg(colors.secondary)
                            .rounded_md()
                            .dispatches_desktop_action(OpenSettingsSearch, self.focus_handle.clone()),
                    )
                    .child(
                        desktop_tab_bar("settings-tabs")
                            .track_scroll(&self.settings_tabs_scroll)
                            .selected_index(selected_index)
                            .on_click(cx.listener(|view, index: &usize, window, cx| {
                                view.select_settings_tab(*index, window, cx)
                            }))
                            .children(SettingsTab::ALL.map(|tab| Tab::new().label(tab.label()).cursor_pointer())),
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
            );
        if self.config_editor.is_some() {
            panel = panel.child(self.render_cli_config_editor(cx));
        }
        panel.into_any_element()
    }

    fn render_appearance_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let swatches = themes::picker_swatches();
        let swatch_rows = swatches.map(|entry| self.render_theme_swatch(entry, cx)).collect::<Vec<_>>();
        let theme = cx.theme();
        let colors = &theme.colors;
        let active_theme_profile = if self.theme_mode == ThemeMode::Light {
            "light"
        } else {
            "dark"
        };
        let visual_style = rust_i18n::t!("app.settings.visual_style").to_string();
        let typography = rust_i18n::t!("app.settings.typography").to_string();
        let layout = rust_i18n::t!("app.settings.layout").to_string();
        let mut content = div()
            .id("appearance-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_5()
            .child(accessible_group_box(
                "appearance-visual-style-group",
                visual_style.clone(),
                GroupBox::new()
                    .flex_shrink_0()
                    .fill()
                    .title(visual_style)
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
                                        .toggled(selected)
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
                            .h(px(96.))
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
                typography.clone(),
                GroupBox::new()
                    .flex_shrink_0()
                    .fill()
                    .title(typography)
                    .child(self.render_font_family(FontTarget::Ui, cx))
                    .child(self.render_font_family(FontTarget::Code, cx))
                    .child(self.render_font_size(false, cx))
                    .child(self.render_font_size(true, cx))
                    .child(
                        div()
                            .id("appearance-code-preview")
                            .test_support()
                            .w_full()
                            .child(render_markdown(
                                "appearance-code-preview-text".into(),
                                "```rust\nfn main() { /* VT Code */ }\n```".into(),
                                false,
                                cx,
                            )),
                    ),
            ));

        let language = self.preferences.ui_language();
        let language_group_title = rust_i18n::t!("app.settings.language.group").to_string();
        content = content.child(accessible_group_box(
            "appearance-language-group",
            language_group_title.clone(),
            GroupBox::new().flex_shrink_0().fill().title(language_group_title).child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(div().child(rust_i18n::t!("app.settings.language.label").to_string()))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(colors.muted_foreground)
                                    .child(rust_i18n::t!("app.settings.language.description").to_string()),
                            ),
                    )
                    .child(div().flex().flex_wrap().gap_1().children(UiLanguage::ALL.map(|option| {
                        let selected = language == option;
                        Toggle::new(format!("ui-language-{}", option.locale()))
                            .label(option.label())
                            .tooltip(
                                rust_i18n::t!("app.settings.language.select", language = option.label()).to_string(),
                            )
                            .outline()
                            .small()
                            .checked(selected)
                            .cursor_pointer()
                            .on_click(cx.listener(move |view, checked: &bool, _, cx| {
                                if *checked {
                                    view.set_ui_language(option, cx);
                                } else {
                                    // Keep one option selected even when the active toggle is clicked again.
                                    cx.notify();
                                }
                            }))
                    }))),
            ),
        ));

        content = content.child(accessible_group_box(
            "appearance-layout-group",
            layout.clone(),
            GroupBox::new()
                .flex_shrink_0()
                .fill()
                .title(layout)
                .child(
                    gpui_kit::component::switch::Switch::new("navigation-visible")
                        .label("Show side navigation")
                        .checked(self.preferences.navigation_visible())
                        .on_click(cx.listener(|view, checked, _, cx| {
                            view.set_navigation_visible(*checked, cx);
                        })),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(colors.muted_foreground)
                        .child("When hidden, project, thread, and workspace actions stay in the top bar."),
                )
                .child(
                    gpui_kit::component::switch::Switch::new("navigation-on-right")
                        .label("Place navigation on the right")
                        .checked(self.preferences.navigation_on_right())
                        .on_click(cx.listener(|view, checked, _, cx| {
                            view.set_navigation_on_right(*checked, cx);
                        })),
                ),
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

    fn set_ui_language(&mut self, language: UiLanguage, cx: &mut Context<Self>) {
        if self.preferences.ui_language() == language {
            return;
        }

        self.preferences.set_ui_language(language);
        super::super::localization::set_language(language);
        // Command rows cache their rendered labels. Drop that projection so a
        // subsequently opened palette reads the newly selected locale.
        self.clear_palette_state();
        self.preferences.save();
        cx.notify();
    }

    fn set_navigation_on_right(&mut self, on_right: bool, cx: &mut Context<Self>) {
        if self.preferences.navigation_on_right() == on_right {
            return;
        }

        self.preferences.navigation_on_right = Some(on_right);
        self.save_preferences();
        cx.notify();
    }

    fn set_navigation_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.preferences.navigation_visible() == visible {
            return;
        }

        self.preferences.navigation_visible = Some(visible);
        self.navigation_peek = false;
        self.save_preferences();
        cx.notify();
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
        let service_info = self.service_info.as_ref().filter(|_| self.backend.is_some());
        let service_connected = service_info.is_some();
        let service_status = if service_connected {
            Tag::success().outline().small().child("Connected")
        } else {
            Tag::warning().outline().small().child("Not connected")
        };
        let service_status_label = if service_connected {
            "CLI service connection is active"
        } else {
            "CLI service has not completed a protocol handshake"
        };
        let service_details = if let Some(info) = service_info {
            DescriptionList::horizontal()
                .columns(1)
                .label_width(px(128.))
                .bordered(false)
                .item("Runtime", "VT Code", 1)
                .item("Version", info.server_version.clone(), 1)
                .item("Protocol", format!("{}.{}", info.protocol.major, info.protocol.minor), 1)
                .item("Workspace", info.workspace.clone(), 1)
                .into_any_element()
        } else {
            div()
                .id("local-service-unavailable")
                .test_support()
                .min_w_0()
                .text_sm()
                .text_color(cx.theme().colors.muted_foreground)
                .child("The local CLI service will appear here after VT Code Desktop connects.")
                .into_any_element()
        };

        div()
            .id("connections-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_4()
            .child(self.render_webmcp_settings(false, cx))
            .child(self.render_webmcp_settings(true, cx))
            .child(accessible_group_box(
                "local-service-settings-group",
                "Local service",
                GroupBox::new().flex_shrink_0().outline().title("Local service").child(
                    div()
                        .id("local-service-details")
                        .test_support()
                        .role(gpui_kit::Role::Group)
                        .aria_label("Local service connection details")
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .id("local-service-status")
                                .test_support()
                                .role(gpui_kit::Role::Status)
                                .aria_label(service_status_label)
                                .child(service_status),
                        )
                        .child(service_details),
                ),
            ))
            .into_any_element()
    }

    pub(super) fn select_settings_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.update_settings_tab(index, false, window, cx);
    }

    pub(super) fn select_settings_tab_from_keyboard(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_settings_tab(index, true, window, cx);
    }

    fn update_settings_tab(&mut self, index: usize, reveal: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = SettingsTab::ALL.get(index) {
            self.settings_tab = *tab;
            if reveal {
                self.settings_tabs_scroll.scroll_to_item(index);
            }
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
        self.select_settings_tab_from_keyboard(index, window, cx);
    }
}
