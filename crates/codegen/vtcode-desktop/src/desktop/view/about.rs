//! About dialog: the native macOS About panel, with a themed fallback.
//!
//! Production opens AppKit's standard About panel with the app name, marketing
//! version, and build commit passed explicitly (an unbundled dev launch has no
//! Info.plist to read); its credits come from `Credits.html` in the bundle. The
//! custom dialog below only covers tests and the off-main-thread edge, styled
//! after the native panel (bare mark, plain version line, copyright) with the
//! project actions. Both the fallback dialog and the Settings → About pane use
//! the monochrome V ([`brand_mark`]) rather than the Dock app-icon art.
use super::bootstrap::HELP_URL;
use super::storage::format_bytes;
use super::updates::ChangelogMode;
use super::*;

/// Present AppKit's standard About panel. Returns false off the main thread
/// or when AppKit is unavailable; callers fall back to the custom dialog.
#[cfg(not(test))]
#[expect(unsafe_code, reason = "AppKit About-panel options are an unsafe Objective-C call")]
fn show_native_about_panel() -> bool {
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{
        NSAboutPanelOptionApplicationName, NSAboutPanelOptionApplicationVersion, NSAboutPanelOptionVersion,
        NSApplication,
    };
    use objc2_foundation::{NSDictionary, NSString};

    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    // Pass the identity explicitly: an unbundled dev launch has no Info.plist,
    // and the build stamp must match the in-app fallback card either way.
    let name = NSString::from_str(app::NAME);
    let version = NSString::from_str(build_info::VERSION);
    let build = NSString::from_str(build_info::BUILD_VERSION);
    // SAFETY: these are immutable localized key objects published by AppKit.
    let keys: [&NSString; 3] = unsafe {
        [
            NSAboutPanelOptionApplicationName,
            NSAboutPanelOptionApplicationVersion,
            NSAboutPanelOptionVersion,
        ]
    };
    let values = NSDictionary::from_slices(&keys, &[&*name, &*version, &*build]);
    // SAFETY: `NSDictionary` is a single Objective-C class, so only the
    // generic value type differs between the two spellings.
    let values: Retained<NSDictionary<NSString, AnyObject>> = unsafe { Retained::cast_unchecked(values) };
    // SAFETY: AppKit reads the dictionary during this call and retains what it
    // needs; every value is a live `NSString` object.
    unsafe { app.orderFrontStandardAboutPanelWithOptions(&values) };
    true
}

#[cfg(test)]
fn show_native_about_panel() -> bool {
    false
}

/// Cloned render inputs for the About pane: the cached update-status line plus
/// the row's advertised release. The changelog row advertises the release a
/// reader can actually open — the newest cached one, else the running
/// version's own notes — which is independent of whether an update is offered.
/// Version and mode are captured together so the description cannot name a
/// release the viewer would not present.
#[derive(Clone)]
pub(super) struct AboutSettingsProps {
    pub(super) changelog: Option<ChangelogReleaseProps>,
    pub(super) update_status_description: String,
    /// Whether a live check is in flight, so the row can show progress and
    /// refuse a second one instead of looking inert.
    pub(super) update_check_pending: bool,
    /// Live install state. `Idle` outside an install.
    pub(super) update_install: protocol::UpdateInstallState,
    /// Whether the row offers an install: an update is available and none is
    /// running. Kept out of the state enum because it is a policy decision,
    /// not a phase.
    pub(super) can_install_update: bool,
}

/// One readable release: the version the viewer will title, plus the mode it
/// will open in. A named struct rather than a pair so a caller cannot swap the
/// two (see the repo's named-struct rule).
#[derive(Clone)]
pub(super) struct ChangelogReleaseProps {
    version: String,
    mode: ChangelogMode,
}

impl AboutSettingsProps {
    pub(super) fn capture(view: &DesktopView) -> Self {
        Self {
            changelog: view
                .best_changelog_release()
                .map(|(version, mode)| ChangelogReleaseProps { version: version.to_owned(), mode }),
            update_status_description: view.update_status_description(),
            update_check_pending: view.update_request_pending,
            update_install: view.update_install_state.clone(),
            // An install is offered only while one is not already running: the
            // service refuses a second concurrent install anyway, so offering
            // the action would just produce a rejection.
            can_install_update: view.can_offer_update_install(),
        }
    }

