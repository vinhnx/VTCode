use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, StatefulInteractiveElement, TestSupportExt, Window, div, px};
use protocol::slash_command::{
    WEBMCP_ROOTS_HELP, WEBMCP_TOOLS_HELP, WebmcpSlashCommand, WebmcpSlashCommandParseError, parse_webmcp_slash_command,
};
use serde_json::{Value, json};
use vtcode_desktop_protocol as protocol;

use super::{DesktopButtonExt, DesktopView, desktop_button};

const WEBMCP_APP_URL: &str = "https://vtcode.vinhnx.chatgpt.site/";
const WEBMCP_COMMANDS_HELP: &str =
    "Commands: /webmcp status | help | tools | roots | connect <origin> | pair <origin> | unpair | stop.";
pub(super) const WEBMCP_DISCONNECTED_NOTICE: &str =
    "The local VT Code service disconnected. Reconnect to refresh WebMCP status.";

#[derive(Debug, PartialEq, Eq)]
enum WebmcpCommand {
    Help,
    Status,
    Pair { origin: String, replace: bool },
    Stop,
    Tools,
    Roots,
}

fn parse_command(input: &str) -> Option<Result<WebmcpCommand, &'static str>> {
    let input = input.trim();
    let rest = input.strip_prefix("/webmcp")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let command = parse_webmcp_slash_command(rest.trim()).map_err(|error| match error {
        WebmcpSlashCommandParseError::UnknownCommand => WEBMCP_COMMANDS_HELP,
        WebmcpSlashCommandParseError::InvalidConnectionArguments => {
            "Usage: /webmcp connect [--replace] <exact-browser-origin>"
        }
    });
    Some(command.and_then(|command| match command {
        WebmcpSlashCommand::Status => Ok(WebmcpCommand::Status),
        WebmcpSlashCommand::Help => Ok(WebmcpCommand::Help),
        WebmcpSlashCommand::Tools => Ok(WebmcpCommand::Tools),
        WebmcpSlashCommand::Roots => Ok(WebmcpCommand::Roots),
        WebmcpSlashCommand::ConnectHelp => Err("Usage: /webmcp connect [--replace] <exact-browser-origin>"),
        WebmcpSlashCommand::Connect { origin, replace } => Ok(WebmcpCommand::Pair { origin, replace }),
        WebmcpSlashCommand::Unpair => Ok(WebmcpCommand::Stop),
    }))
}

impl DesktopView {
    pub(super) fn reset_webmcp_state(&mut self, notice: Option<&str>) {
        self.webmcp_status = None;
        self.webmcp_pairing = None;
        self.webmcp_request_pending = None;
        self.webmcp_status_dirty = false;
        self.webmcp_notice = notice.map(str::to_owned);
    }

