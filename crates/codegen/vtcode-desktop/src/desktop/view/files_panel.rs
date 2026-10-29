//! Workspace files: scanning, tree, attachments, and the preview overlay.

use super::*;

struct FileScanCancellationGuard(Arc<AtomicBool>);

impl Drop for FileScanCancellationGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl DesktopView {
    /// Pick workspace text files to attach to the next prompt.
    pub(super) fn attach_files(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        const MAX_ATTACHMENTS: usize = 5;

        if self.attachments_picker_open || self.workspace.is_none() || self.session_request.is_some() {
            return;
        }
        self.attachments_picker_open = true;
        let Some(workspace) = self.workspace.clone() else {
            self.attachments_picker_open = false;
            return;
        };
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach files".into()),
        });
        cx.notify();
        let session_id = self.selected_session_id.clone();
        self.attachments_task = Some(cx.spawn(async move |view, cx| {
            let result = picked.await;
            let result = match result {
                Ok(Ok(Some(paths))) => {
                    let root = workspace.clone();
                    // Bound both work and retained data, even for a very large native selection.
                    Ok(cx
                        .background_spawn(async move {
                            let overflow = paths.len() > MAX_ATTACHMENTS;
                            let results = paths
                                .into_iter()
                                .take(MAX_ATTACHMENTS)
                                .map(|path| files::read_attachment(&root, &path))
                                .collect::<Vec<_>>();
                            (results, overflow)
                        })
                        .await)
                }
                Ok(Ok(None)) => Ok((Vec::new(), false)),
                Ok(Err(error)) => Err(format!("Unable to open the file picker: {error}")),
                Err(_) => Err("The file picker was disconnected.".to_string()),
            };
            let _updated = view.update(cx, |view, cx| {
                view.attachments_picker_open = false;
                if view.workspace.as_ref() != Some(&workspace) || view.selected_session_id != session_id {
                    cx.notify();
                    return;
                }
                match result {
                    Err(error) => view.error = Some(error),
                    Ok((results, overflow)) => {
                        if overflow {
                            view.error = Some(format!("At most {MAX_ATTACHMENTS} attachments per prompt."));
                        }
                        for result in results {
                            match result {
                                Ok(attachment) => {
                                    if view.attachments.iter().any(|existing| existing.path == attachment.path) {
                                        continue;
                                    }
                                    if view.attachments.len() >= MAX_ATTACHMENTS {
                                        view.error = Some(format!("At most {MAX_ATTACHMENTS} attachments per prompt."));
                                        break;
                                    }
                                    view.attachments.push(attachment);
                                }
                                Err(error) => view.error = Some(error),
                            }
                        }
                    }
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn remove_attachment(&mut self, path: String, cx: &mut Context<Self>) {
        self.attachments.retain(|attachment| attachment.path != path);
        cx.notify();
    }

    pub(super) fn refresh_files(&mut self, cx: &mut Context<Self>) {
        if let Some(previous) = self.files_scan_cancel.take() {
            previous.store(true, Ordering::Relaxed);
        }
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        let cancellation = Arc::new(AtomicBool::new(false));
        self.files_scan_cancel = Some(cancellation.clone());
        let worker_cancellation = cancellation.clone();
        self.files_task = Some(cx.spawn(async move |view, cx| {
            let _cancel_on_drop = FileScanCancellationGuard(cancellation.clone());
            let root = workspace.clone();
            let result = cx
                .background_spawn(async move {
                    let entries =
                        files::scan_workspace_cancellable(&root, || worker_cancellation.load(Ordering::Relaxed))?;
                    if worker_cancellation.load(Ordering::Relaxed) {
                        return None;
                    }
                    Some((entries, files::git_branch(&root)))
                })
                .await;
            let _updated = view.update_in(cx, |view, window, cx| {
                let is_current_request = view
                    .files_scan_cancel
                    .as_ref()
                    .is_some_and(|active| Arc::ptr_eq(active, &cancellation));
                if !is_current_request {
                    return;
                }
                view.files_scan_cancel = None;
                if view.workspace.as_ref() != Some(&workspace) {
                    return;
                }
                let Some((entries, branch)) = result else {
                    return;
                };
                view.replace_file_entries(entries, cx);
                view.git_branch = branch;
                view.update_composer_suggestions(window, cx);
                cx.notify();
            });
        }));
    }

    pub(super) fn refresh_workspace_view(&mut self, cx: &mut Context<Self>) {
        if self.review_open {
            self.refresh_workspace_review(cx);
        } else {
            self.refresh_files(cx);
        }
    }

    pub(super) fn focus_workspace_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.inspector_open = true;
        self.set_inspector_tab(InspectorTab::Files, cx);

        let view = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            let _updated = view.update(cx, |view, cx| {
                if !view.inspector_open || view.inspector_tab != InspectorTab::Files {
                    return;
                }
                view.file_tree.update(cx, |tree, cx| {
                    if tree.selected_index().is_none() && tree.entry(0).is_some() {
                        tree.set_selected_index(Some(0), cx);
                    }
                    tree.focus(window, cx);
                });
            });
        });
    }

    fn replace_file_entries(&mut self, entries: Vec<FileEntry>, cx: &mut Context<Self>) {
        let selected_path = self.file_tree.read(cx).selected_item().map(|item| item.id.to_string());
        let directory_paths: HashSet<_> = entries
            .iter()
            .filter(|entry| entry.is_dir)
            .map(|entry| entry.rel_path.clone())
            .collect();
        self.expanded_file_directories.retain(|path| directory_paths.contains(path));
        let tree_items = file_tree_items(&entries, None, &self.expanded_file_directories);
        let selected_item = selected_path
            .as_deref()
            .and_then(|path| find_file_tree_item(&tree_items, path))
            .cloned();
        self.file_tree.update(cx, |tree, cx| {
            tree.set_items(tree_items, cx);
            tree.set_selected_item(selected_item.as_ref(), cx);
        });
        self.files = entries;
        self.file_directory_paths = Arc::new(directory_paths);
    }

    pub(super) fn open_selected_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.file_tree.read(cx).selected_item().map(|item| item.id.to_string()) else {
            return;
        };
        if self.files.iter().any(|entry| entry.rel_path == path && !entry.is_dir) {
            self.open_file_preview(path, window, cx);
        }
    }

    pub(super) fn open_file_preview(&mut self, rel_path: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.preferences.file_open_destination().external_editor() {
            // External mode never shows the overlay: cancel an in-flight viewer
            // load and drop a stale preview. Selection focus stays untouched.
            self.preview_task = None;
            self.file_preview = None;
            self.preview_editor_error = None;
            self.open_file_in_editor(&rel_path, editor, window, cx);
            return;
        }
        if self.review_open {
            self.close_review(window, cx);
        }
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        if self.file_preview.is_none() {
            self.preview_return_focus = window.focused(cx);
        }
        self.preview_task = Some(cx.spawn_in(window, async move |view, cx| {
            let root = workspace.clone();
            let path = rel_path.clone();
            let preview = cx.background_spawn(async move { files::read_preview(&root, &path) }).await;
            let _updated = view.update_in(cx, |view, window, cx| {
                if view.workspace.as_ref() != Some(&workspace) {
                    return;
                }
                match preview {
                    Some(preview) => {
                        view.preview_editor_error = None;
                        view.file_preview = Some(preview);
                        view.preview_focus.focus(window, cx);
                        view.clear_error();
                    }
                    None => {
                        view.error =
                            Some(rust_i18n::t!("app.files.preview_unavailable", path = rel_path.as_str()).to_string())
                    }
                }
                cx.notify();
            });
        }));
    }

    /// Open a file directly in an external editor without the preview overlay.
    /// Unlike the viewer path this never closes the review sheet: an external
    /// launch is non-modal and must not destroy the user's open context.
    fn open_file_in_editor(
        &mut self,
        rel_path: &str,
        editor: files::ExternalEditor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        #[cfg(test)]
        if let Some(result) = self.intercept_test_launch(rel_path, editor) {
            self.apply_external_launch_result(&workspace, result, cx);
            return;
        }
        let rel_path = rel_path.to_owned();
        // Stored, not detached: replacing the task supersedes an older launch
        // so only the latest completion can publish or clear the error notice.
        self.external_launch_task = Some(cx.spawn_in(window, async move |view, cx| {
            let root = workspace.clone();
            let path = rel_path.clone();
            let result = cx
                .background_spawn(async move { files::open_in_editor(&root, &path, editor) })
                .await;
            let _updated = view.update_in(cx, |view, _, cx| {
                view.apply_external_launch_result(&workspace, result, cx);
            });
        }));
    }

    /// Shared completion for direct external launches, used by production and tests.
    pub(super) fn apply_external_launch_result(
        &mut self,
        workspace: &Path,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.as_deref() != Some(workspace) {
            return;
        }
        match result {
            Ok(()) => self.clear_error(),
            Err(error) => {
                self.error =
                    Some(rust_i18n::t!("app.file_preview.editor_error", error = error.to_string()).to_string());
            }
        }
        cx.notify();
    }

    #[cfg(test)]
    fn intercept_test_launch(&mut self, rel_path: &str, editor: files::ExternalEditor) -> Option<Result<(), String>> {
        let intercept = self.test_launch_intercept.as_mut()?;
        intercept
            .calls
            .push(ExternalLaunchCall { rel_path: rel_path.to_owned(), editor });
        Some(intercept.fail_with.clone().map_or(Ok(()), Err))
    }

    fn open_preview_in_editor(&mut self, editor: files::ExternalEditor, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(workspace), Some(preview)) = (self.workspace.clone(), self.file_preview.as_ref()) else {
            return;
        };
        let rel_path = preview.rel_path.clone();
        self.preview_editor_error = None;
        cx.spawn_in(window, async move |view, cx| {
            let root = workspace.clone();
            let path = rel_path.clone();
            let result = cx
                .background_spawn(async move { files::open_in_editor(&root, &path, editor) })
                .await;
            let _updated = view.update_in(cx, |view, _, cx| {
                if view.workspace.as_ref() != Some(&workspace)
                    || view.file_preview.as_ref().is_none_or(|preview| preview.rel_path != rel_path)
                {
                    return;
                }
                view.preview_editor_error = result
                    .err()
                    .map(|error| rust_i18n::t!("app.file_preview.editor_error", error = error.to_string()).to_string());
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_file_preview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let Some(preview) = self.file_preview.as_ref() else {
            return div();
        };
        let text = preview.text.clone();
        let mono = theme.mono_font_family.clone();
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .bg(colors.background.opacity(0.6))
            .child(
                div()
                    .id("file-preview")
                    .test_support()
                    .track_focus(&self.preview_focus)
                    .role(gpui_kit::Role::Dialog)
                    .accessibility_id("file-preview")
                    .aria_label(
                        rust_i18n::t!("app.file_preview.accessibility_title", path = preview.rel_path.as_str())
                            .to_string(),
                    )
                    .aria_description(rust_i18n::t!("app.file_preview.accessibility_description").to_string())
                    .w(px(860.))
                    .max_w_full()
                    .max_h_full()
                    .flex()
                    .flex_col()
                    .rounded_md()
                    .bg(colors.secondary)
                    .shadow_md()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .flex_wrap()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .font_medium()
                                    .font_family(mono.clone())
                                    .child(preview.rel_path.clone()),
                            )
                            .child(
                                desktop_button("open-preview-editor")
                                    .label(rust_i18n::t!("app.file_preview.open_in_editor").to_string())
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.open_preview_in_editor(files::ExternalEditor::System, window, cx)
                                    })),
                            )
                            .child(desktop_button("open-preview-vscode").label("VS Code").ghost().small().on_click(
                                cx.listener(|view, _, window, cx| {
                                    view.open_preview_in_editor(files::ExternalEditor::VsCode, window, cx)
                                }),
                            ))
                            .child(
                                desktop_button("copy-preview")
                                    .label(rust_i18n::t!("app.file_preview.copy").to_string())
                                    .ghost()
                                    .small()
                                    .on_click(move |_, window, app| {
                                        app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.to_string()));
                                        super::super::notifications::copied_to_clipboard(window, app);
                                    }),
                            )
                            .child(
                                desktop_button("close-preview")
                                    .tooltip_with_action(
                                        rust_i18n::t!("app.file_preview.close_tooltip").to_string(),
                                        &DismissOverlay,
                                        Some(key_context::DESKTOP),
                                    )
                                    .label(rust_i18n::t!("app.file_preview.close").to_string())
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _, window, cx| this.close_file_preview(window, cx))),
                            ),
                    )
                    .when_some(self.preview_editor_error.as_ref(), |panel, error| {
                        panel.child(warning_alert("preview-editor-error", error.clone(), colors))
                    })
                    .child(
                        div()
                            .id("file-preview-body")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .id("file-preview-body")
                            .p_3()
                            .text_xs()
                            .font_family(mono)
                            .text_size(theme.mono_font_size)
                            .child(preview.text.clone()),
                    )
                    .when(preview.truncated, |panel| {
                        panel.child(
                            div()
                                .px_3()
                                .py_2()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(rust_i18n::t!("app.file_preview.truncated", size = "64 KiB").to_string()),
                        )
                    }),
            )
    }
}
