//! Full-access confirmation modal: risk disclosure with explicit acknowledge.
//!
//! Selecting Full access from the permissions palette opens this modal
//! instead of switching immediately. Confirm stays disabled until the risk
//! checkbox is checked; confirming stamps a one-shot attestation onto the
//! session-options request, which the CLI server enforces.
use super::*;
use gpui_kit::component::checkbox::Checkbox;

/// Pending Full access switch awaiting risk acknowledgement.
pub(super) struct FullAccessConfirmState {
    /// Session options to send once acknowledged (attestation stamped at send).
    options: protocol::SessionOptionsSelectParams,
    /// Workspace the request was opened from; guards against cross-workspace send.
    workspace: Option<PathBuf>,
    /// Whether the risk checkbox is checked; Confirm stays disabled until set.
    acknowledged: bool,
}

impl DesktopView {
    pub(super) fn open_full_access_confirm(
        &mut self,
        options: protocol::SessionOptionsSelectParams,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.disarm_double_escape();
        self.full_access_confirm = Some(FullAccessConfirmState {
            options,
            workspace: self.workspace.clone(),
            acknowledged: false,
        });
        self.full_access_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn cancel_full_access_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.disarm_double_escape();
        self.full_access_confirm = None;
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(super) fn confirm_full_access(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.full_access_confirm.take() else {
            return;
        };
        self.disarm_double_escape();
        self.focus_composer(window, cx);
        if !state.acknowledged || self.workspace != state.workspace {
            cx.notify();
            return;
        }
        let mut options = state.options;
        options.full_access_confirmed = Some(true);
        let _selected = self.select_session_options(options, cx);
        cx.notify();
    }

    /// Centered consent card over the shared modal scrim. Confirm is visibly
    /// disabled until the risk checkbox is checked.
    pub(super) fn render_full_access_confirm(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let Some(state) = self.full_access_confirm.as_ref() else {
            return div().into_any_element();
        };
        let acknowledged = state.acknowledged;
        let capability = |icon: IconName, title: String, body: String| {
            div()
                .flex()
                .items_start()
                .gap_3()
                .py_2()
                .child(Icon::new(icon).small().text_color(colors.primary))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().font_semibold().text_sm().text_color(colors.foreground).child(title))
                        .child(div().text_sm().text_color(colors.muted_foreground).child(body)),
                )
        };
        let hairline = || div().h(px(1.)).w_full().bg(themes::hairline(colors.popover, colors.foreground));
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .bg(modal_scrim(&colors))
            .child(
                div()
                    .id("full-access-confirm-dialog")
                    .test_support()
                    .track_focus(&self.full_access_focus)
                    .role(gpui_kit::Role::Dialog)
                    .accessibility_id("full-access-confirm-dialog")
                    .aria_label(rust_i18n::t!("app.permissions.confirm_full_title").to_string())
                    .w(px(480.))
                    .max_w_full()
                    .max_h_full()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .rounded_md()
                    .bg(colors.popover)
                    .border_1()
                    .border_color(colors.border)
                    .shadow_md()
                    .p_5()
                    .child(consent_title_row(rust_i18n::t!("app.permissions.confirm_full_title").to_string(), &colors))
                    .child(
                        div()
                            .text_sm()
                            .text_color(colors.muted_foreground)
                            .child(rust_i18n::t!("app.permissions.confirm_full_body").to_string()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .rounded_md()
                            .border_1()
                            .border_color(colors.border)
                            .px_4()
                            .py_2()
                            .child(capability(
                                IconName::Folder,
                                rust_i18n::t!(i18n::PERMISSIONS_CONFIRM_FILES_TITLE).to_string(),
                                rust_i18n::t!(i18n::PERMISSIONS_CONFIRM_FILES_BODY).to_string(),
                            ))
                            .child(hairline())
                            .child(capability(
                                IconName::Terminal,
                                rust_i18n::t!(i18n::PERMISSIONS_CONFIRM_TERMINAL_TITLE).to_string(),
                                rust_i18n::t!(i18n::PERMISSIONS_CONFIRM_TERMINAL_BODY).to_string(),
                            ))
                            .child(hairline())
                            .child(capability(
                                IconName::Globe,
                                rust_i18n::t!(i18n::PERMISSIONS_CONFIRM_NETWORK_TITLE).to_string(),
                                rust_i18n::t!(i18n::PERMISSIONS_CONFIRM_NETWORK_BODY).to_string(),
                            )),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(colors.muted_foreground)
                            .child(rust_i18n::t!(i18n::PERMISSIONS_CONFIRM_RISK).to_string()),
                    )
                    .child(
                        Checkbox::new("full-access-acknowledge")
                            .label(rust_i18n::t!(i18n::PERMISSIONS_CONFIRM_ACKNOWLEDGE).to_string())
                            .checked(acknowledged)
                            .cursor_pointer()
                            .on_change(cx.listener(|view, checked: &bool, _, cx| {
                                if let Some(state) = view.full_access_confirm.as_mut() {
                                    state.acknowledged = *checked;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .child(
                                desktop_button("full-access-cancel")
                                    .label(rust_i18n::t!(i18n::CONFIGURATION_CANCEL).to_string())
                                    .ghost()
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.cancel_full_access_confirm(window, cx);
                                    })),
                            )
                            .child(
                                desktop_button("full-access-confirm")
                                    .label(rust_i18n::t!("app.permissions.enable_full").to_string())
                                    .danger()
                                    .disabled(!acknowledged)
                                    .when(!acknowledged, |button| button.cursor_default())
                                    .when(acknowledged, |button| button.cursor_pointer())
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.confirm_full_access(window, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}