    /// The row's description: the live install phase while one runs, else the
    /// cached status. One line, so a long download never displaces the actions.
    pub(super) fn update_row_description(&self) -> String {
        match &self.update_install {
            protocol::UpdateInstallState::Idle => {}
            protocol::UpdateInstallState::Downloading { downloaded, total } => {
                return match total {
                    Some(total) if *total > 0 => rust_i18n::t!(
                        "app.settings.about.download_progress",
                        downloaded = format_bytes(*downloaded),
                        total = format_bytes(*total)
                    )
                    .to_string(),
                    // No Content-Length: a byte count, never a fake percentage.
                    _ => rust_i18n::t!(
                        "app.settings.about.download_progress_unknown_total",
                        downloaded = format_bytes(*downloaded)
                    )
                    .to_string(),
                };
            }
            protocol::UpdateInstallState::Verifying => {
                return rust_i18n::t!("app.settings.about.verifying").to_string();
            }
            protocol::UpdateInstallState::Extracting => {
                return rust_i18n::t!("app.settings.about.extracting").to_string();
            }
            protocol::UpdateInstallState::Installing => {
                return rust_i18n::t!("app.settings.about.replacing").to_string();
            }
            protocol::UpdateInstallState::Completed { version, needs_gatekeeper_approval } => {
                let done = format!("{} ({version})", rust_i18n::t!("app.settings.about.installed"));
                if *needs_gatekeeper_approval {
                    // Ad hoc-signed builds need a one-time manual approval
                    // before macOS launches the replaced bundle; without this
                    // the restart lands on a Gatekeeper block with no guidance.
                    return rust_i18n::t!("app.settings.about.installed_gatekeeper", done = done.as_str()).to_string();
                }
                return done;
            }
            protocol::UpdateInstallState::Failed { message } => {
                return rust_i18n::t!("app.settings.about.install_failed", message = message.as_str()).to_string();
            }
        }
        self.update_status_description.clone()
    }
}

