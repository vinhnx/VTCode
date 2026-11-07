//! Structured attachments for user echo rows.
//!
//! A submitted turn's attachments render as [`gpui_kit::component::attachment`]
//! cards stacked above the prompt inside the user bubble. The full bounded
//! content travels with the turn request; the echo row keeps only what its
//! cards render (path, byte count, image flag) plus the search/accessibility
//! projections that must keep working after the old `Attached: …` text
//! suffix was removed from the echo body.

use gpui_kit::SharedString;

use super::TranscriptItem;

/// One file attached to a user submission, projected for transcript display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserAttachment {
    /// Display path: workspace-relative for workspace files, absolute for
    /// files attached from anywhere else (mirrors `TurnAttachment::path`).
    pub path: String,
    /// Byte length of the bounded attachment content (`0` for images, whose
    /// card shows the image label instead of a byte count).
    pub byte_len: usize,
    /// Whether the path is a renderable image (`files::is_image_path`).
    pub is_image: bool,
}

impl UserAttachment {
    /// Localized `Attached: a, b` label for a submission's attachments.
    /// Shared by the echo accessibility suffix and the card-stack aria label
    /// so the two can never disagree. `None` when there is nothing to label.
    pub(crate) fn attached_list_label(attachments: &[Self]) -> Option<String> {
        if attachments.is_empty() {
            return None;
        }
        let paths = attachments
            .iter()
            .map(|attachment| attachment.path.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        Some(rust_i18n::t!("app.composer.attached_list", paths = paths).to_string())
    }
}

impl TranscriptItem {
    /// Append the localized attached-paths suffix to the prepared accessible
    /// text. User echoes render attachments as structured cards above the
    /// prompt, so the screen-reader label re-attaches the paths here instead
    /// of in the Markdown body. No-op for other roles and attachment-free
    /// rows, keeping their projections byte-identical.
    pub(crate) fn append_attachment_accessibility_suffix(&mut self) {
        if !self.is_user() {
            return;
        }
        let Some(label) = UserAttachment::attached_list_label(&self.user_attachments) else {
            return;
        };
        self.accessibility_text = format!("{}\n\n{label}", self.accessibility_text).into();
    }

    /// Keyword surfaces for conversation search: title, body, and attached
    /// file paths. Shared by the palette keywords and the overlay-cycle
    /// substring matcher so the two can never disagree on what matches.
    pub(crate) fn search_keywords(&self) -> Vec<SharedString> {
        let mut keywords = Vec::with_capacity(2 + self.user_attachments.len());
        keywords.push(self.text.clone());
        keywords.push(self.title.clone());
        keywords.extend(
            self.user_attachments
                .iter()
                .map(|attachment| SharedString::from(attachment.path.as_str())),
        );
        keywords
    }
}
