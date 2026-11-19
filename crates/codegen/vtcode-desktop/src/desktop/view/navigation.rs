use super::*;

/// Actions that can move out of an icon track into a single `...` overflow.
/// One mapping drives every overflow trigger, so labels, state labels, and
/// enable state share a single source across the rail and the toolbar.
#[derive(Clone, Copy)]
enum NavMenuAction {
    NewThread,
    CloseThread,
    ToggleNavigation,
    ToggleInspector,
    ToggleZen,
    SearchCommands,
    ToggleStatusBar,
    Settings,
    Utilities,
    Terminal,
}

impl NavMenuAction {
    /// State-bearing labels keep the old rail/tooltips' accessible state in
    /// menu form, where there is no pressed/highlighted treatment to read it.
    fn label(self, view: &DesktopView) -> String {
        match self {
            Self::NewThread => rust_i18n::t!(i18n::NAVIGATION_NEW_THREAD).to_string(),
            Self::CloseThread => rust_i18n::t!(i18n::NAVIGATION_CLOSE_THREAD).to_string(),
            Self::ToggleNavigation if !view.preferences.navigation_visible() => {
                rust_i18n::t!("app.navigation.show_navigation").to_string()
            }
            Self::ToggleNavigation => rust_i18n::t!(i18n::NAVIGATION_TOGGLE_NAVIGATION).to_string(),
            Self::ToggleInspector => rust_i18n::t!(i18n::NAVIGATION_TOGGLE_INSPECTOR).to_string(),
            Self::ToggleZen => rust_i18n::t!("app.navigation.toggle_zen_mode").to_string(),
            Self::SearchCommands => rust_i18n::t!(i18n::NAVIGATION_SEARCH_COMMANDS).to_string(),
            Self::ToggleStatusBar => rust_i18n::t!("app.navigation.toggle_status_bar").to_string(),
            Self::Settings => rust_i18n::t!("app.navigation.settings_menu").to_string(),
            Self::Utilities if view.inspector_open && view.inspector_tab == InspectorTab::Utilities => {
                rust_i18n::t!("app.navigation.workspace_utilities_selected").to_string()
            }
            Self::Utilities => rust_i18n::t!(i18n::NAVIGATION_WORKSPACE_UTILITIES).to_string(),
            Self::Terminal if view.terminal_visible => rust_i18n::t!("app.navigation.terminal_hide").to_string(),
            Self::Terminal => rust_i18n::t!("app.navigation.terminal").to_string(),
        }
    }

    fn action(self) -> Box<dyn gpui_kit::Action> {
        match self {
            Self::NewThread => Box::new(NewSession),
            Self::CloseThread => Box::new(CloseSession),
            Self::ToggleNavigation => Box::new(ToggleSidebar),
            Self::ToggleInspector => Box::new(ToggleInspector),
            Self::ToggleZen => Box::new(ToggleZenMode),
            Self::SearchCommands => Box::new(OpenCommandPalette),
            Self::ToggleStatusBar => Box::new(ToggleStatusBar),
            Self::Settings => Box::new(OpenSettings),
            Self::Utilities => Box::new(OpenUtilities),
            Self::Terminal => Box::new(ToggleTerminal),
        }
    }

    fn disabled(self, view: &DesktopView) -> bool {
        match self {
            Self::NewThread => !view.new_thread_available(),
            Self::CloseThread => view.selected_session_id.is_none() || !view.new_thread_available(),
            Self::ToggleInspector => view.workspace.is_none() || view.current_route() != Route::Workspace,
            Self::Utilities => view.workspace.is_none(),
            _ => false,
        }
    }
}

impl DesktopView {
    /// Single `...` trigger for a capped nav track. Taking the item list as
    /// data keeps the rail and toolbar overflows consistent by construction.
    fn render_nav_overflow(&self, id: &'static str, label: String, small: bool, items: &[NavMenuAction]) -> AnyElement {
        let items: Vec<NavMenuAction> = items.to_vec();
        let entries: Vec<(String, bool)> = items.iter().map(|item| (item.label(self), item.disabled(self))).collect();
        desktop_ghost_button(id)
            .icon(IconName::Ellipsis)
            .when(small, |button| button.small())
            .accessibility_label(label.as_str())
            .tooltip(label)
            .dropdown_menu(move |menu, _, _| {
                items.iter().zip(entries.iter()).fold(menu, |menu, (item, (label, disabled))| {
                    menu.menu_with_disabled(label.as_str(), item.action(), *disabled)
                })
            })
            .into_any_element()
    }

    pub(super) fn open_utilities(&mut self, cx: &mut Context<Self>) {
        self.open_inspector_tab(InspectorTab::Utilities, cx);
    }

