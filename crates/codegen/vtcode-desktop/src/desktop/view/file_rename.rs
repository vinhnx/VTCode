//! In-app file rename dialog: validated same-directory rename for one
//! workspace entry, opened from the file context menu.
//!
//! Structured after `full_access_confirm.rs`: transient Option state,
//! return-focus restore, Escape via `dismiss_overlay`. The name input commits
//! on Enter through `InputEvent::PressEnter` and on the Rename button; the
//! filesystem write runs on the background executor and the tree refreshes on
//! success. Validation is the shared `files::validated_file_name` predicate,
//! so the dialog and the write cannot drift.
use super::*;

/// Rebase one workspace-relative path onto a renamed entry: the entry itself
/// and its descendants move, everything else stays. The separator boundary
/// keeps `src2/x` put when `src` is renamed. Returns `None` for paths
/// outside the renamed subtree.
fn rebased_path(old: &str, new: &str, path: &str) -> Option<String> {
    if path == old {
        return Some(new.to_string());
    }
    path.strip_prefix(old)
        .filter(|rest| rest.starts_with('/'))
        .map(|rest| format!("{new}{rest}"))
}

/// Base name prefilled into the rename input. Shared by the opener, the
/// scrim dirt check, and the same-name commit check so the three cannot drift.
fn rename_base_name(rel_path: &str) -> &str {
    rel_path.rsplit('/').next().unwrap_or(rel_path)
}

pub(super) struct FileRenameState {
    /// Workspace-relative path of the entry being renamed.
    rel_path: String,
    /// Workspace the dialog was opened from; guards against cross-workspace commit.
    workspace: Option<PathBuf>,
    /// Prefilled with the current base name, fully selected for overwrite typing.
    name_input: Entity<InputState>,
    /// Enter-to-commit subscription; dropped with the dialog.
    _input_subscription: Subscription,
    /// Validation or filesystem failure, scoped to the dialog.
    error: Option<String>,
    /// A rename write is in flight; the dialog stays open but inert.
    pending: bool,
}

