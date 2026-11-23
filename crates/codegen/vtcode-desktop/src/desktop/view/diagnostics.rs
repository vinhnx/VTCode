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
pub(super) const DIAGNOSTICS_REVIEW_LABEL: &str = "Review";
pub(super) const DIAGNOSTICS_CHOOSE_LABEL: &str = "Choose…";
pub(super) const DIAGNOSTICS_RESET_LABEL: &str = "Reset…";
pub(super) const DIAGNOSTICS_APPLY_LABEL: &str = "Apply";
pub(super) const DIAGNOSTICS_CANCEL_LABEL: &str = "Cancel";
pub(super) const DIAGNOSTICS_HEALTHY_TEXT: &str = "No setup issues found.";
pub(super) const DIAGNOSTICS_UNCHECKED_TEXT: &str = "Run diagnostics to find broken setup essentials.";
pub(super) const DIAGNOSTICS_SECRET_HINT: &str = "Use the Secrets form in the provider group above.";
pub(super) const DIAGNOSTICS_UNREADABLE_TEXT: &str =
    "Diagnostics response was unreadable. Reconnect or update the app to match versions.";
pub(super) const DIAGNOSTICS_TERMINAL_HINT: &str = "Open the built-in terminal (⌘J) to manage CLI configs, or continue this session in the CLI from Settings → Connections.";

#[derive(Clone)]
pub(super) struct DiagnosticsSettingsProps {
    pub(super) repairs: Option<Vec<DiagnosticsRepair>>,
    pub(super) pending: bool,
    pub(super) expanded: Option<String>,
    pub(super) notice: Option<String>,
    pub(super) connected: bool,
    pub(super) focus_handle: FocusHandle,
}

