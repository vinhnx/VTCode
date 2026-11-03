//! License and open-source disclosure: the Settings section, the bundled
//! document set, and the in-app viewer.
//!
//! First-party and third-party texts are embedded at compile time so the
//! viewer works even when `Contents/Resources` is unavailable. "Open in
//! default editor" still resolves the packaged copy first, falling back to the
//! workspace checkout during development.

use std::sync::OnceLock;

use super::*;

/// Bundled file names in the app resources directory (packaging installs the
/// same names). `Cargo.toml` declares `MIT OR Apache-2.0`.
const LICENSE_FILE: &str = "LICENSE";
const LICENSE_MIT_FILE: &str = "LICENSE-MIT";
const NOTICES_FILE: &str = "THIRD-PARTY-NOTICES";

const MIT_LICENSE: &str = include_str!("../../../../../../LICENSE-MIT");
const APACHE_LICENSE: &str = include_str!("../../../../../../LICENSE");
const THIRD_PARTY_NOTICES: &str = include_str!("../../../../../../THIRD-PARTY-NOTICES");

/// Section markers written by `scripts/generate-notices.sh`. The notices file
/// is one document with three parts; the viewer exposes each part directly.
const DEPENDENCIES_MARKER: &str = "PART I — CRATES.IO / GIT DEPENDENCIES";
const LICENSE_TEXTS_MARKER: &str = "PART II — LICENSE TEXTS";

/// One readable document in the viewer rail, grouped by owner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum LicenseDocument {
    #[default]
    Mit,
    Apache,
    Notices,
    Dependencies,
    LicenseTexts,
}

impl LicenseDocument {
    pub(super) const ALL: [Self; 5] = [
        Self::Mit,
        Self::Apache,
        Self::Notices,
        Self::Dependencies,
        Self::LicenseTexts,
    ];

    pub(super) fn is_first_party(self) -> bool {
        matches!(self, Self::Mit | Self::Apache)
    }

    /// Packaged file backing this document.
    pub(super) fn file_name(self) -> &'static str {
        match self {
            Self::Mit => LICENSE_MIT_FILE,
            Self::Apache => LICENSE_FILE,
            Self::Notices | Self::Dependencies | Self::LicenseTexts => NOTICES_FILE,
        }
    }

    /// Stable element id for the rail entry.
    pub(super) fn element_id(self) -> &'static str {
        match self {
            Self::Mit => "license-document-mit",
            Self::Apache => "license-document-apache",
            Self::Notices => "license-document-notices",
            Self::Dependencies => "license-document-dependencies",
            Self::LicenseTexts => "license-document-license-texts",
        }
    }

    /// Display label. License names stay untranslated: they are legal names.
    pub(super) fn label(self) -> String {
        match self {
            Self::Mit => "MIT License".to_string(),
            Self::Apache => "Apache License 2.0".to_string(),
            Self::Notices => rust_i18n::t!("app.licenses.notices").to_string(),
            Self::Dependencies => rust_i18n::t!("app.licenses.dependencies").to_string(),
            Self::LicenseTexts => rust_i18n::t!("app.licenses.license_texts").to_string(),
        }
    }

    /// Document body. A notices part is empty when its marker is missing, and
    /// the rail hides empty entries.
    pub(super) fn text(self) -> &'static str {
        match self {
            Self::Mit => MIT_LICENSE,
            Self::Apache => APACHE_LICENSE,
            Self::Notices => notices_parts().overview,
            Self::Dependencies => notices_parts().dependencies,
            Self::LicenseTexts => notices_parts().license_texts,
        }
    }

    pub(super) fn is_available(self) -> bool {
        !self.text().trim().is_empty()
    }
}

/// `THIRD-PARTY-NOTICES` split at its generated part markers.
struct NoticesParts {
    overview: &'static str,
    dependencies: &'static str,
    license_texts: &'static str,
}

