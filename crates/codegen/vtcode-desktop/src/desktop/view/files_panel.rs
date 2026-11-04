//! Workspace files: scanning, tree, attachments, and the preview overlay.

use super::*;
use gpui_kit::Action;
use gpui_kit::component::menu::PopupMenu;

#[derive(Action, Clone, PartialEq, Debug)]
#[action(namespace = desktop, no_json)]
pub(super) struct CopyFilePath {
    pub path: gpui_kit::SharedString,
}

#[derive(Action, Clone, PartialEq, Debug)]
#[action(namespace = desktop, no_json)]
pub(super) struct OpenFilePath {
    pub path: gpui_kit::SharedString,
}

#[derive(Action, Clone, PartialEq, Debug)]
#[action(namespace = desktop, no_json)]
pub(super) struct RevealFilePath {
    pub path: gpui_kit::SharedString,
}

#[derive(Action, Clone, PartialEq, Debug)]
#[action(namespace = desktop, no_json)]
pub(super) struct RenameFilePath {
    pub path: gpui_kit::SharedString,
}

#[derive(Action, Clone, PartialEq, Debug)]
#[action(namespace = desktop, no_json)]
pub(super) struct DeleteFilePath {
    pub path: gpui_kit::SharedString,
}

/// English-literal file UI strings, shared until full native-quality
/// translations ship for every locale.
pub(super) const FILTER_FILES_LABEL: &str = "Filter files";
pub(super) const NO_MATCHING_FILES_NOTE: &str = "No files match this filter";

/// Folder/file icon for a file-tree row, following the folder's expansion.
pub(super) fn file_tree_row_icon(is_dir: bool, expanded: bool) -> IconName {
    if is_dir {
        if expanded {
            IconName::FolderOpen
        } else {
            IconName::FolderClosed
        }
    } else {
        IconName::FileText
    }
}

/// Shared file-row label: type icon, file name, and the muted parent path in
/// search mode. Both file trees render through this so rows stay identical.
pub(super) fn file_tree_row_label(
    display_name: String,
    search_parent: Option<&str>,
    icon: IconName,
    app: &mut App,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_x_2()
        .child(Icon::new(icon).small().text_color(app.theme().muted_foreground))
        .child(div().text_sm().child(display_name))
        .when_some(search_parent, |row, parent| {
            row.child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(app.theme().muted_foreground)
                    .child(parent.to_string()),
            )
        })
}

/// Right-click menu shared by every workspace-file surface (today: the
/// inspector file tree). Primary action first, mutating entries after a
/// separator. Mirrors `tabs::session_row_menu`.
pub(super) fn file_row_menu(menu: PopupMenu, rel_path: &str, is_dir: bool) -> PopupMenu {
    let menu = if is_dir {
        menu
    } else {
        menu.menu("Open", Box::new(OpenFilePath { path: rel_path.into() }))
    };
    menu.menu("Reveal in Finder", Box::new(RevealFilePath { path: rel_path.into() }))
        .menu("Copy path", Box::new(CopyFilePath { path: rel_path.into() }))
        .separator()
        .menu("Rename…", Box::new(RenameFilePath { path: rel_path.into() }))
        .menu("Delete…", Box::new(DeleteFilePath { path: rel_path.into() }))
}

struct FileScanCancellationGuard(Arc<AtomicBool>);

