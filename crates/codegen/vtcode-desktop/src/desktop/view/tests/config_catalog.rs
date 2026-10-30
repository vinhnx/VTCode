use super::super::config_search::ConfigSearchCorpus;
use super::super::configuration::{CONFIG_GROUP_PAGE_SIZE, build_config_catalog_cache, config_group_icon};
use super::*;

fn paged_snapshot(prefix: &str, group: &str, count: usize) -> vtcode_desktop_protocol::ConfigSettingsSnapshot {
    vtcode_desktop_protocol::ConfigSettingsSnapshot {
        settings: (0..count)
            .map(|index| config_page_setting(&format!("{prefix}.row_{index:03}"), json!(index), group))
            .collect(),
        target_versions: Vec::new(),
        omitted_sensitive: 0,
        omitted_large: 0,
        omitted_limit: 0,
    }
}

fn two_group_snapshot(count_per_group: usize) -> vtcode_desktop_protocol::ConfigSettingsSnapshot {
    let mut first = paged_snapshot("tools", "Tools", count_per_group);
    let second = paged_snapshot("agent", "Model & Provider", count_per_group);
    first.settings.extend(second.settings);
    first
}

fn open_paged_config(
    cx: &mut TestAppContext,
    snapshot: vtcode_desktop_protocol::ConfigSettingsSnapshot,
    group: &str,
) -> (AnyWindowHandle, Entity<DesktopView>) {
    let (window, view) = open_desktop(cx);
    let group = group.to_string();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands: tokio::sync::mpsc::channel(2).0 });
            view.cli_config_settings = Some(snapshot);
            view.config_page_group = Some(group);
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
    (window, view)
}

/// `find_all` sees invisible elements too, so an empty result proves the row
/// was never built rather than merely scrolled out of view.
fn built_config_rows(window: &mut gpui_kit::Window, prefix: &str, count: usize) -> usize {
    (0..count)
        .filter(|index| !window.find_all(format!("{prefix}{index:03}")).is_empty())
        .count()
}

fn scroll_config_show_more_into_view(
    window: &mut gpui_kit::Window,
    view: &Entity<DesktopView>,
    cx: &mut gpui_kit::App,
) {
    for _ in 0..120 {
        let viewport = window.find("cli-config-page-content").bounds();
        let item = window
            .find_all("config-show-more")
            .into_iter()
            .next()
            .expect("show-more renders while rows remain")
            .bounds();
        if item.top() >= viewport.top() - px(1.) && item.bottom() <= viewport.bottom() + px(1.) {
            return;
        }
        view.update(cx, |view, cx| {
            let mut offset = view.settings_scroll.offset();
            offset.y -= px(600.);
            view.settings_scroll.set_offset(offset);
            cx.notify();
        });
        window.render_frame(cx);
    }
    panic!("config-show-more did not scroll into view");
}

#[gpui_kit::test]
fn config_group_pages_rows_and_grows_on_show_more(cx: &mut TestAppContext) {
    // Asymmetric fixture: one full page plus a partial second page.
    let total = CONFIG_GROUP_PAGE_SIZE + 10;
    let (window, view) = open_paged_config(cx, paged_snapshot("tools", "Tools", total), "Tools");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            built_config_rows(window, "config-edit-tools.row_", total),
            CONFIG_GROUP_PAGE_SIZE,
            "only the first page is built"
        );
        assert!(window.try_find("config-group-status").is_some());
        let show_more = window.find_all("config-show-more");
        assert_eq!(show_more.len(), 1);
        let expected = format!("Show more settings, {CONFIG_GROUP_PAGE_SIZE} of {total} shown");
        assert_eq!(show_more[0].label(), Some(expected.as_str()));

        scroll_config_show_more_into_view(window, &view, cx);
        window.click("config-show-more", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).config_visible_row_limit, CONFIG_GROUP_PAGE_SIZE * 2);
        assert_eq!(
            built_config_rows(window, "config-edit-tools.row_", total),
            total,
            "paging builds every remaining row"
        );
        assert!(window.find_all("config-show-more").is_empty(), "control hides once every row shows");
    })
    .unwrap();
}