fn notices_parts() -> &'static NoticesParts {
    static PARTS: OnceLock<NoticesParts> = OnceLock::new();
    PARTS.get_or_init(|| split_notices(THIRD_PARTY_NOTICES))
}

/// Split the notices at [`DEPENDENCIES_MARKER`] and [`LICENSE_TEXTS_MARKER`].
/// Without markers the whole file stays the overview so hand-edited or future
/// formats still render as one document. Every offset comes from `find`, which
/// returns a char boundary; `get` is used because this crate denies
/// `clippy::string_slice`, so the split stays panic-free by construction.
fn split_notices(text: &'static str) -> NoticesParts {
    let Some(dependencies_at) = text.find(DEPENDENCIES_MARKER) else {
        return NoticesParts {
            overview: text,
            dependencies: "",
            license_texts: "",
        };
    };
    let license_texts_at = text
        .get(dependencies_at..)
        .and_then(|rest| rest.find(LICENSE_TEXTS_MARKER))
        .map_or(text.len(), |offset| dependencies_at + offset);
    NoticesParts {
        overview: text.get(..dependencies_at).unwrap_or(text),
        dependencies: text.get(dependencies_at..license_texts_at).unwrap_or_default(),
        license_texts: text.get(license_texts_at..).unwrap_or_default(),
    }
}

/// Viewer state: the active document and its scroll position. The error is
/// scoped to the external-open action, so switching documents clears it.
#[derive(Default)]
pub(super) struct LicenseViewerState {
    document: LicenseDocument,
    scroll: ScrollHandle,
    open_error: Option<String>,
}

impl LicenseViewerState {
    fn new(document: LicenseDocument) -> Self {
        Self { document, ..Self::default() }
    }

    pub(super) fn document(&self) -> LicenseDocument {
        self.document
    }

    #[cfg(test)]
    pub(super) fn set_open_error(&mut self, error: Option<String>) {
        self.open_error = error;
    }

    #[cfg(test)]
    pub(super) fn open_error(&self) -> Option<&str> {
        self.open_error.as_deref()
    }
}

/// Installed documents live beside the executable in a packaged app and in the
/// workspace root during development.
fn resource_directory(executable: &Path, workspace: &Path) -> PathBuf {
    if let Some(contents) = executable
        .parent()
        .filter(|dir| dir.file_name().is_some_and(|name| name == bundle::MACOS_DIR))
        .and_then(Path::parent)
        .filter(|dir| dir.file_name().is_some_and(|name| name == bundle::CONTENTS_DIR))
        .filter(|dir| {
            dir.parent()
                .and_then(Path::extension)
                .is_some_and(|ext| ext == bundle::APP_EXTENSION)
        })
    {
        return contents.join(bundle::RESOURCES_DIR);
    }
    workspace.to_path_buf()
}