impl Drop for FileScanCancellationGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl DesktopView {
    /// Copy one tree row's absolute path, falling back to the workspace
    /// relative path when no workspace is set. When the file exists on disk
    /// the clipboard also carries a file entry, so pasting into the composer
    /// attaches it — mirroring a Finder copy.
    pub(super) fn copy_file_path(&self, rel_path: &str, cx: &mut App) {
        let absolute = self.workspace.as_ref().map(|root| root.join(rel_path).display().to_string());
        let file_exists = absolute
            .as_deref()
            .is_some_and(|path| std::fs::metadata(path).is_ok_and(|meta| meta.is_file()));
        let entries = match (file_exists, absolute.as_deref()) {
            (true, Some(absolute)) => vec![
                gpui_kit::ClipboardEntry::String(gpui_kit::ClipboardString::new(absolute.to_string())),
                gpui_kit::ClipboardEntry::ExternalPaths(ExternalPaths([PathBuf::from(absolute)].into_iter().collect())),
            ],
            _ => vec![gpui_kit::ClipboardEntry::String(gpui_kit::ClipboardString::new(
                absolute.unwrap_or_else(|| rel_path.to_string()),
            ))],
        };
        cx.write_to_clipboard(gpui_kit::ClipboardItem { entries });
    }

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
            prompt: Some(rust_i18n::t!("app.composer.attach_files").to_string().into()),
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
                Ok(Err(error)) => Err(rust_i18n::t!("app.files.picker_failed", error = error.to_string()).to_string()),
                Err(_) => Err(rust_i18n::t!("app.files.picker_disconnected").to_string()),
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
                            view.error =
                                Some(rust_i18n::t!(i18n::COMPOSER_ATTACHMENTS_LIMIT, n = MAX_ATTACHMENTS).to_string());
                        }
                        for result in results {
                            match result {
                                Ok(attachment) => {
                                    if view.attachments.iter().any(|existing| existing.path == attachment.path) {
                                        continue;
                                    }
                                    if view.attachments.len() >= MAX_ATTACHMENTS {
                                        view.error = Some(
                                            rust_i18n::t!(i18n::COMPOSER_ATTACHMENTS_LIMIT, n = MAX_ATTACHMENTS)
                                                .to_string(),
                                        );
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
        let _ = self.attachment_thumbnails.remove(&path);
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
                    let snapshot = files::scan_snapshot(&root, || worker_cancellation.load(Ordering::Relaxed))?;
                    if worker_cancellation.load(Ordering::Relaxed) {
                        return None;
                    }
                    Some((snapshot, files::git_branch(&root)))
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
                let Some((snapshot, branch)) = result else {
                    return;
                };
                view.files_truncated = snapshot.truncated;
                view.replace_file_entries(snapshot.entries, cx);
                // Stale picker rows fail closed on toggle, but rebuilding
                // keeps an open picker honest without its own scan.
                view.rebuild_picker_tree(cx);
                view.git_branch = branch;
                view.update_composer_suggestions(window, cx);
                cx.notify();
            });
        }));
    }

    pub(super) fn refresh_workspace_view(&mut self, cx: &mut Context<Self>) {
        if self.current_route() == Route::Review {
            self.refresh_workspace_review(cx);
        } else {
            self.refresh_files(cx);
        }
    }

    pub(super) fn focus_workspace_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_inspector_tab(InspectorTab::Files, cx);

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
        let directory_paths: HashSet<_> = entries
            .iter()
            .filter(|entry| entry.is_dir)
            .map(|entry| entry.rel_path.clone())
            .collect();
        self.expanded_file_directories.retain(|path| directory_paths.contains(path));
        self.files = entries;
        self.file_directory_paths = Arc::new(directory_paths);
        self.rebuild_file_tree(cx);
    }

    /// Rebuild tree rows from the current scan and filter query. Browse mode
    /// shows the hierarchy; a filter query shows flat ranked hits. Runs on
    /// every scan and on every filter keystroke; selection follows its path
    /// across rebuilds while the row still exists.
    pub(super) fn rebuild_file_tree(&mut self, cx: &mut Context<Self>) {
        let selected_path = self
            .pending_tree_select
            .take()
            .or_else(|| self.file_tree.read(cx).selected_item().map(|item| item.id.to_string()));
        let query = self.file_filter_input.read(cx).value().trim().to_string();
        let tree_items = if query.is_empty() {
            self.file_search_hit_count = None;
            file_tree_items(&self.files, None, &self.expanded_file_directories)
        } else {
            let hits = files::fuzzy_search(&self.files, &query, files::SEARCH_MAX_HITS);
            self.file_search_hit_count = Some(hits.len());
            hits.iter()
                .map(|hit| TreeItem::new(hit.rel_path.clone(), hit.rel_path.clone()))
                .collect()
        };
        let selected_item = selected_path
            .as_deref()
            .and_then(|path| find_file_tree_item(&tree_items, path))
            .cloned();
        self.file_tree.update(cx, |tree, cx| {
            tree.set_items(tree_items, cx);
            tree.set_selected_item(selected_item.as_ref(), cx);
        });
        cx.notify();
    }

    pub(super) fn open_selected_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.file_tree.read(cx).selected_item().map(|item| item.id.to_string()) else {
            return;
        };
        if self.files.iter().any(|entry| entry.rel_path == path && !entry.is_dir) {
            self.open_file_preview(path, window, cx);
        }
    }

    pub(super) fn rename_selected_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.file_tree.read(cx).selected_item().map(|item| item.id.to_string()) else {
            return;
        };
        self.open_file_rename(path, window, cx);
    }

    pub(super) fn delete_selected_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.file_tree.read(cx).selected_item().map(|item| item.id.to_string()) else {
            return;
        };
        self.confirm_delete_file(&path, window, cx);
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
        self.open_file_preview_overlay(rel_path, window, cx);
    }

    /// In-app preview overlay for one workspace file (text or image),
    /// bypassing the external-editor preference — the composer chips' View
    /// action uses this so viewing stays in-app regardless of configuration.
    pub(super) fn open_file_preview_overlay(&mut self, rel_path: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_route() == Route::Review {
            self.close_review(window, cx);
        }
        if self.current_route() == Route::Browser {
            self.close_browser(window, cx);
        }
        // Deferred popovers paint above modal scrims: the picker cannot stay open.
        self.close_attachment_picker(window, cx);
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        self.disarm_double_escape();
        // Capture the restore target for both preview kinds, matching the text
        // path, so Escape always returns focus to the opening surface.
        if self.file_preview.is_none() {
            self.preview_return_focus = window.focused(cx);
        }
        // Images take the image viewer: bounded bytes decode in the renderer,
        // and text previews never apply.
        if files::is_image_path(&rel_path) {
            self.preview_task = Some(cx.spawn_in(window, async move |view, cx| {
                let root = workspace.clone();
                let path = rel_path.clone();
                let image = cx.background_spawn(async move { files::read_image(&root, &path) }).await;
                let _updated = view.update_in(cx, |view, window, cx| {
                    if view.workspace.as_ref() != Some(&workspace) {
                        return;
                    }
                    match image {
                        Some(image) => {
                            view.preview_editor_error = None;
                            view.file_preview = Some(FilePreview {
                                rel_path: rel_path.clone(),
                                text: String::new(),
                                truncated: false,
                                image: Some(image),
                            });
                            view.preview_focus.focus(window, cx);
                            view.clear_error();
                        }
                        None => {
                            view.error = Some(
                                rust_i18n::t!("app.files.preview_unavailable", path = rel_path.as_str()).to_string(),
                            )
                        }
                    }
                    cx.notify();
                });
            }));
            return;
        }
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
    /// Unlike the viewer path this never closes the review surface: an external
    /// launch is non-modal and must not destroy the user's open context.
    pub(super) fn open_file_in_editor(
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

    /// Reveal one workspace entry in Finder (`open -R`), for files and
    /// directories alike. Same interception and completion channel as
    /// [`Self::open_file_in_editor`] so headless tests observe the launch.
    pub(super) fn reveal_file_in_finder(&mut self, rel_path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        #[cfg(test)]
        if let Some(result) = self.intercept_test_launch(rel_path, files::ExternalEditor::Finder) {
            self.apply_external_launch_result(&workspace, result, cx);
            return;
        }
        let rel_path = rel_path.to_owned();
        // Stored, not detached: replacing the task supersedes an older launch
        // so only the latest completion can publish or clear the error notice.
        self.external_launch_task = Some(cx.spawn_in(window, async move |view, cx| {
            let root = workspace.clone();
            let path = rel_path.clone();
            let result = cx.background_spawn(async move { files::reveal_in_finder(&root, &path) }).await;
            let _updated = view.update_in(cx, |view, _, cx| {
                view.apply_external_launch_result(&workspace, result, cx);
            });
        }));
    }

    /// Delete one workspace entry from the file context menu, behind the
    /// same native warning prompt as thread deletes. The prompt is
    /// headless-untestable, so the post-confirm write lives in
    /// [`Self::delete_file_after_confirm`], which tests drive directly.
    pub(super) fn confirm_delete_file(&mut self, rel_path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        if !self.files.iter().any(|entry| entry.rel_path == rel_path) {
            return;
        }
        let base = rel_path.rsplit('/').next().unwrap_or(rel_path).to_string();
        let title = format!("Delete “{base}”?");
        let detail = "The item moves to the Trash. You can restore it from there.".to_string();
        let answer = window.prompt(
            gpui_kit::PromptLevel::Warning,
            title.as_str(),
            Some(detail.as_str()),
            &["Delete", "Cancel"],
            cx,
        );
        let rel_path = rel_path.to_owned();
        cx.spawn_in(window, async move |view, cx| {
            if matches!(answer.await, Ok(0)) {
                let _updated = view.update_in(cx, |view, window, cx| {
                    view.delete_file_after_confirm(workspace, rel_path, window, cx);
                });
            }
        })
        .detach();
    }

    /// Trash one entry after its delete prompt was confirmed. Split from the
    /// prompt so headless tests cover the whole post-confirm path: the write
    /// runs on the background executor and the tree refreshes on success.
    pub(super) fn delete_file_after_confirm(
        &mut self,
        workspace: PathBuf,
        rel_path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.as_ref() != Some(&workspace) {
            return;
        }
        let root = workspace.clone();
        let path = rel_path.clone();
        cx.spawn_in(window, async move |view, cx| {
            let outcome = cx.background_spawn(async move { files::trash_entry(&root, &path) }).await;
            let _updated = view.update_in(cx, |view, window, cx| {
                view.finish_file_delete(&workspace, &rel_path, outcome, window, cx);
            });
        })
        .detach();
    }

    /// Publish one background trash. Success closes a preview of the deleted
    /// entry (there is nothing to repoint at) and refreshes the tree; failure
    /// surfaces on the view.
    fn finish_file_delete(
        &mut self,
        workspace: &Path,
        rel_path: &str,
        outcome: Result<(), String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.as_deref() != Some(workspace) {
            return;
        }
        match outcome {
            Ok(()) => {
                if self.file_preview.as_ref().is_some_and(|preview| preview.rel_path == rel_path) {
                    self.close_file_preview(window, cx);
                }
                self.refresh_files(cx);
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
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
    pub(super) fn intercept_test_launch(
        &mut self,
        rel_path: &str,
        editor: files::ExternalEditor,
    ) -> Option<Result<(), String>> {
        let intercept = self.test_launch_intercept.as_mut()?;
        intercept
            .calls
            .push(ExternalLaunchCall { rel_path: rel_path.to_owned(), editor });
        Some(intercept.fail_with.clone().map_or(Ok(()), Err))
    }

    /// Test-only interception for handoff script launches, mirroring
    /// [`intercept_test_launch`]: records the script content instead of
    /// writing a file and spawning `/usr/bin/open`. Returns `None` when no
    /// hook is set.
    #[cfg(test)]
    pub(super) fn intercept_test_handoff_script(
        &mut self,
        content: String,
        editor: files::ExternalEditor,
    ) -> Option<Result<(), String>> {
        let intercept = self.test_launch_intercept.as_mut()?;
        intercept.handoff_scripts.push(HandoffScriptCall { content, editor });
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
        let weak = cx.entity().downgrade();
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
            .px_4()
            .pb_4()
            .pt(px(40.))
            .bg(modal_scrim(colors))
            .child(
                div()
                    .id("file-preview")
                    .test_support()
                    .track_focus(&self.preview_focus)
                    // Click-away dismissal, mirroring the palette panel: a
                    // press on the scrim closes the preview.
                    .on_mouse_down_out(move |_, window, cx| {
                        let _updated = weak.update(cx, |view, cx| {
                            // The rename dialog paints above the preview and
                            // repoints it on commit: presses inside the rename
                            // surface must not dismiss the preview beneath.
                            if view.file_rename.is_none() {
                                view.close_file_preview(window, cx);
                            }
                        });
                    })
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
                            .when(preview.image.is_none(), |header| {
                                header.child(
                                    desktop_button("copy-preview")
                                        .label(rust_i18n::t!(i18n::FILE_PREVIEW_COPY).to_string())
                                        .ghost()
                                        .small()
                                        .on_click(move |_, window, app| {
                                            app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                                text.to_string(),
                                            ));
                                            super::super::notifications::copied_to_clipboard(window, app);
                                        }),
                                )
                            })
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
                    .child(match preview.image.clone() {
                        // Image previews render the picture itself; everything
                        // else keeps the bounded mono text surface.
                        Some(image) => div()
                            .id("file-preview-body")
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .overflow_hidden()
                            .p_3()
                            .child(
                                img(image)
                                    .max_w_full()
                                    .max_h_full()
                                    .object_fit(gpui_kit::ObjectFit::Contain)
                                    .rounded_md(),
                            )
                            .into_any_element(),
                        None => div()
                            .id("file-preview-body")
                            .flex_1()
                            .min_h_0()
                            .lock_scroll_axis()
                            .overflow_y_scrollbar()
                            .p_3()
                            .text_xs()
                            .font_family(mono)
                            .text_size(theme.mono_font_size)
                            .child(preview.text.clone())
                            .into_any_element(),
                    })
                    .when(preview.truncated && preview.image.is_none(), |panel| {
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
