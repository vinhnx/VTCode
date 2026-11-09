//! Curated configuration overview: header, value rows with tier markers, effort switches.

use super::*;

/// Cloned render inputs for the curated overview (see
/// `NotificationSettingsProps` for the Kit-hosting rationale). The
/// settings snapshot stays in the page props and is borrowed by the
/// row builders.
#[derive(Clone)]
pub(super) struct CuratedProps {
    pub(super) can_open: bool,
    pub(super) approval_label: String,
    pub(super) can_select_session: bool,
    pub(super) can_edit: bool,
    pub(super) efforts: Vec<String>,
}

impl CuratedProps {
    /// Snapshot the view fields the curated overview reads. Both the
    /// legacy shell wrapper and the Kit adapter build props here.
    pub(super) fn capture(view: &DesktopView) -> Self {
        Self {
            can_open: view.can_open_config_file(),
            approval_label: view.approval_policy_label(),
            can_select_session: view.can_select_session_options(),
            can_edit: view.can_edit_cli_settings(),
            efforts: view.preferences.available_efforts(),
        }
    }
}

impl DesktopView {
    /// Curated overview: the settings most users change, with the layer file
    /// one click away. The catalog groups stay the complete browser beneath.
    pub(super) fn render_curated_configuration_content(
        props: &ConfigPageProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let snapshot = props.snapshot.as_deref();
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(Self::render_curated_header_content(&props.curated, view.clone(), &colors))
            .child(Self::render_curated_overview_card_content(snapshot, &props.curated, view.clone(), &colors))
            .child(
                div()
                    .text_lg()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(rust_i18n::t!("app.configuration.model_features").to_string()),
            )
            .child(Self::render_curated_efforts_card_content(&props.curated, view.clone(), &colors))
            .child(Self::render_config_page_footer_content(&props.cli, props.server_version.as_deref(), &colors))
            .into_any_element()
    }

