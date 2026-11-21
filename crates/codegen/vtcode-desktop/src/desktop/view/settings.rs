use super::*;
use crate::desktop::localization::UiLanguage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum SettingsTab {
    VtCode,
    Appearance,
    Accessibility,
    Shortcuts,
    Connections,
    Notifications,
    Storage,
    ComputerUse,
    About,
}

/// One entry in the unified settings navigation: the nine tabs plus the CLI
/// configuration page. This is the single ordering source for the Kit sidebar,
/// the legacy nav, keyboard cycling, and index→page dispatch, so a reorder
/// cannot desync one surface from another. About is deliberately last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SettingsPage {
    Tab(SettingsTab),
    Configuration,
}

impl SettingsPage {
    pub(super) const ALL: [Self; 10] = [
        Self::Tab(SettingsTab::VtCode),
        Self::Tab(SettingsTab::Appearance),
        Self::Tab(SettingsTab::Accessibility),
        Self::Tab(SettingsTab::Shortcuts),
        Self::Tab(SettingsTab::Connections),
        Self::Tab(SettingsTab::Notifications),
        Self::Tab(SettingsTab::Storage),
        Self::Tab(SettingsTab::ComputerUse),
        Self::Configuration,
        Self::Tab(SettingsTab::About),
    ];

    pub(super) const CONFIG_INDEX: usize = 8;
}

impl SettingsTab {
    /// Tab-only order for the search palette, canonical deeplink names, and
    /// the icon-coverage test. Navigation ordering lives in [`SettingsPage`].
    pub(super) const ALL: [Self; 9] = [
        Self::VtCode,
        Self::Appearance,
        Self::Accessibility,
        Self::Shortcuts,
        Self::Connections,
        Self::Notifications,
        Self::Storage,
        Self::ComputerUse,
        Self::About,
    ];

    /// Index of the CLI configuration entry in the unified settings
    /// navigation: it sits just before About, which closes the list.
    pub(super) const CONFIG_INDEX: usize = SettingsPage::CONFIG_INDEX;

    /// Index of the About page: the last entry in the unified navigation.
    #[cfg(test)]
    pub(super) const ABOUT_INDEX: usize = Self::ALL.len();

    /// Tabs plus the configuration entry.
    pub(super) const PAGE_COUNT: usize = SettingsPage::ALL.len();

