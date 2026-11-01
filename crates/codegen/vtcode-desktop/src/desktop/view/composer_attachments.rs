//! Composer attachments: the workspace file picker flow and external file drops.
use super::*;

/// Background read for one candidate attachment: the turn attachment plus,
/// for images, bounded bytes for the composer thumbnail.
fn load_attachment(
    root: PathBuf,
    path: String,
) -> (Result<protocol::TurnAttachment, String>, Option<Arc<gpui_kit::Image>>) {
    let attachment = files::read_attachment(&root, &root.join(&path));
    let thumbnail = attachment
        .as_ref()
        .ok()
        .filter(|attachment| files::is_image_path(&attachment.path))
        .and_then(|attachment| files::read_image(&root, &attachment.path));
    (attachment, thumbnail)
}

impl DesktopView {
    /// Drop cached image thumbnails no live attachment references.
    ///
    /// Thumbnails are keyed by attachment path while attachments travel: the
    /// composer moves them into submission drafts on send, drafts move between
    /// the pending/first/retained slots, and every slot is dropped on session
    /// switches. Callers invoke this after dropping a holder so consumed sends
    /// cannot leak decoded images while restorable drafts keep their chips.
    pub(super) fn prune_attachment_thumbnails(&mut self) {
        let mut live_paths: Vec<&str> = Vec::new();
        for attachment in &self.attachments {
            live_paths.push(attachment.path.as_str());
        }
        for draft in [
            &self.pending_submission,
            &self.first_submission,
            &self.retained_submission,
        ]
        .into_iter()
        .flatten()
        {
            for attachment in &draft.attachments {
                live_paths.push(attachment.path.as_str());
            }
        }
        self.attachment_thumbnails.retain(|path, _| live_paths.contains(&path.as_str()));
    }

    /// Reset the load gate, then push freshly read attachments — skipping
    /// duplicates and surfacing the last failure — unless the workspace or
    /// selected session changed while the reads ran. Image results also cache
    /// their decoded-ready thumbnails for the composer chips.
    fn settle_attachments(
        &mut self,
        workspace: &PathBuf,
        session: &Option<String>,
        results: Vec<(Result<protocol::TurnAttachment, String>, Option<Arc<gpui_kit::Image>>)>,
        cx: &mut Context<Self>,
    ) {
        self.attachments_picker_open = false;
        if self.workspace.as_ref() != Some(workspace) || self.selected_session_id != *session {
            return;
        }
        for (attachment, thumbnail) in results {
            match attachment {
                Ok(attachment) if !self.attachments.iter().any(|existing| existing.path == attachment.path) => {
                    if let Some(image) = thumbnail {
                        let _ = self.attachment_thumbnails.insert(attachment.path.clone(), image);
                    }
                    self.attachments.push(attachment)
                }
                Ok(_) => {}
                Err(error) => self.error = Some(error),
            }
        }
        cx.notify();
    }

    pub(super) fn attach_context_file(&mut self, path: String, cx: &mut Context<Self>) {
        if self.attachments_picker_open || self.session_request.is_some() {
            return;
        }
        if self.attachments.len() >= 5 {
            self.error = Some(rust_i18n::t!(i18n::COMPOSER_ATTACHMENTS_LIMIT, n = 5).to_string());
            cx.notify();
            return;
        }
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        let session = self.selected_session_id.clone();
        self.attachments_picker_open = true;
        self.attachments_task = Some(cx.spawn(async move |view, cx| {
            let root = workspace.clone();
            let (attachment, thumbnail) = cx.background_spawn(async move { load_attachment(root, path) }).await;
            let _updated = view.update(cx, |view, cx| {
                view.settle_attachments(&workspace, &session, vec![(attachment, thumbnail)], cx)
            });
        }));
        cx.notify();
    }

    /// Attach files dropped onto the window from Finder, from anywhere on
    /// disk — not only inside the workspace.
    ///
    /// GPUI hands external drops over as absolute file paths (there is no file
    /// promise receiver below GPUI), so Safari/Mail/Photos promise-only drops
    /// never reach the app. Reads run on the background executor like the
    /// picker flow.
    pub(super) fn attach_dropped_files(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        if paths.is_empty() || self.attachments_picker_open || self.session_request.is_some() {
            return;
        }
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        let mut queued: Vec<String> = Vec::new();
        for path in paths {
            if self.attachments.len() + queued.len() >= 5 {
                self.error = Some(rust_i18n::t!(i18n::COMPOSER_ATTACHMENTS_LIMIT, n = 5).to_string());
                break;
            }
            let path = path.to_string_lossy().into_owned();
            if !queued.contains(&path) {
                queued.push(path);
            }
        }
        if queued.is_empty() {
            cx.notify();
            return;
        }
        let session = self.selected_session_id.clone();
        self.attachments_picker_open = true;
        self.attachments_task = Some(cx.spawn(async move |view, cx| {
            let root = workspace.clone();
            let results = cx
                .background_spawn(async move {
                    queued
                        .into_iter()
                        .map(|path| load_attachment(root.clone(), path))
                        .collect::<Vec<_>>()
                })
                .await;
            let _updated = view.update(cx, |view, cx| view.settle_attachments(&workspace, &session, results, cx));
        }));
        cx.notify();
    }
}