#[gpui_kit::test]
fn config_group_paging_resets_on_search_and_group_change(cx: &mut TestAppContext) {
    let total = CONFIG_GROUP_PAGE_SIZE + 5;
    let (window, view) = open_paged_config(cx, two_group_snapshot(total), "Tools");
    cx.update_window(window, |_, window, cx| {
        scroll_config_show_more_into_view(window, &view, cx);
        window.click("config-show-more", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).config_visible_row_limit, CONFIG_GROUP_PAGE_SIZE * 2);

        view.update(cx, |view, cx| view.set_config_search_query("row_001", window, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).config_visible_row_limit, CONFIG_GROUP_PAGE_SIZE);
        assert_eq!(built_config_rows(window, "config-edit-tools.row_", total), 1);
        assert_eq!(
            built_config_rows(window, "config-edit-agent.row_", total),
            0,
            "the unselected group stays unbuilt while filtered"
        );
        assert!(window.find_all("config-show-more").is_empty(), "filtered match fits on one page");

        view.update(cx, |view, cx| view.set_config_search_query("", window, cx));
        window.render_frame(cx);
        window.click("config-category-Model & Provider", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).config_visible_row_limit, CONFIG_GROUP_PAGE_SIZE);
        assert_eq!(
            built_config_rows(window, "config-edit-agent.row_", total),
            CONFIG_GROUP_PAGE_SIZE,
            "switching groups restarts paging on the new group"
        );
        assert_eq!(window.find_all("config-show-more").len(), 1);
    })
    .unwrap();
}

#[gpui_kit::test]
fn config_reload_preserves_paging_position(cx: &mut TestAppContext) {
    let total = CONFIG_GROUP_PAGE_SIZE + 5;
    let (window, view) = open_paged_config(cx, paged_snapshot("tools", "Tools", total), "Tools");
    cx.update_window(window, |_, window, cx| {
        scroll_config_show_more_into_view(window, &view, cx);
        window.click("config-show-more", cx);
        window.render_frame(cx);
        assert_eq!(built_config_rows(window, "config-edit-tools.row_", total), total);

        view.update(cx, |view, cx| {
            view.handle_cli_config_read_response(
                serde_json::to_value(paged_snapshot("tools", "Tools", total)).unwrap(),
                cx,
            );
        });
        window.render_frame(cx);
        assert_eq!(view.read(cx).config_visible_row_limit, CONFIG_GROUP_PAGE_SIZE * 2);
        assert_eq!(
            built_config_rows(window, "config-edit-tools.row_", total),
            total,
            "post-write reloads keep the reader on their page"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn config_catalog_cache_refreshes_on_snapshot_and_query(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.config_catalog_cache.is_none());
            view.handle_cli_config_read_response(
                serde_json::to_value(paged_snapshot("agent", "Model & Provider", 3)).unwrap(),
                cx,
            );
        });
        let cache = view.read(cx).config_catalog_cache.clone().expect("read populates the cache");
        assert_eq!(view.read(cx).config_catalog_epoch, 1);
        assert!(cache.is_fresh("", 1));
        assert_eq!(cache.total, 3);
        assert_eq!(cache.visible_count, 3);
        assert_eq!(cache.groups.len(), 1);

        view.update(cx, |view, cx| view.set_config_search_query("row_002", window, cx));
        let cache = view
            .read(cx)
            .config_catalog_cache
            .clone()
            .expect("query change refreshes the cache");
        assert!(cache.is_fresh("row_002", 1));
        assert_eq!(cache.visible_count, 1, "filtered cache keeps the surviving row only");
        assert_eq!(cache.groups.len(), 1);
    })
    .unwrap();
}

#[gpui_kit::test]
fn config_rail_labels_drop_counts_for_scannability(cx: &mut TestAppContext) {
    let (window, _view) = open_paged_config(cx, paged_snapshot("agent", "Model & Provider", 3), "Model & Provider");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("config-category-Model & Provider").label(), Some("Model & Provider"));
    })
    .unwrap();
}

#[test]
fn config_group_icons_cover_the_catalog_with_a_neutral_fallback() {
    use gpui_kit::assets::IconName;
    assert_eq!(config_group_icon("Configuration"), IconName::House);
    assert_eq!(config_group_icon("Model & Provider"), IconName::Bot);
    assert_eq!(config_group_icon("Agent & Automation"), IconName::RefreshCw);
    assert_eq!(config_group_icon("Approvals & Security"), IconName::ShieldCheck);
    assert_eq!(config_group_icon("Tools & Integrations"), IconName::Wrench);
    assert_eq!(config_group_icon("Context & Memory"), IconName::FileText);
    assert_eq!(config_group_icon("Interface & Terminal"), IconName::Terminal);
    assert_eq!(config_group_icon("Performance & Diagnostics"), IconName::Activity);
    assert_eq!(config_group_icon("Advanced settings"), IconName::Settings2);
    assert_eq!(config_group_icon("Tools"), IconName::Ellipsis);
    assert_eq!(config_group_icon("Some future group"), IconName::Ellipsis);
}

