use super::*;

impl DesktopView {
    pub(super) fn open_utilities(&mut self, cx: &mut Context<Self>) {
        self.inspector_open = true;
        self.inspector_tab = InspectorTab::Utilities;
        cx.notify();
    }

    pub(super) fn render_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let expanded = self.sidebar_open || self.navigation_peek;
        let rail_surface = themes::navigation_surfaces(colors.background, colors.secondary, colors.foreground).rail;
        div()
            .id("left-navigation")
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
            }))
            .child(
                div()
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
                                    .icon(IconName::Folder)
                                    .ghost()
                                    .accessibility_label("Projects and threads")
                                    .tooltip_with_action("Projects and threads", &ToggleSidebar, Some("VTCodeDesktop"))
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.sidebar_open = !view.sidebar_open;
                                        view.navigation_peek = false;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        desktop_button("nav-new-thread")
                            .icon(IconName::Plus)
                            .ghost()
                            .accessibility_label("New thread")
                            .tooltip_with_action("New thread", &NewSession, Some("VTCodeDesktop"))
                            .desktop_disabled(!self.session_change_allowed(None))
                            .on_click(cx.listener(|view, _, _, cx| view.create_session(cx))),
                    )
                    .child(
                        desktop_button("nav-search")
                            .icon(IconName::Search)
                            .ghost()
                            .accessibility_label("Search commands")
                            .tooltip_with_action("Search commands", &OpenCommandPalette, Some("VTCodeDesktop"))
                            .on_click(cx.listener(|view, _, window, cx| view.open_command_palette(window, cx))),
                    )
                    .child(
                        desktop_button("nav-utilities")
                            .icon(IconName::GitCompare)
                            .ghost()
                            .accessibility_label("Workspace utilities")
                            .tooltip_with_action("Workspace utilities", &OpenUtilities, Some("VTCodeDesktop"))
                            .desktop_disabled(self.workspace.is_none())
                            .on_click(cx.listener(|view, _, _, cx| view.open_utilities(cx))),
                    )
                    .child(self.render_chrome_menu("navigation-overflow", "Navigation actions"))
                    .child(div().flex_1())
                    .child(
                        desktop_button("nav-settings")
                            .icon(IconName::Settings2)
                            .ghost()
                            .accessibility_label("Settings")
                            .tooltip_with_action("Settings", &OpenSettings, Some("VTCodeDesktop"))
                            .on_click(cx.listener(|view, _, window, cx| view.open_settings(window, cx))),
                    ),
            )
            .when(expanded, |navigation| navigation.child(self.render_sidebar(cx)))
            .bg(rail_surface)
            .into_any_element()
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
                    .menu("Search commands", Box::new(OpenCommandPalette))
                    .separator()
                    .menu("Toggle status bar", Box::new(ToggleStatusBar))
                    .menu("Settings…", Box::new(OpenSettings))
            })
            .into_any_element()
    }
}
