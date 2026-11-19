use super::*;

const COMPUTER_USE_SETTINGS_GROUP_ID: &str = "computer-use-settings-group";
const COMPUTER_USE_ENABLED_PATH: &str = "computer_use.enabled";
const COMPUTER_USE_DECISIONS_PATH: &str = "computer_use.decisions_review";
const COMPUTER_USE_BROWSER_BUILD_PATH: &str = "computer_use.browser_build";

/// Cloned render inputs for the Computer Use tab. Values come from the
/// shared CLI config snapshot; writes go through `save_cli_config_value`.
#[derive(Clone)]
pub(super) struct ComputerUseSettingsProps {
    pub(super) available: bool,
    pub(super) can_edit: bool,
    pub(super) enabled: bool,
    pub(super) decisions_review: bool,
    pub(super) target: Option<String>,
    pub(super) image: Option<String>,
    pub(super) max_actions: Option<String>,
    pub(super) timeout_secs: Option<String>,
}

impl ComputerUseSettingsProps {
    pub(super) fn capture(view: &DesktopView) -> Self {
        let snapshot = view.cli_config_settings.as_deref();
        Self {
            available: snapshot.is_some(),
            can_edit: view.can_edit_cli_settings(),
            enabled: snapshot_bool(snapshot, COMPUTER_USE_ENABLED_PATH),
            decisions_review: snapshot_bool(snapshot, COMPUTER_USE_DECISIONS_PATH),
            target: snapshot_string(snapshot, COMPUTER_USE_BROWSER_BUILD_PATH),
            image: snapshot_string(snapshot, "computer_use.image"),
            max_actions: snapshot_number(snapshot, "computer_use.max_actions_per_turn"),
            timeout_secs: snapshot_number(snapshot, "computer_use.turn_timeout_secs"),
        }
    }
}

fn snapshot_bool(snapshot: Option<&protocol::ConfigSettingsSnapshot>, path: &str) -> bool {
    snapshot.is_some_and(|snapshot| {
        snapshot
            .settings
            .iter()
            .any(|setting| setting.path == path && setting.value.as_bool() == Some(true))
    })
}

fn snapshot_string(snapshot: Option<&protocol::ConfigSettingsSnapshot>, path: &str) -> Option<String> {
    snapshot.and_then(|snapshot| {
        snapshot
            .settings
            .iter()
            .find(|setting| setting.path == path)
            .and_then(|setting| setting.value.as_str().map(str::to_string))
    })
}

fn snapshot_number(snapshot: Option<&protocol::ConfigSettingsSnapshot>, path: &str) -> Option<String> {
    snapshot.and_then(|snapshot| {
        snapshot
            .settings
            .iter()
            .find(|setting| setting.path == path)
            .and_then(|setting| setting.value.as_u64().map(|value| value.to_string()))
    })
}

/// Group title row: the Computer glyph, the section label, and — while the
/// CLI snapshot is available — the master-switch state as a filled pill
/// sharing the composer's status idiom. `None` (service unreachable)
/// shows the label without state, since the switch position is unknown.
fn computer_use_group_title(status: Option<bool>, colors: &gpui_kit::component::theme::ThemeColor) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(Icon::new(IconName::Computer))
        .child(group_box_title(rust_i18n::t!("app.settings.computer_use.group").to_string(), colors))
        .when_some(status, |row, enabled| {
            let state = if enabled {
                rust_i18n::t!("app.settings.computer_use.status_on").to_string()
            } else {
                rust_i18n::t!("app.settings.computer_use.status_off").to_string()
            };
            row.child(status_pill(
                "computer-use-status",
                rust_i18n::t!("app.settings.computer_use.status_aria", state = state.clone()).to_string(),
                state,
                enabled,
                None,
                colors,
            ))
        })
}

/// User guide block: title plus the three facts a first-time user needs
/// (where the target lives, where turns run, what the requirements are).
/// Muted help-text styling shared with row descriptions; shown only while
/// the snapshot is available, since the steps assume a live service.
fn computer_use_guide(colors: &gpui_kit::component::theme::ThemeColor) -> impl IntoElement {
    let title = rust_i18n::t!("app.settings.computer_use.guide_title").to_string();
    div()
        .id("computer-use-guide")
        .test_support()
        .role(gpui_kit::Role::Group)
        .aria_label(title.clone())
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(themes::muted_text(colors))
                .child(title),
        )
        .child(row_description(rust_i18n::t!("app.settings.computer_use.guide_setup").to_string(), colors))
        .child(row_description(rust_i18n::t!("app.settings.computer_use.guide_run").to_string(), colors))
        .child(row_description(rust_i18n::t!("app.settings.computer_use.guide_safety").to_string(), colors))
}

impl DesktopView {
    /// Whether the shared config enables the computer-use runtime.
    pub(super) fn computer_use_enabled(&self) -> bool {
        snapshot_bool(self.cli_config_settings.as_deref(), COMPUTER_USE_ENABLED_PATH)
    }

    /// Flip the master switch through the shared CLI config service.
    pub(super) fn toggle_computer_use(&mut self, cx: &mut Context<Self>) {
        let enabled = !self.computer_use_enabled();
        self.set_computer_use_enabled(enabled, cx);
    }

