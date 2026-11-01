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

    /// Index of the CLI configuration entry in the unified settings
    /// navigation: it follows every `SettingsTab` instead of swapping the
    /// whole surface through `config_page_open` alone.
    pub(super) const CONFIG_INDEX: usize = Self::ALL.len();

    /// Tabs plus the configuration entry.
    pub(super) const PAGE_COUNT: usize = Self::ALL.len() + 1;

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
        if self.toggle_palette_kind(PaletteKind::SettingsSearch, window, cx) {
            return;
        }

        let settings = SettingsTab::ALL
            .into_iter()
            .map(|tab| {
                let mut entry = command(tab.label(), PaletteAction::SelectSettingsTab(tab));
                entry.keywords = tab.search_keywords().iter().map(|keyword| (*keyword).to_owned()).collect();
                if tab == SettingsTab::VtCode {
                    entry.keywords.push(rust_i18n::t!(i18n::LICENSES_TITLE).to_string());
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

    /// Unified full-screen settings: left navigation over the seven tabs
    /// plus the CLI configuration entry, right content for the selection.
    /// The dialog identity stays `Settings` in every language so reopening
    /// and assistive technology see one stable surface.
    pub(super) fn render_settings_page(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let title = rust_i18n::t!(i18n::SETTINGS_TITLE).to_string();
        let content = self.render_settings_content(window, cx);
        // Compact layout below the shared breakpoint (see
        // NARROW_PANEL_BREAKPOINT): the nav stacks above the content and
        // shares the height, mirroring the config rail breakpoint.
        let narrow = window.viewport_size().width < px(NARROW_PANEL_BREAKPOINT);
        route_page(
            div()
                .id("settings-panel")
                .test_support()
                .track_focus(&self.settings_focus)
                .role(gpui_kit::Role::Dialog)
                .aria_label(title.clone())
                .relative()
                .size_full()
                .flex()
                .flex_col()
                .rounded_xl()
                .bg(colors.secondary)
                .text_sm()
                // The embedded search has no scrim: any other press inside
                // the panel dismisses it. The palette and its toggle button
                // keep their own mouse-downs, so neither ever dismisses it.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, _, window, cx| {
                        if view.palette.is_some() && view.palette_kind == PaletteKind::SettingsSearch {
                            view.close_palette(window, cx);
                        }
                    }),
                )
                .child(panel_header(
                    div()
                        .text_lg()
                        .font_semibold()
                        .pl(overlay_title_inset(cx.theme().font_size, 2.25))
                        .child(div().id("settings-title").test_support().child(title)),
                    // Generic "Close" word shared across panels (owned by the browser surface).
                    desktop_button("settings-dismiss")
                        .icon(IconName::X)
                        .accessibility_label(rust_i18n::t!("app.browser.close").to_string())
                        .tooltip(rust_i18n::t!("app.browser.close").to_string())
                        .ghost()
                        .small()
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.close_settings(window, cx);
                        })),
                ))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .flex()
                        .when(narrow, |row| row.flex_col())
                        // Two gaps flank the divider, so each shrinks to 4px: the 9px
                        // total keeps the content width at its pre-divider size while
                        // the hairline owns the boundary.
                        .gap_1()
                        .px_3()
                        .pb_3()
                        .child(self.render_settings_navigation(window, cx, narrow))
                        .child(pane_divider("settings-pane-divider", !narrow, &colors))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .flex()
                                .flex_col()
                                .when(narrow, |content| content.w_full())
                                .child(content),
                        ),
                )
                .when(self.config_editor.is_some(), |panel| panel.child(self.render_cli_config_editor(cx))),
        )
        .into_any_element()
    }

    /// Left navigation: shared search plus one button per settings page,
    /// grouped into desktop tabs and the CLI configuration entry.
    /// Buttons keep Kit Button keyboard handling; the panel fill is
    /// `secondary`, so selection uses `popover` (a `secondary` fill would
    /// be pixel-identical to the background).
    fn render_settings_navigation(&self, window: &Window, cx: &mut Context<Self>, narrow: bool) -> AnyElement {
        let colors = cx.theme().colors;
        let selected = self.settings_page_index();
        let mut desktop_entries = Vec::with_capacity(SettingsTab::ALL.len());
        for (index, tab) in SettingsTab::ALL.iter().enumerate() {
            let active = index == selected;
            desktop_entries.push(
                desktop_button(format!("settings-nav-{tab:?}").to_lowercase())
                    .accessibility_label(tab.label())
                    .child(div().flex_1().min_w_0().truncate().child(tab.label()))
                    .toggled(active)
                    .ghost()
                    .w_full()
                    .justify_start()
                    .when(active, |button| button.bg(colors.popover).text_color(colors.foreground))
                    .on_click(cx.listener(move |view, _, window, cx| view.select_settings_page(index, window, cx)))
                    .into_any_element(),
            );
        }
        let config_active = selected == SettingsTab::CONFIG_INDEX;
        let config_label = rust_i18n::t!("app.configuration.page_title").to_string();
        let config_entry = desktop_button("settings-nav-configuration")
            .accessibility_label(config_label.clone())
            .child(div().flex_1().min_w_0().truncate().child(config_label))
            .toggled(config_active)
            .ghost()
            .w_full()
            .justify_start()
            .when(config_active, |button| button.bg(colors.popover).text_color(colors.foreground))
            .on_click(cx.listener(|view, _, window, cx| {
                view.select_settings_page(SettingsTab::CONFIG_INDEX, window, cx);
            }))
            .into_any_element();
        // Two groups mirror the settings search palette: the desktop tabs,
        // then the CLI configuration entry behind a divider and header.
        let entries = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(nav_group_header(
                "settings-nav-group-settings",
                rust_i18n::t!("app.settings.search.group_settings"),
                &colors,
            ))
            .children(desktop_entries)
            .child(
                div()
                    .id("settings-nav-group-divider")
                    .test_support()
                    .w_full()
                    .child(row_divider(&colors)),
            )
            .child(nav_group_header(
                "settings-nav-group-cli-config",
                rust_i18n::t!("app.settings.search.group_cli_config"),
                &colors,
            ))
            .child(config_entry);
        div()
            .id("settings-navigation")
            .test_support()
            .track_focus(&self.settings_navigation_focus)
            .when(!self.capture_suppresses_bindings(window), |nav| nav.key_context(key_context::SETTINGS))
            .role(gpui_kit::Role::Group)
            .aria_label(rust_i18n::t!("app.settings.categories").to_string())
            .aria_description(rust_i18n::t!("app.settings.categories_navigation_hint").to_string())
            .w(px(240.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .when(narrow, |nav| nav.w_full().flex_1().min_h_0())
            .on_action(cx.listener(|view, _: &NextSettingsTab, window, cx| view.cycle_settings_tab(true, window, cx)))
            .on_action(
                cx.listener(|view, _: &PreviousSettingsTab, window, cx| view.cycle_settings_tab(false, window, cx)),
            )
            .on_action(
                cx.listener(|view, _: &OpenSettingsSearch, window, cx| view.open_settings_search_palette(window, cx)),
            )
            .on_action(cx.listener(|view, _: &OpenSettings, window, cx| view.toggle_settings(window, cx)))
            .on_action(cx.listener(|view, _: &FirstSettingsTab, window, cx| {
                view.select_settings_page_from_keyboard(0, window, cx)
            }))
            .on_action(cx.listener(|view, _: &LastSettingsTab, window, cx| {
                view.select_settings_page_from_keyboard(SettingsTab::PAGE_COUNT - 1, window, cx)
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
                        rust_i18n::t!(i18n::SETTINGS_SEARCH_TITLE).to_string(),
                        &OpenSettingsSearch,
                        Some(key_context::DESKTOP),
                    )
                    .ghost()
                    .small()
                    .w_full()
                    .justify_start()
                    .bg(colors.secondary)
                    .rounded_md()
                    // Keep the press: the panel dismisses the search on any
                    // other mouse-down, so the click below toggles cleanly.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .dispatches_desktop_action(OpenSettingsSearch, self.focus_handle.clone()),
            )
            .child(scroll_column("settings-nav-list", &self.settings_nav_scroll, entries))
            .into_any_element()
    }

    /// Right content for the selected page. The settings search palette
    /// renders here while the navigation stays visible; selecting a tab
    /// clears the configuration page and vice versa.
    fn render_settings_content(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        if self.palette_kind == PaletteKind::SettingsSearch
            && let Some(state) = self.palette.clone()
        {
            return self.render_command_palette(state, window, cx);
        }
        if self.config_page_open {
            // Side-by-side rail and rows need room for both navigations plus
            // readable rows (settings nav 240 + chrome + config rail 240 +
            // ~320 row minimum); narrower windows stack the rail above.
            let wide = window.viewport_size().width >= px(960.);
            return self.render_cli_config_page(wide, cx);
        }
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
        scroll_column("settings-content", &self.settings_scroll, content).into_any_element()
    }

    pub(super) fn set_ui_language(&mut self, language: UiLanguage, window: &mut Window, cx: &mut Context<Self>) {
        if self.preferences.ui_language() == language {
            return;
        }

        self.preferences.set_ui_language(language);
        super::super::localization::set_language(language);
        menu_bar::refresh_labels(cx);
        let placeholder = rust_i18n::t!(i18n::COMPOSER_INPUT_PLACEHOLDER).to_string();
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
                            .label(rust_i18n::t!(i18n::FILE_PREVIEW_COPY).to_string())
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
                settings_group_box(
                    rust_i18n::t!("app.settings.connections.local_service").to_string(),
                    &cx.theme().colors,
                )
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

    /// Navigation index of the visible page: the tab position, or the
    /// trailing configuration entry while the config page is open.
    pub(super) fn settings_page_index(&self) -> usize {
        if self.config_page_open {
            return SettingsTab::CONFIG_INDEX;
        }
        SettingsTab::ALL
            .iter()
            .position(|tab| *tab == self.settings_tab)
            .unwrap_or_default()
    }

    fn select_settings_page(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.update_settings_page(index, false, window, cx);
    }

    fn select_settings_page_from_keyboard(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.update_settings_page(index, true, window, cx);
    }

    fn update_settings_page(&mut self, index: usize, reveal: bool, window: &mut Window, cx: &mut Context<Self>) {
        if index == SettingsTab::CONFIG_INDEX {
            self.config_page_open = true;
            if reveal {
                self.settings_nav_scroll.scroll_to_item(index);
            }
            self.settings_navigation_focus.focus(window, cx);
            cx.notify();
            return;
        }
        if let Some(tab) = SettingsTab::ALL.get(index) {
            self.settings_tab = *tab;
            self.config_page_open = false;
            if *tab == SettingsTab::Storage {
                self.maybe_request_storage_summary(cx);
            }
            if reveal {
                self.settings_nav_scroll.scroll_to_item(index);
            }
            self.settings_navigation_focus.focus(window, cx);
            cx.notify();
        }
    }

    fn cycle_settings_tab(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let count = SettingsTab::PAGE_COUNT;
        let current = self.settings_page_index();
        let index = if forward {
            (current + 1) % count
        } else {
            (current + count - 1) % count
        };
        self.select_settings_page_from_keyboard(index, window, cx);
    }
}
