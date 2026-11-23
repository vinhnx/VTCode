//! Tool-row rendering extracted from the transcript renderer.

use super::*;

impl DesktopView {
    pub(super) fn render_tool_item(
        &self,
        item: &TranscriptItem,
        tool_tail: Option<gpui_kit::SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let id = item.id.clone();
        let status = if item.has_activity_status {
            item.text.lines().next().unwrap_or_default().to_string()
        } else {
            rust_i18n::t!("app.transcript.archived_output").to_string()
        };
        let state = ActivityState::from_status(&status);
        let icon_color = Self::activity_status_color(state, colors);
        let kind_icon = Self::transcript_tool_kind_icon(item.tool_name.as_deref(), item.activity_category);
        let change_id = format!("{}:changes", item.id);
        let edit_path = item.edit_path.as_ref().map(ToString::to_string);
        let header = if let Some(path) = edit_path {
            let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
            let title = item.title.to_string();
            let suffix = title
                .strip_prefix("Edited ")
                .and_then(|rest| rest.strip_prefix(&name))
                .unwrap_or("")
                .to_string();
            let toggle_id = item.id.clone();
            let chevron_id = item.id.clone();
            let item_id = item.id.clone();
            let details_label = || {
                rust_i18n::t!(
                    "app.transcript.details_label",
                    toggle = if item.expanded {
                        rust_i18n::t!("app.transcript.hide_details").to_string()
                    } else {
                        rust_i18n::t!("app.transcript.show_details").to_string()
                    },
                    title = title.as_str(),
                    status = status.as_str()
                )
                .to_string()
            };
            div()
                .w_full()
                .flex()
                .items_center()
                .min_w_0()
                .gap(themes::layout::gap::COMPACT)
                .child(
                    desktop_ghost_button(format!("tool-{}", item.id))
                        .accessibility_label(details_label())
                        .small()
                        .text_color(themes::muted_text(&colors))
                        .child(transcript_icon_slot(Self::render_transcript_tool_indicator(
                            "tool-indicator",
                            &item.id,
                            state,
                            icon_color,
                            kind_icon,
                        )))
                        .child("Edited")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.transcript.toggle_tool(&toggle_id);
                            this.request_mcp_app_previews(&toggle_id, cx);
                            this.request_computer_previews(&toggle_id, cx);
                            cx.notify();
                        })),
                )
                .child(
                    desktop_ghost_button(format!("tool-file-{item_id}"))
                        .accessibility_label(format!("View changes: {path}"))
                        .small()
                        .text_color(themes::muted_text(&colors))
                        .min_w_0()
                        .child(
                            div()
                                .relative()
                                .min_w_0()
                                .truncate()
                                .child(name)
                                .child(Self::filename_dotted_underline(themes::muted_text(&colors))),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.select_change_from_item(&change_id, &path, cx) {
                                this.open_inspector_tab(InspectorTab::Changes, cx);
                            }
                        })),
                )
                .child(div().text_color(themes::muted_text(&colors)).child(suffix))
                .child(div().flex_1())
                .when(state != ActivityState::Completed, |row| {
                    row.child(
                        div()
                            .text_size(themes::layout::typography::ROW)
                            .text_color(themes::muted_text(&colors))
                            .child(status.clone()),
                    )
                })
                .child(
                    desktop_ghost_button(format!("tool-chevron-{item_id}"))
                        .accessibility_label(details_label())
                        .small()
                        .text_color(themes::muted_text(&colors))
                        .child(transcript_chevron_lane(item.expanded))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.transcript.toggle_tool(&chevron_id);
                            this.request_mcp_app_previews(&chevron_id, cx);
                            this.request_computer_previews(&chevron_id, cx);
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else {
            desktop_ghost_button(format!("tool-{}", item.id))
                .accessibility_label(
                    rust_i18n::t!(
                        "app.transcript.details_label",
                        toggle = if item.expanded {
                            rust_i18n::t!("app.transcript.hide_details").to_string()
                        } else {
                            rust_i18n::t!("app.transcript.show_details").to_string()
                        },
                        title = item.title.as_str(),
                        status = status.as_str()
                    )
                    .to_string(),
                )
                .tooltip(format!("{} · {status}", item.title))
                // Small keeps the shared title spine: every transcript
                // disclosure row (tool, activity, notice, turn header) starts
                // its title at the same inset + icon slot + gap.
                .small()
                .w_full()
                .justify_start()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(themes::layout::gap::TITLE)
                        .min_w_0()
                        .w_full()
                        .child(transcript_icon_slot(Self::render_transcript_tool_indicator(
                            "tool-indicator",
                            &item.id,
                            state,
                            icon_color,
                            kind_icon,
                        )))
                        .child(Self::transcript_tool_title(
                            &item.id,
                            item.title.as_str(),
                            Self::tool_row_title_color(state, colors),
                            cx.theme().mono_font_family.clone(),
                        ))
                        // Completed rows drop the redundant visible label;
                        // the accessible name retains status for every row.
                        .when(state != ActivityState::Completed, |row| {
                            row.child(
                                div()
                                    .id(format!("tool-status-{}", item.id))
                                    .test_support()
                                    .flex_shrink_0()
                                    // Bound so backend strings ellipsize instead of
                                    // pushing the title and chevron out of the row.
                                    .max_w(px(320.))
                                    .truncate()
                                    .text_size(themes::layout::typography::ROW)
                                    .text_color(themes::muted_text(&colors))
                                    .child(status.to_string()),
                            )
                        })
                        .child(transcript_chevron_lane(item.expanded).text_color(themes::muted_text(&colors))),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.transcript.toggle_tool(&id);
                    // The fetch notifies when it queues; this covers the
                    // tier-off and already-loaded paths too.
                    this.request_mcp_app_previews(&id, cx);
                    this.request_computer_previews(&id, cx);
                    cx.notify();
                }))
                .into_any_element()
        };
        let mut panel = div()
            .w_full()
            .max_w(px(self.content_width()))
            .flex()
            .flex_col()
            .gap(themes::layout::gap::COMPACT)
            .child(header);
        // Grouped rows nest under their shared header; single tool rows stay flush.
        panel = panel.when(item.activity_grouped, |panel| panel.pl_6());
        // Standalone tracker rows show the current snapshot inline. Grouped
        // tracker calls stay as compact chronology rows; the Tasks panel owns
        // the full snapshot for that activity group.
        if item.activity_category == Some(ToolCategory::Tracker)
            && !item.activity_grouped
            && let Some(block) = self.render_tracker_inline_block(item.id.as_str(), item.expanded, &colors)
        {
            panel = panel.child(div().id(format!("tracker-inline-{}", item.id)).test_support().child(block));
        }
        if item.expanded {
            let output = item.output.as_ref().cloned().unwrap_or_else(|| item.text.clone());
            let full_output = inspector::output_with_truncation_notice(&output, item.output_truncated);
            panel = panel.child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(themes::layout::gap::COMPACT)
                    .when_some(item.command.as_ref(), |panel, command| {
                        panel.child(self.render_tool_command(item, command, cx))
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(themes::layout::gap::COMPACT)
                            .child(
                                desktop_ghost_button(format!("copy-tool-output-{}", item.id))
                                    .icon(IconName::Copy)
                                    .accessibility_label(
                                        rust_i18n::t!("app.transcript.copy_output_for", title = item.title.to_string())
                                            .to_string(),
                                    )
                                    .tooltip(if item.output_truncated {
                                        rust_i18n::t!("app.inspector.copy_loaded_notice").to_string()
                                    } else {
                                        rust_i18n::t!("app.transcript.copy_full_output").to_string()
                                    })
                                    .xsmall()
                                    .on_click(move |_, window, app| {
                                        app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                            full_output.to_string(),
                                        ));
                                        crate::desktop::notifications::copied_to_clipboard(window, app);
                                    }),
                            )
                            .when(item.has_output() || item.has_archived_output(), |row| {
                                row.child(Self::tool_output_cta(item, cx))
                            })
                            .when(item.is_running_tool() && item.activity_category == Some(ToolCategory::Run), |row| {
                                row.child(Self::tool_terminal_cta(item, cx))
                            }),
                    )
                    .when(item.shows_inline_tail(), |panel| {
                        let output_excerpt = inspector::output_with_truncation_notice(
                            &tool_tail.unwrap_or_else(|| inspector::compact_output_tail(&output)),
                            item.output_truncated,
                        );
                        panel.child(
                            div()
                                .id(format!("tool-output-{}", item.id))
                                .role(gpui_kit::Role::Label)
                                .aria_label(output_excerpt.clone())
                                .test_support()
                                // Align under the title: status icon plus the row gap.
                                .ml_6()
                                .px_3()
                                .py_2()
                                .rounded_md()
                                .bg(colors.secondary)
                                .overflow_hidden()
                                .overflow_x_scroll()
                                .whitespace_nowrap()
                                .text_color(colors.muted_foreground)
                                .font_family(cx.theme().mono_font_family.clone())
                                .text_size(cx.theme().mono_font_size)
                                .child(render_output_text(&output_excerpt)),
                        )
                    })
                    .child(self.render_mcp_app_previews(item, cx))
                    .child(self.render_computer_previews(item, cx)),
            );
        }
        panel.into_any_element()
    }
}