    fn open_webmcp_settings_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_tab = super::settings::SettingsTab::Connections;
        self.open_settings(window, cx);
        cx.notify();
    }

    pub(super) fn open_webmcp_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_webmcp_settings_panel(window, cx);
        self.request_webmcp_status();
    }

    pub(super) fn maybe_handle_webmcp_command(
        &mut self,
        input: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(command) = parse_command(input) else {
            return false;
        };
        let command = match command {
            Ok(command) => command,
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
                return true;
            }
        };
        if !self.attachments.is_empty() {
            self.error = Some(
                "Run /webmcp commands without draft attachments; the attachments are still in the composer.".into(),
            );
            cx.notify();
            return true;
        }
        self.composer.update(cx, |input, cx| input.set_value("", window, cx));
        self.error = None;
        self.open_webmcp_settings_panel(window, cx);
        match command {
            WebmcpCommand::Help => {
                self.webmcp_notice = Some(WEBMCP_COMMANDS_HELP.into());
                self.request_webmcp_status();
            }
            WebmcpCommand::Status => self.request_webmcp_status(),
            WebmcpCommand::Pair { origin, replace } => {
                self.webmcp_origin_input
                    .update(cx, |input, cx| input.set_value(origin.clone(), window, cx));
                self.request_webmcp_pair(origin, replace);
            }
            WebmcpCommand::Tools => {
                self.webmcp_notice = Some(WEBMCP_TOOLS_HELP.into());
            }
            WebmcpCommand::Roots => {
                self.webmcp_notice = Some(WEBMCP_ROOTS_HELP.into());
            }
            WebmcpCommand::Stop => self.request_webmcp_stop(),
        }
        cx.notify();
        true
    }

    /// The WebMCP connection group.
    ///
    /// `origin_controls` draws the origin list, pairing input, and pairing
    /// actions after the browser section; `false` yields the connection status
    /// alone (the origin controls depend on browser output).
    pub(super) fn render_webmcp_settings(&self, origin_controls: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let status = self.webmcp_status.as_ref();
        let busy = self.webmcp_request_pending.is_some();
        let configured_origins = status.map(|status| status.allowed_origins.as_slice()).unwrap_or_default();
        let mut section = div()
            .id(if origin_controls {
                "webmcp-pairing"
            } else {
                "webmcp-status"
            })
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(if origin_controls {
                "WebMCP browser pairing"
            } else {
                "WebMCP connection status"
            })
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_3()
            .rounded_lg()
            .border_1()
            .border_color(colors.border)
            .bg(colors.secondary)
            .p_4()
            .child(div().font_semibold().child(if origin_controls {
                "Pair a browser app"
            } else {
                "WebMCP connection"
            }));

        if origin_controls {
            section = section
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child("Pair a browser app with this workspace and the active VT Code session."),
                )
                .child(self.render_webmcp_origin_controls(configured_origins, busy, cx));

            if let Some(pairing) = self.webmcp_pairing.as_ref() {
                let endpoint = pairing.endpoint.clone();
                let pairing_code = pairing.pairing_code.clone();
                section = section.child(
                    div()
                        .id("webmcp-pairing-details")
                        .test_support()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .rounded_md()
                        .bg(colors.secondary)
                        .p_3()
                        .child(div().text_sm().font_semibold().child("Pair this browser"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(format!("{} · expires in {} seconds", pairing.origin, pairing.expires_in_secs)),
                        )
                        .child(copy_value_row("WebSocket URL", endpoint.clone(), "copy-webmcp-endpoint", cx))
                        .child(copy_value_row("One-time code", pairing_code.clone(), "copy-webmcp-code", cx))
                        .child(div().text_xs().text_color(colors.muted_foreground).child(
                            "In the browser app, open Settings → Connect to a local VT Code bridge and paste both values.",
                        )),
                );
            }
        } else {
            let status_label = match status {
                Some(status) if status.running && (self.session_request.is_some() || self.running) => {
                    "Connected · the selected desktop session cannot accept a turn right now"
                }
                Some(status) if status.running && busy => "Connected · checking desktop session availability",
                Some(status) if status.running && self.webmcp_status_dirty => {
                    "Connected · desktop session availability unavailable"
                }
                Some(status) if status.running && status.turns_available => "Connected · desktop session ready",
                Some(status) if status.running && status.session_id.is_some() => {
                    "Connected · the selected desktop session cannot accept a turn right now"
                }
                Some(status) if status.running => "Connected · select or create a desktop session for agent turns",
                Some(_) => "Not connected",
                None if busy => "Checking bridge state…",
                None => "Bridge state unavailable",
            };
            section = section
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child("The browser app shares this workspace and the active VT Code session."),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .id("webmcp-session-status")
                                .test_support()
                                .role(gpui_kit::Role::Status)
                                .aria_label(status_label)
                                .min_w_0()
                                .text_sm()
                                .text_color(colors.foreground)
                                .child(status_label),
                        )
                        .child(
                            desktop_button("refresh-webmcp-status")
                                .icon(IconName::RefreshCw)
                                .accessibility_label("Refresh WebMCP connection status")
                                .ghost()
                                .small()
                                .desktop_disabled(busy)
                                .on_click(cx.listener(|view, _, _, cx| view.request_webmcp_status_with_notify(cx))),
                        ),
                );
        }

        section.into_any_element()
    }

    /// Origin list, pairing input, and pairing actions for one connection.
    fn render_webmcp_origin_controls(
        &self,
        configured_origins: &[String],
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let status = self.webmcp_status.as_ref();
        let mut controls = div().flex().flex_col().gap_3();
        if !configured_origins.is_empty() {
            controls = controls.child(div().flex().flex_wrap().gap_1().children(configured_origins.iter().map(
                |origin| {
                    let origin_value = origin.clone();
                    desktop_button(format!(
                        "pair-origin-{}",
                        origin.chars().filter(char::is_ascii_alphanumeric).collect::<String>()
                    ))
                    .label(origin.clone())
                    .ghost()
                    .small()
                    .desktop_disabled(busy)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.request_webmcp_pair(origin_value.clone(), false);
                        cx.notify();
                    }))
                },
            )));
        }

        controls = controls.child(
            div()
                .id("webmcp-origin-input")
                .test_support()
                .w_full()
                .min_w_0()
                .rounded_md()
                .border_1()
                .border_color(colors.border)
                .bg(colors.popover)
                .px_2()
                .py_1()
                .child(
                    Textarea::new(&self.webmcp_origin_input)
                        .accessibility_id("webmcp-origin")
                        .aria_label("Browser app origin")
                        .cursor_text()
                        .bg(colors.popover)
                        .w_full(),
                ),
        );

        let origin = self.webmcp_origin_input.read(cx).value().trim().to_string();
        controls = controls.child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    desktop_button("pair-webmcp")
                        .icon(IconName::ExternalLink)
                        .label("Create pairing code")
                        .desktop_disabled(busy || origin.is_empty())
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.request_webmcp_pair(origin.clone(), false);
                            cx.notify();
                        })),
                )
                .child(
                    desktop_button("open-webmcp-app")
                        .label("Open browser app")
                        .ghost()
                        .on_click(|_, _, cx| cx.open_url(WEBMCP_APP_URL)),
                )
                .when(status.is_some_and(|status| status.running), |row| {
                    row.child(
                        desktop_button("stop-webmcp")
                            .icon(IconName::X)
                            .label("Disconnect")
                            .ghost()
                            .desktop_disabled(busy)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.request_webmcp_stop();
                                cx.notify();
                            })),
                    )
                }),
        );

        if let Some(notice) = self.webmcp_notice.as_ref() {
            controls = controls.child(
                div()
                    .id("webmcp-notice")
                    .role(gpui_kit::Role::Status)
                    .aria_label(notice.clone())
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(notice.clone()),
            );
        }
        controls.into_any_element()
    }

    pub(super) fn request_webmcp_status(&mut self) {
        if self.webmcp_request_pending.is_some() {
            return;
        }
        self.webmcp_request_pending = Some(protocol::method::WEBMCP_STATUS);
        if !self.request(protocol::method::WEBMCP_STATUS, json!({})) {
            self.webmcp_request_pending = None;
            self.webmcp_status_dirty = true;
            self.webmcp_notice = Some("The local service request queue is unavailable.".into());
        } else {
            self.webmcp_status_dirty = false;
        }
    }

    pub(super) fn invalidate_webmcp_status(&mut self) {
        if self.webmcp_status.is_none() && self.webmcp_request_pending.is_none() && !self.webmcp_status_dirty {
            return;
        }
        self.webmcp_status_dirty = true;
        if self.webmcp_request_pending.is_none() {
            self.request_webmcp_status();
        }
    }

    fn request_webmcp_status_with_notify(&mut self, cx: &mut Context<Self>) {
        self.request_webmcp_status();
        cx.notify();
    }

    fn request_webmcp_pair(&mut self, origin: String, replace: bool) {
        if self.webmcp_request_pending.is_some() {
            return;
        }
        self.webmcp_pairing = None;
        self.webmcp_notice = None;
        self.webmcp_request_pending = Some(protocol::method::WEBMCP_PAIR);
        if !self.request(protocol::method::WEBMCP_PAIR, json!({ "origin": origin, "replace": replace })) {
            self.webmcp_request_pending = None;
            self.webmcp_notice = Some("The local service request queue is unavailable.".into());
        }
    }

    fn request_webmcp_stop(&mut self) {
        if self.webmcp_request_pending.is_some() {
            return;
        }
        self.webmcp_pairing = None;
        self.webmcp_notice = None;
        self.webmcp_request_pending = Some(protocol::method::WEBMCP_STOP);
        if !self.request(protocol::method::WEBMCP_STOP, json!({})) {
            self.webmcp_request_pending = None;
            self.webmcp_notice = Some("The local service request queue is unavailable.".into());
        }
    }

    pub(super) fn handle_webmcp_response(&mut self, method: &str, result: Value, cx: &mut Context<Self>) -> bool {
        if !matches!(
            method,
            protocol::method::WEBMCP_STATUS | protocol::method::WEBMCP_PAIR | protocol::method::WEBMCP_STOP
        ) {
            return false;
        }
        self.webmcp_request_pending = None;
        match method {
            protocol::method::WEBMCP_STATUS if self.webmcp_status_dirty => {
                self.request_webmcp_status();
            }
            protocol::method::WEBMCP_STATUS => match serde_json::from_value::<protocol::WebmcpBridgeStatus>(result) {
                Ok(status) => {
                    self.webmcp_status = Some(status);
                    if matches!(
                        self.webmcp_notice.as_deref(),
                        Some(WEBMCP_DISCONNECTED_NOTICE)
                            | Some("The desktop service returned an invalid WebMCP status.")
                            | Some("The local service request queue is unavailable.")
                    ) {
                        self.webmcp_notice = None;
                    }
                }
                Err(_) => {
                    self.webmcp_status_dirty = true;
                    self.webmcp_notice = Some("The desktop service returned an invalid WebMCP status.".into());
                }
            },
            protocol::method::WEBMCP_PAIR => match serde_json::from_value::<protocol::WebmcpPairResult>(result) {
                Ok(pairing) => {
                    self.webmcp_notice =
                        Some("Pairing code created. It is valid for a short time and can be used once.".into());
                    self.webmcp_pairing = Some(pairing);
                    self.request_webmcp_status();
                }
                Err(_) => self.webmcp_notice = Some("The desktop service returned invalid pairing details.".into()),
            },
            protocol::method::WEBMCP_STOP => {
                self.webmcp_pairing = None;
                self.webmcp_notice =
                    Some("WebMCP disconnected. Browser sessions and pairing codes were revoked.".into());
                self.request_webmcp_status();
            }
            _ => {}
        }
        cx.notify();
        true
    }

    pub(super) fn handle_webmcp_error(&mut self, method: &str, error: &str) -> bool {
        if !matches!(
            method,
            protocol::method::WEBMCP_STATUS | protocol::method::WEBMCP_PAIR | protocol::method::WEBMCP_STOP
        ) {
            return false;
        }
        self.webmcp_request_pending = None;
        self.webmcp_status_dirty = true;
        self.webmcp_notice = Some(error.to_string());
        self.error = Some(error.to_string());
        true
    }
}

