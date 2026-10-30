use super::*;
use crate::desktop::localization::UiLanguage;

/// Provider group id shared by the empty and loaded states so the group
/// keeps stable identity across configuration changes.
const PROVIDER_SETTINGS_GROUP_ID: &str = "provider-settings-group";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum SettingsTab {
    Appearance,
    Accessibility,
    Shortcuts,
    VtCode,
    Connections,
    Notifications,
    Storage,
}

impl SettingsTab {
    const ALL: [Self; 7] = [
        Self::Appearance,
        Self::Accessibility,
        Self::Shortcuts,
        Self::VtCode,
        Self::Connections,
        Self::Notifications,
        Self::Storage,
    ];

    fn label(self) -> String {
        match self {
            Self::Appearance => rust_i18n::t!("app.settings.tab.appearance"),
            Self::Accessibility => rust_i18n::t!("app.settings.tab.accessibility"),
            Self::Shortcuts => rust_i18n::t!("app.settings.tab.shortcuts"),
            Self::VtCode => rust_i18n::t!("app.settings.tab.vtcode"),
            Self::Connections => rust_i18n::t!("app.settings.tab.connections"),
            Self::Notifications => rust_i18n::t!("app.settings.tab.notifications"),
            Self::Storage => rust_i18n::t!("app.settings.tab.storage"),
        }
        .to_string()
    }

