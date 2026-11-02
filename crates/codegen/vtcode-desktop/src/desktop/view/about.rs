//! About dialog: the native macOS About panel, with a themed fallback.
//!
//! Production opens AppKit's standard About panel (name, icon, version, and
//! copyright all come from the bundle's Info.plist); the custom dialog below
//! only covers tests and the off-main-thread edge, styled after the native
//! panel (bare icon, plain version line, copyright) with the project actions.
use super::bootstrap::HELP_URL;
use super::*;

/// Present AppKit's standard About panel. Returns false off the main thread
/// or when AppKit is unavailable; callers fall back to the custom dialog.
#[cfg(not(test))]
fn show_native_about_panel() -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    app.orderFrontStandardAboutPanel(None);
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

    /// Centered brand card over the shared modal scrim: logo tile, name,
    /// version chip, then project pointers. Escape dismisses (topmost layer).
    pub(super) fn render_about(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
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
                            .child(rust_i18n::t!("app.about.version", version = env!("CARGO_PKG_VERSION")).to_string()),
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
                                desktop_button("about-license")
                                    .label(rust_i18n::t!("app.licenses.about_license").to_string())
                                    .ghost()
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.open_license(window, cx);
                                    })),
                            )
                            .child(
                                desktop_button("about-acknowledgments")
                                    .label(rust_i18n::t!("app.about.acknowledgments").to_string())
                                    .ghost()
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.open_acknowledgments(window, cx);
                                    })),
                            )
                            .child(
                                desktop_button("about-website")
                                    .label(rust_i18n::t!("app.menus.help_website").to_string())
                                    .ghost()
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.close_about(window, cx);
                                        cx.open_url(HELP_URL);
                                    })),
                            )
                            .child(
                                desktop_button("about-done")
                                    .label(rust_i18n::t!("app.about.done").to_string())
                                    .primary()
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.close_about(window, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}
