//! Inspector frame: tab state, dispatch, and the Changes panel with review.

use super::*;

/// Reuse fixed virtual-list geometry until its row count or height changes.
#[derive(Default)]
pub(super) struct FixedRowSizeCache {
    row_height_bits: u32,
    sizes: Rc<Vec<Size<Pixels>>>,
}

impl FixedRowSizeCache {
    pub(super) fn get(&mut self, row_count: usize, row_height: f32) -> Rc<Vec<Size<Pixels>>> {
        let row_height_bits = row_height.to_bits();
        if self.sizes.len() != row_count || self.row_height_bits != row_height_bits {
            self.sizes = Rc::new(vec![size(px(0.), px(row_height)); row_count]);
            self.row_height_bits = row_height_bits;
        }
        Rc::clone(&self.sizes)
    }
}

impl DesktopView {
    fn select_inspector_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.update_inspector_tab(index, false, Some(window), cx);
    }

    fn select_inspector_tab_from_keyboard(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.update_inspector_tab(index, true, Some(window), cx);
    }

    pub(super) fn set_inspector_tab(&mut self, tab: InspectorTab, cx: &mut Context<Self>) {
        if let Some(index) = InspectorTab::ALL.iter().position(|candidate| *candidate == tab) {
            self.update_inspector_tab(index, true, None, cx);
        }
    }

    fn update_inspector_tab(&mut self, index: usize, reveal: bool, focus: Option<&mut Window>, cx: &mut Context<Self>) {
        if let Some(tab) = InspectorTab::ALL.get(index) {
            self.inspector_tab = *tab;
            if reveal {
                self.inspector_tabs_scroll.scroll_to_item(index);
            }
            if let Some(window) = focus {
                self.inspector_focus.focus(window, cx);
            }
            cx.notify();
        }
    }

    fn cycle_inspector_tab(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let count = InspectorTab::ALL.len();
        let current = InspectorTab::ALL
            .iter()
            .position(|tab| *tab == self.inspector_tab)
            .unwrap_or_default();
        let index = if forward {
            (current + 1) % count
        } else {
            (current + count - 1) % count
        };
        self.select_inspector_tab_from_keyboard(index, window, cx);
    }

    /// Right-hand context inspector: derived Changes, Files, and Activity
    /// views over the same transcript the conversation renders.
    pub(super) fn render_inspector(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let colors = theme.colors;
        let panel = div()
            .w(px(self.inspector_width))
            .min_w(px(MIN_PANE_WIDTH))
            .h_full()
            .min_h_0()
            .flex()
            .flex_col()
            .rounded_lg()
            .bg(themes::mix(colors.background, colors.secondary, 0.3))
            .child(
                div()
                    .id("inspector-navigation")
                    .test_support()
                    .key_context("VTCodeInspector")
                    .role(gpui_kit::Role::Group)
                    .aria_label("Context inspector tabs")
                    .aria_description("Use Left and Right arrows, Home, or End to choose a tab.")
                    .border_1()
                    .border_color(colors.border.opacity(0.))
                    .focus_visible(|style| style.border_color(colors.ring))
                    .on_action(
                        cx.listener(|view, _: &NextInspectorTab, window, cx| {
                            view.cycle_inspector_tab(true, window, cx)
                        }),
                    )
                    .on_action(cx.listener(|view, _: &PreviousInspectorTab, window, cx| {
                        view.cycle_inspector_tab(false, window, cx)
                    }))
                    .on_action(cx.listener(|view, _: &FirstInspectorTab, window, cx| {
                        view.select_inspector_tab_from_keyboard(0, window, cx)
                    }))
                    .on_action(cx.listener(|view, _: &LastInspectorTab, window, cx| {
                        view.select_inspector_tab_from_keyboard(InspectorTab::ALL.len().saturating_sub(1), window, cx)
                    }))
                    .child(
                        div()
                            .id("inspector-tabs-focus")
                            .test_support()
                            .track_focus(&self.inspector_focus)
                            .child(
                                desktop_tab_bar("inspector-tabs")
                                    .track_scroll(&self.inspector_tabs_scroll)
                                    .selected_index(
                                        InspectorTab::ALL
                                            .iter()
                                            .position(|tab| *tab == self.inspector_tab)
                                            .unwrap_or_default(),
                                    )
                                    .on_click(cx.listener(|view, index: &usize, window, cx| {
                                        view.select_inspector_tab(*index, window, cx)
                                    }))
                                    .children(
                                        InspectorTab::ALL.map(|tab| Tab::new().label(tab.label()).cursor_pointer()),
                                    ),
                            ),
                    ),
            );

        let content = match self.inspector_tab {
            InspectorTab::Changes => self.render_changes_tab(&colors, cx),
            InspectorTab::Files => self.render_inspector_files_tab(cx),
            InspectorTab::Activity => self.render_activity_tab(&colors, cx),
            InspectorTab::Output => self.render_output_tab(cx),
            InspectorTab::Utilities => self.render_utilities(cx),
        };
        panel
            .child(content)
            .when(self.inspector_tab == InspectorTab::Changes, |panel| {
                panel
                    .key_context(key_context::DIFF_REVIEW)
                    .on_action(cx.listener(|view, _: &NextReviewFile, _, cx| view.navigate_change_file(true, cx)))
                    .on_action(cx.listener(|view, _: &PreviousReviewFile, _, cx| view.navigate_change_file(false, cx)))
            })
            .with_animation("inspector-fade", zen_fade_animation(cx.theme()), |el, delta| el.opacity(delta))
            .into_any_element()
    }

    fn render_changes_tab(
        &mut self,
        colors: &gpui_kit::component::theme::ThemeColor,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.review_open {
            return div()
                .p_4()
                .text_sm()
                .when(self.restored_review_history_incomplete, |container| {
                    container.child(warning_alert("review-history-warning", PARTIAL_REVIEW_HISTORY_MESSAGE, colors))
                })
                .child(rust_i18n::t!("app.workspace_review.expanded_in_inspector").to_string())
                .into_any_element();
        }
        let content = self.render_changes_content(false, colors, cx);
        let mut content_panel = div().w_full().flex_1().min_h_0().flex().flex_col();
        if self.restored_review_history_incomplete {
            content_panel = content_panel.child(div().px_3().pt_2().child(warning_alert(
                "review-history-warning",
                PARTIAL_REVIEW_HISTORY_MESSAGE,
                colors,
            )));
        }
        div()
            .id("inspector-changes-tab")
            .w_full()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div().px_3().py_2().child(
                    desktop_button("review-working-tree")
                        .label(rust_i18n::t!("app.workspace_review.review_working_tree").to_string())
                        .icon(IconName::GitCompare)
                        .tooltip_with_action(
                            rust_i18n::t!("app.workspace_review.review_working_tree").to_string(),
                            &OpenWorkspaceReview,
                            Some(key_context::DESKTOP),
                        )
                        .ghost()
                        .small()
                        .desktop_disabled(self.backend.is_none() || self.service_info.is_none())
                        .dispatches_desktop_action(OpenWorkspaceReview, self.focus_handle.clone()),
                ),
            )
            .child(content_panel.child(content))
            .test_support()
            .into_any_element()
    }

    fn render_changes_content(
        &mut self,
        expanded: bool,
        colors: &gpui_kit::component::theme::ThemeColor,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let diff_foreground =
            themes::diff_text_foreground(colors.background, colors.foreground, colors.success, colors.danger);
        let mut body = div().w_full().flex_1().min_h_0().flex().flex_col();

        if self.changes.is_empty() {
            return empty_state(
                "No changes yet",
                "Edits made during this session will appear here for review.",
                IconName::GitCompare,
            )
            .into_any_element();
        }

        let totals = self.changes.totals();
        body = body.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_3()
                .py_2()
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_w_0()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .child(format!("{} files", self.changes.entries().len()))
                        .when_some(totals, |summary, (additions, deletions)| {
                            summary
                                .child(div().text_color(colors.success).child(format!("+{additions}")))
                                .child(div().text_color(colors.danger).child(format!("−{deletions}")))
                        })
                        .when(totals.is_none(), |summary| {
                            summary.child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(colors.muted_foreground)
                                    .child("Partial totals"),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .gap_1()
                        .child(
                            desktop_button("previous-change-file")
                                .icon(IconName::ChevronLeft)
                                .accessibility_label("Previous changed file")
                                .tooltip_with_action(
                                    "Previous changed file",
                                    &PreviousReviewFile,
                                    Some(key_context::DIFF_REVIEW),
                                )
                                .ghost()
                                .small()
                                .desktop_disabled(self.changes.preview_count() < 2)
                                .on_click(cx.listener(|view, _, _, cx| view.navigate_change_file(false, cx))),
                        )
                        .child(
                            desktop_button("next-change-file")
                                .icon(IconName::ChevronRight)
                                .accessibility_label("Next changed file")
                                .tooltip_with_action(
                                    "Next changed file",
                                    &NextReviewFile,
                                    Some(key_context::DIFF_REVIEW),
                                )
                                .ghost()
                                .small()
                                .desktop_disabled(self.changes.preview_count() < 2)
                                .on_click(cx.listener(|view, _, _, cx| view.navigate_change_file(true, cx))),
                        ),
                ),
        );

        let change_row_height = scale_row_height_for_ui_font(CHANGE_ROW_HEIGHT, cx.theme().font_size);
        let change_sizes = self.change_row_sizes.get(self.changes.entries().len(), change_row_height);
        let max_height = if expanded { 220. } else { 112. };
        let mut visible_rows = 0;
        let mut list_height = 0.;
        while visible_rows < self.changes.entries().len() && list_height + change_row_height <= max_height {
            visible_rows += 1;
            list_height += change_row_height;
        }
        if visible_rows == 0 {
            list_height = change_row_height;
        }
        let change_row_colors = *colors;
        body = body.child(
            div()
                .id("change-list")
                .test_support()
                .role(gpui_kit::Role::List)
                .aria_label("Changed files")
                .relative()
                .w_full()
                .h(px(list_height))
                .max_h(px(max_height))
                .min_h_0()
                .overflow_hidden()
                .child(
                    v_virtual_list(
                        cx.entity(),
                        "change-virtual-list",
                        change_sizes,
                        move |view, visible_range, _, cx| {
                            visible_range
                                .filter_map(|index| {
                                    let entry = view.changes.entries().get(index)?;
                                    let path = entry.path.clone();
                                    let selected = view.selected_change.as_deref() == Some(entry.path.as_str());
                                    let preview_available = view.changes.has_preview(&path);
                                    let file_summary = match (entry.additions, entry.deletions) {
                                        (Some(additions), Some(deletions)) => format!("+{additions} −{deletions}"),
                                        _ if entry.preview_incomplete => "Partial diff".to_string(),
                                        _ => "Preview unavailable".to_string(),
                                    };
                                    let kind_label = match entry.kind {
                                        DiffFileKind::Add => ("A", change_row_colors.success),
                                        DiffFileKind::Delete => ("D", change_row_colors.danger),
                                        DiffFileKind::Update => ("M", change_row_colors.muted_foreground),
                                    };
                                    let kind_name = match entry.kind {
                                        DiffFileKind::Add => "Added",
                                        DiffFileKind::Delete => "Deleted",
                                        DiffFileKind::Update => "Modified",
                                    };
                                    let row_accessibility_label = format!("{kind_name} {path}, {file_summary}");
                                    Some(
                                        div()
                                            .id(opaque_path_element_id("change-list-item", &path))
                                            .test_support()
                                            .role(gpui_kit::Role::ListItem)
                                            .aria_label(row_accessibility_label)
                                            .aria_position_in_set(index + 1)
                                            .aria_size_of_set(view.changes.entries().len())
                                            .aria_selected(selected)
                                            .h(px(change_row_height))
                                            .w_full()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .px_2()
                                            .rounded_md()
                                            .when(selected, |row| row.bg(change_row_colors.secondary))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_semibold()
                                                    .w(px(14.))
                                                    .text_color(kind_label.1)
                                                    .child(kind_label.0),
                                            )
                                            .child(div().flex_1().min_w_0().text_sm().truncate().child(path.clone()))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(change_row_colors.muted_foreground)
                                                    .child(file_summary),
                                            )
                                            .child(
                                                desktop_button(opaque_path_element_id("change-row", &path))
                                                    .icon(IconName::ChevronRight)
                                                    .accessibility_label(format!(
                                                        "{} {path}{}",
                                                        if preview_available {
                                                            "Review"
                                                        } else {
                                                            "Diff preview unavailable for"
                                                        },
                                                        if selected { " (selected)" } else { "" },
                                                    ))
                                                    .tooltip(if preview_available {
                                                        path.clone()
                                                    } else {
                                                        format!("Diff preview unavailable for {path}")
                                                    })
                                                    .ghost()
                                                    .small()
                                                    .desktop_disabled(!preview_available)
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.select_change(path.clone(), cx)
                                                    })),
                                            )
                                            .into_any_element(),
                                    )
                                })
                                .collect::<Vec<_>>()
                        },
                    )
                    .track_scroll(&self.change_list_scroll),
                )
                .child(Scrollbar::vertical(&self.change_list_scroll)),
        );

        // Full syntax-highlighted diff of the selected file.
        if let (Some(path), Some(viewer)) = (self.selected_change.clone(), self.change_viewer.clone()) {
            let patch = self.changes.patch_for(&self.transcript, &path);
            let rel_path = path.clone();
            let mut viewer_panel = div().w_full().flex_1().min_h_0().flex().flex_col();
            let mut header = div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_3()
                .py_2()
                .child(div().flex_1().min_w_0().truncate().text_sm().font_medium().child(path));
            if expanded {
                let mode = viewer.read(cx).mode();
                for (id, label, target) in [
                    ("review-unified", rust_i18n::t!("app.workspace_review.unified").to_string(), DiffMode::Unified),
                    ("review-split", rust_i18n::t!("app.workspace_review.split").to_string(), DiffMode::Split),
                ] {
                    let state = viewer.clone();
                    let accessible_label =
                        format!("{label} diff view{}", if mode == target { " (selected)" } else { "" });
                    header = header.child(
                        desktop_button(id)
                            .label(label)
                            .accessibility_label(accessible_label)
                            .ghost()
                            .small()
                            .when(mode == target, |button| button.bg(colors.secondary))
                            .on_click(move |_, _, cx| state.update(cx, |state, cx| state.set_mode(target, cx))),
                    );
                }
                header = header
                    .child(
                        desktop_button("review-previous-change")
                            .icon(IconName::ArrowUp)
                            .accessibility_label("Previous change")
                            .tooltip_with_action(
                                "Previous change",
                                &PreviousReviewChange,
                                Some(key_context::DIFF_REVIEW),
                            )
                            .ghost()
                            .small()
                            .on_click(cx.listener(|view, _, _, cx| view.navigate_review_change(false, cx))),
                    )
                    .child(
                        desktop_button("review-next-change")
                            .icon(IconName::ArrowDown)
                            .accessibility_label("Next change")
                            .tooltip_with_action("Next change", &NextReviewChange, Some(key_context::DIFF_REVIEW))
                            .ghost()
                            .small()
                            .on_click(cx.listener(|view, _, _, cx| view.navigate_review_change(true, cx))),
                    );
            } else {
                header = header.child(
                    desktop_button("expand-change-review")
                        .icon(IconName::Maximize2)
                        .accessibility_label("Expand diff review")
                        .tooltip("Expand diff review")
                        .ghost()
                        .small()
                        .on_click(cx.listener(|view, _, window, cx| view.open_review(window, cx))),
                );
            }
            if let Some(patch) = patch {
                header = header.child(
                    desktop_button("copy-change-patch")
                        .icon(IconName::Copy)
                        .accessibility_label("Copy patch")
                        .tooltip("Copy patch")
                        .ghost()
                        .small()
                        .on_click(move |_, window, app| {
                            app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(patch.to_string()));
                            super::super::notifications::copied_to_clipboard(window, app);
                        }),
                );
            }
            header = header.child(
                desktop_button("open-change-file")
                    .icon(IconName::ExternalLink)
                    .accessibility_label("Open file")
                    .tooltip("Open file")
                    .ghost()
                    .small()
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.open_file_preview(rel_path.clone(), window, cx)),
                    ),
            );
            viewer_panel = viewer_panel.child(header).child(
                div()
                    .id("change-diff-view")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        Diff::new(&viewer)
                            .line_number(true)
                            .soft_wrap(expanded)
                            .text_color(diff_foreground),
                    ),
            );
            body = body.child(viewer_panel);
        }
        body.into_any_element()
    }

    pub(super) fn render_change_review(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let workspace_review = self.workspace_review.is_some();
        let content = if workspace_review {
            self.render_workspace_review_content(cx)
        } else {
            self.render_changes_content(true, &colors, cx)
        };
        let title = if workspace_review {
            rust_i18n::t!("app.workspace_review.title").to_string()
        } else {
            rust_i18n::t!("app.workspace_review.session_title").to_string()
        };
        let description = if workspace_review {
            rust_i18n::t!("app.workspace_review.description").to_string()
        } else {
            rust_i18n::t!("app.workspace_review.session_description").to_string()
        };
        div()
            .absolute()
            .inset_0()
            .p_6()
            .bg(colors.background.opacity(0.85))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, cx.listener(|view, _, window, cx| view.close_review(window, cx)))
            .child(
                div()
                    .w_full()
                    .h_full()
                    .max_w(px(1500.))
                    .child(
                        div()
                            .id("diff-review-panel")
                            .test_support()
                            .role(gpui_kit::Role::Dialog)
                            .aria_label(title.clone())
                            .key_context(key_context::DIFF_REVIEW)
                            .size_full()
                            .flex()
                            .flex_col()
                            .rounded_xl()
                            .bg(colors.secondary)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_action(
                                cx.listener(|view, _: &NextReviewChange, _, cx| view.navigate_review_change(true, cx)),
                            )
                            .on_action(cx.listener(|view, _: &PreviousReviewChange, _, cx| {
                                view.navigate_review_change(false, cx)
                            }))
                            .on_action(cx.listener(|view, _: &NextReviewFile, _, cx| {
                                view.navigate_workspace_review_file(true, cx)
                            }))
                            .on_action(cx.listener(|view, _: &PreviousReviewFile, _, cx| {
                                view.navigate_workspace_review_file(false, cx)
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .px_4()
                                    .py_3()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(div().text_lg().font_semibold().child(title))
                                            .child(
                                                div().text_xs().text_color(colors.muted_foreground).child(description),
                                            ),
                                    )
                                    .child(
                                        desktop_button("close-diff-review")
                                            .icon(IconName::X)
                                            .accessibility_label(
                                                rust_i18n::t!("app.workspace_review.close_review").to_string(),
                                            )
                                            .tooltip_with_action(
                                                rust_i18n::t!("app.workspace_review.close_review").to_string(),
                                                &DismissOverlay,
                                                Some(key_context::DESKTOP),
                                            )
                                            .ghost()
                                            .on_click(cx.listener(|view, _, window, cx| view.close_review(window, cx))),
                                    ),
                            )
                            .child(content),
                    )
                    .focus_trap("diff-review-trap", &self.review_focus),
            )
            .into_any_element()
    }

    fn navigate_review_change(&self, next: bool, cx: &mut Context<Self>) {
        let viewer = match &self.workspace_review {
            Some(review) => review.viewer.as_ref(),
            None => self.change_viewer.as_ref(),
        };
        if let Some(viewer) = viewer {
            viewer.update(cx, |viewer, cx| {
                if next {
                    viewer.next_change(cx);
                } else {
                    viewer.previous_change(cx);
                }
            });
        }
    }
}