impl DesktopView {
    pub(super) fn open_license_viewer(
        &mut self,
        document: LicenseDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.disarm_double_escape();
        // The About dialog unmounts its buttons as it closes, so it must not
        // become the restore target; fall back to the composer instead.
        let return_focus = if self.about_open { None } else { window.focused(cx) };
        self.about_open = false;
        if self.license_viewer.is_none() {
            self.license_return_focus = return_focus;
        }
        // A launch from a previous viewer instance must not publish into this
        // one, even when it targets the same document.
        self.license_open_task = None;
        self.license_viewer = Some(LicenseViewerState::new(document));
        self.license_viewer_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_license_viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.disarm_double_escape();
        if self.license_viewer.take().is_none() {
            return;
        }
        self.license_open_task = None;
        if let Some(focus) = self.license_return_focus.take() {
            focus.focus(window, cx);
        } else {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    /// Drop viewer state when leaving for a route or overlay that replaces it,
    /// without stealing focus from the destination.
    pub(super) fn discard_license_viewer(&mut self) {
        self.license_viewer = None;
        self.license_return_focus = None;
        self.license_open_task = None;
    }

    pub(super) fn select_license_document(
        &mut self,
        document: LicenseDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(viewer) = self.license_viewer.as_mut() else {
            return;
        };
        if viewer.document == document {
            return;
        }
        viewer.document = document;
        viewer.open_error = None;
        viewer.scroll = ScrollHandle::default();
        self.license_open_task = None;
        self.license_viewer_focus.focus(window, cx);
        cx.notify();
    }

    fn open_license_in_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(document) = self.license_viewer.as_ref().map(LicenseViewerState::document) else {
            return;
        };
        // Stored, not detached: replacing the task supersedes an older launch
        // so only the latest completion can publish or clear the error notice.
        self.license_open_task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = cx
                .background_spawn(async move {
                    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
                    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
                    let directory = resource_directory(&executable, &workspace);
                    files::open_in_editor(&directory, document.file_name(), files::ExternalEditor::System)
                })
                .await;
            let _updated = view.update_in(cx, |view, _, cx| {
                // A completion for a document the viewer no longer shows is
                // stale; dropping it keeps the notice honest.
                if view.license_viewer.as_ref().is_none_or(|viewer| viewer.document != document) {
                    return;
                }
                if let Some(viewer) = view.license_viewer.as_mut() {
                    viewer.open_error = result.err().map(|error| {
                        rust_i18n::t!("app.licenses.open_error", document = document.file_name(), error = error)
                            .to_string()
                    });
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn render_licenses_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let title = rust_i18n::t!(i18n::LICENSES_TITLE).to_string();
        accessible_group_box(
            "licenses-settings-group",
            title.clone(),
            settings_group_box(title, &colors)
                .child(settings_row(
                    rust_i18n::t!("app.licenses.first_party_title").to_string(),
                    rust_i18n::t!("app.licenses.first_party_description").to_string(),
                    desktop_button("open-vtcode-license")
                        .label(rust_i18n::t!(i18n::LICENSES_VIEW).to_string())
                        .accessibility_label(rust_i18n::t!("app.licenses.first_party_title").to_string())
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.open_license_viewer(LicenseDocument::Mit, window, cx)
                        })),
                    &colors,
                ))
                .child(settings_row(
                    rust_i18n::t!("app.licenses.third_party_title").to_string(),
                    rust_i18n::t!("app.licenses.third_party_description").to_string(),
                    desktop_button("open-third-party-notices")
                        .label(rust_i18n::t!(i18n::LICENSES_VIEW).to_string())
                        .accessibility_label(rust_i18n::t!("app.licenses.third_party_title").to_string())
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.open_license_viewer(LicenseDocument::Notices, window, cx)
                        })),
                    &colors,
                )),
        )
    }

    /// In-app document viewer: a grouped section rail beside one scrollable
    /// text surface, with Copy and a secondary external-open action.
    pub(super) fn render_license_viewer(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let mono = theme.mono_font_family.clone();
        let mono_size = theme.mono_font_size;
        let Some(viewer) = self.license_viewer.as_ref() else {
            return div().into_any_element();
        };
        let document = viewer.document;
        let text = document.text();
        let rail = div()
            .id("license-viewer-rail")
            .test_support()
            .role(gpui_kit::Role::Navigation)
            .aria_label(rust_i18n::t!(i18n::LICENSES_TITLE).to_string())
            .w(px(200.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_1()
            .overflow_y_scroll();
        let mut entries: Vec<AnyElement> = Vec::new();
        for first_party in [true, false] {
            if !first_party {
                entries.push(pane_divider("license-viewer-rail-divider", false, colors));
            }
            entries.push(nav_group_header(
                if first_party {
                    "license-viewer-group-vtcode"
                } else {
                    "license-viewer-group-third-party"
                },
                if first_party {
                    app::NAME.to_string()
                } else {
                    rust_i18n::t!("app.licenses.third_party_group").to_string()
                },
                colors,
            ));
            for candidate in LicenseDocument::ALL
                .into_iter()
                .filter(|candidate| candidate.is_first_party() == first_party && candidate.is_available())
            {
                let active = candidate == document;
                let label = candidate.label();
                entries.push(
                    desktop_button(candidate.element_id())
                        .accessibility_label(label.clone())
                        .child(div().flex_1().min_w_0().truncate().child(label))
                        .ghost()
                        .small()
                        .w_full()
                        .justify_start()
                        .toggled(active)
                        // The rail sits on the viewer's popover surface, so the
                        // selected entry fills with `secondary` (the inverse of
                        // `desktop_selected`, which is for window-background
                        // rails) or the highlight matches the panel exactly.
                        .when(active, |button| button.bg(colors.secondary).text_color(colors.foreground))
                        .on_click(
                            cx.listener(move |view, _, window, cx| view.select_license_document(candidate, window, cx)),
                        )
                        .into_any_element(),
                );
            }
        }
        let rail = rail.children(entries);
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .px_4()
            .pb_4()
            .pt(px(40.))
            .bg(modal_scrim(colors))
            .child(
                div()
                    .id("license-viewer-panel")
                    .test_support()
                    .track_focus(&self.license_viewer_focus)
                    .role(gpui_kit::Role::Dialog)
                    .accessibility_id("license-viewer")
                    .aria_label(rust_i18n::t!(i18n::LICENSES_TITLE).to_string())
                    .aria_description(rust_i18n::t!("app.licenses.viewer_description").to_string())
                    .w(px(980.))
                    .max_w_full()
                    .h(px(720.))
                    .max_h_full()
                    .flex()
                    .flex_col()
                    .rounded_md()
                    .bg(colors.popover)
                    .border_1()
                    .border_color(colors.border)
                    .shadow_md()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .flex_wrap()
                            .gap_2()
                            .px_4()
                            .py_3()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(eyebrow_label(
                                        if document.is_first_party() {
                                            app::NAME.to_string()
                                        } else {
                                            rust_i18n::t!("app.licenses.third_party_group").to_string()
                                        },
                                        colors,
                                    ))
                                    .child(
                                        div()
                                            .id("license-viewer-title")
                                            .test_support()
                                            .truncate()
                                            .text_base()
                                            .font_semibold()
                                            .text_color(colors.foreground)
                                            .child(document.label()),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_xs()
                                            .font_family(mono.clone())
                                            .text_color(colors.muted_foreground)
                                            .child(document.file_name()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        desktop_button("copy-license-document")
                                            .label(rust_i18n::t!("app.licenses.copy").to_string())
                                            .ghost()
                                            .small()
                                            .on_click(move |_, window, app| {
                                                app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                                    text.to_string(),
                                                ));
                                                super::super::notifications::copied_to_clipboard(window, app);
                                            }),
                                    )
                                    .child(
                                        desktop_button("open-license-in-editor")
                                            .label(rust_i18n::t!("app.licenses.open_in_editor").to_string())
                                            .ghost()
                                            .small()
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                view.open_license_in_editor(window, cx)
                                            })),
                                    )
                                    .child(
                                        desktop_button("close-license-viewer")
                                            .label(rust_i18n::t!("app.licenses.close").to_string())
                                            .ghost()
                                            .small()
                                            .on_click(
                                                cx.listener(|view, _, window, cx| {
                                                    view.close_license_viewer(window, cx)
                                                }),
                                            ),
                                    ),
                            ),
                    )
                    .when_some(viewer.open_error.as_ref(), |panel, error| {
                        panel.child(warning_alert("license-viewer-error", error.clone(), colors))
                    })
                    .child(pane_divider("license-viewer-header-divider", false, colors))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .px_2()
                            .py_3()
                            .child(rail)
                            .child(pane_divider("license-viewer-body-divider", true, colors))
                            .child(
                                div()
                                    .id("license-viewer-body")
                                    .lock_scroll_axis()
                                    .test_support()
                                    .flex_1()
                                    .min_h_0()
                                    .min_w_0()
                                    .track_scroll(&viewer.scroll)
                                    .overflow_y_scroll()
                                    .vertical_scrollbar(&viewer.scroll)
                                    .px_4()
                                    .child(
                                        div()
                                            .id("license-document-text")
                                            .test_support()
                                            .text_xs()
                                            .font_family(mono)
                                            .text_size(mono_size)
                                            .text_color(colors.foreground)
                                            .child(text),
                                    ),
                            ),
                    )
                    .focus_trap("license-viewer", &self.license_viewer_focus),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_directory_resolves_to_bundle_or_development_workspace() {
        let workspace = Path::new("/workspace");
        assert_eq!(
            resource_directory(Path::new("/Apps/VT Code.app/Contents/MacOS/vtcode-desktop"), workspace),
            PathBuf::from("/Apps/VT Code.app/Contents/Resources")
        );
        assert_eq!(resource_directory(Path::new("/tmp/Contents/MacOS/vtcode-desktop"), workspace), workspace);
        assert_eq!(resource_directory(Path::new("/workspace/target/debug/vtcode-desktop"), workspace), workspace);
    }

    #[test]
    fn embedded_first_party_documents_match_their_declarations() {
        assert!(MIT_LICENSE.contains("MIT License"), "LICENSE-MIT carries the MIT text");
        assert!(MIT_LICENSE.contains("Copyright (c) 2025 Vinh Nguyen"), "MIT keeps the copyright line");
        assert!(APACHE_LICENSE.contains("Apache License"), "LICENSE carries the Apache text");
        assert_eq!(LicenseDocument::Mit.text(), MIT_LICENSE);
        assert_eq!(LicenseDocument::Apache.text(), APACHE_LICENSE);
        assert_eq!(LicenseDocument::Mit.file_name(), "LICENSE-MIT");
        assert_eq!(LicenseDocument::Apache.file_name(), "LICENSE");
    }

    #[test]
    fn notices_split_into_overview_dependencies_and_license_texts() {
        assert!(THIRD_PARTY_NOTICES.contains(DEPENDENCIES_MARKER), "generated notices keep the PART I marker");
        assert!(THIRD_PARTY_NOTICES.contains(LICENSE_TEXTS_MARKER), "generated notices keep the PART II marker");
        let parts = notices_parts();
        assert!(parts.overview.starts_with("THIRD-PARTY NOTICES"), "manual header comes first");
        assert!(parts.dependencies.starts_with(DEPENDENCIES_MARKER));
        assert!(parts.license_texts.starts_with(LICENSE_TEXTS_MARKER));
        assert_eq!(
            parts.overview.len() + parts.dependencies.len() + parts.license_texts.len(),
            THIRD_PARTY_NOTICES.len(),
            "the split covers every byte"
        );
        for document in [
            LicenseDocument::Notices,
            LicenseDocument::Dependencies,
            LicenseDocument::LicenseTexts,
        ] {
            assert!(document.is_available(), "{document:?} renders");
            assert_eq!(document.file_name(), NOTICES_FILE);
        }
    }

    #[test]
    fn notices_without_markers_fall_back_to_one_document() {
        let parts = split_notices("HAND-WRITTEN NOTICES\nno generated parts");
        assert_eq!(parts.overview, "HAND-WRITTEN NOTICES\nno generated parts");
        assert!(parts.dependencies.is_empty(), "no PART I marker leaves the dependencies part empty");
        assert!(parts.license_texts.is_empty(), "no PART II marker leaves the license-texts part empty");
    }

    #[test]
    fn first_party_documents_group_ahead_of_the_notices() {
        assert_eq!(LicenseDocument::ALL.map(LicenseDocument::is_first_party), [true, true, false, false, false]);
        assert!(
            LicenseDocument::ALL
                .iter()
                .all(|document| document.element_id().starts_with("license-document-"))
        );
        assert_eq!(LicenseDocument::default(), LicenseDocument::Mit);
    }
}
