//! Cached updater status: refresh requests, snapshot storage, notifications,
//! and the changelog viewer.
//!
//! The reader only ever presents the running (installed) binary's notes. A
//! cached newer release remains readable to the updater as an *offer* — it
//! drives the update notification and the About install action — but its
//! notes are never shown before it is actually installed, so the app cannot
//! advertise a release the user has not received.

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

    /// Re-run the update check against GitHub instead of re-reading the cache.
    ///
    /// `update/status` is read-only, so the old "Check for updates" button
    /// returned an identical snapshot and looked inert. This one does network
    /// I/O and reports the refreshed result, which is what the label promises.
    /// Returns whether the check was actually sent.
    pub(super) fn request_update_check(&mut self) -> bool {
        if self.update_request_pending {
            return false;
        }
        self.update_request_pending = true;
        if !self.request(protocol::method::UPDATE_CHECK, json!({})) {
            self.update_request_pending = false;
            return false;
        }
        true
    }

    /// Fetch the running version's notes when a status response carries none.
    ///
    /// `update/status` only reads the local cache, so a desktop-only install —
    /// which never runs the CLI's startup refresh — would otherwise stay quiet
    /// on its first launch after an external upgrade. One live `update/check`
    /// backfills the cache; its response re-renders through the normal path
    /// and arms the banner, staying passive (no auto-opened dialog: the
    /// backfill lands after launch settles). Best-effort: failures stay silent
    /// (see the `update/check` error path), and opted-out checks are a service
    /// no-op that simply re-serves the quiet state. Check responses never
    /// re-trigger this — status responses only — so it cannot loop.
    fn maybe_backfill_release_notes(&mut self) {
        let needs_backfill = self
            .update_status
            .as_ref()
            .is_some_and(|status| !status.current_release_seen && status.current_release_highlights.is_empty());
        if !needs_backfill {
            return;
        }
        if self.request_update_check() {
            self.update_backfill_in_flight = true;
        }
    }

    /// Ask the service to install the newest release. Progress arrives on
    /// `update/installProgress`; the response only starts the install.
    ///
    /// The optimistic `Downloading` state is reverted when the request cannot
    /// even be sent, so a dead transport never leaves the row claiming a
    /// download that never began. A request the service *rejects* is handled
    /// by the error path in `handle.rs`, which records the failure.
    pub(super) fn request_update_install(&mut self) {
        if self.update_install_state.is_in_flight() {
            return;
        }
        self.update_install_state = protocol::UpdateInstallState::Downloading { downloaded: 0, total: None };
        if !self.request(protocol::method::UPDATE_INSTALL, json!({})) {
            self.update_install_state = protocol::UpdateInstallState::Idle;
        }
    }

    /// Fetch the status only when no snapshot is stored yet. The Settings →
    /// About row reads a snapshot and renders an inert "no notes" state without
    /// one, so a missing snapshot is the one case worth a round-trip. Called
    /// from selection paths only, never from render: a failing service would
    /// otherwise be retried once per frame, and this page has no Retry control.
    pub(super) fn maybe_request_update_status(&mut self) {
        if self.update_status.is_none() {
            self.request_update_status();
        }
    }

    pub(super) fn handle_update_status_response(&mut self, result: Value) {
        self.update_request_pending = false;
        self.apply_update_status(result);
        self.maybe_backfill_release_notes();
    }

    /// `update/check` and `update/status` share a snapshot shape, so one
    /// applier serves both: the live check simply refreshes what is displayed.
    pub(super) fn handle_update_check_response(&mut self, result: Value) {
        self.update_request_pending = false;
        self.update_backfill_in_flight = false;
        self.apply_update_status(result);
    }

    /// Apply an `update/installProgress` notification. Coalescable: only the
    /// newest phase matters, so a burst of download ticks keeps the last one.
    pub(super) fn handle_update_install_progress(&mut self, state: protocol::UpdateInstallState) {
        self.update_install_state = state;
    }

    /// Reset in-flight update bookkeeping when the service goes away. The
    /// client sets an optimistic `Downloading 0 bytes` phase the moment Install
    /// is clicked, and a crashing or exiting service has no terminal progress
    /// tick (or error response) to replace it. Without this the About row
    /// would claim a download that can never finish, keep Install suppressed,
    /// and show "Checking…" forever for a request that died with the service.
    /// Failing locally re-arms both actions; the service becomes authoritative
    /// again on the next successful status/check response.
    pub(super) fn reset_update_state_on_disconnect(&mut self) {
        if self.update_install_state.is_in_flight() {
            self.update_install_state = protocol::UpdateInstallState::Failed {
                message: rust_i18n::t!("app.service.review_disconnected").to_string(),
            };
        }
        self.update_request_pending = false;
        self.update_backfill_in_flight = false;
    }

    /// Store a status snapshot and re-derive the release-notes banner. Shared
    /// by the status request and the acknowledgement response (which also
    /// carries a snapshot) without touching the status-request in-flight flag.
    pub(super) fn apply_update_status(&mut self, result: Value) {
        match serde_json::from_value::<protocol::UpdateStatusSnapshot>(result) {
            Ok(status) => {
                // The snapshot is the service's authoritative install state:
                // adopting it keeps a restarted client (or one that missed a
                // progress tick) consistent with the install. Progress
                // notifications still drive the live row between refreshes.
                self.update_install_state = status.install.clone();
                self.update_status = Some(status);
                // The viewer implies viewable content; a refresh that drops
                // the installed version's notes closes it rather than
                // stranding an empty dialog.
                if self.changelog_open && !self.can_view_current_changelog() {
                    self.changelog_open = false;
                    self.changelog_return_focus = None;
                }
                // The release-notes banner is passive: it never steals focus
                // or blocks input, so arming it here needs no window and
                // survives any number of refreshes until dismissed.
                self.release_notes_banner_visible_now = self.should_show_release_notes_banner();
            }
            Err(_) => self.error = Some(rust_i18n::t!("app.service.invalid_updates").to_string()),
        }
    }

    /// Whether the stored status carries unseen notes for the running
    /// version. The seen marker lives in the shared update cache (served as
    /// `current_release_seen`), so notes the TUI transcript already announced
    /// are never repeated here, and the desktop records the same marker back.
    /// (`vtcode update` prints notes without marking, so the first app launch
    /// still announces them.) Versions without notes never record, so a
    /// notes-less launch retries instead of marking seen.
    ///
    /// The `current_release_highlights` field only ever describes the running
    /// version: the service self-matches cached notes against its own binary
    /// before sending them, so a newer cached release cannot arm this banner.
    fn should_show_release_notes_banner(&self) -> bool {
        let Some(status) = self.update_status.as_ref() else {
            return false;
        };
        !status.current_release_highlights.is_empty()
            && !status.current_release_seen
            && self.dismissed_release_notes_version.as_deref() != Some(status.current_version.as_str())
    }

    /// The banner is shown on every route (welcome, workspace, zen) because it
    /// sits in the conversation column and owns no overlay or focus.
    pub(super) fn release_notes_banner_visible(&self) -> bool {
        self.release_notes_banner_visible_now
    }

    /// Dismiss the banner and mark the running version's notes seen. The
    /// acknowledgement goes to the service so the shared cache — and the CLI —
    /// agree this release was announced; a failed request only risks one
    /// reminder on the next launch.
    ///
    /// `dismissed_release_notes_version` is the local guard: an older service
    /// rejects `update/acknowledge` and keeps reporting the notes unseen, so
    /// the banner must stay retired for this launch regardless of the echo.
    pub(super) fn dismiss_release_notes_banner(&mut self, cx: &mut Context<Self>) {
        if !self.release_notes_banner_visible_now {
            return;
        }
        if let Some(status) = self.update_status.as_ref() {
            self.dismissed_release_notes_version = Some(status.current_version.clone());
        }
        self.release_notes_banner_visible_now = false;
        let _ = self.request(protocol::method::UPDATE_ACKNOWLEDGE, json!({}));
        cx.notify();
    }

    /// Open the viewer for the running version's notes from the banner, then
    /// retire the banner: the reader has seen what it advertised.
    pub(super) fn view_release_notes_from_banner(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_changelog(window, cx);
        self.dismiss_release_notes_banner(cx);
    }

    /// Auto-open the running version's notes on first sight. The launch-time
    /// `update/status` response carries unseen highlights exactly once per
    /// updated version, and the dialog is the release announcement. Viewing
    /// retires the banner through the shared acknowledge path, so a later
    /// refresh — or a failed acknowledgement still echoing unseen — never
    /// re-opens it. An already-open viewer keeps its mode: the banner stays
    /// as the fallback instead of yanking what the user is reading.
    pub(super) fn maybe_autoshow_release_notes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.changelog_open || !self.release_notes_banner_visible_now {
            return;
        }
        self.view_release_notes_from_banner(window, cx);
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
            // The toast announces an *offer*, so clicking it lands on the
            // About page's Update status row (Check / Install) rather than
            // opening notes for a release that is not installed yet.
            deeplinks::click_handler(deeplinks::NotificationTarget::UpdateSettings, weak),
            cx,
        );
        self.preferences.notifications.last_notified_update = Some(latest.to_string());
        self.preferences.save();
        cx.notify();
    }

    /// The installed version whose notes the reader may present. `None`
    /// means no reader entry point should appear: a cached newer release is
    /// an update *offer*, not something the user has already received.
    pub(super) fn installed_changelog_version(&self) -> Option<&str> {
        let status = self.update_status.as_ref()?;
        self.can_view_current_changelog().then_some(status.current_version.as_str())
    }

    /// Whether the running version's own notes are known (the post-update
    /// "what's new" moment). Needs no available update: the notes belong to
    /// the binary the user is running, not to a cached offer.
    pub(super) fn can_view_current_changelog(&self) -> bool {
        self.update_status
            .as_ref()
            .is_some_and(|status| !status.current_release_highlights.is_empty())
    }

    /// Alias used by the `/go changelog` reachability gate: the only readable
    /// notes are the installed binary's.
    pub(super) fn can_view_any_changelog(&self) -> bool {
        self.can_view_current_changelog()
    }

    /// Open the viewer for the installed binary's release notes. Reads
    /// `update_status` live at render time instead of snapshotting it, so a
    /// refresh that lands while the viewer is open updates the notes rather
    /// than showing stale ones.
    pub(super) fn open_changelog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_view_current_changelog() {
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

    /// Changelog viewer: the installed binary's Highlights over the shared
    /// modal scrim, with its full release notes one click away in the browser.
    /// An invalid or missing release URL hides the browser action entirely.
    /// There is no upgrade subtitle: the viewer only ever describes a release
    /// the user already has, never a cached offer. The empty-body branch is a
    /// rendering fallback; `open_changelog` refuses to open without Highlights.
    pub(super) fn render_changelog_viewer(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let weak = cx.entity().downgrade();
        let status = self.update_status.as_ref();
        let shown_version = status.map(|status| status.current_version.as_str()).unwrap_or_default();
        let highlights: &[String] = status
            .map(|status| status.current_release_highlights.as_slice())
            .unwrap_or_default();
        let release_url = status
            .and_then(|status| status.current_release_url.as_deref())
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
                    .focus_trap("changelog-dialog", &self.changelog_focus)
                    // Click-away dismissal, mirroring the palette panel: a
                    // press on the scrim closes through the Done path.
                    .on_mouse_down_out(move |_, window, cx| {
                        let _updated = weak.update(cx, |view, cx| {
                            view.close_changelog(window, cx);
                        });
                    })
                    .role(gpui_kit::Role::Dialog)
                    .accessibility_id("changelog-dialog")
                    .aria_label(rust_i18n::t!("app.changelog.title", version = shown_version).to_string())
                    .w(px(520.))
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
                    .p_6()
                    .child(
                        div()
                            .id("changelog-title")
                            .test_support()
                            .text_base()
                            .font_semibold()
                            .text_color(colors.foreground)
                            .child(rust_i18n::t!("app.changelog.title", version = shown_version).to_string()),
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
                                    desktop_secondary_button("open-release-notes")
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
                    ),
            )
            .navigation_fade("changelog-fade", true, false, cx.theme())
            .into_any_element()
    }

    /// Passive "what's new" banner: the running version's highlights inline in
    /// the conversation column. It never steals focus, blocks input, or covers
    /// the launch screen — it simply occupies one row above the transcript and
    /// stays until dismissed, so a first-launch user sees it on welcome and a
    /// post-update user sees it on their workspace without losing their place.
    pub(super) fn render_release_notes_banner(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let Some(status) = self.update_status.as_ref() else {
            return div().into_any_element();
        };
        let highlights = &status.current_release_highlights;
        let summary = highlights.first().map(String::as_str).unwrap_or_default();
        let more = highlights.len().saturating_sub(1);
        let title = rust_i18n::t!("app.changelog.banner_title", version = status.current_version.as_str()).to_string();
        let message = if more == 0 {
            summary.to_owned()
        } else {
            rust_i18n::t!("app.changelog.banner_more", summary = summary, count = more).to_string()
        };
        let view_label = rust_i18n::t!("app.changelog.banner_view").to_string();
        let dismiss_label = rust_i18n::t!("app.changelog.banner_dismiss").to_string();

        div()
            .w_full()
            .flex()
            .justify_center()
            .px_3()
            .child(
                div()
                    .id("release-notes-banner")
                    .test_support()
                    // Info severity, not Alert: this is a passive announcement,
                    // not an interruption, so it takes the same quiet
                    // secondary+border pairing as `status_alert` without
                    // borrowing the warning/danger tones.
                    .role(gpui_kit::Role::Status)
                    .aria_label(format!("{title}. {message}"))
                    .w_full()
                    .max_w(px(self.content_width()))
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(themes::soft_edge_color(&colors))
                    .bg(colors.secondary)
                    .text_sm()
                    .text_color(colors.secondary_foreground)
                    .child(Icon::new(IconName::Info).small().text_color(colors.primary))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(div().flex_shrink_0().font_semibold().child(title))
                            .child(div().min_w_0().truncate().child(message)),
                    )
                    .child(
                        desktop_ghost_button("release-notes-open")
                            .label(view_label)
                            .small()
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.view_release_notes_from_banner(window, cx);
                            })),
                    )
                    .child(desktop_dismiss_button("release-notes-dismiss", dismiss_label).small().on_click(
                        cx.listener(|view, _, _, cx| {
                            view.dismiss_release_notes_banner(cx);
                        }),
                    )),
            )
            // Passive announcement: it fades in with the chrome it joins
            // instead of snapping onto an already-settled column.
            .fade_in("release-notes-banner-fade", cx.theme())
            .into_any_element()
    }
}