impl DiagnosticsSettingsProps {
    pub(super) fn capture(view: &DesktopView) -> Self {
        Self {
            repairs: view.diagnostics_repairs.clone(),
            pending: view.diagnostics_pending,
            expanded: view.diagnostics_expanded.clone(),
            notice: view.diagnostics_notice.clone(),
            connected: view.backend.is_some(),
            focus_handle: view.focus_handle.clone(),
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
        self.diagnostics_expanded = None;
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
        self.diagnostics_expanded = None;
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
        if self.diagnostics_pending || self.diagnostics_expanded.is_some() {
            self.diagnostics_pending = false;
            self.diagnostics_expanded = None;
            let message = rust_i18n::t!("app.configuration.save_disconnected").to_string();
            self.error = Some(match self.error.take() {
                Some(error) => format!("{error}\n\n{message}"),
                None => message,
            });
        }
    }

    /// Expand a repair row to show its preview, or collapse it again.
    pub(super) fn toggle_diagnostics_repair(&mut self, repair_id: String) {
        if self.diagnostics_expanded.as_deref() == Some(repair_id.as_str()) {
            self.diagnostics_expanded = None;
        } else {
            self.diagnostics_expanded = Some(repair_id);
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
        // Built-in terminal escape hatch: the panel is client-side, so the
        // toggle stays available while disconnected. The tooltip carries the
        // ⌘J chord via the shared action.
        let terminal_label = rust_i18n::t!("app.navigation.terminal").to_string();
        group = group.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(muted_foreground)
                        .child(DIAGNOSTICS_TERMINAL_HINT),
                )
                .child(
                    desktop_ghost_button("diagnostics-terminal")
                        .label(terminal_label.clone())
                        .icon(IconName::Terminal)
                        .accessibility_label(terminal_label.clone())
                        .tooltip_with_action(terminal_label, &ToggleTerminal, Some(key_context::DESKTOP))
                        .dispatches_desktop_action(ToggleTerminal, props.focus_handle.clone()),
                ),
        );
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
            DiagnosticsRepairKind::Apply => {
                let label = if repair.destructive {
                    DIAGNOSTICS_RESET_LABEL
                } else {
                    DIAGNOSTICS_REVIEW_LABEL
                };
                let button = if repair.destructive {
                    desktop_danger_button(format!("diagnostics-review-{}", repair.id))
                        .label(label.to_string())
                        .disabled(disabled)
                } else {
                    desktop_secondary_button(format!("diagnostics-review-{}", repair.id))
                        .label(label.to_string())
                        .disabled(disabled)
                };
                row = row.child(button.on_click({
                    let (view, repair_id) = (view.clone(), repair_id.clone());
                    move |_, _, app| {
                        let repair_id = repair_id.clone();
                        view.update(app, |view, _| {
                            view.toggle_diagnostics_repair(repair_id);
                        });
                    }
                }));
            }
            DiagnosticsRepairKind::ModelPicker => {
                row = row.child(
                    desktop_secondary_button(format!("diagnostics-fix-{}", repair.id))
                        .label(DIAGNOSTICS_CHOOSE_LABEL.to_string())
                        .disabled(disabled)
                        .on_click({
                            let view = view.clone();
                            move |_, window, app| {
                                view.update(app, |view, cx| {
                                    view.open_model_palette(window, cx);
                                });
                            }
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
        let mut cell = div().flex().flex_col().gap_2().child(row);
        if props.expanded.as_deref() == Some(repair_id.as_str()) && matches!(repair.kind, DiagnosticsRepairKind::Apply)
        {
            cell = cell.child(Self::render_diagnostics_preview(repair, props, view, cx));
        }
        cell.into_any_element()
    }

    /// Expanded preview for one repair: target, per-key changes, created
    /// files, and notes, with explicit Apply/Cancel actions.
    fn render_diagnostics_preview(
        repair: &DiagnosticsRepair,
        props: &DiagnosticsSettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let theme = cx.theme();
        let mono_font_family = theme.mono_font_family.clone();
        let muted_foreground = theme.colors.muted_foreground;
        let mut panel = div().flex().flex_col().gap_1().rounded_md().bg(theme.colors.muted).p_3().child(
            div()
                .text_xs()
                .font_family(mono_font_family.clone())
                .text_color(muted_foreground)
                .child(repair.preview.target_display.clone()),
        );
        for change in &repair.preview.changes {
            panel = panel.child(
                div()
                    .text_xs()
                    .font_family(mono_font_family.clone())
                    .child(format!("{}: {} \u{2192} {}", change.path, change.old, change.new)),
            );
        }
        for create in &repair.preview.creates {
            panel = panel.child(
                div()
                    .text_xs()
                    .font_family(mono_font_family.clone())
                    .child(format!("+ creates {create}")),
            );
        }
        if let Some(note) = repair.preview.note.as_ref() {
            panel = panel.child(div().text_xs().text_color(muted_foreground).child(note.clone()));
        }
        let repair_id = repair.id.clone();
        let disabled = props.pending || !props.connected;
        let apply = if repair.destructive {
            desktop_danger_button(format!("diagnostics-apply-{}", repair.id))
                .label(DIAGNOSTICS_APPLY_LABEL.to_string())
                .disabled(disabled)
        } else {
            desktop_primary_button(format!("diagnostics-apply-{}", repair.id))
                .label(DIAGNOSTICS_APPLY_LABEL.to_string())
                .disabled(disabled)
        };
        panel = panel.child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .child(
                    desktop_secondary_button(format!("diagnostics-cancel-{}", repair.id))
                        .label(DIAGNOSTICS_CANCEL_LABEL.to_string())
                        .disabled(disabled)
                        .on_click({
                            let (view, repair_id) = (view.clone(), repair_id.clone());
                            move |_, _, app| {
                                let repair_id = repair_id.clone();
                                view.update(app, |view, _| {
                                    view.toggle_diagnostics_repair(repair_id);
                                });
                            }
                        }),
                )
                .child(apply.on_click({
                    let (view, repair_id) = (view.clone(), repair_id.clone());
                    move |_, _, app| {
                        let repair_id = repair_id.clone();
                        view.update(app, |view, _| {
                            view.request_diagnostics_apply(repair_id);
                        });
                    }
                })),
        );
        panel.into_any_element()
    }
}
