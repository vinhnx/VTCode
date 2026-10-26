use super::*;

impl DesktopView {
    pub(super) fn open_utilities(&mut self, cx: &mut Context<Self>) {
        self.inspector_open = true;
        self.set_inspector_tab(InspectorTab::Utilities, cx);
    }

    pub(super) fn render_navigation(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        if self.zen_mode {
            let rail = div()
                .id("navigation-rail")
                .test_support()
                .w(px(48.))
                .flex_shrink_0()
                .h_full()
                .flex()
                .flex_col()
                .items_center()
                .py_2()
                .child(
                    desktop_button("zen-mode-exit")
                        .icon(IconName::Focus)
                        .rounded_lg()
                        .bg(colors.primary.opacity(0.18))
                        .text_color(colors.primary)
                        .accessibility_label("Exit Zen mode")
                        .tooltip_with_action("Exit Zen mode", &ToggleZenMode, Some("VTCodeDesktop"))
                        .dispatches_desktop_action(ToggleZenMode, self.focus_handle.clone()),
                )
                .child(div().flex_1());
            return div()
                .id("primary-navigation")
                .test_support()
                .role(gpui_kit::Role::Navigation)
                .aria_label("Zen mode navigation")
                .aria_description("One control exits Zen mode.")
                .h_full()
                .flex_shrink_0()
                .bg(chrome_background(&colors))
                .child(rail)
                .into_any_element();
        }

        let expanded = self.sidebar_open || self.navigation_peek;
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
                                "Projects and threads (expanded)"
                            } else {
                                "Projects and threads"
                            })
                            .toggled(expanded)
                            .tooltip_with_action("Projects and threads", &ToggleSidebar, Some("VTCodeDesktop"))
                            .when(expanded, |button| button.bg(colors.secondary).text_color(colors.foreground))
                            .dispatches_desktop_action(ToggleSidebar, self.focus_handle.clone()),
                    ),
            )
            .when(!expanded, |rail| {
                rail.child(
                    desktop_button("nav-new-thread")
                        .icon(IconName::Plus)
                        .ghost()
                        .accessibility_label("New thread")
                        .tooltip_with_action("New thread", &NewSession, Some("VTCodeDesktop"))
                        .desktop_disabled(!self.session_change_allowed(None))
                        .dispatches_desktop_action(NewSession, self.focus_handle.clone()),
                )
            })
            .child(
                desktop_button("nav-utilities")
                    .icon(IconName::GitBranch)
                    .ghost()
                    .rounded_lg()
                    .accessibility_label(if utilities_selected {
                        "Workspace utilities (selected)"
                    } else {
                        "Workspace utilities"
                    })
                    .tooltip_with_action("Workspace utilities", &OpenUtilities, Some("VTCodeDesktop"))
                    .desktop_disabled(self.workspace.is_none())
                    .when(utilities_selected, |button| button.bg(colors.secondary).text_color(colors.foreground))
                    .dispatches_desktop_action(OpenUtilities, self.focus_handle.clone()),
            )
            .child(div().flex_1())
            .child(
                desktop_button("zen-mode-toggle")
                    .icon(IconName::Focus)
                    .ghost()
                    .rounded_lg()
                    .accessibility_label("Enter Zen mode")
                    .tooltip_with_action("Enter Zen mode", &ToggleZenMode, Some("VTCodeDesktop"))
                    .dispatches_desktop_action(ToggleZenMode, self.focus_handle.clone()),
            )
            .into_any_element();
        let navigation = div()
            .id("primary-navigation")
            .test_support()
            .role(gpui_kit::Role::Navigation)
            .aria_label("VT Code navigation")
            .aria_description("Projects, threads, and workspace tools.")
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
        navigation.bg(chrome_background(&colors)).into_any_element()
    }
    pub(super) fn render_chrome_menu(&self, id: &'static str, label: &'static str) -> AnyElement {
        let can_new = self.session_change_allowed(None);
        let can_close = self.selected_session_id.is_some() && self.session_change_allowed(None);
        let has_workspace = self.workspace.is_some();
        desktop_button(id)
            .icon(IconName::Ellipsis)
            .ghost()
            .small()
            .rounded_lg()
            .accessibility_label(label)
            .tooltip(label)
            .dropdown_menu(move |menu, _, _| {
                menu.menu_with_disabled("New thread", Box::new(NewSession), !can_new)
                    .menu_with_disabled("Close thread", Box::new(CloseSession), !can_close)
                    .separator()
                    .menu("Toggle navigation", Box::new(ToggleSidebar))
                    .menu_with_disabled("Toggle inspector", Box::new(ToggleInspector), !has_workspace)
                    .menu("Toggle Zen mode", Box::new(ToggleZenMode))
                    .menu("Search commands", Box::new(OpenCommandPalette))
                    .separator()
                    .menu("Toggle status bar", Box::new(ToggleStatusBar))
                    .menu("Settings…", Box::new(OpenSettings))
            })
            .into_any_element()
    }
}
