//! Cached updater status: refresh requests, snapshot storage, notifications,
//! and the changelog viewer.

use super::*;

/// Which release the changelog viewer presents: the available update, or the
/// running version's own notes for the post-update "what's new" moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChangelogMode {
    Latest,
    Current,
}

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
    }

    /// Store a status snapshot and re-derive the release-notes banner. Shared
    /// by the status request and the acknowledgement response (which also
    /// carries a snapshot) without touching the status-request in-flight flag.
    pub(super) fn apply_update_status(&mut self, result: Value) {
        match serde_json::from_value::<protocol::UpdateStatusSnapshot>(result) {
            Ok(status) => {
                self.update_status = Some(status);
                // The viewer implies viewable content; a refresh that clears
                // the open mode (updated elsewhere) closes it rather than
                // stranding an empty dialog.
                if self.changelog_open && !self.can_view_changelog_in(self.changelog_mode) {
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
    /// `current_release_seen`), so notes the CLI already printed are never
    /// repeated here, and the desktop records the same marker back. Versions
    /// without notes never record, so a notes-less launch retries instead of
    /// marking seen.
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
        self.open_current_changelog(window, cx);
        self.dismiss_release_notes_banner(cx);
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

    /// Whether the viewer has an update to show. Drives the update
    /// notification and the `Latest` viewer's upgrade subtitle: both advertise
    /// an *update*, which needs both the offer and a version. Reading the
    /// changelog without one is [`Self::can_view_latest_changelog`].
    pub(super) fn can_view_changelog(&self) -> bool {
        self.update_status
            .as_ref()
            .is_some_and(|status| status.update_available && status.latest_version.is_some())
    }

    /// Whether the latest known release is readable, with or without an
    /// offered update. Drives the Settings → About row and the relaxed
    /// `Latest` viewer gate: reading notes is not the same as being offered an
    /// update. A release whose cached notes carry no Highlights section still
    /// counts — the viewer renders its link-only state.
    pub(super) fn can_view_latest_changelog(&self) -> bool {
        self.latest_changelog_version().is_some()
    }

    /// The release the `Latest` viewer would present. `None` means no reader
    /// entry point should appear.
    fn latest_changelog_version(&self) -> Option<&str> {
        self.update_status
            .as_ref()
            .filter(|status| status.latest_version.is_some())
            .and_then(|status| status.latest_version.as_deref())
    }

    /// The release a reader entry point should offer: the latest known release
    /// with notes, else the running version's own notes. Version and mode stay
    /// paired so a caller cannot name one release while the viewer opens
    /// another.
    pub(super) fn best_changelog_release(&self) -> Option<(&str, ChangelogMode)> {
        if let Some(latest) = self.latest_changelog_version() {
            return Some((latest, ChangelogMode::Latest));
        }
        let current = self.update_status.as_ref()?.current_version.as_str();
        self.can_view_current_changelog().then_some((current, ChangelogMode::Current))
    }

    /// Whether the running version's own notes are known (the post-update
    /// "what's new" moment). Unlike [`Self::can_view_changelog`], this needs
    /// no available update.
    pub(super) fn can_view_current_changelog(&self) -> bool {
        self.update_status
            .as_ref()
            .is_some_and(|status| !status.current_release_highlights.is_empty())
    }

    /// Whether the viewer can present `mode` from the stored status.
    pub(super) fn can_view_changelog_in(&self, mode: ChangelogMode) -> bool {
        match mode {
            ChangelogMode::Latest => self.can_view_latest_changelog(),
            ChangelogMode::Current => self.can_view_current_changelog(),
        }
    }

    /// Whether either viewer mode has content (the `/go changelog` verb).
    pub(super) fn can_view_any_changelog(&self) -> bool {
        self.can_view_latest_changelog() || self.can_view_current_changelog()
    }

    /// Open the changelog viewer for the latest known release.
    pub(super) fn open_changelog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_changelog_in(ChangelogMode::Latest, window, cx);
    }

    /// Open the viewer for the running version's own notes (post-update).
    pub(super) fn open_current_changelog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_changelog_in(ChangelogMode::Current, window, cx);
    }

    /// Open the viewer for the `/go changelog` verb and the Settings → About
    /// row: the latest known release when its notes are cached, else the
    /// running version's known notes.
    pub(super) fn open_best_changelog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.can_view_latest_changelog() {
            self.open_changelog_in(ChangelogMode::Latest, window, cx);
        } else {
            self.open_changelog_in(ChangelogMode::Current, window, cx);
        }
    }

    /// Open the viewer in `mode`. Reads `update_status` live at render time
    /// instead of snapshotting it, so a refresh that lands while the viewer
    /// is open updates the notes rather than showing stale ones.
    fn open_changelog_in(&mut self, mode: ChangelogMode, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_view_changelog_in(mode) {
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
        self.changelog_mode = mode;
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

    /// Changelog viewer: the open mode's Highlights over the shared modal
    /// scrim, with the full release notes one click away in the browser.
    /// Without cached highlights it renders a link-only state; an invalid
    /// or missing release URL hides the browser action entirely. The upgrade
    /// subtitle only makes sense for an available update, so it renders only
    /// when the presented release is one.
    pub(super) fn render_changelog_viewer(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let weak = cx.entity().downgrade();
        let status = self.update_status.as_ref();
        let current = status.map(|status| status.current_version.as_str()).unwrap_or_default();
        let latest = status.and_then(|status| status.latest_version.as_deref()).unwrap_or_default();
        let (shown_version, highlights, raw_url): (&str, &[String], Option<&str>) = match self.changelog_mode {
            ChangelogMode::Latest => (
                latest,
                status.map(|status| status.release_highlights.as_slice()).unwrap_or_default(),
                status.and_then(|status| status.release_url.as_deref()),
            ),
            ChangelogMode::Current => (
                current,
                status
                    .map(|status| status.current_release_highlights.as_slice())
                    .unwrap_or_default(),
                status.and_then(|status| status.current_release_url.as_deref()),
            ),
        };
        let release_url = raw_url
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
                            .child(rust_i18n::t!("app.changelog.title", version = shown_version).to_string()),
                    )
                    .when(self.changelog_mode == ChangelogMode::Latest && self.can_view_changelog(), |dialog| {
                        // Only an offered upgrade reads as "current → latest";
                        // reading a release's notes without one would render a
                        // backwards or self-referential arrow.
                        dialog.child(
                            div()
                                .id("changelog-subtitle")
                                .test_support()
                                .text_sm()
                                .text_color(colors.muted_foreground)
                                .child(
                                    rust_i18n::t!("app.changelog.subtitle", current = current, latest = latest)
                                        .to_string(),
                                ),
                        )
                    })
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
                                desktop_glass_popover_primary_button("close-changelog", &colors, false)
                                    .label(rust_i18n::t!("app.changelog.done").to_string())
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.close_changelog(window, cx);
                                    })),
                            ),
                    ),
            )
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
                    .border_color(colors.border)
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
                    .child(desktop_button("release-notes-open").label(view_label).ghost().small().on_click(
                        cx.listener(|view, _, window, cx| {
                            view.view_release_notes_from_banner(window, cx);
                        }),
                    ))
                    .child(desktop_dismiss_button("release-notes-dismiss", dismiss_label).small().on_click(
                        cx.listener(|view, _, _, cx| {
                            view.dismiss_release_notes_banner(cx);
                        }),
                    )),
            )
            .into_any_element()
    }
}
