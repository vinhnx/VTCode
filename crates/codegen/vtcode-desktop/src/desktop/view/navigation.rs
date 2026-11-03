use super::*;

impl DesktopView {
    pub(super) fn open_utilities(&mut self, cx: &mut Context<Self>) {
        self.open_inspector_tab(InspectorTab::Utilities, cx);
    }

    pub(super) fn render_navigation(&mut self, narrow_panel: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let expanded = (self.sidebar_open || self.navigation_peek) && !narrow_panel;
        let navigation_on_right = self.preferences.navigation_on_right();
        let utilities_selected = self.inspector_open && self.inspector_tab == InspectorTab::Utilities;
        let rail = div()
            .id("navigation-rail")
            .test_support()
            .w(px(48.))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .py_2()
            .when(!navigation_on_right, |rail| rail.pt(px(TOOLBAR_HEIGHT)))
            .bg(chrome_background(&colors))
            .child(
                div()
                    .id("nav-home-hover")
                    .on_hover(cx.listener(|view, hovered: &bool, _, cx| {
                        if *hovered && !view.sidebar_open {
                            view.navigation_peek = true;
                            cx.notify();
                        }
                    }))
                    .child(
                        desktop_button("nav-home")
                            .icon(IconName::House)
                            .ghost()
                            .rounded_lg()
                            .accessibility_label(if expanded {
                                rust_i18n::t!("app.navigation.projects_threads_expanded").to_string()
                            } else {
                                rust_i18n::t!("app.navigation.projects_threads").to_string()
                            })
                            .toggled(expanded)
                            .tooltip_with_action(
                                rust_i18n::t!("app.navigation.projects_threads").to_string(),
                                &ToggleSidebar,
                                Some(key_context::DESKTOP),
                            )
                            .when(expanded, |button| button.bg(colors.secondary).text_color(colors.foreground))
                            .dispatches_desktop_action(ToggleSidebar, self.focus_handle.clone()),
                    ),
            )
            .when(!expanded, |rail| {
                rail.child(
                    desktop_button("nav-new-thread")
                        .icon(IconName::Plus)
                        .ghost()
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
            // the lane so Utilities never shifts when the rail expands.
            .when(expanded, |rail| rail.child(div().size_8()))
            .child(
                desktop_button("nav-utilities")
                    .icon(IconName::GitBranch)
                    .ghost()
                    .rounded_lg()
                    .accessibility_label(if utilities_selected {
                        rust_i18n::t!("app.navigation.workspace_utilities_selected").to_string()
                    } else {
                        rust_i18n::t!(i18n::NAVIGATION_WORKSPACE_UTILITIES).to_string()
                    })
                    .tooltip_with_action(
                        rust_i18n::t!(i18n::NAVIGATION_WORKSPACE_UTILITIES).to_string(),
                        &OpenUtilities,
                        Some(key_context::DESKTOP),
                    )
                    .desktop_disabled(self.workspace.is_none())
                    .when(utilities_selected, |button| button.bg(colors.secondary).text_color(colors.foreground))
                    .dispatches_desktop_action(OpenUtilities, self.focus_handle.clone()),
            )
            .child(
                desktop_button("nav-browse-threads")
                    .icon(IconName::Maximize2)
                    .ghost()
                    .rounded_lg()
                    .accessibility_label(rust_i18n::t!(i18n::BROWSER_BROWSE_THREADS).to_string())
                    .tooltip_with_action(
                        rust_i18n::t!(i18n::BROWSER_BROWSE_THREADS).to_string(),
                        &OpenThreadBrowser,
                        Some(key_context::DESKTOP),
                    )
                    .when(self.current_route() == Route::Browser, |button| {
                        button.bg(colors.secondary).text_color(colors.foreground)
                    })
                    .dispatches_desktop_action(OpenThreadBrowser, self.focus_handle.clone()),
            )
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
            .when(!self.preferences.translucent_sidebar, |navigation| navigation.bg(chrome_background(&colors)))
            .with_animation("navigation-fade", zen_fade_animation(cx.theme()), |el, delta| el.opacity(delta))
            .into_any_element()
    }
    pub(super) fn render_chrome_menu(&self, id: &'static str, label: String) -> AnyElement {
        let can_new = self.new_thread_available();
        let can_close = self.selected_session_id.is_some() && self.new_thread_available();
        let has_workspace = self.workspace.is_some();
        let panel_open = self.current_route() != Route::Workspace;
        desktop_button(id)
            .icon(IconName::Ellipsis)
            .ghost()
            .small()
            .accessibility_label(label.as_str())
            .tooltip(label)
            .dropdown_menu(move |menu, _, _| {
                menu.menu_with_disabled(
                    rust_i18n::t!(i18n::NAVIGATION_NEW_THREAD).to_string(),
                    Box::new(NewSession),
                    !can_new,
                )
                .menu_with_disabled(
                    rust_i18n::t!(i18n::NAVIGATION_CLOSE_THREAD).to_string(),
                    Box::new(CloseSession),
                    !can_close,
                )
                .separator()
                .menu(rust_i18n::t!(i18n::NAVIGATION_TOGGLE_NAVIGATION).to_string(), Box::new(ToggleSidebar))
                .menu_with_disabled(
                    rust_i18n::t!(i18n::NAVIGATION_TOGGLE_INSPECTOR).to_string(),
                    Box::new(ToggleInspector),
                    !has_workspace || panel_open,
                )
                .menu(rust_i18n::t!("app.navigation.toggle_zen_mode").to_string(), Box::new(ToggleZenMode))
                .menu(rust_i18n::t!(i18n::NAVIGATION_SEARCH_COMMANDS).to_string(), Box::new(OpenCommandPalette))
                .separator()
                .menu(rust_i18n::t!("app.navigation.toggle_status_bar").to_string(), Box::new(ToggleStatusBar))
                .menu(rust_i18n::t!("app.navigation.settings_menu").to_string(), Box::new(OpenSettings))
            })
            .into_any_element()
    }
}
