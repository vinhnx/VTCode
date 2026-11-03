use super::*;

impl DesktopView {
    /// Continue a session from any directory; bounded archive IDs are safe to
    /// interpolate unquoted.
    pub(super) fn terminal_command_for_session(selected_session_id: Option<&str>) -> String {
        match selected_session_id {
            Some(id) => format!("vtcode --resume {id} --all"),
            None => "vtcode".to_string(),
        }
    }

    pub(super) fn render_connections_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let service_info = self.service_info.as_ref().filter(|_| self.backend.is_some());
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
            .child(self.render_webmcp_settings(false, cx))
            .child(self.render_webmcp_settings(true, cx))
            .child({
                let group_title = rust_i18n::t!(i18n::SETTINGS_CONNECTIONS_TERMINAL).to_string();
                let command = Self::terminal_command_for_session(self.selected_session_id.as_deref());
                accessible_group_box(
                    "terminal-settings-group",
                    group_title.clone(),
                    settings_group_box(group_title, &cx.theme().colors).child(settings_row(
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
}