    pub(super) fn render_browse_threads_button(&self, cx: &mut Context<Self>) -> Button {
        let colors = cx.theme().colors;
        let label = rust_i18n::t!(i18n::BROWSER_BROWSE_THREADS).to_string();
        desktop_ghost_button("nav-browse-threads")
            .icon(IconName::Clock)
            .rounded_lg()
            .when(self.zen_mode, |button| button.small())
            .desktop_selected(self.current_route() == Route::Browser, &colors)
            .accessibility_label(label.clone())
            .tooltip_with_action(label, &OpenThreadBrowser, Some(key_context::DESKTOP))
            .dispatches_desktop_action(OpenThreadBrowser, self.focus_handle.clone())
    }

    /// The House button returns to the main workspace landing page from any
    /// route. Shared by the rail and Zen's floating chrome so every Home
    /// affordance dispatches the same action.
    pub(super) fn render_go_home_button(&self, small: bool) -> Button {
        let label = rust_i18n::t!("app.navigation.workspace_fallback").to_string();
        desktop_ghost_button("nav-home")
            .icon(IconName::House)
            .rounded_lg()
            .when(small, |button| button.small())
            .accessibility_label(label.clone())
            .tooltip_with_action(label, &GoHome, Some(key_context::DESKTOP))
            .dispatches_desktop_action(GoHome, self.focus_handle.clone())
    }

    pub(super) fn render_navigation(&mut self, narrow_panel: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let expanded = (self.sidebar_open || self.navigation_peek) && !narrow_panel;
        let navigation_on_right = self.preferences.navigation_on_right();
        let rail = div()
            .id("navigation-rail")
            .test_support()
            .w(px(NAVIGATION_RAIL_WIDTH))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .py_2()
            .when(!navigation_on_right, |rail| rail.pt(px(TOOLBAR_HEIGHT)))
            // The rail is part of the navigation panel the translucent
            // sidebar setting makes glassy: leave it unpainted so the window
            // vibrancy shows through, matching the container gate below.
            .when(!self.effective_translucent_sidebar(), |rail| rail.bg(chrome_background(&colors)))
            .child(
                div()
                    .id("nav-home-hover")
                    .on_hover(cx.listener(|view, hovered: &bool, _, cx| {
                        if *hovered && !view.sidebar_open {
                            view.navigation_peek = true;
                            cx.notify();
                        }
                    }))
                    .child(self.render_go_home_button(false)),
            )
            .when(!expanded, |rail| {
                rail.child(
                    desktop_ghost_button("nav-new-thread")
                        .icon(IconName::Plus)
                        .rounded_lg()
                        .accessibility_label(rust_i18n::t!(i18n::NAVIGATION_NEW_THREAD).to_string())
                        .tooltip_with_action(
                            rust_i18n::t!(i18n::NAVIGATION_NEW_THREAD).to_string(),
                            &NewSession,
                            Some(key_context::DESKTOP),
                        )
                        .desktop_disabled(!self.new_thread_available())
                        .dispatches_desktop_action(NewSession, self.focus_handle.clone()),
                )
            })
            // The sidebar owns New thread while expanded; the empty slot holds
            // the lane so Browse threads never shifts when the rail expands.
            .when(expanded, |rail| rail.child(div().size_8()))
            .child(self.render_browse_threads_button(cx))
            // Three pinned icons above; everything else lives in one overflow.
            .child(self.render_nav_overflow(
                "nav-overflow",
                rust_i18n::t!("app.navigation.workspace_actions").to_string(),
                false,
                &[
                    NavMenuAction::Utilities,
                    NavMenuAction::Terminal,
                    NavMenuAction::Settings,
                ],
            ))
            .child(div().flex_grow(1.0))
            .into_any_element();
        let navigation = div()
            .id("primary-navigation")
            .test_support()
            .role(gpui_kit::Role::Navigation)
            .aria_label(rust_i18n::t!("app.navigation.navigation_label").to_string())
            .aria_description(rust_i18n::t!("app.navigation.navigation_description").to_string())
            .h_full()
            .flex_shrink_0()
            .flex()
            .gap_1()
            .on_hover(cx.listener(|view, hovered: &bool, _, cx| {
                if !hovered && view.navigation_peek {
                    view.navigation_peek = false;
                    cx.notify();
                }
            }));
        let navigation = if expanded && navigation_on_right {
            navigation.child(self.render_sidebar(cx)).child(rail)
        } else {
            navigation
                .child(rail)
                .when(expanded, |navigation| navigation.child(self.render_sidebar(cx)))
        };
        navigation
            .when(!self.effective_translucent_sidebar(), |navigation| navigation.bg(chrome_background(&colors)))
            .fade_in("navigation-fade", cx.theme())
            .into_any_element()
    }
    pub(super) fn render_chrome_menu(&self, id: &'static str, label: String) -> AnyElement {
        self.render_nav_overflow(
            id,
            label,
            true,
            &[
                NavMenuAction::NewThread,
                NavMenuAction::CloseThread,
                NavMenuAction::ToggleNavigation,
                NavMenuAction::ToggleInspector,
                NavMenuAction::ToggleZen,
                NavMenuAction::SearchCommands,
                NavMenuAction::ToggleStatusBar,
                NavMenuAction::Settings,
                NavMenuAction::Utilities,
            ],
        )
    }
}
