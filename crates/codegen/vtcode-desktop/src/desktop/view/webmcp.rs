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

use super::{DesktopButtonExt, DesktopView, accessible_group_box, desktop_button, i18n, settings_group_box};

const WEBMCP_APP_URL: &str = "https://vtcode.vinhnx.chatgpt.site/";
/// Slash-parse failure: help text goes through the localized notice catalog,
/// usage renders from the `app.webmcp` catalog at the call site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WebmcpParseError {
    CommandsHelp,
    ConnectUsage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum WebmcpNotice {
    CommandsHelp,
    Disconnected,
    RequestQueueUnavailable,
    InvalidStatus,
    PairingCreated,
    InvalidPairing,
    Stopped,
    Text(String),
}

impl WebmcpNotice {
    pub(super) fn text(&self) -> String {
        match self {
            Self::CommandsHelp => rust_i18n::t!("app.webmcp_notice.commands_help").to_string(),
            Self::Disconnected => rust_i18n::t!("app.webmcp_notice.disconnected").to_string(),
            Self::RequestQueueUnavailable => rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string(),
            Self::InvalidStatus => rust_i18n::t!("app.webmcp_notice.invalid_status").to_string(),
            Self::PairingCreated => rust_i18n::t!("app.webmcp_notice.pairing_created").to_string(),
            Self::InvalidPairing => rust_i18n::t!("app.webmcp_notice.invalid_pairing").to_string(),
            Self::Stopped => rust_i18n::t!("app.webmcp_notice.stopped").to_string(),
            Self::Text(text) => text.clone(),
        }
    }

