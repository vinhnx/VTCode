//! About dialog: the native macOS About panel, with a themed fallback.
//!
//! Production opens AppKit's standard About panel with the app name, marketing
//! version, and build commit passed explicitly (an unbundled dev launch has no
//! Info.plist to read); its credits come from `Credits.html` in the bundle. The
//! custom dialog below only covers tests and the off-main-thread edge, styled
//! after the native panel (bare icon, plain version line, copyright) with the
//! project actions.
use super::bootstrap::HELP_URL;
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

    /// Settings → About page: brand identity plus the license, acknowledgments,
    /// and project link the native About panel exposes.
    pub(super) fn render_about_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        Self::render_about_settings_content(cx.entity(), cx)
    }

    /// App-context content builder, hostable inside a Kit `SettingItem`
    /// closure. The rows open the shared license viewer through the view
    /// handle; the website action is a plain external launch.
    pub(super) fn render_about_settings_content(view: Entity<Self>, cx: &mut App) -> AnyElement {
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
            .child(brand_logo(64.))
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
                    .child(brand_logo(64.))
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
                                desktop_primary_button("about-done")
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
