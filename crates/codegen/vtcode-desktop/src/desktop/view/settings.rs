use super::*;
use crate::desktop::localization::UiLanguage;

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
    pub(super) const ALL: [Self; 7] = [
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

    pub(super) fn label(self) -> String {
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

    pub(super) fn icon(self) -> IconName {
        match self {
            Self::Appearance => IconName::Palette,
            Self::Accessibility => IconName::Accessibility,
            Self::Shortcuts => IconName::Keyboard,
            Self::VtCode => IconName::Bot,
            Self::Connections => IconName::Plug,
            Self::Notifications => IconName::Bell,
            Self::Storage => IconName::HardDrive,
        }
    }

    pub(super) fn search_keywords(self) -> &'static [&'static str] {
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
                "license",
                "licenses",
                "open source",
                "open source licenses",
                "third-party notices",
                "attribution",
                "legal",
                "copyright",
                "apache",
                "mit",
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
                        // The shared header row insets at `px_3`; one extra
                        // step lands the title on the `px_4` body spine.
                        .pl_1()
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
                        // Two gaps flank the divider at one panel step each, so
                        // the nav/content separation reads at major-region
                        // scale (33px total) while the hairline owns the
                        // boundary alone. The same gap stacks nav above
                        // content in narrow mode.
                        .gap_4()
                        .px_4()
                        .pb_4()
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
                    .child(nav_entry_content(
                        format!("settings-nav-label-{tab:?}").to_lowercase(),
                        tab.icon(),
                        tab.label(),
                    ))
                    .desktop_selected(active, &colors)
                    .w_full()
                    .justify_start()
                    .on_click(cx.listener(move |view, _, window, cx| view.select_settings_page(index, window, cx)))
                    .into_any_element(),
            );
        }
        let config_active = selected == SettingsTab::CONFIG_INDEX;
        let config_label = rust_i18n::t!("app.configuration.page_title").to_string();
        let config_entry = desktop_button("settings-nav-configuration")
            .accessibility_label(config_label.clone())
            .child(nav_entry_content("settings-nav-label-configuration", IconName::Settings2, config_label))
            .desktop_selected(config_active, &colors)
            .w_full()
            .justify_start()
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
        let title = self.settings_tab.label();
        let page = match self.settings_tab {
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
        let content = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .id("settings-page-title")
                    .test_support()
                    .role(gpui_kit::Role::Heading)
                    .aria_label(title.clone())
                    .child(content_page_title(title, &cx.theme().colors)),
            )
            .child(page);
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
        // An explicit selection always persists, even when it matches the
        // effective default, so the saved file records the user's choice.
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
            // Live connection state, like the storage summary: passive
            // selection refreshes only when a service exists to answer, so a
            // backend-less page never marks status dirty or posts a queue
            // notice. The request itself is pending-gated, so re-selecting
            // refreshes only after the previous round-trip settles.
            if *tab == SettingsTab::Connections && self.backend.is_some() {
                self.request_webmcp_status();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_tab_icons_cover_every_tab_exactly_once() {
        assert_eq!(SettingsTab::Appearance.icon(), IconName::Palette);
        assert_eq!(SettingsTab::Accessibility.icon(), IconName::Accessibility);
        assert_eq!(SettingsTab::Shortcuts.icon(), IconName::Keyboard);
        assert_eq!(SettingsTab::VtCode.icon(), IconName::Bot);
        assert_eq!(SettingsTab::Connections.icon(), IconName::Plug);
        assert_eq!(SettingsTab::Notifications.icon(), IconName::Bell);
        assert_eq!(SettingsTab::Storage.icon(), IconName::HardDrive);
        let icons = SettingsTab::ALL.map(SettingsTab::icon);
        for (index, icon) in icons.iter().enumerate() {
            assert!(icons.iter().skip(index + 1).all(|other| other != icon), "duplicate nav icon {icon:?}");
        }
    }
}
