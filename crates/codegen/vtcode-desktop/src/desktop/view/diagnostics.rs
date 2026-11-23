//! Setup-health self-solve entry for Settings: Diagnose button, repair rows,
//! and one-click fixes over the diagnostics protocol methods.
//!
//! Detection and apply live server-side on the shared repair core (the same
//! one `/doctor` uses); this module only renders state and routes actions.
//! Picker and secret repairs open native UI instead of calling apply.
use super::*;
use vtcode_desktop_protocol::{
    DiagnosticsApplyParams, DiagnosticsApplyResult, DiagnosticsCheckupResult, DiagnosticsRepair, DiagnosticsRepairKind,
};

pub(super) const DIAGNOSTICS_GROUP_TITLE: &str = "Setup health";
pub(super) const DIAGNOSTICS_DIAGNOSE_LABEL: &str = "Diagnose";
pub(super) const DIAGNOSTICS_RUNNING_LABEL: &str = "Diagnosing…";
pub(super) const DIAGNOSTICS_FIX_LABEL: &str = "Fix";
pub(super) const DIAGNOSTICS_CHOOSE_LABEL: &str = "Choose…";
pub(super) const DIAGNOSTICS_RESET_LABEL: &str = "Reset…";
pub(super) const DIAGNOSTICS_CONFIRM_RESET_LABEL: &str = "Confirm reset";
pub(super) const DIAGNOSTICS_HEALTHY_TEXT: &str = "No setup issues found.";
pub(super) const DIAGNOSTICS_UNCHECKED_TEXT: &str = "Run diagnostics to find broken setup essentials.";
pub(super) const DIAGNOSTICS_SECRET_HINT: &str = "Use the Secrets form in the provider group above.";
pub(super) const DIAGNOSTICS_UNREADABLE_TEXT: &str =
    "Diagnostics response was unreadable. Reconnect or update the app to match versions.";

#[derive(Clone, Default)]
pub(super) struct DiagnosticsSettingsProps {
    pub(super) repairs: Option<Vec<DiagnosticsRepair>>,
    pub(super) pending: bool,
    pub(super) armed: Option<String>,
    pub(super) notice: Option<String>,
    pub(super) connected: bool,
}

impl DiagnosticsSettingsProps {
    pub(super) fn capture(view: &DesktopView) -> Self {
        Self {
            repairs: view.diagnostics_repairs.clone(),
            pending: view.diagnostics_pending,
            armed: view.diagnostics_armed.clone(),
            notice: view.diagnostics_notice.clone(),
            connected: view.backend.is_some(),
        }
    }
}

impl DesktopView {
    pub(super) fn request_diagnostics_checkup(&mut self) -> bool {
        if !self.request(protocol::method::DIAGNOSTICS_CHECKUP, json!({})) {
            self.error = Some(rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string());
            return false;
        }
        self.diagnostics_pending = true;
        self.diagnostics_armed = None;
        self.diagnostics_notice = None;
        true
    }

    pub(super) fn request_diagnostics_apply(&mut self, repair_id: String) {
        let params = DiagnosticsApplyParams { repair_id };
        if !self.request(protocol::method::DIAGNOSTICS_APPLY, json!(params)) {
            self.error = Some(rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string());
            return;
        }
        self.diagnostics_pending = true;
        self.diagnostics_armed = None;
        self.diagnostics_notice = None;
    }

    pub(super) fn handle_diagnostics_checkup_response(&mut self, result: Value) {
        self.diagnostics_pending = false;
        match serde_json::from_value::<DiagnosticsCheckupResult>(result) {
            Ok(snapshot) => {
                self.diagnostics_repairs = Some(snapshot.repairs);
            }
            Err(_) => {
                self.diagnostics_notice = Some(DIAGNOSTICS_UNREADABLE_TEXT.to_string());
            }
        }
    }

    pub(super) fn handle_diagnostics_apply_response(&mut self, result: Value) {
        self.diagnostics_pending = false;
        match serde_json::from_value::<DiagnosticsApplyResult>(result) {
            Ok(applied) => {
                if applied.applied {
                    // Refresh the list like the CLI re-runs checkup after apply.
                    // The notice is set after: the refresh request clears it.
                    let _ = self.request_diagnostics_checkup();
                }
                self.diagnostics_notice = Some(applied.message);
            }
            Err(_) => {
                self.diagnostics_notice = Some(DIAGNOSTICS_UNREADABLE_TEXT.to_string());
            }
        }
    }

