use super::*;

const NOTICES_FILE: &str = "THIRD-PARTY-NOTICES";

fn notices_directory(executable: &Path, workspace: &Path) -> PathBuf {
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
    pub(super) fn render_licenses_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let title = rust_i18n::t!(i18n::LICENSES_TITLE).to_string();
        accessible_group_box(
            "licenses-settings-group",
            title.clone(),
            settings_group_box(title.clone(), &colors).child(settings_row(
                title,
                rust_i18n::t!("app.settings.licenses.description").to_string(),
                desktop_button("open-source-licenses")
                    .label(rust_i18n::t!("app.settings.licenses.view").to_string())
                    .accessibility_label(rust_i18n::t!(i18n::LICENSES_TITLE).to_string())
                    .on_click(cx.listener(|_, _, window, cx| {
                        cx.spawn_in(window, async move |view, cx| {
                            let result = cx
                                .background_spawn(async move {
                                    let executable = std::env::current_exe().map_err(|error| {
                                        rust_i18n::t!("app.settings.licenses.locate_failed", error = error.to_string())
                                            .to_string()
                                    })?;
                                    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
                                    let directory = notices_directory(&executable, &workspace);
                                    files::open_in_editor(&directory, NOTICES_FILE, files::ExternalEditor::System)
                                })
                                .await;
                            if let Err(error) = result {
                                let _updated = view.update_in(cx, |_, window, cx| {
                                    window.push_notification(
                                        gpui_kit::component::notification::Notification::error(
                                            rust_i18n::t!("app.settings.licenses.error", error = error).to_string(),
                                        )
                                        .placement(Anchor::BottomRight),
                                        cx,
                                    );
                                });
                            }
                        })
                        .detach();
                    })),
                &colors,
            )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_resolve_to_bundle_or_development_workspace() {
        let workspace = Path::new("/workspace");
        assert_eq!(
            notices_directory(Path::new("/Apps/VT Code.app/Contents/MacOS/vtcode-desktop"), workspace),
            PathBuf::from("/Apps/VT Code.app/Contents/Resources")
        );
        assert_eq!(notices_directory(Path::new("/tmp/Contents/MacOS/vtcode-desktop"), workspace), workspace);
        assert_eq!(notices_directory(Path::new("/workspace/target/debug/vtcode-desktop"), workspace), workspace);
    }
}