fn copy_value_row(
    value_label: &'static str,
    value: String,
    id: &'static str,
    cx: &mut Context<DesktopView>,
) -> impl IntoElement {
    let copy = value.clone();
    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(96.))
                .text_xs()
                .text_color(cx.theme().colors.muted_foreground)
                .child(value_label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(cx.theme().mono_font_family.clone())
                .text_xs()
                .child(value),
        )
        .child(
            desktop_button(id)
                .icon(IconName::Copy)
                .accessibility_label(format!("Copy {value_label}"))
                .tooltip(format!("Copy {value_label}"))
                .ghost()
                .small()
                .on_click(move |_, window, cx| {
                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(copy.clone()));
                    super::super::notifications::copied_to_clipboard(window, cx);
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webmcp_slash_commands_share_cli_pairing_verbs_and_reject_ambiguous_input() {
        assert_eq!(parse_command("/webmcp").unwrap().unwrap(), WebmcpCommand::Status);
        assert_eq!(parse_command("/webmcp status").unwrap().unwrap(), WebmcpCommand::Status);
        assert_eq!(
            parse_command("/webmcp connect https://example.test").unwrap().unwrap(),
            WebmcpCommand::Pair {
                origin: "https://example.test".into(),
                replace: false,
            }
        );
        assert_eq!(
            parse_command("/webmcp pair --replace http://localhost:5173").unwrap().unwrap(),
            WebmcpCommand::Pair {
                origin: "http://localhost:5173".into(),
                replace: true,
            }
        );
        assert_eq!(
            parse_command("/webmcp connect --replace --origin http://localhost:5173")
                .unwrap()
                .unwrap(),
            WebmcpCommand::Pair {
                origin: "http://localhost:5173".into(),
                replace: true,
            }
        );
        assert_eq!(parse_command("/webmcp unpair").unwrap().unwrap(), WebmcpCommand::Stop);
        assert_eq!(parse_command("/webmcp stop").unwrap().unwrap(), WebmcpCommand::Stop);
        assert_eq!(parse_command("/webmcp tools").unwrap().unwrap(), WebmcpCommand::Tools);
        assert_eq!(parse_command("/webmcp roots").unwrap().unwrap(), WebmcpCommand::Roots);
        assert!(parse_command("/webmcp pair").unwrap().is_err());
        assert!(parse_command("/webmcp pair https://a.test https://b.test").unwrap().is_err());
        assert!(parse_command("/webmcp pair --unknown https://a.test").unwrap().is_err());
        assert!(
            parse_command("/webmcp pair --replace --replace https://a.test")
                .unwrap()
                .is_err()
        );
        assert!(parse_command("/webmcp-never").is_none());
    }
}
