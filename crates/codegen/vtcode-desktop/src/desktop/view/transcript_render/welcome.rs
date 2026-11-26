//! Empty-conversation prelude shared by the workspace and Zen views.

use super::*;

impl DesktopView {
    pub(super) fn render_welcome(&self, connecting: bool, window: &Window, cx: &App) -> impl IntoElement {
        let colors = cx.theme().colors;
        let (group_id, group_label, title_id) = if self.zen_mode {
            ("zen-welcome", rust_i18n::t!("app.transcript.zen_start_label"), "zen-welcome-prompt")
        } else {
            ("welcome", rust_i18n::t!("app.transcript.welcome_label"), "welcome-prompt")
        };
        let title = self.welcome_period.label();
        let workspace_context = if connecting {
            let label = rust_i18n::t!("app.workspace.starting_service").to_string();
            Some(
                div()
                    .id("workspace-connecting")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(label.clone())
                    .min_w_0()
                    .truncate()
                    .child(shimmer_label("workspace-connecting-title", label, colors.foreground, cx))
                    .into_any_element(),
            )
        } else {
            self.workspace.as_deref().map(|path| {
                // A filesystem root has no final component. Show the path itself in that case.
                let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned();
                self.render_welcome_workspace(name, colors.muted_foreground)
            })
        };

        // Compact signature: the brand V, a quiet hairline, and the greeting
        // share one row on the composer's leading spine. The mark owns the
        // measured id so the row still starts exactly where the draft below
        // starts in both normal and Zen modes; the workspace context keeps its
        // own row underneath so validation cannot shift the heading.
        let mark_size = gpui_kit::rems(1.25).to_pixels(window.rem_size()).as_f32();
        let signature = div()
            .id("welcome-signature")
            .test_support()
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap_3()
            .child(div().id("welcome-brand-mark").test_support().flex_shrink_0().child(brand_mark(
                "welcome-brand-glyph",
                mark_size,
                colors.primary,
            )))
            .child(
                div()
                    .id("welcome-signature-divider")
                    .test_support()
                    .w(px(1.))
                    .h(px(mark_size))
                    .flex_shrink_0()
                    .bg(themes::soft_edge_color(&colors)),
            )
            .child(
                div()
                    .id(title_id)
                    .test_support()
                    .role(gpui_kit::Role::Heading)
                    .aria_label(title.clone())
                    .min_w_0()
                    .truncate()
                    .text_xl()
                    .font_semibold()
                    .text_color(colors.foreground)
                    .child(title),
            );
        let prelude = div().flex().flex_col().items_start().gap_3().pb_6().child(signature).when(
            workspace_context.is_some() || self.workspace_selection_pending,
            |prelude| {
                // Keep the heading steady as folder validation gives way to
                // initialization and then the workspace name.
                prelude.child(
                    div()
                        .w_full()
                        .h_7()
                        .flex()
                        .items_center()
                        .text_sm()
                        .text_color(colors.muted_foreground)
                        .children(workspace_context),
                )
            },
        );
        // First-workspace onboarding sits below the steady heading row so it
        // never shifts the greeting or brand mark.
        let prelude =
            prelude.when(self.should_show_onboarding(), |prelude| prelude.child(self.render_onboarding_panel(cx)));

        div()
            .id(group_id)
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(group_label.to_string())
            .size_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .child(div().flex_1())
            .child(self.conversation_content_column(prelude).flex_shrink_0())
    }

    fn render_welcome_workspace(&self, name: String, color: gpui_kit::Hsla) -> AnyElement {
        let label = rust_i18n::t!("app.transcript.change_workspace", name = name.as_str()).to_string();
        desktop_ghost_button("welcome-workspace")
            .text_sm()
            .text_color(color)
            .h_full()
            .p_0()
            .min_w_0()
            .max_w_full()
            .accessibility_label(label.clone())
            .tooltip_with_action(label, &ChooseWorkspace, Some(key_context::DESKTOP))
            .desktop_disabled(
                !self.workspace_switch_idle() || self.workspace_picker_open || self.retained_submission.is_some(),
            )
            .dispatches_desktop_action(ChooseWorkspace, self.focus_handle.clone())
            .child(div().min_w_0().truncate().child(name))
            .child(Icon::new(IconName::ChevronDown).size_3().flex_shrink_0())
            .into_any_element()
    }
}