    fn search_keywords(self) -> &'static [&'static str] {
        match self {
            Self::Appearance => &[
                "appearance",
                "themes",
                "color",
                "contrast",
                "font",
                "typography",
                "visual style",
                "language",
                "navigation",
                "sidebar",
                "layout",
                "files",
                "editor",
                "destination",
                "viewer",
                "open",
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
                "vt code menu bar",
                "config",
                "configuration",
                "provider",
                "model",
                "agent",
                "cli",
                "permissions",
                "third-party notices",
            ],
            Self::Connections => &["connections", "mcp", "webmcp", "server", "bridge"],
            Self::Notifications => &[
                "notifications",
                "notification",
                "alerts",
                "alert",
                "updates",
                "update",
                "banner",
                "toast",
                "completion",
                "permission",
                "question",
                "thông báo",
            ],
            Self::Storage => &[
                "storage",
                "disk",
                "space",
                "cleanup",
                "clean up",
                "worktree",
                "worktrees",
                "archive",
                "delete",
            ],
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
                if tab == SettingsTab::VtCode {
                    entry.keywords.push(rust_i18n::t!("app.settings.licenses.title").to_string());
                }
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
        let panel_height = sheet_panel_height(window, cx);
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
            SettingsTab::Notifications => self.render_notification_settings(cx),
            SettingsTab::Storage => self.render_storage_settings(cx),
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
                    .key_context(key_context::SETTINGS)
                    .role(gpui_kit::Role::Group)
                    .aria_label(rust_i18n::t!("app.settings.categories").to_string())
                    .aria_description(rust_i18n::t!("app.settings.categories_navigation_hint").to_string())
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
                                rust_i18n::t!("app.settings.search.title").to_string(),
                                &OpenSettingsSearch,
                                Some(key_context::DESKTOP),
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
                            .children(SettingsTab::ALL.map(|tab| Tab::new().label(tab.label()))),
                    ),
            )
            .child(scroll_column("settings-content", &self.settings_scroll, content));
        if self.config_editor.is_some() {
            panel = panel.child(self.render_cli_config_editor(cx));
        }
        panel.into_any_element()
    }

    pub(super) fn set_ui_language(&mut self, language: UiLanguage, window: &mut Window, cx: &mut Context<Self>) {
        if self.preferences.ui_language() == language {
            return;
        }

        self.preferences.set_ui_language(language);
        super::super::localization::set_language(language);
        menu_bar::refresh_labels(cx);
        let placeholder = rust_i18n::t!("app.composer.input_placeholder").to_string();
        self.composer
            .update(cx, |state, cx| state.set_placeholder(placeholder, window, cx));
        // Command rows cache their rendered labels. Drop that projection so a
        // subsequently opened palette reads the newly selected locale.
        self.clear_palette_state();
        self.preferences.save();
        cx.notify();
    }

    pub(super) fn set_file_open_destination(
        &mut self,
        destination: preferences::FileOpenDestination,
        cx: &mut Context<Self>,
    ) {
        if self.preferences.file_open_destination() == destination {
            return;
        }

        self.preferences.set_file_open_destination(destination);
        self.preferences.save();
        cx.notify();
    }

    pub(super) fn set_navigation_on_right(&mut self, on_right: bool, cx: &mut Context<Self>) {
        if self.preferences.navigation_on_right() == on_right {
            return;
        }

        self.preferences.navigation_on_right = Some(on_right);
        self.save_preferences();
        cx.notify();
    }

    pub(super) fn set_navigation_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
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

    fn render_notification_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let notifications = &self.preferences.notifications;
        let group_title = rust_i18n::t!("app.settings.notifications.group").to_string();
        div()
            .id("notification-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_4()
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
                            &colors,
                        ),
                        &colors,
                    ))
                    .child(row_divider(&colors))
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
                    .child(row_divider(&colors))
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
                            &colors,
                        ),
                        &colors,
                    ))
                    .child(row_divider(&colors))
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
                            &colors,
                        ),
                        &colors,
                    ))
                    .child(row_divider(&colors))
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
                            &colors,
                        ),
                        &colors,
                    ))
                    .child(row_divider(&colors))
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

    fn render_vtcode_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let content = div()
            .id("vtcode-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_4()
            .child(self.render_provider_settings(cx))
            .child(self.render_shared_preferences(cx))
            .child(self.render_cli_configuration(cx))
            .child(self.render_licenses_settings(cx))
            .child(self.render_menu_bar_settings(cx));
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

    /// Continue a session from any directory; bounded archive IDs are safe to
    /// interpolate unquoted.
    pub(super) fn terminal_command_for_session(selected_session_id: Option<&str>) -> String {
        match selected_session_id {
            Some(id) => format!("vtcode --resume {id} --all"),
            None => "vtcode".to_string(),
        }
    }

    fn render_connections_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let service_info = self.service_info.as_ref().filter(|_| self.backend.is_some());
        let service_connected = service_info.is_some();
        let service_status = if service_connected {
            Tag::success()
                .outline()
                .small()
                .child(rust_i18n::t!("app.settings.connections.connected").to_string())
        } else {
            Tag::warning()
                .outline()
                .small()
                .child(rust_i18n::t!("app.settings.connections.not_connected").to_string())
        };
        let service_status_label = if service_connected {
            rust_i18n::t!("app.settings.connections.active_status").to_string()
        } else {
            rust_i18n::t!("app.settings.connections.waiting_status").to_string()
        };
        let service_details = if let Some(info) = service_info {
            DescriptionList::horizontal()
                .columns(1)
                .label_width(px(128.))
                .bordered(false)
                .item(rust_i18n::t!("app.settings.connections.runtime").to_string(), app::NAME, 1)
                .item(rust_i18n::t!("app.settings.connections.version").to_string(), info.server_version.clone(), 1)
                .item(
                    rust_i18n::t!("app.settings.connections.protocol").to_string(),
                    format!("{}.{}", info.protocol.major, info.protocol.minor),
                    1,
                )
                .item(rust_i18n::t!("app.settings.connections.workspace").to_string(), info.workspace.clone(), 1)
                .into_any_element()
        } else {
            div()
                .id("local-service-unavailable")
                .test_support()
                .min_w_0()
                .child(row_description(
                    rust_i18n::t!("app.settings.connections.local_service_unavailable").to_string(),
                    &cx.theme().colors,
                ))
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
            .child({
                let group_title = rust_i18n::t!("app.settings.connections.terminal").to_string();
                let command = Self::terminal_command_for_session(self.selected_session_id.as_deref());
                accessible_group_box(
                    "terminal-settings-group",
                    group_title.clone(),
                    settings_group_box(group_title, &cx.theme().colors).child(settings_row(
                        rust_i18n::t!("app.settings.connections.terminal").to_string(),
                        command.clone(),
                        desktop_button("copy-terminal-command")
                            .label(rust_i18n::t!("app.file_preview.copy").to_string())
                            .ghost()
                            .small()
                            .on_click({
                                move |_, window: &mut Window, cx: &mut App| {
                                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(command.clone()));
                                    super::super::notifications::copied_to_clipboard(window, cx);
                                }
                            }),
                        &cx.theme().colors,
                    )),
                )
            })
            .child(accessible_group_box(
                "local-service-settings-group",
                rust_i18n::t!("app.settings.connections.local_service").to_string(),
                GroupBox::new()
                    .flex_shrink_0()
                    .outline()
                    .title(group_box_title(
                        rust_i18n::t!("app.settings.connections.local_service").to_string(),
                        &cx.theme().colors,
                    ))
                    .child(
                        div()
                            .id("local-service-details")
                            .test_support()
                            .role(gpui_kit::Role::Group)
                            .aria_label(rust_i18n::t!("app.settings.connections.local_service_details").to_string())
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

    fn select_settings_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.update_settings_tab(index, false, window, cx);
    }

    fn select_settings_tab_from_keyboard(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.update_settings_tab(index, true, window, cx);
    }

    fn update_settings_tab(&mut self, index: usize, reveal: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = SettingsTab::ALL.get(index) {
            self.settings_tab = *tab;
            if *tab == SettingsTab::Storage {
                self.maybe_request_storage_summary(cx);
            }
            if reveal {
                self.settings_tabs_scroll.scroll_to_item(index);
            }
            self.settings_navigation_focus.focus(window, cx);
            cx.notify();
        }
    }

    fn cycle_settings_tab(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
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
