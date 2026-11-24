//! Command detail surface for expanded transcript tool rows.

use super::*;

impl DesktopView {
    pub(super) fn render_tool_command(
        &self,
        item: &TranscriptItem,
        command: &gpui_kit::SharedString,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let command_copy = command.clone();
        let revealed = self.transcript_row_revealed(item.id.as_str());
        // Refine the shared capped code-block style: replacing it would drop
        // the height cap and leave a long command unbounded in the transcript.
        let code_style = div()
            .whitespace_normal()
            .rounded_md()
            .refine_style(&code_block_style(cx))
            .style()
            .clone();
        div()
            .w_full()
            .pl_6()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div().flex().justify_end().child(
                    desktop_ghost_button(format!("copy-tool-command-{}", item.id))
                        .icon(IconName::Copy)
                        .accessibility_label(rust_i18n::t!("app.markdown.copy_code").to_string())
                        .tooltip(rust_i18n::t!("app.markdown.copy_code").to_string())
                        .xsmall()
                        .when(!revealed, |button| button.opacity(0.).focus_visible(|style| style.opacity(1.)))
                        .on_click(move |_, window, app| {
                            app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(command_copy.to_string()));
                            super::super::notifications::copied_to_clipboard(window, app);
                        }),
                ),
            )
            .child(
                div()
                    .id(format!("tool-command-surface-{}", item.id))
                    .test_support()
                    .role(gpui_kit::Role::Label)
                    .aria_label(command.clone())
                    .child(
                        render_code_preview(format!("tool-command-{}", item.id), "command.sh", command.clone(), cx)
                            .style(markdown_style(cx).with_code_block(code_style))
                            .code_block_actions(|_, _, _| div()),
                    ),
            )
    }
}