    fn clears_on_status_refresh(&self) -> bool {
        matches!(self, Self::Disconnected | Self::RequestQueueUnavailable | Self::InvalidStatus)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum WebmcpCommand {
    Help,
    Status,
    Pair { origin: String, replace: bool },
    Stop,
    Tools,
    Roots,
}

fn parse_command(input: &str) -> Option<Result<WebmcpCommand, WebmcpParseError>> {
    let input = input.trim();
    // The CLI dispatcher lowercases the command token, so `/WebMCP` must take
    // the same native path as `/webmcp`; otherwise pairing bypasses the
    // desktop panel refresh. `split_at_checked` keeps the byte split
    // panic-free on non-boundary input.
    let rest = input
        .split_at_checked("/webmcp".len())
        .filter(|(prefix, _)| prefix.eq_ignore_ascii_case("/webmcp"))
        .map(|(_, rest)| rest)?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let command = parse_webmcp_slash_command(rest.trim()).map_err(|error| match error {
        WebmcpSlashCommandParseError::UnknownCommand => WebmcpParseError::CommandsHelp,
        WebmcpSlashCommandParseError::InvalidConnectionArguments => WebmcpParseError::ConnectUsage,
    });
    Some(command.and_then(|command| match command {
        WebmcpSlashCommand::Status => Ok(WebmcpCommand::Status),
        WebmcpSlashCommand::Help => Ok(WebmcpCommand::Help),
        WebmcpSlashCommand::Tools => Ok(WebmcpCommand::Tools),
        WebmcpSlashCommand::Roots => Ok(WebmcpCommand::Roots),
        WebmcpSlashCommand::ConnectHelp => Err(WebmcpParseError::ConnectUsage),
        WebmcpSlashCommand::Connect { origin, replace } => Ok(WebmcpCommand::Pair { origin, replace }),
        WebmcpSlashCommand::Unpair => Ok(WebmcpCommand::Stop),
    }))
}

impl DesktopView {
    pub(super) fn reset_webmcp_state(&mut self, notice: Option<WebmcpNotice>) {
        self.webmcp_status = None;
        self.webmcp_pairing = None;
        self.webmcp_request_pending = None;
        self.webmcp_status_dirty = false;
        self.webmcp_notice = notice;
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
                self.error = Some(match error {
                    WebmcpParseError::CommandsHelp => WebmcpNotice::CommandsHelp.text(),
                    WebmcpParseError::ConnectUsage => rust_i18n::t!("app.webmcp.connect_usage").to_string(),
                });
                cx.notify();
                return true;
            }
        };
        if !self.attachments.is_empty() {
            self.error = Some(rust_i18n::t!("app.webmcp.attachments_error").to_string());
            cx.notify();
            return true;
        }
        self.composer.update(cx, |input, cx| input.set_value("", window, cx));
        self.error = None;
        self.open_webmcp_settings_panel(window, cx);
        match command {
            WebmcpCommand::Help => {
                self.webmcp_notice = Some(WebmcpNotice::CommandsHelp);
                self.request_webmcp_status();
            }
            WebmcpCommand::Status => self.request_webmcp_status(),
            WebmcpCommand::Pair { origin, replace } => {
                self.webmcp_origin_input
                    .update(cx, |input, cx| input.set_value(origin.clone(), window, cx));
                self.request_webmcp_pair(origin, replace);
            }
            WebmcpCommand::Tools => {
                self.webmcp_notice = Some(WebmcpNotice::Text(WEBMCP_TOOLS_HELP.into()));
            }
            WebmcpCommand::Roots => {
                self.webmcp_notice = Some(WebmcpNotice::Text(WEBMCP_ROOTS_HELP.into()));
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
        let (section_id, section_title, section_label) = if origin_controls {
            (
                "webmcp-pairing",
                rust_i18n::t!("app.webmcp.pair_app").to_string(),
                rust_i18n::t!("app.webmcp.pairing_title").to_string(),
            )
        } else {
            (
                "webmcp-status",
                rust_i18n::t!("app.webmcp.connection_title").to_string(),
                rust_i18n::t!("app.webmcp.status_title").to_string(),
            )
        };
        let mut group = settings_group_box(section_title, &colors);

        if origin_controls {
            group = group
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(rust_i18n::t!("app.webmcp.pair_description").to_string()),
                )
                .child(self.render_webmcp_origin_controls(configured_origins, busy, cx));

            if let Some(pairing) = self.webmcp_pairing.as_ref() {
                let endpoint = pairing.endpoint.clone();
                let pairing_code = pairing.pairing_code.clone();
                group = group.child(
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
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .child(rust_i18n::t!("app.webmcp.pair_browser").to_string()),
                        )
                        .child(
                            div().text_xs().text_color(colors.muted_foreground).child(
                                rust_i18n::t!(
                                    "app.webmcp.pairing_expires",
                                    origin = pairing.origin.as_str(),
                                    secs = pairing.expires_in_secs
                                )
                                .to_string(),
                            ),
                        )
                        .child(copy_value_row(
                            rust_i18n::t!("app.webmcp.ws_url").to_string(),
                            endpoint.clone(),
                            "copy-webmcp-endpoint",
                            cx,
                        ))
                        .child(copy_value_row(
                            rust_i18n::t!("app.webmcp.onetime_code").to_string(),
                            pairing_code.clone(),
                            "copy-webmcp-code",
                            cx,
                        ))
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(rust_i18n::t!("app.webmcp.pairing_instructions").to_string()),
                        ),
                );
            }
        } else {
            let connected = rust_i18n::t!("app.webmcp.connected_prefix").to_string();
            let status_label = match status {
                Some(status)
                    if status.running
                        && (self.session_request.is_some() || self.running || self.interaction.is_some()) =>
                {
                    format!("{connected} · {}", rust_i18n::t!("app.webmcp.connected_detail_busy"))
                }
                Some(status) if status.running && busy => {
                    format!("{connected} · {}", rust_i18n::t!("app.webmcp.connected_detail_checking"))
                }
                Some(status) if status.running && self.webmcp_status_dirty => {
                    format!("{connected} · {}", rust_i18n::t!("app.webmcp.connected_detail_unavailable"))
                }
                Some(status) if status.running && status.turns_available => {
                    format!("{connected} · {}", rust_i18n::t!("app.webmcp.connected_detail_ready"))
                }
                Some(status) if status.running && status.session_id.is_some() => {
                    format!("{connected} · {}", rust_i18n::t!("app.webmcp.connected_detail_busy"))
                }
                Some(status) if status.running => {
                    format!("{connected} · {}", rust_i18n::t!("app.webmcp.connected_detail_select"))
                }
                Some(_) => rust_i18n::t!("app.webmcp.not_connected").to_string(),
                None if busy => rust_i18n::t!("app.webmcp.checking_state").to_string(),
                None => rust_i18n::t!("app.webmcp.state_unavailable").to_string(),
            };
            group = group
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(rust_i18n::t!("app.webmcp.shares_description").to_string()),
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
                                .aria_label(status_label.clone())
                                .min_w_0()
                                .text_sm()
                                .text_color(colors.foreground)
                                .child(status_label),
                        )
                        .child(
                            desktop_button("refresh-webmcp-status")
                                .icon(IconName::RefreshCw)
                                .accessibility_label(rust_i18n::t!("app.webmcp.refresh_status").to_string())
                                .ghost()
                                .small()
                                .desktop_disabled(busy)
                                .on_click(cx.listener(|view, _, _, cx| view.request_webmcp_status_with_notify(cx))),
                        ),
                );
        }

        accessible_group_box(section_id, section_label, group)
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
                .bg(colors.popover)
                .px_2()
                .py_1()
                .child(
                    Textarea::new(&self.webmcp_origin_input)
                        .accessibility_id("webmcp-origin")
                        .aria_label(rust_i18n::t!("app.webmcp.origin_aria").to_string())
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
                        .label(rust_i18n::t!("app.webmcp.create_code").to_string())
                        .desktop_disabled(busy || origin.is_empty())
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.request_webmcp_pair(origin.clone(), false);
                            cx.notify();
                        })),
                )
                .child(
                    desktop_button("open-webmcp-app")
                        .label(rust_i18n::t!("app.webmcp.open_app").to_string())
                        .ghost()
                        .on_click(|_, _, cx| cx.open_url(WEBMCP_APP_URL)),
                )
                .when(status.is_some_and(|status| status.running), |row| {
                    row.child(
                        desktop_button("stop-webmcp")
                            .icon(IconName::X)
                            .label(rust_i18n::t!("app.webmcp.disconnect").to_string())
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
            let notice = notice.text();
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
            self.webmcp_notice = Some(WebmcpNotice::RequestQueueUnavailable);
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
            self.webmcp_notice = Some(WebmcpNotice::RequestQueueUnavailable);
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
            self.webmcp_notice = Some(WebmcpNotice::RequestQueueUnavailable);
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
                    if self.webmcp_notice.as_ref().is_some_and(WebmcpNotice::clears_on_status_refresh) {
                        self.webmcp_notice = None;
                    }
                }
                Err(_) => {
                    self.webmcp_status_dirty = true;
                    self.webmcp_notice = Some(WebmcpNotice::InvalidStatus);
                }
            },
            protocol::method::WEBMCP_PAIR => match serde_json::from_value::<protocol::WebmcpPairResult>(result) {
                Ok(pairing) => {
                    self.webmcp_notice = Some(WebmcpNotice::PairingCreated);
                    self.webmcp_pairing = Some(pairing);
                    self.request_webmcp_status();
                }
                Err(_) => self.webmcp_notice = Some(WebmcpNotice::InvalidPairing),
            },
            protocol::method::WEBMCP_STOP => {
                self.webmcp_pairing = None;
                self.webmcp_notice = Some(WebmcpNotice::Stopped);
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
        self.webmcp_notice = Some(WebmcpNotice::Text(error.to_string()));
        self.error = Some(error.to_string());
        true
    }
}

fn copy_value_row(
    value_label: String,
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
                .child(value_label.clone()),
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
                .accessibility_label(rust_i18n::t!("app.webmcp.copy_value", label = value_label.as_str()).to_string())
                .tooltip(rust_i18n::t!("app.webmcp.copy_value", label = value_label.as_str()).to_string())
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

    #[test]
    fn webmcp_command_prefix_matches_cli_case_insensitivity() {
        assert_eq!(parse_command("/WebMCP").unwrap().unwrap(), WebmcpCommand::Status);
        assert_eq!(parse_command("/WEBMCP status").unwrap().unwrap(), WebmcpCommand::Status);
        assert_eq!(
            parse_command("/Webmcp pair https://example.test").unwrap().unwrap(),
            WebmcpCommand::Pair {
                origin: "https://example.test".into(),
                replace: false
            }
        );
        // Short, over-long, and non-boundary prefixes still miss without panicking.
        assert!(parse_command("/webmc").is_none());
        assert!(parse_command("/webmcp-never").is_none());
        assert!(parse_command("/webmcpé").is_none());
        assert!(parse_command("é/webmcp").is_none());
        assert!(parse_command("abcdefé").is_none());
    }
}
