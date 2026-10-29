//! CLI configuration page, setting rows, and editor dialog renders.
use super::configuration::{
    ConfigValueKind, config_group_name, config_search_terms, setting_matches_query, value_summary,
};
use super::*;
use gpui_kit::component::select::Select;

impl DesktopView {
    pub(super) fn render_cli_config_page(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let query = self.config_search_query(cx);
        let terms = config_search_terms(&query);
        let mut groups = std::collections::BTreeMap::<String, Vec<&protocol::ConfigSetting>>::new();
        if let Some(snapshot) = self.cli_config_settings.as_ref() {
            for setting in &snapshot.settings {
                if setting_matches_query(setting, &terms) {
                    groups.entry(config_group_name(setting).to_owned()).or_default().push(setting);
                }
            }
        }
        let (total, editable_total) = self.cli_config_settings.as_ref().map_or((0, 0), |snapshot| {
            (snapshot.settings.len(), snapshot.settings.iter().filter(|setting| setting.editable).count())
        });
        let visible_count: usize = groups.values().map(Vec::len).sum();
        let selected = self
            .config_page_group
            .as_ref()
            .filter(|group| groups.contains_key(*group))
            .or_else(|| groups.keys().next())
            .cloned();
        let counts = if terms.is_empty() {
            format!("{editable_total} editable of {total} settings")
        } else {
            format!("{visible_count} of {total} match \"{query}\"")
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
                            .aria_label("Search configuration settings")
                            .prefix(Icon::new(IconName::Search).size_4())
                            .cleanable(true),
                    )
                    .child(
                        desktop_button("browse-cli-config")
                            .label("Browse all settings…")
                            .accessibility_label("Search and edit CLI configuration settings")
                            .ghost()
                            .small()
                            .desktop_disabled(self.cli_config_settings.is_none() || !self.can_edit_cli_settings())
                            .on_click(cx.listener(|view, _, window, cx| view.open_cli_config_palette(window, cx))),
                    )
                    .child(
                        desktop_button("reload-cli-config")
                            .label("Reload")
                            .ghost()
                            .small()
                            .tooltip_with_action("Reload CLI settings", &ReloadCliSettings, Some(key_context::DESKTOP))
                            .desktop_disabled(!self.can_reload_cli_settings())
                            .dispatches_desktop_action(ReloadCliSettings, self.focus_handle.clone()),
                    ),
            );
        let targets = [
            (protocol::ConfigTarget::User, "config-target-user", "User"),
            (protocol::ConfigTarget::Workspace, "config-target-workspace", "Workspace"),
            (protocol::ConfigTarget::Project, "config-target-project", "Project"),
        ]
        .into_iter()
        .fold(div().flex().flex_wrap().items_center().gap_2().flex_shrink_0(), |row, (target, id, label)| {
            let selected = self.config_target == target;
            row.child(
                desktop_button(id)
                    .label(label)
                    .accessibility_label(format!(
                        "Write CLI settings to {label} configuration{}",
                        if selected { " (selected)" } else { "" }
                    ))
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
                .child(section_label("Write to", &colors))
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

        let body = match (self.cli_config_settings.as_ref(), selected.as_ref()) {
            (None, _) if self.workspace.is_none() => self.render_config_content_frame(centered_empty_state(
                "Choose a workspace",
                "Choose a workspace to browse CLI configuration settings.",
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
                            .child("Loading shared CLI settings…"),
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
                            "No matching settings",
                            "Try a different search or browse another group.",
                            IconName::Search,
                        )
                        .gap_3()
                        .child(
                            desktop_button("clear-config-search")
                                .label("Clear search")
                                .ghost()
                                .small()
                                .on_click(
                                    cx.listener(|view, _, window, cx| view.set_config_search_query("", window, cx)),
                                ),
                        ),
                    )
                    .child(self.render_config_page_footer(cx)),
            ),
            (Some(_), Some(name)) => {
                let mut rail = div()
                    .id("config-rail")
                    .lock_scroll_axis()
                    .test_support()
                    .role(gpui_kit::Role::Navigation)
                    .aria_label("Configuration groups")
                    .w_56()
                    .flex_shrink_0()
                    .min_h_0()
                    .track_scroll(&self.config_rail_scroll)
                    .overflow_y_scroll()
                    .vertical_scrollbar(&self.config_rail_scroll)
                    .flex()
                    .flex_col()
                    .gap_1();
                for (group, settings) in &groups {
                    let group = group.clone();
                    let active = Some(&group) == selected.as_ref();
                    rail = rail.child(
                        desktop_button(format!("config-category-{group}"))
                            .label(format!("{group} ({})", settings.len()))
                            .small()
                            .w_full()
                            .justify_start()
                            .desktop_selected(active, &colors)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.config_page_group = Some(group.clone());
                                cx.notify();
                            })),
                    );
                }
                let mut list = div().flex().flex_col();
                if let Some(settings) = groups.get(name) {
                    for (index, setting) in settings.iter().enumerate() {
                        if index > 0 {
                            list = list.child(row_divider(&colors));
                        }
                        list = list.child(self.render_config_setting_row(setting, cx));
                    }
                }
                let description = groups
                    .get(name)
                    .and_then(|settings| settings.iter().find_map(|setting| setting.group_description.as_ref()));
                let mut group = div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_lg().font_weight(gpui_kit::FontWeight::SEMIBOLD).child(name.clone()));
                if let Some(description) = description {
                    group = group.child(div().text_sm().text_color(colors.muted_foreground).child(description.clone()));
                }
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
        };

        let panel_height = sheet_panel_height(window, cx);
        let mut panel = div()
            .id("cli-config-page")
            .test_support()
            .role(gpui_kit::Role::Dialog)
            .aria_label("VT Code configuration")
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
                    .child("<unset>".to_string()),
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
                    .tooltip(if enabled {
                        "Turn off".to_string()
                    } else {
                        "Turn on".to_string()
                    })
                    .on_click(cx.listener(move |view, enabled, _, cx| {
                        view.save_cli_config_value(path.clone(), Value::Bool(*enabled), cx);
                    })),
            );
        } else if setting.editable {
            let path = setting.path.clone();
            header = header.child(
                desktop_button(format!("config-edit-{path}"))
                    .label("Edit")
                    .accessibility_label(format!("Edit {}", setting.path))
                    .ghost()
                    .small()
                    .desktop_disabled(!can_edit)
                    .on_click(
                        cx.listener(move |view, _, window, cx| view.open_cli_config_editor(path.clone(), window, cx)),
                    ),
            );
        } else {
            header = header.child(Tag::secondary().outline().small().child("Reference only"));
        }
        let mut meta = setting.path.clone();
        if let Some(origin) = setting.origin.as_deref() {
            meta.push_str(&format!(" · from {origin}"));
        }
        if let Some(default) = setting.default_value.as_deref() {
            meta.push_str(&format!(" · default {default}"));
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
        let mut notes = vec![
            "Saving uses VT Code's validated configuration writer and reloads the local CLI service. Higher-precedence settings can still override the selected layer."
                .to_string(),
        ];
        if let Some(snapshot) = self.cli_config_settings.as_ref() {
            if snapshot.omitted_sensitive > 0 {
                notes.push(
                    "Credential-bearing fields are excluded; manage credentials with `vtcode auth` or the configured environment variable."
                        .to_string(),
                );
            }
            if snapshot.omitted_large > 0 || snapshot.omitted_limit > 0 {
                notes.push("Some unusually large or numerous settings are omitted from this browser.".to_string());
            }
            let read_only = snapshot.settings.iter().filter(|setting| !setting.editable).count();
            if read_only > 0 {
                notes.push(format!(
                    "{read_only} documented schema entries are reference-only until they have a concrete value."
                ));
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
                .child("Reset a layer from the terminal with `vtcode config reset` (`--global` for user, `--project` for project)."),
        );
        footer.into_any_element()
    }

    pub(super) fn render_cli_configuration(&self, cx: &Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let snapshot = self.cli_config_settings.as_ref();
        let count = snapshot.map_or(0, |snapshot| snapshot.settings.len());
        let editable_count =
            snapshot.map_or(0, |snapshot| snapshot.settings.iter().filter(|setting| setting.editable).count());
        let mut group = settings_group_box("CLI configuration", &cx.theme().colors);
        group = group.child(div().text_sm().child(match snapshot {
            Some(_) => format!("{editable_count} editable settings across {count} CLI config fields · values follow the active config layers"),
            None if self.workspace.is_some() => "Loading shared CLI settings…".into(),
            None => "Choose a workspace to browse CLI settings.".into(),
        }));

        let target_buttons = [
            (protocol::ConfigTarget::User, "User"),
            (protocol::ConfigTarget::Workspace, "Workspace"),
            (protocol::ConfigTarget::Project, "Project"),
        ]
        .into_iter()
        .fold(div().flex().flex_wrap().items_center().gap_2(), |row, (target, label)| {
            let selected = self.config_target == target;
            row.child(
                desktop_button(match target {
                    protocol::ConfigTarget::User => "config-target-user",
                    protocol::ConfigTarget::Workspace => "config-target-workspace",
                    protocol::ConfigTarget::Project => "config-target-project",
                })
                .label(label)
                .accessibility_label(format!(
                    "Write CLI settings to {label} configuration{}",
                    if selected { " (selected)" } else { "" }
                ))
                .desktop_selected(selected, &colors)
                .small()
                .desktop_disabled(!self.can_edit_cli_settings())
                .on_click(cx.listener(move |view, _, _, cx| view.set_cli_config_target(target, cx))),
            )
        });

        group = group.child(target_buttons).child(
            desktop_button("browse-cli-config")
                .label("Browse all settings…")
                .accessibility_label("Search and edit CLI configuration settings")
                .ghost()
                .desktop_disabled(snapshot.is_none() || !self.can_edit_cli_settings())
                .on_click(cx.listener(|view, _, window, cx| view.open_cli_config_palette(window, cx))),
        );

        let mut note = "Saving uses VT Code's validated configuration writer and reloads the local CLI service. Higher-precedence settings can still override the selected layer.".to_owned();
        if let Some(snapshot) = snapshot {
            if snapshot.omitted_sensitive > 0 {
                note.push_str(" Credential-bearing fields are excluded; manage credentials with `vtcode auth` or the configured environment variable.");
            }
            if snapshot.omitted_large > 0 || snapshot.omitted_limit > 0 {
                note.push_str(" Some unusually large or numerous settings are omitted from this browser.");
            }
            let read_only = snapshot.settings.iter().filter(|setting| !setting.editable).count();
            if read_only > 0 {
                note.push_str(&format!(
                    " {read_only} documented schema entries are reference-only until they have a concrete value."
                ));
            }
        }
        group = group.footer(note);
        accessible_group_box("cli-configuration-group", "CLI configuration", group)
    }

    pub(super) fn render_cli_config_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(editor) = self.config_editor.as_ref() else {
            return div().into_any_element();
        };
        let colors = cx.theme().colors;
        let format_hint = match editor.kind {
            ConfigValueKind::Boolean => "Boolean",
            ConfigValueKind::String => "Text",
            ConfigValueKind::Integer => "Whole number",
            ConfigValueKind::Float => "Decimal number",
            ConfigValueKind::Array => "JSON array",
            ConfigValueKind::Object => "JSON object",
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
                    .test_support()
                    .role(gpui_kit::Role::Dialog)
                    .aria_label(format!("Edit CLI setting {}", editor.path))
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
                            .child(DialogTitle::new().child("Edit CLI setting"))
                            .child(DialogDescription::new().child(editor.path.clone())),
                    )
                    .child(div().text_xs().text_color(colors.muted_foreground).child(
                        if editor.option_select.is_some() {
                            "Choose one of the documented values.".to_string()
                        } else {
                            format_hint.to_string()
                        },
                    ))
                    .when_some(editor.option_select.as_ref(), |dialog, state| {
                        dialog.child(
                            Select::new(state)
                                .id("cli-config-option-select")
                                .placeholder("Choose a documented value")
                                .accessibility_label(format!("Value for {}", editor.path))
                                .menu_max_h(px(320.))
                                .w_full(),
                        )
                    })
                    .when(editor.option_select.is_none(), |dialog| {
                        dialog.child(
                            Textarea::new(&self.config_value_input)
                                .aria_label(format!("Value for {}", editor.path))
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
                                desktop_button("cancel-cli-config-edit").label("Cancel").ghost().on_click(
                                    cx.listener(|view, _, window, cx| view.close_cli_config_editor(window, cx)),
                                ),
                            )
                            .child(
                                desktop_button("save-cli-config-edit")
                                    .label(if self.config_write_pending {
                                        "Saving…"
                                    } else {
                                        "Save and reload"
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