    /// Canonical deeplink segment for the tab. Shared by every `settings/<tab>`
    /// writer and parser so the two directions cannot drift.
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::VtCode => "vtcode",
            Self::Appearance => "appearance",
            Self::Accessibility => "accessibility",
            Self::Shortcuts => "shortcuts",
            Self::Connections => "connections",
            Self::Notifications => "notifications",
            Self::Storage => "storage",
            Self::ComputerUse => "computer-use",
            Self::About => "about",
        }
    }

    /// Parse a deeplink tab segment or `/config`/`/settings` keyword. Every
    /// alias maps back to the canonical [`Self::as_str`] spelling.
    pub(super) fn from_name(name: &str) -> Option<Self> {
        match name {
            "appearance" | "theme" => Some(Self::Appearance),
            "accessibility" | "a11y" => Some(Self::Accessibility),
            "shortcuts" | "keybindings" => Some(Self::Shortcuts),
            "vtcode" | "general" => Some(Self::VtCode),
            "connections" | "mcp" | "webmcp" => Some(Self::Connections),
            "notifications" | "notification" | "alerts" | "updates" => Some(Self::Notifications),
            "storage" | "disk" | "cleanup" => Some(Self::Storage),
            "computer-use" | "computer_use" | "computeruse" | "cua" => Some(Self::ComputerUse),
            "about" | "info" => Some(Self::About),
            _ => None,
        }
    }

    /// i18n key for the tab label, resolved once in [`SettingsTab::label`].
    const fn label_key(self) -> &'static str {
        match self {
            Self::VtCode => "app.settings.tab.vtcode",
            Self::Appearance => "app.settings.tab.appearance",
            Self::Accessibility => "app.settings.tab.accessibility",
            Self::Shortcuts => "app.settings.tab.shortcuts",
            Self::Connections => "app.settings.tab.connections",
            Self::Notifications => "app.settings.tab.notifications",
            Self::Storage => "app.settings.tab.storage",
            Self::ComputerUse => "app.settings.tab.computer_use",
            Self::About => "app.settings.tab.about",
        }
    }

    pub(super) fn label(self) -> String {
        rust_i18n::t!(self.label_key()).to_string()
    }

    /// Lucide glyph for the tab, or `None` when the tab uses the brand V
    /// mark ([`vtcode_nav_mark`]) instead of a catalog icon.
    pub(super) fn icon(self) -> Option<IconName> {
        match self {
            Self::VtCode => None,
            Self::Appearance => Some(IconName::SunMoon),
            Self::Accessibility => Some(IconName::PersonStanding),
            Self::Shortcuts => Some(IconName::Keyboard),
            Self::Connections => Some(IconName::Plug),
            Self::Notifications => Some(IconName::Bell),
            Self::Storage => Some(IconName::HardDrive),
            Self::ComputerUse => Some(IconName::Computer),
            Self::About => Some(IconName::Info),
        }
    }

    pub(super) fn search_keywords(self) -> &'static [&'static str] {
        match self {
            Self::VtCode => &[
                "vt code menu bar",
                "startup",
                "login",
                "launch at login",
                "khởi động",
                "config",
                "configuration",
                "provider",
                "model",
                "agent",
                "cli",
                "permissions",
            ],
            Self::Appearance => &[
                "appearance",
                "chat width",
                "column width",
                "width",
                "themes",
                "color",
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
                "diff",
                "diffs",
                "wrap",
                "syntax",
                "highlight",
                "context",
                "hunk",
                "hover",
                "numbers",
                "headers",
                "markers",
                "separators",
                "tint",
                "emphasis",
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
            Self::ComputerUse => &[
                "computer use",
                "computer-use",
                "cua",
                "browser",
                "automation",
                "control",
            ],
            Self::About => &[
                "about",
                "version",
                "build",
                "updates",
                "update",
                "check for updates",
                "new version",
                "release notes",
                "acknowledgments",
                "acknowledgements",
                "credits",
                "thanks",
                "third-party notices",
                "licenses",
                "license",
                "open source",
                "open source licenses",
                "attribution",
                "legal",
                "copyright",
                "apache",
                "mit",
                "website",
                "repository",
                "giới thiệu",
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

    /// Unified full-screen settings: left navigation over the nine tabs
    /// plus the CLI configuration entry, right content for the selection.
    /// The dialog identity stays `Settings` in every language so reopening
    /// and assistive technology see one stable surface.
    pub(super) fn render_settings_page(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let title = rust_i18n::t!(i18n::SETTINGS_TITLE).to_string();
        let content = self.render_settings_content(window, cx);
        // Compact layout below the shared breakpoint (see
        // NARROW_PANEL_BREAKPOINT): the nav stacks above the content and
        // shares the height, mirroring the config rail breakpoint.
        let narrow = Self::settings_use_legacy_layout(window);
        let content = if narrow && !(self.palette_kind == PaletteKind::SettingsSearch && self.palette.is_some()) {
            self.render_settings_section_motion(self.settings_page_index(), content, window, cx)
        } else {
            content
        };
        dismissable_route_page_with_layout(
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
                // Inner presses stop here so the gutter only sees true
                // background presses.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, _, window, cx| {
                        if view.palette.is_some() && view.palette_kind == PaletteKind::SettingsSearch {
                            view.close_palette(window, cx);
                        }
                        cx.stop_propagation();
                    }),
                )
                // Wide viewports host the Kit container instead of the
                // legacy navigation, so the panel carries the settings key
                // context and tab actions the nav owns in narrow mode.
                .when(!narrow && !self.capture_suppresses_bindings(window), |panel| {
                    panel.key_context(key_context::SETTINGS)
                })
                .when(!narrow, |panel| {
                    panel
                        .on_action(cx.listener(|view, _: &NextSettingsTab, window, cx| {
                            view.run_settings_panel_action(window, cx, |view, window, cx| {
                                view.cycle_settings_tab(true, window, cx)
                            })
                        }))
                        .on_action(cx.listener(|view, _: &PreviousSettingsTab, window, cx| {
                            view.run_settings_panel_action(window, cx, |view, window, cx| {
                                view.cycle_settings_tab(false, window, cx)
                            })
                        }))
                        .on_action(cx.listener(|view, _: &OpenSettingsSearch, window, cx| {
                            view.run_settings_panel_action(window, cx, |view, window, cx| {
                                view.open_settings_search_palette(window, cx)
                            })
                        }))
                        .on_action(cx.listener(|view, _: &OpenSettings, window, cx| {
                            view.run_settings_panel_action(window, cx, |view, window, cx| {
                                view.toggle_settings(window, cx)
                            })
                        }))
                        .on_action(cx.listener(|view, _: &FirstSettingsTab, window, cx| {
                            view.run_settings_panel_action(window, cx, |view, window, cx| {
                                view.select_settings_page_from_keyboard(0, window, cx)
                            })
                        }))
                        .on_action(cx.listener(|view, _: &LastSettingsTab, window, cx| {
                            view.run_settings_panel_action(window, cx, |view, window, cx| {
                                view.select_settings_page_from_keyboard(SettingsTab::PAGE_COUNT - 1, window, cx)
                            })
                        }))
                })
                .child(panel_header(
                    div()
                        // The shared header row insets at `px_3`; one extra
                        // step lands the title on the `px_4` body spine.
                        .pl_1()
                        .child(div().id("settings-title").test_support().child(panel_title(title, &colors))),
                    // Dismissal stays on Escape, the bottom gutter, and toolbar
                    // Back; the header no longer duplicates a close control.
                    div(),
                ))
                .child(if narrow {
                    div()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .flex()
                        .flex_col()
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
                        .child(div().flex_1().min_w_0().min_h_0().flex().flex_col().w_full().child(content))
                        .into_any_element()
                } else {
                    // Wide viewports host the Kit container, which owns its
                    // sidebar and search; the panel keeps the title row and
                    // dismissal above it.
                    div()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .flex()
                        .flex_col()
                        .px_4()
                        .pb_4()
                        .child(content)
                        .into_any_element()
                })
                .when(self.config_editor.is_some(), |panel| panel.child(self.render_cli_config_editor(cx))),
            cx,
            RoutePageLayout::FullWidth,
        )
        .into_any_element()
    }

    /// Left navigation: shared search plus one button per settings page,
    /// grouped into desktop tabs and the CLI configuration entry.
    /// Buttons keep Kit Button keyboard handling; the panel fill is
    /// `secondary`, so selection uses the shared stronger interaction fill.
    fn render_settings_navigation(&self, window: &Window, cx: &mut Context<Self>, narrow: bool) -> AnyElement {
        let colors = cx.theme().colors;
        let selected = self.settings_page_index();
        // Single ordered pass over `SettingsPage::ALL`: tabs, the CLI config
        // entry behind its divider/header, then About as the closing entry.
        // Building in page order keeps keyboard cycling, the Kit sidebar, and
        // this legacy rail on exactly the same index model.
        let mut items = Vec::with_capacity(SettingsPage::ALL.len() + 3);
        items.push(nav_group_header(
            "settings-nav-group-settings",
            rust_i18n::t!("app.settings.search.group_settings"),
            &colors,
        ));
        for (index, page) in SettingsPage::ALL.iter().enumerate() {
            if *page == SettingsPage::Configuration {
                items.push(
                    div()
                        .id("settings-nav-group-divider")
                        .test_support()
                        .w_full()
                        .child(row_divider(&colors))
                        .into_any_element(),
                );
                items.push(nav_group_header(
                    "settings-nav-group-cli-config",
                    rust_i18n::t!("app.settings.search.group_cli_config"),
                    &colors,
                ));
                let config_label = rust_i18n::t!("app.configuration.page_title").to_string();
                items.push(
                    desktop_ghost_button("settings-nav-configuration")
                        .accessibility_label(config_label.clone())
                        .child(nav_entry_content(
                            "settings-nav-label-configuration",
                            IconName::Settings2,
                            "settings-nav-icon-configuration",
                            config_label,
                        ))
                        .desktop_selected(selected == index, &colors)
                        .w_full()
                        .justify_start()
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.select_settings_page(SettingsTab::CONFIG_INDEX, window, cx);
                        }))
                        .into_any_element(),
                );
                continue;
            }
            let SettingsPage::Tab(tab) = page else {
                continue;
            };
            let active = index == selected;
            let icon = match tab.icon() {
                Some(name) => Icon::new(name),
                None => vtcode_nav_mark(),
            };
            items.push(
                desktop_ghost_button(format!("settings-nav-{tab:?}").to_lowercase())
                    .accessibility_label(tab.label())
                    .child(nav_entry_content(
                        format!("settings-nav-label-{tab:?}").to_lowercase(),
                        icon,
                        format!("settings-nav-icon-{tab:?}").to_lowercase(),
                        tab.label(),
                    ))
                    .desktop_selected(active, &colors)
                    .w_full()
                    .justify_start()
                    .on_click(cx.listener(move |view, _, window, cx| view.select_settings_page(index, window, cx)))
                    .into_any_element(),
            );
        }
        let entries = div().w_full().flex().flex_col().gap_1().children(items);
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
                desktop_ghost_button("settings-filter-search")
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
        if !Self::settings_use_legacy_layout(window) {
            return self.render_settings_kit(cx);
        }
        if self.config_page_open {
            // Side-by-side rail and rows need room for both navigations plus
            // readable rows (settings nav 240 + chrome + config rail 240 +
            // ~320 row minimum); narrower windows stack the rail above.
            let wide = window.viewport_size().width >= px(configuration_render::CONFIG_WIDE_VIEWPORT);
            return self.render_cli_config_page(wide, cx);
        }
        let title = self.settings_tab.label();
        let page = match self.settings_tab {
            SettingsTab::VtCode => self.render_vtcode_settings(cx),
            SettingsTab::Appearance => self.render_appearance_settings(cx),
            SettingsTab::Accessibility => self.render_accessibility_settings(cx),
            SettingsTab::Shortcuts => div()
                .id("keyboard-settings")
                .test_support()
                .child(self.render_shortcuts(window, cx))
                .into_any_element(),
            SettingsTab::Connections => self.render_connections_settings(cx),
            SettingsTab::Notifications => self.render_notification_settings(cx),
            SettingsTab::Storage => self.render_storage_settings(cx),
            SettingsTab::ComputerUse => self.render_computer_use_settings(cx),
            SettingsTab::About => self.render_about_settings(cx),
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

    /// Legacy stacked layout (navigation above content) below the shared
    /// breakpoint; wide viewports host the Kit `Settings` container.
    fn settings_use_legacy_layout(window: &Window) -> bool {
        window.viewport_size().width < px(NARROW_PANEL_BREAKPOINT)
    }

    /// Focus the settings selection chrome: the legacy navigation in
    /// narrow stacked mode, the panel root under the Kit host (whose
    /// sidebar is click-only until upstream keyboard navigation lands).
    pub(super) fn focus_settings_selection(&self, window: &mut Window, cx: &mut App) {
        if Self::settings_use_legacy_layout(window) {
            self.settings_navigation_focus.focus(window, cx);
        } else {
            self.settings_focus.focus(window, cx);
        }
    }

    /// Run a panel-level settings action only from the panel root. The
    /// legacy navigation scoped its key context to itself; the panel-level
    /// replacement covers the hosted content too, so without this gate
    /// tab-cycle arrows would hijack sliders, inputs, and lists while a
    /// control holds focus. Matched actions stop propagation by default,
    /// so the gate re-propagates for focused controls.
    fn run_settings_panel_action(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        act: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>),
    ) {
        if self.settings_focus.is_focused(window) {
            act(self, window, cx);
        } else {
            cx.propagate();
        }
    }

    /// Navigation index of the visible page in [`SettingsPage::ALL`]. The
    /// config page wins while open (the view keeps a tab selected behind it),
    /// and About resolves to the trailing slot past the config entry.
    pub(super) fn settings_page_index(&self) -> usize {
        let current = if self.config_page_open {
            SettingsPage::Configuration
        } else {
            SettingsPage::Tab(self.settings_tab)
        };
        SettingsPage::ALL.iter().position(|page| *page == current).unwrap_or_default()
    }

    fn select_settings_page(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.update_settings_page(index, false, window, cx);
    }

    fn select_settings_page_from_keyboard(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.update_settings_page(index, true, window, cx);
    }

    fn update_settings_page(&mut self, index: usize, reveal: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = SettingsPage::ALL.get(index).copied() else {
            return;
        };
        if page == SettingsPage::Configuration {
            self.config_page_open = true;
            if reveal {
                self.settings_nav_scroll.scroll_to_item(index);
            }
            self.focus_settings_selection(window, cx);
            cx.notify();
            return;
        }
        let SettingsPage::Tab(tab) = page else {
            return;
        };
        self.select_settings_tab(tab);
        if tab == SettingsTab::VtCode {
            self.refresh_login_item_status(cx);
        }
        if tab == SettingsTab::Storage {
            self.maybe_request_storage_summary(cx);
        }
        // The About page reads the cached release notes; ask for a status
        // only when launch never delivered one. Pending-gated and
        // snapshot-gated, so a repeat selection costs nothing.
        if tab == SettingsTab::About {
            self.maybe_request_update_status();
        }
        // Live connection state, like the storage summary: passive
        // selection refreshes only when a service exists to answer, so a
        // backend-less page never marks status dirty or posts a queue
        // notice. The request itself is pending-gated, so re-selecting
        // refreshes only after the previous round-trip settles.
        if tab == SettingsTab::Connections && self.backend.is_some() {
            self.request_webmcp_status();
        }
        if reveal {
            self.settings_nav_scroll.scroll_to_item(index);
        }
        self.focus_settings_selection(window, cx);
        cx.notify();
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

/// Kit-compatible switch handler: the `cx.listener` one-liner replacement
/// for plain boolean setters. The view handle updates outside render, so
/// the setter runs with full view context and no double-borrow.
pub(super) fn switch_action(
    view: &Entity<DesktopView>,
    set: fn(&mut DesktopView, bool, &mut Context<DesktopView>),
) -> impl Fn(&bool, &mut Window, &mut App) + use<> {
    let view = view.clone();
    move |checked: &bool, _: &mut Window, cx: &mut App| {
        view.update(cx, |view, cx| {
            set(view, *checked, cx);
        });
    }
}

#[cfg(test)]
mod tests;