#[test]
fn catalog_cache_groups_filters_and_counts_in_catalog_order() {
    let setting = |path: &str, group: Option<&str>, editable: bool| {
        let mut setting = config_page_setting(path, json!(true), group.unwrap_or("Advanced settings"));
        setting.group = group.map(str::to_owned);
        setting.editable = editable;
        setting
    };
    let snapshot = vtcode_desktop_protocol::ConfigSettingsSnapshot {
        settings: vec![
            setting("b.second", Some("Zulu"), true),
            setting("a.first", Some("Alpha"), false),
            setting("c.ungrouped", None, true),
        ],
        target_versions: Vec::new(),
        omitted_sensitive: 0,
        omitted_large: 0,
        omitted_limit: 0,
    };
    let corpus = ConfigSearchCorpus::build(&snapshot, 7);
    let cache = build_config_catalog_cache(&snapshot, &corpus, "", 7);
    assert!(cache.is_fresh("", 7));
    assert!(!cache.is_fresh("", 8), "epoch mismatch is stale");
    assert!(!cache.is_fresh("x", 7), "query mismatch is stale");
    assert_eq!(cache.total, 3);
    assert_eq!(cache.editable_total, 2);
    assert_eq!(cache.visible_count, 3);
    let names: Vec<&str> = cache.groups.iter().map(|group| group.name.as_str()).collect();
    assert_eq!(names, ["Advanced settings", "Alpha", "Zulu"]);
    assert_eq!(cache.group("Alpha").expect("alpha group").indices, vec![1]);
    assert_eq!(cache.group("Advanced settings").expect("fallback group").indices, vec![2]);
    assert!(cache.group("Missing").is_none());

    let filtered = build_config_catalog_cache(&snapshot, &corpus, "first", 7);
    assert_eq!(filtered.total, 3, "totals ignore the filter");
    assert_eq!(filtered.visible_count, 1);
    assert_eq!(filtered.groups.len(), 1);
    assert_eq!(filtered.groups[0].name, "Alpha");
}

#[test]
fn search_corpus_tracks_epochs_and_matches_direct_filtering() {
    use super::super::config_search::{config_search_terms, setting_matches_query};

    let snapshot = vtcode_desktop_protocol::ConfigSettingsSnapshot {
        settings: vec![
            config_page_setting("agent.first", json!("one"), "Alpha"),
            config_page_setting("agent.second", json!("two"), "Zulu"),
            config_page_setting("agent.third", json!("three"), "Alpha"),
        ],
        target_versions: Vec::new(),
        omitted_sensitive: 0,
        omitted_large: 0,
        omitted_limit: 0,
    };
    let corpus = ConfigSearchCorpus::build(&snapshot, 7);
    assert!(corpus.is_fresh(&snapshot, 7));
    assert!(!corpus.is_fresh(&snapshot, 8), "epoch mismatch is stale");
    let mut grown = snapshot.clone();
    grown.settings.push(config_page_setting("agent.fourth", json!("four"), "Zulu"));
    assert!(!corpus.is_fresh(&grown, 7), "length mismatch is stale");

    // Corpus filtering agrees with direct matching on every query shape:
    // empty, hit, case-insensitive hit, multi-term conjunction, and miss.
    for query in ["", "first", "SECOND", "agent alpha", "zulu", "missing"] {
        let cache = build_config_catalog_cache(&snapshot, &corpus, query, 7);
        let terms = config_search_terms(query);
        let expected: Vec<usize> = snapshot
            .settings
            .iter()
            .enumerate()
            .filter(|(_, setting)| setting_matches_query(setting, &terms))
            .map(|(index, _)| index)
            .collect();
        let mut actual: Vec<usize> = cache.groups.iter().flat_map(|group| group.indices.iter().copied()).collect();
        actual.sort_unstable();
        assert_eq!(actual, expected, "query {query:?}");
        assert_eq!(cache.visible_count, expected.len(), "query {query:?}");
    }
}