    /// Write the master switch; the service confirmation refreshes the
    /// snapshot that render reads, so the Switch follows asynchronously.
    /// Unlike `/computer-use on`, this does not refuse a missing target:
    /// the tab shows "Not configured" beside the Switch, and turns still
    /// fail closed until a target is set.
    pub(super) fn set_computer_use_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.save_cli_config_value(COMPUTER_USE_ENABLED_PATH.to_string(), Value::Bool(enabled), cx);
    }

    pub(super) fn set_computer_use_decisions_review(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.save_cli_config_value(COMPUTER_USE_DECISIONS_PATH.to_string(), Value::Bool(enabled), cx);
    }

    pub(super) fn render_computer_use_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        Self::render_computer_use_settings_content(ComputerUseSettingsProps::capture(self), cx.entity(), cx)
    }

    /// App-context content builder, hostable inside a Kit `SettingItem`
    /// closure.
    pub(super) fn render_computer_use_settings_content(
        props: ComputerUseSettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let theme = cx.theme();
        if !props.available {
            return div()
                .id("computer-use-settings")
                .test_support()
                .child(accessible_group_box(
                    COMPUTER_USE_SETTINGS_GROUP_ID,
                    rust_i18n::t!("app.settings.computer_use.group").to_string(),
                    settings_group_box_untitled(&theme.colors)
                        .title(computer_use_group_title(None, &theme.colors))
                        .child(
                            div()
                                .text_color(theme.colors.muted_foreground)
                                .child(rust_i18n::t!("app.settings.computer_use.unavailable").to_string()),
                        ),
                ))
                .into_any_element();
        }
        let title = rust_i18n::t!("app.settings.computer_use.enable").to_string();
        let group = settings_group_box_untitled(&theme.colors)
            .title(computer_use_group_title(Some(props.enabled), &theme.colors))
            .child(settings_row(
                title.clone(),
                rust_i18n::t!("app.settings.computer_use.enable_description").to_string(),
                gpui_kit::component::switch::Switch::new("computer-use-enabled")
                    .checked(props.enabled)
                    .disabled(!props.can_edit)
                    .accessibility_label(title)
                    .on_click({
                        let view = view.clone();
                        move |checked: &bool, _: &mut Window, cx: &mut App| {
                            view.update(cx, |view, cx| view.set_computer_use_enabled(*checked, cx));
                        }
                    }),
                &theme.colors,
            ))
            .child(settings_row(
                rust_i18n::t!("app.settings.computer_use.decisions").to_string(),
                rust_i18n::t!("app.settings.computer_use.decisions_description").to_string(),
                gpui_kit::component::switch::Switch::new("computer-use-decisions")
                    .checked(props.decisions_review)
                    .disabled(!props.can_edit || !props.enabled)
                    .accessibility_label(rust_i18n::t!("app.settings.computer_use.decisions").to_string())
                    .on_click({
                        let view = view.clone();
                        move |checked: &bool, _: &mut Window, cx: &mut App| {
                            view.update(cx, |view, cx| view.set_computer_use_decisions_review(*checked, cx));
                        }
                    }),
                &theme.colors,
            ))
            .child(
                div()
                    .id("computer-use-details")
                    .test_support()
                    .role(gpui_kit::Role::Group)
                    .aria_label(rust_i18n::t!("app.settings.computer_use.group").to_string())
                    .child(
                        DescriptionList::horizontal()
                            .columns(1)
                            .label_width(px(128.))
                            .bordered(false)
                            .item(
                                rust_i18n::t!("app.settings.computer_use.target").to_string(),
                                props.target.unwrap_or_else(|| {
                                    rust_i18n::t!("app.settings.computer_use.not_configured").to_string()
                                }),
                                1,
                            )
                            .item(
                                rust_i18n::t!("app.settings.computer_use.image").to_string(),
                                props.image.unwrap_or_else(|| {
                                    rust_i18n::t!("app.settings.computer_use.not_configured").to_string()
                                }),
                                1,
                            )
                            .item(
                                rust_i18n::t!("app.settings.computer_use.limits").to_string(),
                                format!(
                                    "{} actions · {}s",
                                    props.max_actions.as_deref().unwrap_or("?"),
                                    props.timeout_secs.as_deref().unwrap_or("?")
                                ),
                                1,
                            ),
                    ),
            )
            .child(
                div().flex().flex_wrap().items_center().gap_2().child(
                    desktop_ghost_button("computer-use-open-config")
                        .icon(IconName::Computer)
                        .label(rust_i18n::t!("app.settings.computer_use.configure").to_string())
                        .on_click(move |_, window: &mut Window, cx: &mut App| {
                            view.update(cx, |view, cx| {
                                view.open_config_link(Some(COMPUTER_USE_BROWSER_BUILD_PATH.to_string()), window, cx)
                            });
                        }),
                ),
            )
            .child(computer_use_guide(&theme.colors));
        div()
            .id("computer-use-settings")
            .test_support()
            .child(accessible_group_box(
                COMPUTER_USE_SETTINGS_GROUP_ID,
                rust_i18n::t!("app.settings.computer_use.group").to_string(),
                group,
            ))
            .into_any_element()
    }
}
