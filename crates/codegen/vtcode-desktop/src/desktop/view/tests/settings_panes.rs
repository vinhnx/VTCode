use super::*;

/// Settings with the configuration page open: asymmetric group names ("Tools"
/// vs "Model & Provider") so alignment assertions compare a short label
/// against a long one instead of two identical fixtures.
fn open_config_panes(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<DesktopView>) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![
                    config_page_setting("agent.default_model", json!("model-a"), "Model & Provider"),
                    config_page_setting("tools.auto_approve", json!(true), "Tools"),
                ],
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            });
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
    (window, view)
}

#[gpui_kit::test]
fn config_rail_entries_share_a_leading_spine_when_wide(cx: &mut TestAppContext) {
    let (window, _view) = open_config_panes(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let curated = window.find("config-category-label-curated").bounds();
        let tools = window.find("config-category-label-Tools").bounds();
        let model = window.find("config-category-label-Model & Provider").bounds();
        assert_eq!(
            tools.origin.x, model.origin.x,
            "short and long rail labels start on one spine: tools={tools:?} model={model:?}"
        );
        assert_eq!(
            curated.origin.x, model.origin.x,
            "the overview entry aligns with catalog groups: curated={curated:?} model={model:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn config_pane_divider_sits_between_rail_and_content(cx: &mut TestAppContext) {
    let (window, _view) = open_config_panes(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let rail = window.find("config-rail").bounds();
        let divider = window.find("config-pane-divider").bounds();
        let content = window.find("cli-config-page-content").bounds();
        assert_eq!(divider.size.width, px(1.), "wide divider is a 1px vertical line: {divider:?}");
        assert!(divider.size.height > px(0.), "the divider spans the body height: {divider:?}");
        assert!(
            rail.origin.x + rail.size.width <= divider.origin.x + px(1.)
                && divider.origin.x + divider.size.width <= content.origin.x + px(1.),
            "wide viewports put the divider between rail and content: rail={rail:?} divider={divider:?} content={content:?}"
        );
    })
    .unwrap();

    cx.simulate_window_resize(window, size(px(600.), px(700.)));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let rail = window.find("config-rail").bounds();
        let divider = window.find("config-pane-divider").bounds();
        let content = window.find("cli-config-page-content").bounds();
        assert_eq!(divider.size.height, px(1.), "stacked divider is a 1px horizontal line: {divider:?}");
        assert!(divider.size.width > px(0.), "the divider spans the body width: {divider:?}");
        assert!(
            rail.origin.y + rail.size.height <= divider.origin.y + px(1.)
                && divider.origin.y + divider.size.height <= content.origin.y + px(1.),
            "narrow viewports stack rail, divider, content: rail={rail:?} divider={divider:?} content={content:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn config_rail_marks_the_selected_group(cx: &mut TestAppContext) {
    let (window, _view) = open_config_panes(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("config-category-curated").checked(), Some(true));
        assert_eq!(window.find("config-category-Tools").checked(), Some(false));
        window.click("config-category-Tools", cx);
        window.render_frame(cx);
        assert_eq!(window.find("config-category-Tools").checked(), Some(true));
        assert_eq!(window.find("config-category-curated").checked(), Some(false));
        assert_eq!(window.find("config-category-Tools").label(), Some("Tools"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_pane_divider_sits_between_navigation_and_content(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        let navigation = window.find("settings-navigation").bounds();
        let divider = window.find("settings-pane-divider").bounds();
        let content = window.find("settings-content").bounds();
        assert_eq!(divider.size.width, px(1.), "wide divider is a 1px vertical line: {divider:?}");
        assert!(divider.size.height > px(0.), "the divider spans the panel height: {divider:?}");
        assert!(
            navigation.origin.x + navigation.size.width <= divider.origin.x + px(1.)
                && divider.origin.x + divider.size.width <= content.origin.x + px(1.),
            "wide viewports put the divider between nav and content: navigation={navigation:?} divider={divider:?} content={content:?}"
        );
    })
    .unwrap();

    cx.simulate_window_resize(window, size(px(600.), px(700.)));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let navigation = window.find("settings-navigation").bounds();
        let divider = window.find("settings-pane-divider").bounds();
        let content = window.find("settings-content").bounds();
        assert_eq!(divider.size.height, px(1.), "stacked divider is a 1px horizontal line: {divider:?}");
        assert!(divider.size.width > px(0.), "the divider spans the panel width: {divider:?}");
        assert!(
            navigation.origin.y + navigation.size.height <= divider.origin.y + px(1.)
                && divider.origin.y + divider.size.height <= content.origin.y + px(1.),
            "narrow viewports stack nav, divider, content: navigation={navigation:?} divider={divider:?} content={content:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_navigation_groups_desktop_tabs_and_cli_config(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        let header_settings = window.find("settings-nav-group-settings").bounds();
        let appearance = window.find("settings-nav-appearance").bounds();
        let storage = window.find("settings-nav-storage").bounds();
        let divider = window.find("settings-nav-group-divider").bounds();
        let header_cli = window.find("settings-nav-group-cli-config").bounds();
        let configuration = window.find("settings-nav-configuration").bounds();
        assert!(
            header_settings.origin.y < appearance.origin.y && appearance.origin.y < storage.origin.y,
            "desktop tabs follow their group header: header={header_settings:?} appearance={appearance:?} storage={storage:?}"
        );
        assert!(
            storage.origin.y + storage.size.height <= divider.origin.y + px(1.)
                && divider.origin.y + divider.size.height <= header_cli.origin.y + px(1.)
                && header_cli.origin.y + header_cli.size.height <= configuration.origin.y + px(1.),
            "a divider and header separate the CLI config entry: storage={storage:?} divider={divider:?} header={header_cli:?} configuration={configuration:?}"
        );
    })
    .unwrap();
}
