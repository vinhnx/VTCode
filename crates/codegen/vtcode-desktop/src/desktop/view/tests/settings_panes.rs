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

/// Windowed zen hides the toolbar that otherwise owns the traffic-light
/// zone, so route pages reserve it themselves: settings, browser, and review
/// start below the lights (fixed 40px: the lights end at y = 29) instead of
/// floating the lights over the panel corner and navigation column.
/// Fullscreen hides the lights, so the clearance collapses there.
#[gpui_kit::test]
fn windowed_zen_route_pages_start_below_the_traffic_lights(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // Normal mode first: the toolbar owns the light zone, so the settings
    // panel starts below it with no extra clearance.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        let back = window.find("navigate-back").bounds();
        let panel = window.find("settings-panel").bounds();
        assert!(
            back.origin.y + back.size.height <= panel.origin.y,
            "toolbar owns the light zone in normal mode: back={back:?} panel={panel:?}"
        );
    })
    .unwrap();
    // Windowed zen: every route page reserves the light zone itself.
    view.update(cx, |view, cx| {
        view.zen_mode = true;
        cx.notify();
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let panel = window.find("settings-panel").bounds();
        let viewport = window.viewport_size();
        assert_eq!(
            panel.origin.y,
            px(40.),
            "windowed-zen settings starts below the traffic lights (lights end at y = 29): panel={panel:?}"
        );
        assert!(
            panel.origin.y + panel.size.height <= viewport.height,
            "the top clearance must not push the panel out of the viewport: panel={panel:?} viewport={viewport:?}"
        );
        view.update(cx, |view, cx| view.open_thread_browser(window, cx));
        window.render_frame(cx);
        let browser = window.find("thread-browser").bounds();
        assert_eq!(
            browser.origin.y,
            px(40.),
            "windowed-zen browser starts below the traffic lights: browser={browser:?}"
        );
        view.update(cx, |view, cx| view.show_review(window, cx));
        window.render_frame(cx);
        let review = window.find("diff-review-panel").bounds();
        assert_eq!(review.origin.y, px(40.), "windowed-zen review starts below the traffic lights: review={review:?}");
    })
    .unwrap();
    // Narrow windowed zen stacks the settings nav above the content; the
    // clearance is vertical-only, so it must survive the reflow.
    cx.simulate_window_resize(window, size(px(600.), px(700.)));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        let panel = window.find("settings-panel").bounds();
        let viewport = window.viewport_size();
        assert_eq!(panel.origin.y, px(40.), "narrow windowed-zen settings keeps the light clearance: panel={panel:?}");
        assert!(
            panel.origin.x + panel.size.width <= viewport.width
                && panel.origin.y + panel.size.height <= viewport.height,
            "narrow windowed-zen settings stays inside the viewport: panel={panel:?} viewport={viewport:?}"
        );
    })
    .unwrap();
    cx.simulate_window_resize(window, size(px(1280.), px(900.)));
    // Fullscreen hides the lights: the clearance collapses to zero on every
    // route, not just the current one.
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::ToggleFullScreen), cx);
    })
    .unwrap();
    cx.run_until_parked();
    view.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let settings = window.find("settings-panel").bounds();
        assert_eq!(
            settings.origin.y,
            px(0.),
            "fullscreen hides the lights, so settings reserves nothing: settings={settings:?}"
        );
        view.update(cx, |view, cx| view.open_thread_browser(window, cx));
        window.render_frame(cx);
        let browser = window.find("thread-browser").bounds();
        assert_eq!(
            browser.origin.y,
            px(0.),
            "fullscreen hides the lights, so the browser reserves nothing: browser={browser:?}"
        );
        view.update(cx, |view, cx| view.show_review(window, cx));
        window.render_frame(cx);
        let review = window.find("diff-review-panel").bounds();
        assert_eq!(
            review.origin.y,
            px(0.),
            "fullscreen hides the lights, so review reserves nothing: review={review:?}"
        );
    })
    .unwrap();
}
