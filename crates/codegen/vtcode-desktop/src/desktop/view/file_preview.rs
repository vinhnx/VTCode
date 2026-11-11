//! Markdown preview source editing: the save flow plus the source viewport
//! sizing constants. (The overlay open/render paths still live in
//! `files_panel.rs`; migrate them here if that file nears the ceiling again.)

use super::*;

/// Visible source-editor rows, clamped. The kit editor has a fixed intrinsic
/// height (textarea-style `auto_grow` is not offered on `EditorState`), so
/// the preview sizes it from the document's line count: short documents keep
/// a compact panel, long ones get a tall viewport and scroll within it.
pub(super) const PREVIEW_SOURCE_MIN_ROWS: usize = 3;
pub(super) const PREVIEW_SOURCE_MAX_ROWS: usize = 24;
/// Matches the kit editor's row advance: input rows lay out at the window
/// line height for the configured mono size, so the preview derives the
/// same height (verified against pinned 0.9.1 `input/base/element.rs`).
pub(super) const PREVIEW_SOURCE_LINE_HEIGHT: f32 = 1.5;

impl DesktopView {
    /// Cmd+S entry point. Mirrors the Save button exactly: only an armed
    /// edit session saves, so the shortcut never writes a draft the user
    /// parked with Done.
    pub(super) fn save_preview_via_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_preview.as_ref().is_some_and(|preview| preview.editing) {
            self.save_preview_source(window, cx);
        }
    }

    /// Persist the source editor draft, mirroring the rename commit flow:
    /// the write runs on a background task and the preview text refreshes
    /// on success so the rendered arm, copy action, and dirty tracking all
    /// follow the saved file. Refuses unarmed, truncated, and clean states.
    pub(super) fn save_preview_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = self.file_preview.as_ref() else {
            return;
        };
        // Only an armed edit session saves: the button exists solely while
        // editing, so this also fails closed on stale drafts (e.g. a
        // non-markdown preview over an older markdown draft).
        if !preview.editing || preview.truncated {
            return;
        }
        let rel_path = preview.rel_path.clone();
        let text = self.file_preview_source.read(cx).value().to_string();
        if text == preview.text {
            return;
        }
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        self.preview_editor_error = None;
        cx.notify();
        let write_text = text.clone();
        let write_rel_path = rel_path.clone();
        let write_workspace = workspace.clone();
        cx.spawn_in(window, async move |view, cx| {
            let outcome = cx
                .background_spawn(async move { files::write_preview_text(&workspace, &write_rel_path, &write_text) })
                .await;
            let _updated = view.update_in(cx, |view, _window, cx| {
                // A workspace switch drops the preview; a late completion
                // must not publish into the new workspace's same-named
                // preview, mirroring the other file-mutation completions.
                if view.workspace.as_ref() != Some(&write_workspace) {
                    return;
                }
                match outcome {
                    Ok(()) => {
                        if let Some(preview) = view.file_preview.as_mut().filter(|preview| preview.rel_path == rel_path)
                        {
                            preview.text = text;
                            preview.truncated = false;
                        }
                        view.preview_editor_error = None;
                    }
                    Err(error) => view.preview_editor_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