    pub(super) fn interrupt_diagnostics(&mut self) {
        if self.diagnostics_pending || self.diagnostics_armed.is_some() {
            self.diagnostics_pending = false;
            self.diagnostics_armed = None;
            let message = rust_i18n::t!("app.configuration.save_disconnected").to_string();
            self.error = Some(match self.error.take() {
                Some(error) => format!("{error}\n\n{message}"),
                None => message,
            });
        }
    }

    /// Arm (first click) or apply (second click) a destructive repair.
    pub(super) fn arm_or_apply_diagnostics_repair(&mut self, repair_id: String) {
        if self.diagnostics_armed.as_deref() == Some(repair_id.as_str()) {
            self.request_diagnostics_apply(repair_id);
        } else {
            self.diagnostics_armed = Some(repair_id);
        }
    }

    pub(super) fn render_diagnostics_settings_content(
        props: &DiagnosticsSettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let muted_foreground = cx.theme().colors.muted_foreground;
        let mut group = settings_group_box(DIAGNOSTICS_GROUP_TITLE.to_string(), &cx.theme().colors);
        let status_text = match (&props.repairs, props.pending) {
            (_, true) => DIAGNOSTICS_RUNNING_LABEL.to_string(),
            (None, false) => DIAGNOSTICS_UNCHECKED_TEXT.to_string(),
            (Some(repairs), false) if repairs.is_empty() => DIAGNOSTICS_HEALTHY_TEXT.to_string(),
            (Some(_), false) => String::new(),
        };
        if !status_text.is_empty() {
            group = group.child(div().text_sm().text_color(muted_foreground).child(status_text));
        }
        if let Some(repairs) = props.repairs.as_ref() {
            for repair in repairs {
                group = group.child(Self::render_diagnostics_repair_row(repair, props, view.clone(), cx));
            }
        }
        if let Some(notice) = props.notice.as_ref() {
            group = group.child(div().text_sm().text_color(muted_foreground).child(notice.clone()));
        }
        let diagnose = desktop_secondary_button("diagnostics-run")
            .label(DIAGNOSTICS_DIAGNOSE_LABEL.to_string())
            .disabled(props.pending || !props.connected)
            .on_click(move |_, _, app| {
                view.update(app, |view, _| {
                    let _ = view.request_diagnostics_checkup();
                });
            });
        group = group.child(div().flex().items_center().justify_end().child(diagnose));
        accessible_group_box("diagnostics-settings-group", DIAGNOSTICS_GROUP_TITLE.to_string(), group)
            .into_any_element()
    }

    fn render_diagnostics_repair_row(
        repair: &DiagnosticsRepair,
        props: &DiagnosticsSettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let theme = cx.theme();
        let mut row = div().flex().items_center().gap_2().child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().text_sm().child(repair.title.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(repair.subtitle.clone()),
                ),
        );
        let repair_id = repair.id.clone();
        let disabled = props.pending || !props.connected;
        match repair.kind {
            DiagnosticsRepairKind::Apply if repair.destructive => {
                let armed = props.armed.as_deref() == Some(repair_id.as_str());
                let label = if armed {
                    DIAGNOSTICS_CONFIRM_RESET_LABEL
                } else {
                    DIAGNOSTICS_RESET_LABEL
                };
                row = row.child(
                    desktop_danger_button(format!("diagnostics-fix-{}", repair.id))
                        .label(label.to_string())
                        .disabled(disabled)
                        .on_click(move |_, _, app| {
                            let repair_id = repair_id.clone();
                            view.update(app, |view, _| {
                                view.arm_or_apply_diagnostics_repair(repair_id);
                            });
                        }),
                );
            }
            DiagnosticsRepairKind::Apply => {
                row = row.child(
                    desktop_primary_button(format!("diagnostics-fix-{}", repair.id))
                        .label(DIAGNOSTICS_FIX_LABEL.to_string())
                        .disabled(disabled)
                        .on_click(move |_, _, app| {
                            let repair_id = repair_id.clone();
                            view.update(app, |view, _| {
                                view.request_diagnostics_apply(repair_id);
                            });
                        }),
                );
            }
            DiagnosticsRepairKind::ModelPicker => {
                row = row.child(
                    desktop_secondary_button(format!("diagnostics-fix-{}", repair.id))
                        .label(DIAGNOSTICS_CHOOSE_LABEL.to_string())
                        .disabled(disabled)
                        .on_click(move |_, window, app| {
                            view.update(app, |view, cx| {
                                view.open_model_palette(window, cx);
                            });
                        }),
                );
            }
            DiagnosticsRepairKind::SecretStore => {
                row = row.child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(DIAGNOSTICS_SECRET_HINT),
                );
            }
        }
        row.into_any_element()
    }
}