impl DesktopView {
    pub(super) fn open_file_rename(&mut self, rel_path: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        if !self.files.iter().any(|entry| entry.rel_path == rel_path) {
            return;
        }
        // Deferred popovers paint above modal scrims: the picker cannot stay open.
        self.close_attachment_picker(window, cx);
        let current = rename_base_name(&rel_path).to_string();
        self.disarm_double_escape();
        if self.file_rename.is_none() {
            self.rename_return_focus = window.focused(cx);
        }
        let input = cx.new(|cx| InputState::new(window, cx).default_value(current));
        let subscription = cx.subscribe_in(&input, window, |view, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                view.commit_file_rename(window, cx);
            }
        });
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        self.file_rename = Some(FileRenameState {
            rel_path,
            workspace: Some(workspace),
            name_input: input,
            _input_subscription: subscription,
            error: None,
            pending: false,
        });
        cx.notify();
    }

    pub(super) fn cancel_file_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_file_rename(window, cx);
    }

    fn close_file_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_rename.take().is_none() {
            return;
        }
        self.disarm_double_escape();
        if let Some(focus) = self.rename_return_focus.take() {
            focus.focus(window, cx);
        } else {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    /// Whether the dialog holds an edited name. Prefill-intact is clean:
    /// reopening restores the current name, so nothing is lost.
    fn rename_has_unsaved_changes(&self, cx: &App) -> bool {
        let Some(state) = self.file_rename.as_ref() else {
            return false;
        };
        state.name_input.read(cx).value() != rename_base_name(&state.rel_path)
    }

    /// Dismiss the dialog through its scrim: edited names confirm first,
    /// prefill-intact dialogs close. A rename write pins the dialog.
    pub(super) fn request_rename_dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_rename.as_ref().is_none_or(|state| state.pending) {
            return;
        }
        if !self.rename_has_unsaved_changes(cx) {
            self.cancel_file_rename(window, cx);
            return;
        }
        let rel_path = self.file_rename.as_ref().map(|state| state.rel_path.clone());
        let Some(rel_path) = rel_path else {
            return;
        };
        self.prompt_discard_changes(DiscardIntent::CancelRename(rel_path), window, cx);
    }

    /// Run a confirmed rename discard after verifying the same dialog is
    /// still open — the prompt answer arrives async.
    pub(super) fn discard_rename_after_confirm(&mut self, rel_path: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_rename.as_ref().is_some_and(|state| state.rel_path == rel_path) {
            self.cancel_file_rename(window, cx);
        }
    }

    pub(super) fn commit_file_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.file_rename.as_mut() else {
            return;
        };
        if state.pending {
            return;
        }
        let raw = state.name_input.read(cx).value().to_string();
        let name = match files::validated_file_name(&raw) {
            Ok(name) => name,
            Err(error) => {
                state.error = Some(error);
                cx.notify();
                return;
            }
        };
        if name == rename_base_name(&state.rel_path) {
            // Same-name rename is a no-op success: close without a write.
            self.close_file_rename(window, cx);
            return;
        }
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        if state.workspace.as_ref() != Some(&workspace) {
            self.close_file_rename(window, cx);
            return;
        }
        state.pending = true;
        state.error = None;
        let rel_path = state.rel_path.clone();
        cx.notify();
        cx.spawn_in(window, async move |view, cx| {
            let root = workspace.clone();
            let path = rel_path.clone();
            let outcome = cx
                .background_spawn(async move { files::rename_entry(&root, &path, &name) })
                .await;
            let _updated = view.update_in(cx, |view, window, cx| {
                view.finish_file_rename(&workspace, &rel_path, outcome, window, cx);
            });
        })
        .detach();
    }

    /// Publish one background rename. The tree always refreshes on success —
    /// even when the dialog was cancelled mid-flight, the filesystem changed
    /// under us. A failure with a live dialog shows inline; without one it
    /// surfaces on the view so it is never swallowed silently.
    fn finish_file_rename(
        &mut self,
        workspace: &Path,
        old_rel: &str,
        outcome: Result<String, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.as_deref() != Some(workspace) {
            return;
        }
        match outcome {
            Ok(new_rel) => {
                self.remap_attachment_paths(old_rel, &new_rel);
                self.remap_expanded_directories(old_rel, &new_rel);
                let repoint = self
                    .file_preview
                    .as_ref()
                    .and_then(|preview| rebased_path(old_rel, &new_rel, &preview.rel_path));
                if let Some(next) = repoint {
                    self.open_file_preview_overlay(next, window, cx);
                }
                self.close_file_rename(window, cx);
                // Keep the selection on the renamed entry across the rebuild.
                self.pending_tree_select = Some(new_rel);
                self.refresh_files(cx);
            }
            Err(error) => {
                if let Some(state) = self.file_rename.as_mut().filter(|state| state.rel_path == old_rel) {
                    state.pending = false;
                    state.error = Some(error);
                } else {
                    self.error = Some(error);
                }
                cx.notify();
            }
        }
    }

    /// Keep composer attachments pointing at a renamed entry: chips, queued
    /// drafts, and cached thumbnails all key on the workspace-relative path.
    /// Directory renames rebase descendants too (`src/x` follows `src`).
    fn remap_attachment_paths(&mut self, old: &str, new: &str) {
        for attachment in &mut self.attachments {
            if let Some(next) = rebased_path(old, new, &attachment.path) {
                attachment.path = next;
            }
        }
        for draft in [
            &mut self.pending_submission,
            &mut self.first_submission,
            &mut self.retained_submission,
        ]
        .into_iter()
        .flatten()
        {
            for attachment in &mut draft.attachments {
                if let Some(next) = rebased_path(old, new, &attachment.path) {
                    attachment.path = next;
                }
            }
        }
        let moves: Vec<(String, String)> = self
            .attachment_thumbnails
            .keys()
            .filter_map(|path| rebased_path(old, new, path).map(|next| (path.clone(), next)))
            .collect();
        for (from, to) in moves {
            if let Some(thumbnail) = self.attachment_thumbnails.remove(&from) {
                let _ = self.attachment_thumbnails.insert(to, thumbnail);
            }
        }
    }

    /// Keep renamed directories expanded across the rebuild, including nested
    /// expansions beneath them.
    fn remap_expanded_directories(&mut self, old: &str, new: &str) {
        let moves: Vec<(String, String)> = self
            .expanded_file_directories
            .iter()
            .filter_map(|path| rebased_path(old, new, path).map(|next| (path.clone(), next)))
            .collect();
        for (from, to) in moves {
            let _removed = self.expanded_file_directories.remove(&from);
            let _inserted = self.expanded_file_directories.insert(to);
        }
    }

    /// Compact rename card over the shared modal scrim: the object in the
    /// title, the parent folder as muted scope, one input, result-labeled
    /// actions.
    pub(super) fn render_file_rename(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let Some(state) = self.file_rename.as_ref() else {
            return div().into_any_element();
        };
        let base_name = state.rel_path.rsplit('/').next().unwrap_or(&state.rel_path).to_string();
        let scope = match state.rel_path.rsplit_once('/') {
            Some((parent, _)) => format!("in {parent}"),
            None => "in the workspace root".to_string(),
        };
        let blank = state.name_input.read(cx).value().trim().is_empty();
        let pending = state.pending;
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .bg(modal_scrim(&colors))
            // Scrim dismissal, guarded for edited names.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, window, cx| {
                    view.request_rename_dismiss(window, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .id("file-rename-dialog")
                    .test_support()
                    .track_focus(&self.rename_focus)
                    .focus_trap("file-rename-dialog", &self.rename_focus)
                    // Card presses stop here so the scrim only sees true
                    // outside presses.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .role(gpui_kit::Role::Dialog)
                    .accessibility_id("file-rename-dialog")
                    .aria_label(format!("Rename “{base_name}”"))
                    .w(px(420.))
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
                    .p_5()
                    .child(
                        div()
                            .font_semibold()
                            .text_lg()
                            .text_color(colors.foreground)
                            .child(format!("Rename “{base_name}”")),
                    )
                    .child(div().text_sm().text_color(colors.muted_foreground).child(scope))
                    .child(
                        Input::new(&state.name_input)
                            .id("file-rename-input")
                            .accessibility_id("file-rename-input")
                            .aria_label("New name".to_string())
                            .disabled(pending)
                            .when(pending, |input| input.cursor_default())
                            .when(!pending, |input| input.cursor_text())
                            .w_full(),
                    )
                    .when_some(state.error.as_ref(), |dialog, error| {
                        dialog.child(
                            div()
                                .id("file-rename-error")
                                .test_support()
                                .role(gpui_kit::Role::Alert)
                                .aria_label(error.clone())
                                .text_sm()
                                .text_color(colors.danger)
                                .child(error.clone()),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .child(
                                desktop_default_button("file-rename-cancel")
                                    .label("Cancel".to_string())
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.cancel_file_rename(window, cx);
                                    })),
                            )
                            .child(
                                desktop_glass_popover_primary_button("file-rename-commit", &colors, blank || pending)
                                    .label(if pending {
                                        "Renaming…".to_string()
                                    } else {
                                        "Rename".to_string()
                                    })
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.commit_file_rename(window, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}
