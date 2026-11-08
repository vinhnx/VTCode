use super::*;

/// Cloned render inputs for the connections pane (see
/// `NotificationSettingsProps` for the Kit-hosting rationale).
#[derive(Clone)]
pub(super) struct ConnectionsSettingsProps {
    pub(super) webmcp: webmcp::WebmcpSettingsProps,
    pub(super) terminal_command: String,
    pub(super) cli_terminal_select: Entity<preferences::CliTerminalSelectState>,
    pub(super) service_info: Option<protocol::InitializeResult>,
}

impl ConnectionsSettingsProps {
    /// Snapshot the view fields the connections builders read. Both the
    /// legacy shell wrapper and the Kit adapter build props here.
    pub(super) fn capture(view: &DesktopView) -> Self {
        Self {
            webmcp: webmcp::WebmcpSettingsProps::capture(view),
            terminal_command: DesktopView::terminal_command_for_session(view.selected_session_id.as_deref()),
            cli_terminal_select: view.cli_terminal_select.clone(),
            service_info: view.service_info.clone().filter(|_| view.backend.is_some()),
        }
    }
}

impl DesktopView {
    /// Continue a session from any directory as an independent fork; bounded
    /// archive IDs are safe to interpolate unquoted. A fork (not `--resume`)
    /// because the desktop service holds the session's exclusive lock while
    /// the thread is open, so an in-place resume would fail with `SessionBusy`.
    /// No `--all`: direct-id lookup is global, the scope only affects the
    /// latest/interactive pickers.
    pub(super) fn terminal_command_for_session(selected_session_id: Option<&str>) -> String {
        match selected_session_id {
            Some(id) => format!("vtcode {} {id}", files::CLI_FORK_SESSION_FLAG),
            None => "vtcode".to_string(),
        }
    }

    pub(super) fn set_cli_terminal(&mut self, terminal: preferences::CliTerminal, cx: &mut Context<Self>) {
        if self.preferences.cli_terminal() == terminal {
            return;
        }
        self.preferences.set_cli_terminal(terminal);
        self.save_preferences();
        cx.notify();
    }

    /// App-context content builder, hostable inside a Kit `SettingItem`
    /// closure.
    pub(super) fn render_connections_settings_content(
        props: ConnectionsSettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let service_info = props.service_info.as_ref();
        let service_connected = service_info.is_some();
        let service_status = if service_connected {
            Tag::success()
                .outline()
                .small()
                .child(rust_i18n::t!("app.settings.connections.connected").to_string())
        } else {
            Tag::warning()
                .outline()
                .small()
                .child(rust_i18n::t!("app.settings.connections.not_connected").to_string())
        };
        let service_status_label = if service_connected {
            rust_i18n::t!("app.settings.connections.active_status").to_string()
        } else {
            rust_i18n::t!("app.settings.connections.waiting_status").to_string()
        };
        let service_details = if let Some(info) = service_info {
            DescriptionList::horizontal()
                .columns(1)
                .label_width(px(128.))
                .bordered(false)
                .item(rust_i18n::t!("app.settings.connections.runtime").to_string(), app::NAME, 1)
                .item(rust_i18n::t!("app.settings.connections.version").to_string(), info.server_version.clone(), 1)
                .item(
                    rust_i18n::t!("app.settings.connections.protocol").to_string(),
                    format!("{}.{}", info.protocol.major, info.protocol.minor),
                    1,
                )
                .item(rust_i18n::t!("app.settings.connections.workspace").to_string(), info.workspace.clone(), 1)
                .into_any_element()
        } else {
            div()
                .id("local-service-unavailable")
                .test_support()
                .min_w_0()
                .child(row_description(
                    rust_i18n::t!("app.settings.connections.local_service_unavailable").to_string(),
                    &cx.theme().colors,
                ))
                .into_any_element()
        };

        div()
            .id("connections-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_6()
            .child(Self::render_webmcp_settings_content(false, &props.webmcp, view.clone(), cx))
            .child(Self::render_webmcp_settings_content(true, &props.webmcp, view.clone(), cx))
            .child({
                let group_title = rust_i18n::t!(i18n::SETTINGS_CONNECTIONS_TERMINAL).to_string();
                let command = props.terminal_command.clone();
                // English literals: the repo does not add locale keys without
                // full native-quality translations for every locale.
                let terminal_row_title = "Terminal application".to_string();
                accessible_group_box(
                    "terminal-settings-group",
                    group_title.clone(),
                    settings_group_box(group_title, &cx.theme().colors)
                        .child(settings_row(
                            rust_i18n::t!(i18n::SETTINGS_CONNECTIONS_TERMINAL).to_string(),
                            command.clone(),
                            desktop_button("copy-terminal-command")
                                .label(rust_i18n::t!(i18n::FILE_PREVIEW_COPY).to_string())
                                .ghost()
                                .small()
                                .on_click({
                                    move |_, window: &mut Window, cx: &mut App| {
                                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(command.clone()));
                                        super::super::notifications::copied_to_clipboard(window, cx);
                                    }
                                }),
                            &cx.theme().colors,
                        ))
                        .child(settings_row(
                            terminal_row_title.clone(),
                            "Terminal runs the handoff automatically; other terminals open with the command copied."
                                .to_string(),
                            Select::new(&props.cli_terminal_select)
                                .id("cli-terminal-select")
                                .accessibility_label(terminal_row_title)
                                .menu_max_h(px(320.))
                                .w(px(240.)),
                            &cx.theme().colors,
                        )),
                )
            })
            .child(accessible_group_box(
                "local-service-settings-group",
                rust_i18n::t!("app.settings.connections.local_service").to_string(),
                settings_group_box(
                    rust_i18n::t!("app.settings.connections.local_service").to_string(),
                    &cx.theme().colors,
                )
                .child(
                    div()
                        .id("local-service-details")
                        .test_support()
                        .role(gpui_kit::Role::Group)
                        .aria_label(rust_i18n::t!("app.settings.connections.local_service_details").to_string())
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .id("local-service-status")
                                .test_support()
                                .role(gpui_kit::Role::Status)
                                .aria_label(service_status_label)
                                .child(service_status),
                        )
                        .child(service_details),
                ),
            ))
            .into_any_element()
    }

    pub(super) fn render_connections_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        Self::render_connections_settings_content(ConnectionsSettingsProps::capture(self), cx.entity(), cx)
    }
}
