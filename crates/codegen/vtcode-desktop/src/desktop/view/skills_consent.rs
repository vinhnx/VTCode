//! Bundled-skills consent card: ask-first install approval shared with the CLI.
//!
//! The CLI first-run wizard and this card ask the same question from the
//! shared [`vtcode_config::consent`] copy and persist the same answer
//! (`skills.bundled.enabled`) to the user layer. The card opens once per
//! launch when a fresh configuration snapshot reports `Unasked`; answering
//! writes through the shared CLI config service and takes effect for new
//! sessions. Dismissing without answering asks again next launch.
use super::*;
use vtcode_config::consent::{
    BUNDLED_SKILLS_SETTING_PATH, CONSENT_ALLOW_LABEL, CONSENT_BODY_REVERSIBLE, CONSENT_BODY_WHAT,
    CONSENT_DECLINE_LABEL, CONSENT_MCP_DISCLOSURE, CONSENT_QUESTION, CONSENT_TITLE,
};

impl DesktopView {
    /// Open the consent card after a configuration snapshot reports `Unasked`.
    ///
    /// Deferred while another modal owns the overlay layer; the next snapshot
    /// retries. Older services omit the state and never trigger the card.
    pub(super) fn maybe_open_skills_consent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.skills_consent_seen || self.skills_consent_open {
            return;
        }
        let unasked = self.configuration.as_ref().is_some_and(|configuration| {
            configuration.bundled_skills_consent == Some(protocol::BundledSkillsConsent::Unasked)
        });
        if !unasked {
            return;
        }
        if self.full_access_confirm.is_some() || self.running_confirm.is_some() || self.about_open {
            return;
        }
        self.disarm_double_escape();
        // Deferred popovers paint above modal scrims: the picker cannot stay open.
        self.close_attachment_picker(window, cx);
        self.skills_consent_open = true;
        self.skills_consent_seen = true;
        self.skills_consent_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn dismiss_skills_consent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.disarm_double_escape();
        self.skills_consent_open = false;
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(super) fn answer_skills_consent(&mut self, allowed: bool, window: &mut Window, cx: &mut Context<Self>) {
        // The answer needs no settings catalog (fixed path, no expected
        // version), so it sends directly instead of through the editor-gated
        // saver: the catalog may still be loading when the user answers.
        // The modal stays open when nothing could be sent.
        if self.backend.is_none() || self.workspace.is_none() {
            self.error = Some(rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string());
            cx.notify();
            return;
        }
        let params = protocol::ConfigSettingsWriteParams {
            target: protocol::ConfigTarget::User,
            path: BUNDLED_SKILLS_SETTING_PATH.to_string(),
            value: Value::Bool(allowed),
            expected_layer_version: None,
        };
        if self.request(protocol::method::CONFIG_SETTINGS_WRITE, json!(params)) {
            self.disarm_double_escape();
            self.skills_consent_open = false;
            self.config_write_pending = true;
            self.session_request = Some(protocol::method::CONFIG_SETTINGS_WRITE);
            self.status = rust_i18n::t!("app.configuration.saving_settings").to_string();
            self.error = None;
            self.focus_composer(window, cx);
        } else {
            self.error = Some(rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string());
        }
        cx.notify();
    }

    /// Centered consent card over the shared modal scrim.
    pub(super) fn render_skills_consent(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let weak = cx.entity().downgrade();
        if !self.skills_consent_open {
            return div().into_any_element();
        }
        let paragraph = |text: &str| div().text_sm().text_color(colors.muted_foreground).child(text.to_string());
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
                    .id("skills-consent-dialog")
                    .test_support()
                    .track_focus(&self.skills_consent_focus)
                    .focus_trap("skills-consent-dialog", &self.skills_consent_focus)
                    // Click-away dismisses without answering (asks again next
                    // launch); only the buttons persist an answer.
                    .on_mouse_down_out(move |_, window, cx| {
                        let _updated = weak.update(cx, |view, cx| {
                            view.dismiss_skills_consent(window, cx);
                        });
                    })
                    .role(gpui_kit::Role::Dialog)
                    .accessibility_id("skills-consent-dialog")
                    .aria_label(CONSENT_TITLE.to_string())
                    .w(px(480.))
                    .max_w_full()
                    .max_h_full()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .rounded_md()
                    .bg(colors.popover)
                    .border_1()
                    .border_color(themes::soft_edge_color(&colors))
                    .shadow_md()
                    .p_5()
                    .child(consent_title_row(CONSENT_TITLE.to_string(), &colors))
                    .child(paragraph(CONSENT_QUESTION))
                    .child(paragraph(CONSENT_BODY_WHAT))
                    .child(paragraph(CONSENT_BODY_REVERSIBLE))
                    .child(paragraph(CONSENT_MCP_DISCLOSURE))
                    .child(paragraph("Takes effect for new sessions."))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .child(
                                desktop_secondary_button("skills-consent-decline")
                                    .label(CONSENT_DECLINE_LABEL.to_string())
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.answer_skills_consent(false, window, cx);
                                    })),
                            )
                            .child(
                                desktop_primary_button("skills-consent-allow")
                                    .label(CONSENT_ALLOW_LABEL.to_string())
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.answer_skills_consent(true, window, cx);
                                    })),
                            ),
                    ),
            )
            .navigation_fade("skills-consent-fade", true, false, cx.theme())
            .into_any_element()
    }
}