    fn render_curated_header_content(
        curated: &CuratedProps,
        view: Entity<Self>,
        colors: &gpui_kit::component::theme::ThemeColor,
    ) -> AnyElement {
        let can_open = curated.can_open;
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
                    .child(row_description(rust_i18n::t!("app.configuration.tagline").to_string(), colors)),
            )
            .child(
                desktop_button("open-config-file")
                    .label(rust_i18n::t!("app.configuration.open_file").to_string())
                    .icon(IconName::ExternalLink)
                    .accessibility_label(rust_i18n::t!("app.configuration.open_file_label").to_string())
                    .ghost()
                    .small()
                    .desktop_disabled(!can_open)
                    .on_click({
                        let view = view.clone();
                        move |_, _: &mut Window, cx: &mut App| {
                            view.update(cx, |view, cx| view.open_config_target_file(cx));
                        }
                    }),
            )
            .into_any_element()
    }

    fn render_curated_overview_card_content(
        snapshot: Option<&protocol::ConfigSettingsSnapshot>,
        curated: &CuratedProps,
        view: Entity<Self>,
        colors: &gpui_kit::component::theme::ThemeColor,
    ) -> AnyElement {
        let mut card = settings_group_box_untitled(colors);
        for row in curated::curated_rows().iter() {
            card = card.child(Self::render_curated_row_content(row, snapshot, curated, view.clone(), colors));
        }
        accessible_group_box(
            "curated-configuration-group",
            rust_i18n::t!("app.configuration.overview_group").to_string(),
            card,
        )
    }

    fn render_curated_row_content(
        row: &curated::CuratedRow,
        snapshot: Option<&protocol::ConfigSettingsSnapshot>,
        curated: &CuratedProps,
        view: Entity<Self>,
        colors: &gpui_kit::component::theme::ThemeColor,
    ) -> AnyElement {
        match row.kind {
            curated::CuratedRowKind::PermissionPolicy => {
                let label = curated.approval_label.clone();
                let control = desktop_button(format!("curated-{}", row.id))
                    .label(label.clone())
                    .accessibility_label(format!("{}: {label}", row.title))
                    .dropdown_caret(true)
                    .ghost()
                    .small()
                    .desktop_disabled(!curated.can_select_session)
                    .on_click({
                        let view = view.clone();
                        move |_, window: &mut Window, cx: &mut App| {
                            view.update(cx, |view, cx| view.open_permissions(window, cx));
                        }
                    });
                settings_row(row.title.as_str(), row.description.as_str(), control, colors).into_any_element()
            }
            curated::CuratedRowKind::ConfigValue => {
                let path = row.path.unwrap_or_default();
                let setting = curated::find_setting(snapshot, path);
                let value = setting.map_or_else(
                    || rust_i18n::t!(i18n::CONFIGURATION_UNAVAILABLE).to_string(),
                    curated::setting_display_value,
                );
                let editable = setting.is_some_and(|setting| setting.editable) && curated.can_edit;
                let overridden = setting.is_some_and(|setting| {
                    configuration::config_value_state(setting) == configuration::ConfigValueState::Overridden
                });
                let path = path.to_string();
                let control = desktop_button(format!("curated-{}", row.id))
                    .label(value.clone())
                    .accessibility_label(format!("{}: {value}", row.title))
                    .dropdown_caret(true)
                    .ghost()
                    .small()
                    .when(overridden, |control| control.font_semibold())
                    .desktop_disabled(!editable)
                    .on_click({
                        let view = view.clone();
                        move |_, window: &mut Window, cx: &mut App| {
                            view.update(cx, |view, cx| view.open_cli_config_editor(path.clone(), window, cx));
                        }
                    });
                let content = settings_row(row.title.as_str(), row.description.as_str(), control, colors);
                match setting.map(configuration::config_setting_meta) {
                    Some(meta) => curated_value_row(row.id, content, meta, colors),
                    None => content.into_any_element(),
                }
            }
            curated::CuratedRowKind::NetworkToggle => {
                let enabled = curated::network_access_enabled(snapshot);
                let blocked = curated::network_policy_blocked(snapshot);
                let editable = curated::find_setting(snapshot, curated::NETWORK_POLICY_PATH)
                    .is_some_and(|setting| setting.editable);
                let available = curated::network_toggle_available(snapshot) && curated.can_edit && editable && !blocked;
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
                    .on_click({
                        let view = view.clone();
                        move |enabled: &bool, _: &mut Window, cx: &mut App| {
                            view.update(cx, |view, cx| view.set_network_access(*enabled, cx));
                        }
                    });
                settings_row(row.title.as_str(), row.description.as_str(), control, colors).into_any_element()
            }
            curated::CuratedRowKind::BoolToggle => {
                let path = row.path.unwrap_or_default();
                let setting = curated::find_setting(snapshot, path);
                let enabled = setting.and_then(|setting| setting.value.as_bool()).unwrap_or(false);
                let available = setting.is_some_and(|setting| setting.editable) && curated.can_edit;
                let path = path.to_string();
                let control = gpui_kit::component::switch::Switch::new(format!("curated-{}", row.id))
                    .small()
                    .checked(enabled)
                    .disabled(!available)
                    .accessibility_label(row.title.as_str())
                    .tooltip(switch_tooltip(enabled))
                    .on_click({
                        let view = view.clone();
                        move |enabled: &bool, _: &mut Window, cx: &mut App| {
                            view.update(cx, |view, cx| {
                                view.save_cli_config_value(path.clone(), Value::Bool(*enabled), cx);
                            });
                        }
                    });
                let content = settings_row(row.title.as_str(), row.description.as_str(), control, colors);
                match setting.map(configuration::config_setting_meta) {
                    Some(meta) => curated_value_row(row.id, content, meta, colors),
                    None => content.into_any_element(),
                }
            }
        }
    }

    fn render_curated_efforts_card_content(
        curated: &CuratedProps,
        view: Entity<Self>,
        colors: &gpui_kit::component::theme::ThemeColor,
    ) -> AnyElement {
        let selected = &curated.efforts;
        let mut card = settings_group_box(rust_i18n::t!("app.configuration.available_efforts").to_string(), colors)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(div().flex_1().min_w_0().child(row_description(
                        rust_i18n::t!("app.configuration.reasoning_effort_hint").to_string(),
                        colors,
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
                .on_click({
                    let view = view.clone();
                    move |enabled: &bool, _: &mut Window, cx: &mut App| {
                        view.update(cx, |view, cx| view.set_available_effort(level.as_str(), *enabled, cx));
                    }
                });
            card = card.child(settings_row(label, "", switch, colors));
        }
        accessible_group_box(
            "curated-efforts-group",
            rust_i18n::t!("app.configuration.available_efforts").to_string(),
            card,
        )
    }
}

/// One curated value row with its tier markers: the settings row plus the
/// same path/origin/default line the catalog renders for the setting.
fn curated_value_row(
    id: &str,
    content: impl IntoElement,
    meta: String,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(content)
        .child(
            div()
                .id(format!("curated-meta-{id}"))
                .test_support()
                .role(gpui_kit::Role::Label)
                .aria_label(meta.clone())
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(meta),
        )
        .into_any_element()
}
