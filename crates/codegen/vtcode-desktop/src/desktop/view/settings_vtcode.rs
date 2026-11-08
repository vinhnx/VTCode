use super::*;

/// Provider group id shared by the empty and loaded states so the group
/// keeps stable identity across configuration changes.
const PROVIDER_SETTINGS_GROUP_ID: &str = "provider-settings-group";

/// Cloned render inputs for the VtCode tab (see `NotificationSettingsProps`
/// for the Kit-hosting rationale). Subsystem groups nest their own props;
/// single-value groups stay flat.
#[derive(Clone)]
pub(super) struct VtcodeSettingsProps {
    pub(super) configured: bool,
    pub(super) provider: String,
    pub(super) model: String,
    pub(super) secret_store: Option<protocol::SecretStoreInfo>,
    pub(super) credential_setup_command: String,
    pub(super) shared: shared_preferences::SharedPreferencesProps,
    pub(super) cli: configuration::CliConfigurationProps,
    pub(super) show_in_menu_bar: bool,
    pub(super) launch_at_login_enabled: bool,
    pub(super) secret: secret_store::SecretStoreProps,
}

impl VtcodeSettingsProps {
    /// Snapshot the view fields the VtCode builders read. Both the legacy
    /// shell wrapper and the Kit adapter build props here.
    pub(super) fn capture(view: &DesktopView) -> Self {
        Self {
            configured: view.configuration.is_some(),
            provider: view
                .configuration
                .as_ref()
                .map_or_else(String::new, |configuration| configuration.provider.clone()),
            model: view
                .configuration
                .as_ref()
                .map_or_else(String::new, |configuration| configuration.model.clone()),
            secret_store: view.secret_store_info().cloned(),
            credential_setup_command: view
                .configuration
                .as_ref()
                .map_or_else(String::new, |configuration| configuration.credential_setup_command.clone()),
            shared: shared_preferences::SharedPreferencesProps::capture(view),
            cli: configuration::CliConfigurationProps::capture(view),
            show_in_menu_bar: view.preferences.show_in_menu_bar,
            launch_at_login_enabled: view.preferences.launch_at_login_enabled(),
            secret: secret_store::SecretStoreProps::capture(view),
        }
    }
}

impl DesktopView {
    /// App-context content builder, hostable inside a Kit `SettingItem`
    /// closure.
    pub(super) fn render_vtcode_settings_content(
        props: VtcodeSettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let content = div()
            .id("vtcode-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_6()
            .child(Self::render_provider_settings_content(&props, view.clone(), cx))
            .child(Self::render_shared_preferences_content(&props.shared, view.clone(), cx))
            .child(Self::render_cli_configuration_content(&props.cli, view.clone(), cx))
            .child(Self::render_licenses_settings_content(view.clone(), cx))
            .child(Self::render_menu_bar_settings_content(props.show_in_menu_bar, view.clone(), cx))
            .child(Self::render_login_items_settings_content(props.launch_at_login_enabled, view, cx));
        content.into_any_element()
    }

    pub(super) fn render_vtcode_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        Self::render_vtcode_settings_content(VtcodeSettingsProps::capture(self), cx.entity(), cx)
    }

    fn render_provider_settings_content(props: &VtcodeSettingsProps, view: Entity<Self>, cx: &mut App) -> AnyElement {
        let theme = cx.theme();
        if !props.configured {
            return accessible_group_box(
                PROVIDER_SETTINGS_GROUP_ID,
                rust_i18n::t!("app.settings.provider.group").to_string(),
                settings_group_box(rust_i18n::t!("app.settings.provider.title").to_string(), &theme.colors).child(
                    div()
                        .text_color(theme.colors.muted_foreground)
                        .child(rust_i18n::t!("app.settings.provider.connect_hint").to_string()),
                ),
            );
        };

        let provider = if props.provider.is_empty() {
            rust_i18n::t!("app.settings.provider.not_configured").to_string()
        } else {
            props.provider.clone()
        };
        let mut group = settings_group_box(rust_i18n::t!("app.settings.provider.title").to_string(), &theme.colors)
            .child(
                div()
                    .id("provider-details")
                    .test_support()
                    .role(gpui_kit::Role::Group)
                    .aria_label(rust_i18n::t!("app.settings.provider.accessibility_label").to_string())
                    .child(
                        DescriptionList::horizontal()
                            .columns(1)
                            .label_width(px(128.))
                            .bordered(false)
                            .item(rust_i18n::t!("app.settings.provider.provider").to_string(), provider, 1)
                            .item(rust_i18n::t!("app.settings.provider.model").to_string(), props.model.clone(), 1),
                    ),
            );
        // Owned copies for the Terminal fallback row so the secret form can
        // borrow `cx` mutably below.
        let mono_font_family = theme.mono_font_family.clone();
        let muted_foreground = theme.colors.muted_foreground;
        if let Some(secret) = props.secret_store.as_ref() {
            group = group.child(Self::render_secret_store_form_content(secret, &props.secret, view, cx));
        }
        if props.secret_store.is_none() && !props.credential_setup_command.is_empty() {
            let command = props.credential_setup_command.clone();
            group = group.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .font_family(mono_font_family)
                            .text_color(muted_foreground)
                            .truncate()
                            .child(command.clone()),
                    )
                    .child(
                        desktop_button("copy-credential-command")
                            .label(rust_i18n::t!("gpui_component.Input.Copy").to_string())
                            .ghost()
                            .on_click(move |_, window, app| {
                                app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(command.to_string()));
                                super::super::notifications::copied_to_clipboard(window, app);
                            }),
                    ),
            );
        }
        accessible_group_box(
            PROVIDER_SETTINGS_GROUP_ID,
            rust_i18n::t!("app.settings.provider.group").to_string(),
            group,
        )
    }
}
