//! Shared attachment card builders: composer chips and transcript echo cards.
//!
//! Both surfaces render the same Kit `Attachment` media + metadata card; only
//! the composer adds view/open/remove actions, so the base card (no actions
//! slot) is the shared unit and actions stay caller-owned.

use super::*;

/// Base file card: media plus title/description, without actions. The caller
/// chains `.actions(...)` when the surface is interactive (composer chips);
/// transcript echo cards render the base card read-only.
pub(super) fn base_attachment_card(
    path: &str,
    byte_len: usize,
    is_image: bool,
    thumbnail: Option<Arc<gpui_kit::Image>>,
) -> Attachment {
    Attachment::new()
        .small()
        .w(px(230.))
        .media(AttachmentMedia::new().child({
            match thumbnail {
                Some(image) => img(image).size(px(40.)).rounded_sm().overflow_hidden().into_any_element(),
                None => Icon::new(if is_image {
                    IconName::FileImage
                } else {
                    IconName::FileCode
                })
                .size_6()
                .into_any_element(),
            }
        }))
        .content(
            AttachmentContent::new()
                .title(AttachmentTitle::new(path.to_owned()))
                .description(AttachmentDescription::new(if is_image {
                    rust_i18n::t!("app.composer.attachment_image").to_string()
                } else {
                    rust_i18n::t!("app.composer.attachment_file", bytes = byte_len).to_string()
                })),
        )
}

impl DesktopView {
    /// Attachment stack for a user echo row: one card per attachment, grouped
    /// for tests and assistive tech. The caller renders the stack above the
    /// prompt text inside the user bubble. Cards offer the same in-app View
    /// action as composer chips (no remove: the turn already sent), under an
    /// echo-scoped id prefix so a file attached in both surfaces never shares
    /// an element id.
    pub(super) fn user_attachment_cards(
        &self,
        item_id: &str,
        attachments: &[UserAttachment],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        debug_assert!(!attachments.is_empty(), "the caller renders the stack only for attachment echoes");
        div()
            .id(format!("user-attachments-{item_id}"))
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(UserAttachment::attached_list_label(attachments).unwrap_or_default())
            .flex()
            .flex_col()
            .min_w_0()
            .gap_2()
            .children(attachments.iter().map(|attachment| {
                let view_path = attachment.path.clone();
                base_attachment_card(&attachment.path, attachment.byte_len, attachment.is_image, None)
                    .max_w_full()
                    .actions(
                        AttachmentActions::new().child(
                            desktop_button(opaque_path_element_id("view-echo-attachment", &attachment.path))
                                .icon(IconName::Eye)
                                .accessibility_label(rust_i18n::t!("app.composer.view_attachment").to_string())
                                .ghost()
                                .xsmall()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_file_preview_overlay(view_path.clone(), window, cx);
                                })),
                        ),
                    )
            }))
            .into_any_element()
    }
}
