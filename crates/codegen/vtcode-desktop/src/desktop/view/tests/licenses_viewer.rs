use super::*;

use super::super::licenses::{LicenseDocument, LicenseViewerState};

/// Open Settings on the VT Code tab, where the legal rows live.
fn open_via_settings(window: AnyWindowHandle, view: &Entity<DesktopView>, cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
            view.settings_tab = super::super::settings::SettingsTab::VtCode;
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

/// Scroll the settings page until `id` sits fully inside the content viewport,
/// the way a reader reaches the legal rows below the fold.
fn scroll_settings_into_view(
    window: AnyWindowHandle,
    view: &Entity<DesktopView>,
    id: &'static str,
    cx: &mut TestAppContext,
) {
    for _ in 0..80 {
        let step = cx
            .update_window(window, |_, window, _| {
                let viewport = window.find("settings-content").bounds();
                let item = window.find(id).bounds();
                if item.top() >= viewport.top() - px(1.) && item.bottom() <= viewport.bottom() + px(1.) {
                    return None;
                }
                // Negative y scrolls down (GPUI clamps the offset to
                // `-max_offset..=0`), which is the direction below-the-fold
                // rows live in.
                Some(if item.bottom() > viewport.bottom() {
                    -px(160.)
                } else {
                    px(160.)
                })
            })
            .unwrap();
        let Some(step) = step else {
            return;
        };
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                let mut offset = view.settings_scroll.offset();
                offset.y += step;
                view.settings_scroll.set_offset(offset);
                cx.notify();
            });
            window.render_frame(cx);
        })
        .unwrap();
    }
    panic!("{id} did not scroll into the settings viewport");
}

fn open_viewer(
    window: AnyWindowHandle,
    view: &Entity<DesktopView>,
    document: LicenseDocument,
    cx: &mut TestAppContext,
) {
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_license_viewer(document, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_rows_open_the_viewer_at_their_document(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_via_settings(window, &view, cx);
    scroll_settings_into_view(window, &view, "open-vtcode-license", cx);
    cx.update_window(window, |_, window, cx| {
        assert_eq!(window.find("open-vtcode-license").label(), Some("VT Code license"));
        assert_eq!(window.find("open-vtcode-license").role(), Some(gpui_kit::Role::Button));
        window.click("open-vtcode-license", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(view.read(cx).license_viewer.as_ref().map(LicenseViewerState::document), Some(LicenseDocument::Mit));
    });

    cx.update_window(window, |_, window, cx| {
        window.click("close-license-viewer", cx);
        window.render_frame(cx);
    })
    .unwrap();
    scroll_settings_into_view(window, &view, "open-third-party-notices", cx);
    cx.update_window(window, |_, window, cx| {
        window.click("open-third-party-notices", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).license_viewer.as_ref().map(LicenseViewerState::document),
            Some(LicenseDocument::Notices)
        );
    });
}

#[gpui_kit::test]
fn viewer_groups_first_party_and_third_party_documents(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_viewer(window, &view, LicenseDocument::Mit, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).license_viewer.is_some(), "open_license_viewer records the viewer state");
        assert!(window.try_find("license-viewer").is_some(), "the viewer opens");
        assert_eq!(window.find("license-viewer-rail").role(), Some(gpui_kit::Role::Navigation));
        assert_eq!(window.find("license-document-mit").checked(), Some(true), "MIT is the active document");
        assert_eq!(window.find("license-document-apache").checked(), Some(false));
        for id in [
            "license-viewer-group-vtcode",
            "license-viewer-group-third-party",
            "license-document-mit",
            "license-document-apache",
            "license-document-notices",
            "license-document-dependencies",
            "license-document-license-texts",
        ] {
            assert!(window.try_find(id).is_some(), "{id} must render in the rail");
        }
        window.click("license-document-apache", cx);
        window.render_frame(cx);
        assert_eq!(window.find("license-document-apache").checked(), Some(true), "the rail follows the click");
        assert_eq!(window.find("license-document-mit").checked(), Some(false));
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).license_viewer.as_ref().map(LicenseViewerState::document),
            Some(LicenseDocument::Apache)
        );
    });
}

#[gpui_kit::test]
fn copy_copies_the_active_document(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_viewer(window, &view, LicenseDocument::Mit, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("copy-license-document", cx);
    })
    .unwrap();
    cx.update(|cx| {
        let copied = cx.read_from_clipboard().expect("the MIT text lands on the clipboard");
        assert!(copied.text().is_some_and(|text| text.starts_with("MIT License")));
    });
}

#[gpui_kit::test]
fn escape_closes_the_viewer_and_restores_the_composer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // The viewer restores whatever was focused when it opened; focus the
    // composer first so the assertion is about the restore target, not luck.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.focus_composer(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    open_viewer(window, &view, LicenseDocument::Mit, cx);
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::DismissOverlay), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).license_viewer.is_none(), "Escape closes the viewer");
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "closing restores composer focus"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn navigation_into_settings_drops_the_viewer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_viewer(window, &view, LicenseDocument::Mit, cx);
    cx.update_window(window, |_, window, cx| {
        // The `/config` slash command reaches Settings while the viewer is
        // still open; the page must not render under a stale overlay.
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).license_viewer.is_none(), "Settings navigation drops the viewer");
        assert!(window.try_find("license-viewer").is_none(), "no viewer layer renders over the page");
    })
    .unwrap();
}

#[gpui_kit::test]
fn viewer_shows_an_error_scoped_to_the_external_open_action(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_viewer(window, &view, LicenseDocument::Mit, cx);
    cx.update(|cx| {
        view.update(cx, |view, cx| {
            if let Some(viewer) = view.license_viewer.as_mut() {
                viewer.set_open_error(Some("could not launch".into()));
            }
            cx.notify();
        });
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("license-viewer-error").label(), Some("could not launch"));
        window.click("license-document-apache", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(
            view.read(cx)
                .license_viewer
                .as_ref()
                .is_some_and(|viewer| viewer.open_error().is_none()),
            "switching documents clears the scoped error"
        );
    });
}

#[gpui_kit::test]
fn clicking_outside_closes_the_license_viewer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    open_viewer(window, &view, LicenseDocument::Mit, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("license-viewer").is_some());
        // The 980x720 viewer covers the composer, so click-away presses the
        // toolbar above the panel; the guard keeps the oracle honest if the
        // layout ever drifts the target inside the panel.
        let panel = window.find("license-viewer").bounds();
        let target = window.find("navigate-back").bounds();
        assert!(
            target.origin.y + target.size.height <= panel.origin.y,
            "click target must sit above the panel: target={target:?}, panel={panel:?}"
        );
        window.click("navigate-back", cx);
        window.render_frame(cx);
        assert!(window.try_find("license-viewer").is_none(), "clicking away closes the viewer");
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).license_viewer.is_none(), "viewer state must be closed");
    });
}

#[gpui_kit::test]
fn license_viewer_traps_tab_from_rail_to_footer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_viewer(window, &view, LicenseDocument::Mit, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("license-viewer").is_some(), "the viewer opens");
        // Tab order runs the document rail first (VT Code group first),
        // then the footer actions.
        assert_tab_trapped_within_dialog(window, "license-document-mit", "close-license-viewer", cx);
    })
    .unwrap();
}