impl DesktopView {
    pub(super) fn open_about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.disarm_double_escape();
        if show_native_about_panel() {
            return;
        }
        // Deferred popovers paint above modal scrims: the picker cannot stay open.
        self.close_attachment_picker(window, cx);
        self.about_open = true;
        self.about_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.about_open {
            return;
        }
        self.disarm_double_escape();
        self.about_open = false;
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Third-party notices: open the in-app viewer at the notices overview.
    fn open_acknowledgments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_license_viewer(licenses::LicenseDocument::Notices, window, cx);
    }

    /// First-party dual license: open the in-app viewer at the MIT text.
    fn open_license(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_license_viewer(licenses::LicenseDocument::Mit, window, cx);
    }

    /// Settings → About page: brand identity plus the update-status,
    /// release-notes, license, acknowledgments, and project link rows.
    pub(super) fn render_about_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        Self::render_about_settings_content(AboutSettingsProps::capture(self), cx.entity(), cx)
    }

    /// App-context content builder, hostable inside a Kit `SettingItem`
    /// closure. The rows open the shared license or changelog viewer through
    /// the view handle; the website action is a plain external launch.
    pub(super) fn render_about_settings_content(
        props: AboutSettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        // The page header already reads "About" and the card opens with the app
        // name, so the card stays untitled (the curated configuration overview
        // uses the same shape) instead of adding a third "About VT Code" line.
        let title = rust_i18n::t!("app.menus.about").to_string();
        let identity = div()
            .id("about-settings-identity")
            .test_support()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            // The About page presents the product identity, not the Dock
            // icon: the monochrome V follows the theme instead of shipping a
            // raster art tile that fights the card.
            .child(brand_mark("about-settings-mark", 56., colors.foreground))
            .child(
                div()
                    .text_lg()
                    .font_semibold()
                    .text_color(colors.foreground)
                    .child(app::NAME.to_string()),
            )
            .child(
                div()
                    .id("about-settings-version")
                    .test_support()
                    .text_sm()
                    .text_color(colors.foreground)
                    .child(rust_i18n::t!("app.about.version", version = build_info::display_version()).to_string()),
            )
            .child(
                div()
                    .id("about-settings-copyright")
                    .test_support()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("Copyright 2025 Vinh Nguyen · MIT OR Apache-2.0"),
            );
        div()
            .id("about-settings")
            .test_support()
            .child(accessible_group_box(
                "about-settings-group",
                title,
                settings_group_box_untitled(&colors)
                    .child(identity)
                    .child(Self::about_update_status_row(&props, &view, &colors))
                    .child(Self::about_changelog_row(&props, &view, &colors))
                    .child(Self::license_viewer_row(
                        &view,
                        "about-settings-license",
                        rust_i18n::t!("app.licenses.first_party_title").to_string(),
                        rust_i18n::t!("app.licenses.first_party_description").to_string(),
                        licenses::LicenseDocument::Mit,
                        &colors,
                    ))
                    .child(Self::license_viewer_row(
                        &view,
                        "about-settings-acknowledgments",
                        rust_i18n::t!("app.about.acknowledgments").to_string(),
                        rust_i18n::t!("app.licenses.third_party_description").to_string(),
                        licenses::LicenseDocument::Notices,
                        &colors,
                    ))
                    .child(settings_row(
                        rust_i18n::t!("app.menus.help_website").to_string(),
                        HELP_URL,
                        desktop_button("about-settings-website")
                            .label(rust_i18n::t!("app.menus.help_website").to_string())
                            .on_click(|_, _, cx: &mut App| cx.open_url(HELP_URL)),
                        &colors,
                    )),
            ))
            .into_any_element()
    }

    /// Update-status row: the cached snapshot, a live check, and — when an
    /// update is on offer — the install action itself. Release notes have
    /// their own row below, so this one carries only update actions and keeps
    /// that stable control slot in every state.
    fn about_update_status_row(
        props: &AboutSettingsProps,
        view: &Entity<Self>,
        colors: &gpui_kit::component::theme::ThemeColor,
    ) -> AnyElement {
        let busy = props.update_check_pending || props.update_install.is_in_flight();
        let check_label = if props.update_check_pending {
            rust_i18n::t!("app.settings.about.checking").to_string()
        } else {
            rust_i18n::t!("app.settings.about.check_for_updates").to_string()
        };

        let controls = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                desktop_button("about-settings-check-for-updates")
                    .label(check_label)
                    .desktop_disabled(busy)
                    .on_click({
                        let view = view.clone();
                        move |_, _: &mut Window, cx: &mut App| {
                            view.update(cx, |view, cx| {
                                view.request_update_check();
                                cx.notify();
                            });
                        }
                    }),
            )
            .when(props.can_install_update, |row| {
                row.child(
                    desktop_primary_button("about-settings-install-update")
                        .label(rust_i18n::t!("app.settings.about.install_update").to_string())
                        .on_click({
                            let view = view.clone();
                            move |_, _: &mut Window, cx: &mut App| {
                                view.update(cx, |view, cx| {
                                    view.request_update_install();
                                    cx.notify();
                                });
                            }
                        }),
                )
            });

        settings_row(
            rust_i18n::t!("app.settings.about.update_status").to_string(),
            props.update_row_description(),
            controls,
            colors,
        )
        .into_any_element()
    }
    pub(super) fn update_status_description(&self) -> String {
        let Some(status) = self.update_status.as_ref() else {
            return rust_i18n::t!("app.settings.about.status_unavailable").to_string();
        };
        match (status.update_available, status.latest_version.as_deref()) {
            (true, Some(latest)) => format!("{} {latest}", rust_i18n::t!("app.settings.about.update_available")),
            (true, None) => rust_i18n::t!("app.settings.about.status_unavailable").to_string(),
            (false, _) => {
                format!("{} ({})", rust_i18n::t!("app.settings.about.up_to_date"), status.current_version)
            }
        }
    }

    /// Whether the row should offer an install. Requires a cached offer, no
    /// install already running, and no finished install: the service refuses
    /// a concurrent install, and a completed one needs a relaunch rather
    /// than a reinstall of the same bundle.
    pub(super) fn can_offer_update_install(&self) -> bool {
        if self.update_install_state.is_in_flight() {
            return false;
        }
        if matches!(self.update_install_state, protocol::UpdateInstallState::Completed { .. }) {
            return false;
        }
        self.update_status
            .as_ref()
            .is_some_and(|status| status.update_available && status.latest_version.is_some())
    }

    /// Release-notes row: offers whichever release a reader can actually open
    /// — the newest cached one, else the running version's own notes — and so
    /// does not require an offered update. The description names that release
    /// and its mode; when nothing is cached the row stays visible but inert
    /// and explains what fills it, because hiding it would leave no hint that
    /// release notes exist at all.
    fn about_changelog_row(
        props: &AboutSettingsProps,
        view: &Entity<Self>,
        colors: &gpui_kit::component::theme::ThemeColor,
    ) -> AnyElement {
        let title = rust_i18n::t!("app.changelog.release_notes").to_string();
        let action_label = rust_i18n::t!("app.changelog.view_release_notes").to_string();
        let Some(release) = props.changelog.as_ref() else {
            return settings_row(
                title,
                rust_i18n::t!("app.changelog.unavailable").to_string(),
                desktop_button("about-settings-changelog")
                    .label(action_label)
                    .desktop_disabled(true),
                colors,
            )
            .into_any_element();
        };
        let description = match release.mode {
            ChangelogMode::Latest => {
                rust_i18n::t!("app.changelog.description", version = release.version.as_str()).to_string()
            }
            ChangelogMode::Current => {
                rust_i18n::t!("app.changelog.description_current", version = release.version.as_str()).to_string()
            }
        };
        let view = view.clone();
        settings_row(
            title.clone(),
            description,
            desktop_button("about-settings-changelog")
                .label(action_label)
                .accessibility_label(title)
                .on_click(move |_, window: &mut Window, cx: &mut App| {
                    view.update(cx, |view, cx| {
                        view.open_best_changelog(window, cx);
                    });
                }),
            colors,
        )
        .into_any_element()
    }

    /// Centered brand card over the shared modal scrim: logo tile, name,
    /// version chip, then project pointers. Escape dismisses (topmost layer).
    pub(super) fn render_about(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let weak = cx.entity().downgrade();
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
                    .id("about-dialog")
                    .test_support()
                    .track_focus(&self.about_focus)
                    .focus_trap("about-dialog", &self.about_focus)
                    // Click-away dismissal, mirroring the palette panel: a
                    // press on the scrim closes through the Done path.
                    .on_mouse_down_out(move |_, window, cx| {
                        let _updated = weak.update(cx, |view, cx| {
                            view.close_about(window, cx);
                        });
                    })
                    .role(gpui_kit::Role::Dialog)
                    .accessibility_id("about-dialog")
                    .aria_label(rust_i18n::t!("app.menus.about").to_string())
                    .w(px(420.))
                    .max_w_full()
                    .max_h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .rounded_md()
                    .bg(colors.popover)
                    .border_1()
                    .border_color(colors.border)
                    .shadow_md()
                    .px_6()
                    .pt_6()
                    .pb_5()
                    // Same ink mark as the About pane: the dialog is the
                    // identity surface, so the Dock art would read as a second
                    // brand.
                    .child(brand_mark("about-dialog-mark", 56., colors.foreground))
                    .child(
                        div()
                            .text_xl()
                            .font_semibold()
                            .text_color(colors.foreground)
                            .child(app::NAME.to_string()),
                    )
                    .child(
                        div()
                            .id("about-version")
                            .test_support()
                            .text_sm()
                            .text_color(colors.foreground)
                            .child(
                                rust_i18n::t!("app.about.version", version = build_info::display_version()).to_string(),
                            ),
                    )
                    // HIG about windows carry the copyright line, kept small
                    // and muted below the version, with the license it is
                    // released under on the same block.
                    .child(
                        div()
                            .id("about-copyright")
                            .test_support()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child("Copyright 2025 Vinh Nguyen · MIT OR Apache-2.0"),
                    )
                    .child(hairline())
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .w_full()
                            .child(
                                desktop_default_button("about-license")
                                    .label(rust_i18n::t!("app.licenses.about_license").to_string())
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.open_license(window, cx);
                                    })),
                            )
                            .child(
                                desktop_default_button("about-acknowledgments")
                                    .label(rust_i18n::t!("app.about.acknowledgments").to_string())
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.open_acknowledgments(window, cx);
                                    })),
                            )
                            .child(
                                desktop_default_button("about-website")
                                    .label(rust_i18n::t!("app.menus.help_website").to_string())
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.close_about(window, cx);
                                        cx.open_url(HELP_URL);
                                    })),
                            )
                            .child(
                                desktop_glass_popover_primary_button("about-done", &colors, false)
                                    .label(rust_i18n::t!("app.about.done").to_string())
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.close_about(window, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}
