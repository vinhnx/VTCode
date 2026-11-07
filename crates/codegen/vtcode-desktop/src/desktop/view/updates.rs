//! Cached updater status: refresh requests, snapshot storage, notifications,
//! and the changelog viewer.

use super::*;

impl DesktopView {
    pub(super) fn request_update_status(&mut self) {
        if self.update_request_pending {
            return;
        }
        self.update_request_pending = true;
        if !self.request(protocol::method::UPDATE_STATUS, json!({})) {
            self.update_request_pending = false;
        }
    }

    pub(super) fn handle_update_status_response(&mut self, result: Value) {
        self.update_request_pending = false;
        match serde_json::from_value::<protocol::UpdateStatusSnapshot>(result) {
            Ok(status) => {
                // The viewer implies a viewable update; a refresh that clears
                // availability (updated elsewhere) closes it rather than
                // stranding an empty dialog.
                if !(status.update_available && status.latest_version.is_some()) {
                    self.changelog_open = false;
                    self.changelog_return_focus = None;
                }
                self.update_status = Some(status);
            }
            Err(_) => self.error = Some(rust_i18n::t!("app.service.invalid_updates").to_string()),
        }
    }

    /// Announce a newly reported release exactly once per version. Runs ahead
    /// of [`DesktopView::handle_backend_event`] so the system banner has a
    /// window, mirroring [`DesktopView::notify_agent_activity`].
    pub(super) fn maybe_notify_update_available(
        &mut self,
        event: &BackendEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let BackendEvent::Response { method, result: Some(result), error: None } = event else {
            return;
        };
        if method != protocol::method::UPDATE_STATUS {
            return;
        }
        let Ok(status) = serde_json::from_value::<protocol::UpdateStatusSnapshot>(result.clone()) else {
            return;
        };
        if !status.update_available {
            return;
        }
        let Some(latest) = status.latest_version.as_deref() else {
            return;
        };
        let notifications = &self.preferences.notifications;
        if !notifications.enabled() || !notifications.updates() {
            return;
        }
        if notifications.last_notified_update.as_deref() == Some(latest) {
            return;
        }
        let system = !window.is_window_active();
        let weak = cx.entity().downgrade();
        super::super::notifications::update_available(
            window,
            latest,
            system,
            deeplinks::click_handler(
                deeplinks::NotificationTarget::UpdateChangelog { version: latest.to_string() },
                weak,
            ),
            cx,
        );
        self.preferences.notifications.last_notified_update = Some(latest.to_string());
        self.preferences.save();
        cx.notify();
    }

    /// Whether the viewer has an update to show. The shared gate behind
    /// `open_changelog` and the Settings row button; the notification click
    /// applies a stricter version-pinned check on top and falls back to the
    /// Settings tab when the status moved on.
    pub(super) fn can_view_changelog(&self) -> bool {
        self.update_status
            .as_ref()
            .is_some_and(|status| status.update_available && status.latest_version.is_some())
    }

    /// Open the changelog viewer for the latest known release. Reads
    /// `update_status` live at render time instead of snapshotting it, so a
    /// refresh that lands while the viewer is open updates the notes rather
    /// than showing stale ones.
    pub(super) fn open_changelog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_view_changelog() {
            return;
        }
        self.disarm_double_escape();
        // Deferred popovers paint above modal scrims: the picker cannot stay open.
        self.close_attachment_picker(window, cx);
        // One modal at a time: the viewer would otherwise stack scrims.
        self.discard_license_viewer();
        if !self.changelog_open {
            self.changelog_return_focus = window.focused(cx);
        }
        self.changelog_open = true;
        self.changelog_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_changelog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.changelog_open {
            return;
        }
        self.disarm_double_escape();
        self.changelog_open = false;
        if let Some(focus) = self.changelog_return_focus.take() {
            focus.focus(window, cx);
        } else {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    /// Changelog viewer: the latest release's Highlights over the shared
    /// modal scrim, with the full release notes one click away in the
    /// browser. Without cached highlights it renders a link-only state; an
    /// invalid or missing release URL hides the browser action entirely.
    pub(super) fn render_changelog_viewer(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let weak = cx.entity().downgrade();
        let status = self.update_status.as_ref();
        let current = status.map(|status| status.current_version.as_str()).unwrap_or_default();
        let latest = status.and_then(|status| status.latest_version.as_deref()).unwrap_or_default();
        let highlights: &[String] = status.map(|status| status.release_highlights.as_slice()).unwrap_or_default();
        let release_url = status
            .and_then(|status| status.release_url.as_deref())
            .and_then(super::super::markdown::external_web_url)
            .map(|url| url.to_string());

        let body: AnyElement = if highlights.is_empty() {
            div()
                .id("changelog-empty")
                .test_support()
                .text_sm()
                .text_color(colors.muted_foreground)
                .child(rust_i18n::t!("app.changelog.empty").to_string())
                .into_any_element()
        } else {
            let rows: Vec<AnyElement> = highlights
                .iter()
                .map(|item| {
                    div()
                        .text_sm()
                        .text_color(colors.foreground)
                        .child(format!("• {item}"))
                        .into_any_element()
                })
                .collect();
            div()
                .id("changelog-highlights")
                .lock_scroll_axis()
                .test_support()
                .track_scroll(&self.changelog_scroll)
                .overflow_y_scroll()
                .max_h(px(360.))
                .flex()
                .flex_col()
                .gap_2()
                .children(rows)
                .into_any_element()
        };

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
                    .id("changelog-dialog")
                    .test_support()
                    .track_focus(&self.changelog_focus)
                    // Click-away dismissal, mirroring the palette panel: a
                    // press on the scrim closes through the Done path.
                    .on_mouse_down_out(move |_, window, cx| {
                        let _updated = weak.update(cx, |view, cx| {
                            view.close_changelog(window, cx);
                        });
                    })
                    .role(gpui_kit::Role::Dialog)
                    .accessibility_id("changelog-dialog")
                    .aria_label(rust_i18n::t!("app.changelog.title", version = latest).to_string())
                    .w(px(520.))
                    .max_w_full()
                    .max_h_full()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .rounded_md()
                    .bg(colors.popover)
                    .border_1()
                    .border_color(colors.border)
                    .shadow_md()
                    .p_6()
                    .child(
                        div()
                            .id("changelog-title")
                            .test_support()
                            .text_base()
                            .font_semibold()
                            .text_color(colors.foreground)
                            .child(rust_i18n::t!("app.changelog.title", version = latest).to_string()),
                    )
                    .child(
                        div().text_sm().text_color(colors.muted_foreground).child(
                            rust_i18n::t!("app.changelog.subtitle", current = current, latest = latest).to_string(),
                        ),
                    )
                    .child(body)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .w_full()
                            .when_some(release_url.as_ref(), |row, url| {
                                let url = url.clone();
                                row.child(
                                    desktop_default_button("open-release-notes")
                                        .label(rust_i18n::t!("app.changelog.view_full_notes").to_string())
                                        .on_click(move |_, _, cx| {
                                            cx.open_url(&url);
                                        }),
                                )
                            })
                            .child(
                                desktop_primary_button("close-changelog")
                                    .label(rust_i18n::t!("app.changelog.done").to_string())
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.close_changelog(window, cx);
                                    })),
                            ),
                    )
                    .focus_trap("changelog-dialog", &self.changelog_focus),
            )
            .into_any_element()
    }
}
