//! CLI configuration page, setting rows, and editor dialog renders.
use super::config_search::ConfigSearchCorpus;
use super::configuration::{
    CONFIG_GROUP_PAGE_SIZE, ConfigCatalogGroup, ConfigValueKind, build_config_catalog_cache, config_group_icon,
    config_target_label, value_summary,
};
use super::*;
use gpui_kit::component::select::Select;

impl DesktopView {
    pub(super) fn render_cli_config_page(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let query = self.config_search_query(cx);
        let searching = !query.is_empty();
        // The cache is fresh on every production path (refresh runs on each
        // snapshot/query change); the cold-start fallback keeps direct
        // snapshot swaps correct without a second grouping implementation.
        let catalog_fallback;
        let catalog = match self.config_catalog_cache.as_ref() {
            Some(cached) if cached.is_fresh(&query, self.config_catalog_epoch) => Some(cached),
            _ => {
                // Cold start only: production paths refresh the stored corpus
                // first, so this one-shot build never runs per keystroke.
                catalog_fallback = self.cli_config_settings.as_ref().map(|snapshot| {
                    let corpus = ConfigSearchCorpus::build(snapshot, self.config_catalog_epoch);
                    build_config_catalog_cache(snapshot, &corpus, &query, self.config_catalog_epoch)
                });
                catalog_fallback.as_ref()
            }
        };
        let groups: &[ConfigCatalogGroup] = catalog.map_or(&[], |catalog| catalog.groups.as_slice());
        let (total, editable_total, visible_count) =
            catalog.map_or((0, 0, 0), |catalog| (catalog.total, catalog.editable_total, catalog.visible_count));
        let selected = self
            .config_page_group
            .as_ref()
            .filter(|group| groups.iter().any(|cached| &cached.name == *group))
            .or_else(|| groups.first().map(|group| &group.name))
            .cloned();
        let counts = if searching {
            rust_i18n::t!(
                "app.configuration.match_summary",
                visible = visible_count,
                total = total,
                query = query.as_str()
            )
            .to_string()
        } else {
            rust_i18n::t!("app.configuration.editable_count", editable = editable_total, total = total).to_string()
        };

        let mut header = div()
            .id("cli-config-header")
            .test_support()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Input::new(&self.config_search_input)
                            .id("config-search")
                            .flex_1()
                            .min_w_0()
                            .small()
                            .aria_label(rust_i18n::t!("app.configuration.search_label").to_string())
                            .prefix(Icon::new(IconName::Search).size_4())
                            .cleanable(true),
                    )
                    .child(
                        desktop_button("browse-cli-config")
                            .label(rust_i18n::t!("app.configuration.browse_all").to_string())
                            .accessibility_label(rust_i18n::t!("app.configuration.browse_all_label").to_string())
                            .ghost()
                            .small()
                            .desktop_disabled(self.cli_config_settings.is_none() || !self.can_edit_cli_settings())
                            .on_click(cx.listener(|view, _, window, cx| view.open_cli_config_palette(window, cx))),
                    )
                    .child(
                        desktop_button("reload-cli-config")
                            .label(rust_i18n::t!("app.configuration.reload").to_string())
                            .ghost()
                            .small()
                            .tooltip_with_action(
                                rust_i18n::t!("app.configuration.reload_tooltip").to_string(),
                                &ReloadCliSettings,
                                Some(key_context::DESKTOP),
                            )
                            .desktop_disabled(!self.can_reload_cli_settings())
                            .dispatches_desktop_action(ReloadCliSettings, self.focus_handle.clone()),
                    ),
            );
        let targets = [
            (protocol::ConfigTarget::User, "config-target-user"),
            (protocol::ConfigTarget::Workspace, "config-target-workspace"),
            (protocol::ConfigTarget::Project, "config-target-project"),
        ]
        .into_iter()
        .fold(div().flex().flex_wrap().items_center().gap_2().flex_shrink_0(), |row, (target, id)| {
            let selected = self.config_target == target;
            let label = config_target_label(target);
            let accessibility = write_target_accessibility_label(&label, selected);
            row.child(
                desktop_button(id)
                    .label(label)
                    .accessibility_label(accessibility)
                    .desktop_selected(selected, &colors)
                    .small()
                    .desktop_disabled(!self.can_edit_cli_settings())
                    .on_click(cx.listener(move |view, _, _, cx| view.set_cli_config_target(target, cx))),
            )
        });
        header = header.child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(section_label(rust_i18n::t!("app.configuration.write_to").to_string(), &colors))
                .child(targets)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_right()
                        .child(row_description(counts, &colors)),
                ),
        );
        if let Some(notice) = self.config_write_notice.as_ref() {
            header = header.child(
                div()
                    .id("config-write-notice")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .text_sm()
                    .text_color(colors.foreground)
                    .child(notice.clone()),
            );
        }

        let curated_selected = !searching
            && self.cli_config_settings.is_some()
            && self
                .config_page_group
                .as_deref()
                .is_none_or(|group| group == curated::CURATED_GROUP_NAME);
        let body = if curated_selected {
            let rail = self.render_config_rail(groups, Some(curated::CURATED_GROUP_NAME), true, cx);
            let content = self.render_config_content_frame(self.render_curated_configuration(cx));
            div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .flex()
                .gap_4()
                .child(rail)
                .child(content)
                .into_any_element()
        } else {
            match (self.cli_config_settings.as_ref(), selected.as_ref()) {
                (None, _) if self.workspace.is_none() => self.render_config_content_frame(centered_empty_state(
                    rust_i18n::t!("app.configuration.empty_workspace_title").to_string(),
                    rust_i18n::t!("app.configuration.empty_workspace_description").to_string(),
                    IconName::Folder,
                )),
                (None, _) => self.render_config_content_frame(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .child(
                            div()
                                .text_sm()
                                .text_color(colors.muted_foreground)
                                .child(rust_i18n::t!("app.configuration.loading").to_string()),
                        )
                        .child(skeleton_line("config-loading-line-one", px(320.), cx))
                        .child(skeleton_line("config-loading-line-two", px(240.), cx)),
                ),
                (Some(_), None) => self.render_config_content_frame(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            centered_empty_state(
                                rust_i18n::t!("app.configuration.empty_search_title").to_string(),
                                rust_i18n::t!("app.configuration.empty_search_hint").to_string(),
                                IconName::Search,
                            )
                            .gap_3()
                            .child(
                                desktop_button("clear-config-search")
                                    .label(rust_i18n::t!("app.configuration.clear_search").to_string())
                                    .ghost()
                                    .small()
                                    .on_click(
                                        cx.listener(|view, _, window, cx| view.set_config_search_query("", window, cx)),
                                    ),
                            ),
                        )
                        .child(self.render_config_page_footer(cx)),
                ),
                (Some(snapshot), Some(name)) => {
                    let rail = self.render_config_rail(groups, selected.as_deref(), !searching, cx);
                    let catalog_group = catalog.and_then(|catalog| catalog.group(name));
                    let row_count = catalog_group.map_or(0, |group| group.indices.len());
                    let limit = self.config_visible_row_limit.max(1);
                    let mut list = div().flex().flex_col();
                    let mut shown = 0;
                    if let Some(catalog_group) = catalog_group {
                        for setting in catalog_group
                            .indices
                            .iter()
                            .filter_map(|index| snapshot.settings.get(*index))
                            .take(limit)
                        {
                            if shown > 0 {
                                list = list.child(row_divider(&colors));
                            }
                            shown += 1;
                            list = list.child(self.render_config_setting_row(setting, cx));
                        }
                    }
                    if row_count > shown {
                        list = list.child(
                            div().pt_2().child(
                                desktop_button("config-show-more")
                                    .label(
                                        rust_i18n::t!("app.configuration.show_more", remaining = row_count - shown)
                                            .to_string(),
                                    )
                                    .accessibility_label(
                                        rust_i18n::t!(
                                            "app.configuration.show_more_label",
                                            shown = shown,
                                            total = row_count
                                        )
                                        .to_string(),
                                    )
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.config_visible_row_limit =
                                            view.config_visible_row_limit.saturating_add(CONFIG_GROUP_PAGE_SIZE);
                                        cx.notify();
                                    })),
                            ),
                        );
                    }
                    let status = if row_count > shown {
                        rust_i18n::t!("app.configuration.showing_summary", shown = shown, count = row_count).to_string()
                    } else {
                        rust_i18n::t!("app.configuration.total_settings", count = row_count).to_string()
                    };
                    let mut group = div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(div().text_lg().font_weight(gpui_kit::FontWeight::SEMIBOLD).child(name.clone()));
                    if let Some(description) =
                        catalog_group.and_then(|catalog_group| catalog_group.description.as_ref())
                    {
                        group =
                            group.child(div().text_sm().text_color(colors.muted_foreground).child(description.clone()));
                    }
                    group = group.child(
                        div()
                            .id("config-group-status")
                            .test_support()
                            .child(row_description(status, &colors)),
                    );
                    group = group.child(list);
                    group = group.child(self.render_config_page_footer(cx));
                    let content = self.render_config_content_frame(group);
                    div()
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        .flex()
                        .gap_4()
                        .child(rail)
                        .child(content)
                        .into_any_element()
                }
            }
        };

        let panel_height = sheet_panel_height(window, cx);
        let mut panel = div()
            .id("cli-config-page")
            .test_support()
            .role(gpui_kit::Role::Dialog)
            .aria_label(rust_i18n::t!("app.configuration.title").to_string())
            .track_focus(&self.settings_focus)
            .relative()
            .w_full()
            .h(panel_height)
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(header)
            .child(body);
        if self.config_editor.is_some() {
            panel = panel.child(self.render_cli_config_editor(cx));
        }
        panel.into_any_element()
    }

    /// Group navigation rail shared by the curated overview and the catalog.
    /// The curated entry leads while the catalog is unfiltered. Entries show
    /// an icon plus the group name; counts live in the content header so the
    /// rail stays scannable.
    fn render_config_rail(
        &self,
        groups: &[ConfigCatalogGroup],
        selected: Option<&str>,
        show_curated_entry: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let mut rail = div()
            .id("config-rail")
            .lock_scroll_axis()
            .test_support()
            .role(gpui_kit::Role::Navigation)
            .aria_label(rust_i18n::t!("app.configuration.groups").to_string())
            .w_56()
            .flex_shrink_0()
            .min_h_0()
            .track_scroll(&self.config_rail_scroll)
            .overflow_y_scroll()
            .vertical_scrollbar(&self.config_rail_scroll)
            .flex()
            .flex_col()
            .gap_1();
        if show_curated_entry {
            let active = selected == Some(curated::CURATED_GROUP_NAME);
            rail = rail.child(
                desktop_button("config-category-curated")
                    .icon(config_group_icon(curated::CURATED_GROUP_NAME))
                    .label(rust_i18n::t!("app.configuration.page_title").to_string())
                    .small()
                    .w_full()
                    .justify_start()
                    .desktop_selected(active, &colors)
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.config_page_group = Some(curated::CURATED_GROUP_NAME.to_string());
                        view.config_visible_row_limit = CONFIG_GROUP_PAGE_SIZE;
                        cx.notify();
                    })),
            );
        }
        for group in groups {
            let name = group.name.clone();
            let active = selected == Some(name.as_str());
            rail = rail.child(
                desktop_button(format!("config-category-{name}"))
                    .icon(config_group_icon(&name))
                    .label(name.clone())
                    .small()
                    .w_full()
                    .justify_start()
                    .desktop_selected(active, &colors)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.config_page_group = Some(name.clone());
                        view.config_visible_row_limit = CONFIG_GROUP_PAGE_SIZE;
                        cx.notify();
                    })),
            );
        }
        rail.into_any_element()
    }

    /// Curated overview: the settings most users change, with the layer file
    /// one click away. The catalog groups stay the complete browser beneath.
    fn render_curated_configuration(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(self.render_curated_header(cx))
            .child(self.render_curated_overview_card(cx))
            .child(
                div()
                    .text_lg()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(rust_i18n::t!("app.configuration.model_features").to_string()),
            )
            .child(self.render_curated_efforts_card(cx))
            .child(self.render_config_page_footer(cx))
            .into_any_element()
    }

    fn render_curated_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let can_open = self.can_open_config_file();
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(rust_i18n::t!("app.configuration.page_title").to_string()),
                    )
                    .child(row_description(rust_i18n::t!("app.configuration.tagline").to_string(), &colors)),
            )
            .child(
                desktop_button("open-config-file")
                    .label(rust_i18n::t!("app.configuration.open_file").to_string())
                    .icon(IconName::ExternalLink)
                    .accessibility_label(rust_i18n::t!("app.configuration.open_file_label").to_string())
                    .ghost()
                    .small()
                    .desktop_disabled(!can_open)
                    .on_click(cx.listener(|view, _, _, cx| view.open_config_target_file(cx))),
            )
            .into_any_element()
    }

    fn render_curated_overview_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let mut card = GroupBox::new().flex_shrink_0().fill();
        for (index, row) in curated::curated_rows().iter().enumerate() {
            if index > 0 {
                card = card.child(row_divider(&colors));
            }
            card = card.child(self.render_curated_row(row, cx));
        }
        accessible_group_box(
            "curated-configuration-group",
            rust_i18n::t!("app.configuration.overview_group").to_string(),
            card,
        )
    }

    fn render_curated_row(&self, row: &curated::CuratedRow, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        match row.kind {
            curated::CuratedRowKind::PermissionPolicy => {
                let label = self.approval_policy_label();
                let control = desktop_button(format!("curated-{}", row.id))
                    .label(label.clone())
                    .accessibility_label(format!("{}: {label}", row.title))
                    .dropdown_caret(true)
                    .ghost()
                    .small()
                    .desktop_disabled(!self.can_select_session_options())
                    .on_click(cx.listener(|view, _, window, cx| view.open_permissions(window, cx)));
                settings_row(row.title.as_str(), row.description.as_str(), control, &colors).into_any_element()
            }
            curated::CuratedRowKind::ConfigValue => {
                let path = row.path.unwrap_or_default();
                let setting = curated::find_setting(self.cli_config_settings.as_ref(), path);
                let value = setting.map_or_else(
                    || rust_i18n::t!("app.configuration.unavailable").to_string(),
                    curated::setting_display_value,
                );
                let editable = setting.is_some_and(|setting| setting.editable) && self.can_edit_cli_settings();
                let path = path.to_string();
                let control = desktop_button(format!("curated-{}", row.id))
                    .label(value.clone())
                    .accessibility_label(format!("{}: {value}", row.title))
                    .dropdown_caret(true)
                    .ghost()
                    .small()
                    .desktop_disabled(!editable)
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.open_cli_config_editor(path.clone(), window, cx);
                    }));
                settings_row(row.title.as_str(), row.description.as_str(), control, &colors).into_any_element()
            }
            curated::CuratedRowKind::NetworkToggle => {
                let snapshot = self.cli_config_settings.as_ref();
                let enabled = curated::network_access_enabled(snapshot);
                let blocked = curated::network_policy_blocked(snapshot);
                let editable = curated::find_setting(snapshot, curated::NETWORK_POLICY_PATH)
                    .is_some_and(|setting| setting.editable);
                let available =
                    curated::network_toggle_available(snapshot) && self.can_edit_cli_settings() && editable && !blocked;
                let control = gpui_kit::component::switch::Switch::new(format!("curated-{}", row.id))
                    .small()
                    .checked(enabled)
                    .disabled(!available)
                    .accessibility_label(row.title.as_str())
                    .tooltip(if blocked {
                        rust_i18n::t!("app.configuration.blocked_in_catalog").to_string()
                    } else {
                        switch_tooltip(enabled)
                    })
                    .on_click(cx.listener(move |view, enabled, _, cx| {
                        view.set_network_access(*enabled, cx);
                    }));
                settings_row(row.title.as_str(), row.description.as_str(), control, &colors).into_any_element()
            }
            curated::CuratedRowKind::BoolToggle => {
                let path = row.path.unwrap_or_default();
                let setting = curated::find_setting(self.cli_config_settings.as_ref(), path);
                let enabled = setting.and_then(|setting| setting.value.as_bool()).unwrap_or(false);
                let available = setting.is_some_and(|setting| setting.editable) && self.can_edit_cli_settings();
                let path = path.to_string();
                let control = gpui_kit::component::switch::Switch::new(format!("curated-{}", row.id))
                    .small()
                    .checked(enabled)
                    .disabled(!available)
                    .accessibility_label(row.title.as_str())
                    .tooltip(switch_tooltip(enabled))
                    .on_click(cx.listener(move |view, enabled, _, cx| {
                        view.save_cli_config_value(path.clone(), Value::Bool(*enabled), cx);
                    }));
                settings_row(row.title.as_str(), row.description.as_str(), control, &colors).into_any_element()
            }
        }
    }

    fn render_curated_efforts_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let selected = self.preferences.available_efforts();
        let mut card = settings_group_box(rust_i18n::t!("app.configuration.available_efforts").to_string(), &colors)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(div().flex_1().min_w_0().child(row_description(
                        rust_i18n::t!("app.configuration.reasoning_effort_hint").to_string(),
                        &colors,
                    )))
                    .child(
                        Tag::secondary().outline().small().child(
                            rust_i18n::t!("app.configuration.selected_count", count = selected.len()).to_string(),
                        ),
                    ),
            );
        for level in vtcode_commons::reasoning::ReasoningEffortLevel::allowed_values() {
            let enabled = selected.iter().any(|entry| entry.as_str() == *level);
            let label = curated::humanize_config_value(level);
            let level = level.to_string();
            let switch = gpui_kit::component::switch::Switch::new(format!("curated-effort-{level}"))
                .small()
                .checked(enabled)
                .accessibility_label(
                    rust_i18n::t!("app.configuration.effort_label", label = label.as_str()).to_string(),
                )
                .tooltip(if enabled {
                    rust_i18n::t!("app.configuration.hide_from_models").to_string()
                } else {
                    rust_i18n::t!("app.configuration.show_in_models").to_string()
                })
                .on_click(cx.listener(move |view, enabled, _, cx| {
                    view.set_available_effort(level.as_str(), *enabled, cx);
                }));
            card = card.child(row_divider(&colors));
            card = card.child(settings_row(label, "", switch, &colors));
        }
        accessible_group_box(
            "curated-efforts-group",
            rust_i18n::t!("app.configuration.available_efforts").to_string(),
            card,
        )
    }

    /// Shared scroll frame for the configuration content column: one
    /// scrollable region keeps the header fixed while the catalog, empty
    /// states, and trailing notes share the remaining height.
    fn render_config_content_frame(&self, child: impl IntoElement) -> AnyElement {
        scroll_column_flex("cli-config-page-content", &self.settings_scroll, child).into_any_element()
    }

    /// One value-first setting row: title and live value on stable lanes,
    /// path and provenance beneath, description last. Boolean rows toggle
    /// in place; every other editable row opens the typed editor dialog.
    fn render_config_setting_row(&self, setting: &protocol::ConfigSetting, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let label = setting.label.as_deref().unwrap_or(&setting.path).to_owned();
        let can_edit = self.can_edit_cli_settings();
        let mut header = div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().min_w_0().truncate().child(row_title(label.clone(), &colors)));
        if setting.value.is_null() {
            header = header.child(
                div()
                    .flex_shrink_0()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child(rust_i18n::t!("app.configuration.unset").to_string()),
            );
        } else {
            header = header.child(
                div()
                    .flex_shrink_0()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_semibold()
                    .text_color(colors.foreground)
                    .child(value_summary(&setting.value)),
            );
        }
        if setting.value.is_boolean() && setting.editable {
            let path = setting.path.clone();
            let enabled = setting.value.as_bool().unwrap_or(false);
            header = header.child(
                gpui_kit::component::switch::Switch::new(format!("config-toggle-{path}"))
                    .small()
                    .checked(enabled)
                    .disabled(!can_edit)
                    .accessibility_label(label.clone())
                    .tooltip(switch_tooltip(enabled))
                    .on_click(cx.listener(move |view, enabled, _, cx| {
                        view.save_cli_config_value(path.clone(), Value::Bool(*enabled), cx);
                    })),
            );
        } else if setting.editable {
            let path = setting.path.clone();
            header = header.child(
                desktop_button(format!("config-edit-{path}"))
                    .label(rust_i18n::t!("app.configuration.edit").to_string())
                    .accessibility_label(
                        rust_i18n::t!("app.configuration.edit_target", target = setting.path.as_str()).to_string(),
                    )
                    .ghost()
                    .small()
                    .desktop_disabled(!can_edit)
                    .on_click(
                        cx.listener(move |view, _, window, cx| view.open_cli_config_editor(path.clone(), window, cx)),
                    ),
            );
        } else {
            header = header.child(
                Tag::secondary()
                    .outline()
                    .small()
                    .child(rust_i18n::t!("app.configuration.reference_only").to_string()),
            );
        }
        let mut meta = setting.path.clone();
        if let Some(origin) = setting.origin.as_deref() {
            meta.push_str(rust_i18n::t!("app.configuration.origin_hint", origin = origin).as_ref());
        }
        if let Some(default) = setting.default_value.as_deref() {
            meta.push_str(rust_i18n::t!("app.configuration.default_hint", value = default).as_ref());
        }
        let mut row = div()
            .flex()
            .flex_col()
            .gap_1()
            .py_2()
            .child(header)
            .child(div().text_xs().text_color(colors.muted_foreground).child(meta));
        if let Some(description) = setting.description.as_ref() {
            row = row.child(row_description(description.clone(), &colors));
        }
        row.into_any_element()
    }

    /// Writer, omission, and reset notes trailing the configuration list.
    /// Reset stays a terminal command: the desktop protocol exposes no
    /// destructive layer reset, so the page documents the CLI instead.
    fn render_config_page_footer(&self, cx: &Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let mut notes = vec![rust_i18n::t!("app.configuration.save_note").to_string()];
        if let Some(info) = self.service_info.as_ref() {
            notes.insert(
                0,
                rust_i18n::t!("app.configuration.current_version", version = info.server_version.as_str()).to_string(),
            );
        }
        if let Some(snapshot) = self.cli_config_settings.as_ref() {
            if snapshot.omitted_sensitive > 0 {
                notes.push(rust_i18n::t!("app.configuration.credential_note").to_string());
            }
            if snapshot.omitted_large > 0 || snapshot.omitted_limit > 0 {
                notes.push(rust_i18n::t!("app.configuration.omitted_large_note").to_string());
            }
            let read_only = snapshot.settings.iter().filter(|setting| !setting.editable).count();
            if read_only > 0 {
                notes.push(rust_i18n::t!("app.configuration.read_only_note", count = read_only).to_string());
            }
        }
        let mut footer = div()
            .id("cli-config-footer")
            .test_support()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(row_divider(&colors));
        for note in notes {
            footer = footer.child(div().text_xs().text_color(colors.muted_foreground).child(note));
        }
        footer = footer.child(
            div()
                .id("config-reset-hint")
                .test_support()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(rust_i18n::t!("app.configuration.reset_hint").to_string()),
        );
        footer.into_any_element()
    }

    pub(super) fn render_cli_configuration(&self, cx: &Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let snapshot = self.cli_config_settings.as_ref();
        let count = snapshot.map_or(0, |snapshot| snapshot.settings.len());
        let editable_count =
            snapshot.map_or(0, |snapshot| snapshot.settings.iter().filter(|setting| setting.editable).count());
        let mut group =
            settings_group_box(rust_i18n::t!("app.configuration.cli_configuration").to_string(), &cx.theme().colors);
        group = group.child(
            div().text_sm().child(match snapshot {
                Some(_) => {
                    rust_i18n::t!("app.configuration.editable_summary", editable = editable_count, count = count)
                        .to_string()
                }
                None if self.workspace.is_some() => rust_i18n::t!("app.configuration.loading").to_string(),
                None => rust_i18n::t!("app.configuration.choose_workspace").to_string(),
            }),
        );

        let target_buttons = [
            protocol::ConfigTarget::User,
            protocol::ConfigTarget::Workspace,
            protocol::ConfigTarget::Project,
        ]
        .into_iter()
        .fold(div().flex().flex_wrap().items_center().gap_2(), |row, target| {
            let selected = self.config_target == target;
            let label = config_target_label(target);
            let accessibility = write_target_accessibility_label(&label, selected);
            row.child(
                desktop_button(match target {
                    protocol::ConfigTarget::User => "config-target-user",
                    protocol::ConfigTarget::Workspace => "config-target-workspace",
                    protocol::ConfigTarget::Project => "config-target-project",
                })
                .label(label)
                .accessibility_label(accessibility)
                .desktop_selected(selected, &colors)
                .small()
                .desktop_disabled(!self.can_edit_cli_settings())
                .on_click(cx.listener(move |view, _, _, cx| view.set_cli_config_target(target, cx))),
            )
        });

        group = group.child(target_buttons).child(
            desktop_button("browse-cli-config")
                .label(rust_i18n::t!("app.configuration.browse_all").to_string())
                .accessibility_label(rust_i18n::t!("app.configuration.browse_all_label").to_string())
                .ghost()
                .desktop_disabled(snapshot.is_none() || !self.can_edit_cli_settings())
                .on_click(cx.listener(|view, _, window, cx| view.open_cli_config_palette(window, cx))),
        );

        let mut note = rust_i18n::t!("app.configuration.save_note").to_string();
        if let Some(snapshot) = snapshot {
            if snapshot.omitted_sensitive > 0 {
                note.push(' ');
                note.push_str(rust_i18n::t!("app.configuration.credential_note").as_ref());
            }
            if snapshot.omitted_large > 0 || snapshot.omitted_limit > 0 {
                note.push(' ');
                note.push_str(rust_i18n::t!("app.configuration.omitted_large_note").as_ref());
            }
            let read_only = snapshot.settings.iter().filter(|setting| !setting.editable).count();
            if read_only > 0 {
                note.push(' ');
                note.push_str(rust_i18n::t!("app.configuration.read_only_note", count = read_only).as_ref());
            }
        }
        group = group.footer(note);
        accessible_group_box(
            "cli-configuration-group",
            rust_i18n::t!("app.configuration.cli_configuration").to_string(),
            group,
        )
    }

    pub(super) fn render_cli_config_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(editor) = self.config_editor.as_ref() else {
            return div().into_any_element();
        };
        let colors = cx.theme().colors;
        let format_hint = match editor.kind {
            ConfigValueKind::Boolean => rust_i18n::t!("app.configuration.format_boolean").to_string(),
            ConfigValueKind::String => rust_i18n::t!("app.configuration.format_text").to_string(),
            ConfigValueKind::Integer => rust_i18n::t!("app.configuration.format_integer").to_string(),
            ConfigValueKind::Float => rust_i18n::t!("app.configuration.format_float").to_string(),
            ConfigValueKind::Array => rust_i18n::t!("app.configuration.format_array").to_string(),
            ConfigValueKind::Object => rust_i18n::t!("app.configuration.format_object").to_string(),
        };
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .bg(colors.background.opacity(0.86))
            .child(
                div()
                    .id("cli-config-editor")
                    .lock_scroll_axis()
                    .test_support()
                    .role(gpui_kit::Role::Dialog)
                    .aria_label(
                        rust_i18n::t!("app.configuration.edit_title_for", path = editor.path.as_str()).to_string(),
                    )
                    .w(px(620.))
                    .max_w_full()
                    .max_h_full()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_5()
                    .rounded_xl()
                    .bg(colors.secondary)
                    .shadow_lg()
                    .track_focus(&self.config_editor_focus)
                    .child(
                        DialogHeader::new()
                            .child(DialogTitle::new().child(rust_i18n::t!("app.configuration.edit_title").to_string()))
                            .child(DialogDescription::new().child(editor.path.clone())),
                    )
                    .child(div().text_xs().text_color(colors.muted_foreground).child(
                        if editor.option_select.is_some() {
                            rust_i18n::t!("app.configuration.choose_one_value").to_string()
                        } else {
                            format_hint
                        },
                    ))
                    .when_some(editor.option_select.as_ref(), |dialog, state| {
                        dialog.child(
                            Select::new(state)
                                .id("cli-config-option-select")
                                .placeholder(rust_i18n::t!("app.configuration.choose_value").to_string())
                                .accessibility_label(
                                    rust_i18n::t!("app.configuration.value_for", target = editor.path.as_str())
                                        .to_string(),
                                )
                                .menu_max_h(px(320.))
                                .w_full(),
                        )
                    })
                    .when(editor.option_select.is_none(), |dialog| {
                        dialog.child(
                            Textarea::new(&self.config_value_input)
                                .aria_label(
                                    rust_i18n::t!("app.configuration.value_for", target = editor.path.as_str())
                                        .to_string(),
                                )
                                .cursor_text()
                                .bg(colors.popover)
                                .rounded_md()
                                .px_3()
                                .py_2()
                                .w_full(),
                        )
                    })
                    .when_some(self.config_editor_error.as_ref(), |dialog, error| {
                        dialog.child(
                            div()
                                .id("cli-config-save-error")
                                .test_support()
                                .role(gpui_kit::Role::Alert)
                                .aria_label(error.clone())
                                .text_sm()
                                .text_color(colors.danger)
                                .child(error.clone()),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                desktop_button("cancel-cli-config-edit")
                                    .label(rust_i18n::t!("app.configuration.cancel").to_string())
                                    .ghost()
                                    .on_click(
                                        cx.listener(|view, _, window, cx| view.close_cli_config_editor(window, cx)),
                                    ),
                            )
                            .child(
                                desktop_button("save-cli-config-edit")
                                    .label(if self.config_write_pending {
                                        rust_i18n::t!("app.configuration.saving").to_string()
                                    } else {
                                        rust_i18n::t!("app.configuration.save_reload").to_string()
                                    })
                                    .primary()
                                    .desktop_disabled(self.config_write_pending || !self.can_edit_cli_settings())
                                    .on_click(
                                        cx.listener(|view, _, window, cx| view.submit_cli_config_edit(window, cx)),
                                    ),
                            ),
                    )
                    .focus_trap("cli-config-editor-trap", &self.config_editor_focus),
            )
            .into_any_element()
    }
}

fn write_target_accessibility_label(label: &str, selected: bool) -> String {
    let suffix = if selected {
        rust_i18n::t!("app.configuration.selected_suffix").to_string()
    } else {
        String::new()
    };
    rust_i18n::t!("app.configuration.write_target_label", label = label, suffix = suffix).to_string()
}

fn switch_tooltip(enabled: bool) -> String {
    if enabled {
        rust_i18n::t!("app.configuration.turn_off").to_string()
    } else {
        rust_i18n::t!("app.configuration.turn_on").to_string()
    }
}
