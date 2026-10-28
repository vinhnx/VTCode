//! Read-only Git review; snapshots never enter the agent transcript.

use gpui_kit::assets::IconName;
use gpui_kit::base::{Disableable, StyledExt};
use gpui_kit::component::Sizable;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::diff::{Diff, DiffFile, DiffMode, DiffState};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClipboardItem, Context, Entity, TestSupportExt, Window, div, px};
use serde_json::json;
use vtcode_desktop_protocol::{self as protocol, WorkspaceReviewResult};

use super::{
    DesktopButtonExt, DesktopView, NextReviewFile, PaletteAction, PaletteKind, PreviousReviewFile, RefreshWorkspace,
    command, desktop_button, empty_state, skeleton_line, uuid_like_id, warning_alert,
};

pub(super) struct WorkspaceReviewState {
    pub(super) request_id: String,
    pub(super) scope_input: Entity<InputState>,
    pub(super) workspace: String,
    pub(super) pending: bool,
    pub(super) snapshot: Option<WorkspaceReviewResult>,
    pub(super) viewer: Option<Entity<DiffState>>,
    pub(super) selected_file: Option<String>,
    include_untracked: bool,
    error: Option<String>,
    mode: DiffMode,
}

fn review_message(
    id: &'static str,
    title: impl Into<gpui_kit::SharedString>,
    description: impl Into<gpui_kit::SharedString>,
    icon: IconName,
) -> AnyElement {
    let title = title.into();
    let description = description.into();
    div()
        .id(id)
        .test_support()
        .role(gpui_kit::Role::Status)
        .aria_label(title.clone())
        .aria_description(description.clone())
        .flex_1()
        .min_h_0()
        .child(empty_state(title, description, icon))
        .into_any_element()
}

impl DesktopView {
    pub(super) fn open_workspace_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(info) = self.service_info.as_ref().filter(|_| self.backend.is_some()) else {
            return;
        };
        self.workspace_review = Some(WorkspaceReviewState {
            request_id: String::new(),
            scope_input: cx.new(|cx| InputState::new(window, cx)),
            workspace: info.workspace.clone(),
            pending: false,
            snapshot: None,
            viewer: None,
            selected_file: None,
            include_untracked: false,
            error: None,
            mode: DiffMode::Unified,
        });
        self.show_review(window, cx);
    }

    pub(super) fn refresh_workspace_review(&mut self, cx: &mut Context<Self>) {
        let Some(review) = self.workspace_review.as_mut().filter(|review| !review.pending) else {
            return;
        };
        let request_id = uuid_like_id();
        let scope = review.scope_input.read(cx).value().to_string();
        let include_untracked = review.include_untracked;
        if let Some(viewer) = &review.viewer {
            review.mode = viewer.read(cx).mode();
        }
        review.request_id = request_id.clone();
        review.pending = true;
        review.error = None;
        if !self.request(
            protocol::method::WORKSPACE_REVIEW,
            json!({ "requestId": request_id, "scope": scope, "includeUntracked": include_untracked }),
        ) {
            self.fail_workspace_review(rust_i18n::t!("app.errors.request_queue_unavailable").to_string());
        }
        cx.notify();
    }

    pub(super) fn fail_workspace_review(&mut self, error: String) {
        if let Some(review) = self.workspace_review.as_mut().filter(|review| review.pending) {
            review.pending = false;
            review.error = Some(error);
        }
    }

    pub(super) fn accept_workspace_review(&mut self, result: WorkspaceReviewResult, cx: &mut Context<Self>) {
        let Some(review) = self.workspace_review.as_mut().filter(|review| {
            review.pending && review.request_id == result.request_id && review.workspace == result.workspace
        }) else {
            return;
        };
        review.pending = false;
        review.viewer = if result.truncated {
            None
        } else {
            result
                .patch
                .as_deref()
                .filter(|patch| !patch.is_empty())
                .and_then(|patch| DiffFile::parse(patch).ok())
                .filter(|files| !files.is_empty())
                .map(|files| cx.new(|cx| DiffState::new(files, cx).with_mode(review.mode)))
        };
        if let Some(viewer) = &review.viewer {
            let files = viewer.read(cx).files();
            let selected = review
                .selected_file
                .as_deref()
                .and_then(|path| files.iter().find(|file| file.path().as_str() == path))
                .or_else(|| files.first())
                .map(|file| file.path().to_string());
            if let Some(path) = &selected {
                viewer.update(cx, |viewer, cx| viewer.scroll_to_file(path, cx));
            }
            review.selected_file = selected;
        } else {
            review.selected_file = None;
        }
        review.snapshot = Some(result);
        review.error = None;
    }

    pub(super) fn navigate_workspace_review_file(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(review) = self.workspace_review.as_mut() else {
            return;
        };
        let Some(viewer) = &review.viewer else {
            return;
        };
        let files = viewer.read(cx).files();
        if files.len() < 2 {
            return;
        }
        let current = files
            .iter()
            .position(|file| Some(file.path().as_str()) == review.selected_file.as_deref())
            .unwrap_or(0);
        let next = if forward {
            (current + 1) % files.len()
        } else if current == 0 {
            files.len() - 1
        } else {
            current - 1
        };
        let Some(file) = files.get(next) else {
            return;
        };
        let path = file.path().to_string();
        viewer.update(cx, |viewer, cx| viewer.scroll_to_file(&path, cx));
        review.selected_file = Some(path);
        cx.notify();
    }

    pub(super) fn open_workspace_review_file_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entries) = self
            .workspace_review
            .as_ref()
            .and_then(|review| review.viewer.as_ref())
            .map(|viewer| {
                viewer
                    .read(cx)
                    .files()
                    .iter()
                    .map(|file| {
                        let path = file.path().to_string();
                        let mut entry = command(path.clone(), PaletteAction::SelectWorkspaceReviewFile(path));
                        entry.description = Some(format!("+{} · −{} lines", file.additions(), file.deletions()));
                        entry.keywords = vec!["working tree".into(), "changed file".into()];
                        entry
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|entries| !entries.is_empty())
        else {
            return;
        };
        self.show_palette(
            PaletteKind::ReviewFiles,
            vec![(rust_i18n::t!("app.workspace_review.changed_files").to_string(), entries)],
            window,
            cx,
        );
    }

    pub(super) fn select_workspace_review_file(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(viewer) = self
            .workspace_review
            .as_ref()
            .and_then(|review| review.viewer.as_ref())
            .cloned()
        else {
            return;
        };
        if !viewer.read(cx).files().iter().any(|file| file.path().as_str() == path) {
            return;
        }
        viewer.update(cx, |viewer, cx| viewer.scroll_to_file(&path, cx));
        if let Some(review) = self.workspace_review.as_mut() {
            review.selected_file = Some(path);
        }
        cx.notify();
    }

    fn set_workspace_review_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        if let Some(review) = self.workspace_review.as_mut() {
            review.mode = mode;
            if let Some(viewer) = &review.viewer {
                viewer.update(cx, |viewer, cx| viewer.set_mode(mode, cx));
            }
        }
    }

    pub(super) fn render_workspace_review_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(review) = &self.workspace_review else {
            return div().into_any_element();
        };
        let colors = cx.theme().colors;
        let diff_foreground =
            super::themes::diff_text_foreground(colors.background, colors.foreground, colors.success, colors.danger);
        let mut body = div().flex().flex_col().w_full().flex_1().min_h_0();
        let can_copy_patch = review
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.patch.as_deref())
            .is_some_and(|patch| !patch.is_empty());
        let has_viewer = review.viewer.is_some();
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .px_4()
                .py_2()
                .child(
                    div()
                        .text_sm()
                        .child(rust_i18n::t!("app.workspace_review.folder_optional").to_string()),
                )
                .child(
                    Input::new(&review.scope_input)
                        .accessibility_id("workspace-review-scope")
                        .aria_label(rust_i18n::t!("app.workspace_review.folder_optional").to_string())
                        .bg(colors.popover)
                        .disabled(review.pending)
                        .when(review.pending, |input| input.cursor_default())
                        .when(!review.pending, |input| input.cursor_text())
                        .flex_1(),
                ),
        );
        body = body.child(
            div().px_4().py_2().child(
                Checkbox::new("workspace-review-include-untracked")
                    .label(rust_i18n::t!("app.workspace_review.include_untracked").to_string())
                    .checked(review.include_untracked)
                    .disabled(review.pending)
                    .when(review.pending, |checkbox| checkbox.cursor_default())
                    .when(!review.pending, |checkbox| checkbox.cursor_pointer())
                    .on_change(cx.listener(|view, checked: &bool, _, cx| {
                        if let Some(review) = view.workspace_review.as_mut().filter(|review| !review.pending) {
                            review.include_untracked = *checked;
                            cx.notify();
                        }
                    })),
            ),
        );
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_4()
                .py_2()
                .child(
                    desktop_button("refresh-workspace-review")
                        .tooltip_with_action(
                            rust_i18n::t!("app.workspace_review.refresh_tooltip").to_string(),
                            &RefreshWorkspace,
                            Some("VTCodeDesktop"),
                        )
                        .label(if review.pending {
                            rust_i18n::t!("app.workspace_review.refreshing").to_string()
                        } else if review.snapshot.is_none() {
                            rust_i18n::t!("app.workspace_review.load_changes").to_string()
                        } else {
                            rust_i18n::t!("app.workspace_review.refresh").to_string()
                        })
                        .small()
                        .desktop_disabled(review.pending)
                        .dispatches_desktop_action(RefreshWorkspace, self.focus_handle.clone()),
                )
                .child(
                    div()
                        .id("workspace-review-layout-options")
                        .test_support()
                        .role(gpui_kit::Role::Group)
                        .aria_label(rust_i18n::t!("app.workspace_review.diff_layout").to_string())
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            desktop_button("workspace-review-unified")
                                .label(rust_i18n::t!("app.workspace_review.unified").to_string())
                                .ghost()
                                .small()
                                .toggled(review.mode == DiffMode::Unified)
                                .desktop_disabled(!has_viewer)
                                .when(review.mode == DiffMode::Unified, |button| button.bg(colors.secondary))
                                .on_click(
                                    cx.listener(|view, _, _, cx| view.set_workspace_review_mode(DiffMode::Unified, cx)),
                                ),
                        )
                        .child(
                            desktop_button("workspace-review-split")
                                .label(rust_i18n::t!("app.workspace_review.split").to_string())
                                .ghost()
                                .small()
                                .toggled(review.mode == DiffMode::Split)
                                .desktop_disabled(!has_viewer)
                                .when(review.mode == DiffMode::Split, |button| button.bg(colors.secondary))
                                .on_click(
                                    cx.listener(|view, _, _, cx| view.set_workspace_review_mode(DiffMode::Split, cx)),
                                ),
                        ),
                )
                .child(div().flex_1())
                .child(
                    desktop_button("copy-workspace-review")
                        .label(rust_i18n::t!("app.workspace_review.copy_preview").to_string())
                        .icon(IconName::Copy)
                        .ghost()
                        .small()
                        .desktop_disabled(!can_copy_patch)
                        .on_click(cx.listener(|view, _, window, cx| {
                            let Some(patch) = view
                                .workspace_review
                                .as_ref()
                                .and_then(|review| review.snapshot.as_ref())
                                .and_then(|snapshot| snapshot.patch.as_deref())
                                .filter(|patch| !patch.is_empty())
                            else {
                                return;
                            };
                            cx.write_to_clipboard(ClipboardItem::new_string(patch.to_owned()));
                            super::super::notifications::copied_to_clipboard(window, cx);
                        })),
                ),
        );
        if review.pending && review.snapshot.is_none() {
            let loading = div()
                .id("workspace-review-loading")
                .test_support()
                .role(gpui_kit::Role::Status)
                .aria_label(rust_i18n::t!("app.workspace_review.reading_git_changes").to_string())
                .aria_description(rust_i18n::t!("app.workspace_review.capturing_snapshot").to_string())
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap_4()
                .p_4()
                .child(
                    div()
                        .font_medium()
                        .child(rust_i18n::t!("app.workspace_review.reading_git_changes_progress").to_string()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(colors.muted_foreground)
                        .child(rust_i18n::t!("app.workspace_review.capturing_snapshot").to_string()),
                )
                .child(
                    div()
                        .w_full()
                        .rounded_lg()
                        .bg(colors.secondary)
                        .p_4()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(skeleton_line("workspace-review-placeholder-file", px(180.), cx))
                        .child(skeleton_line("workspace-review-placeholder-hunk", px(120.), cx))
                        .child(skeleton_line("workspace-review-placeholder-line-1", px(320.), cx))
                        .child(skeleton_line("workspace-review-placeholder-line-2", px(248.), cx))
                        .child(skeleton_line("workspace-review-placeholder-line-3", px(286.), cx))
                        .child(skeleton_line("workspace-review-placeholder-line-4", px(196.), cx)),
                );
            return body.child(loading).into_any_element();
        }
        if review.pending {
            body = body.child(
                div()
                    .id("workspace-review-refreshing")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(rust_i18n::t!("app.workspace_review.refreshing").to_string())
                    .aria_description(rust_i18n::t!("app.workspace_review.refreshing_status").to_string())
                    .px_4()
                    .py_2()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child(rust_i18n::t!("app.workspace_review.refreshing_status").to_string()),
            );
        }
        if let Some(viewer) = &review.viewer {
            let files = viewer.read(cx).files();
            let (additions, deletions, position) = files.iter().enumerate().fold(
                (0_usize, 0_usize, None),
                |(additions, deletions, position), (index, file)| {
                    let position = position
                        .or_else(|| (Some(file.path().as_str()) == review.selected_file.as_deref()).then_some(index));
                    (additions.saturating_add(file.additions()), deletions.saturating_add(file.deletions()), position)
                },
            );
            let file_count = files.len();
            let file_label = if file_count == 1 {
                rust_i18n::t!("app.workspace_review.file").to_string()
            } else {
                rust_i18n::t!("app.workspace_review.files").to_string()
            };
            let count_label =
                rust_i18n::t!("app.workspace_review.changed_files_count", count = file_count, files = file_label)
                    .to_string();
            let summary = rust_i18n::t!(
                "app.workspace_review.changed_files_summary",
                count = file_count,
                files = file_label,
                additions = additions,
                deletions = deletions
            )
            .to_string();
            body = body.child(
                div()
                    .id("workspace-review-summary")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(summary.clone())
                    .aria_description(rust_i18n::t!("app.workspace_review.added_deleted_description").to_string())
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .py_1()
                    .child(Tag::secondary().outline().small().child(count_label))
                    .child(Tag::success().outline().small().child(format!("+{additions}")))
                    .child(Tag::danger().outline().small().child(format!("−{deletions}"))),
            );
            let label = rust_i18n::t!(
                "app.workspace_review.file_position",
                current = position.map_or(0, |index| index + 1),
                total = files.len(),
                path = review.selected_file.as_deref().unwrap_or("")
            )
            .to_string();
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .py_2()
                    .child(
                        desktop_button("workspace-review-previous-file")
                            .label(rust_i18n::t!("app.workspace_review.previous_file").to_string())
                            .tooltip_with_action(
                                rust_i18n::t!("app.workspace_review.previous_file").to_string(),
                                &PreviousReviewFile,
                                Some("VTCodeDiffReview"),
                            )
                            .ghost()
                            .small()
                            .desktop_disabled(files.len() < 2)
                            .on_click(cx.listener(|view, _, _, cx| view.navigate_workspace_review_file(false, cx))),
                    )
                    .child(
                        desktop_button("workspace-review-current-file")
                            .label(label.clone())
                            .icon(IconName::ChevronDown)
                            .tooltip(rust_i18n::t!("app.workspace_review.choose_changed_file").to_string())
                            .ghost()
                            .small()
                            .flex_1()
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.open_workspace_review_file_palette(window, cx);
                            })),
                    )
                    .child(
                        desktop_button("workspace-review-next-file")
                            .label(rust_i18n::t!("app.workspace_review.next_file").to_string())
                            .tooltip_with_action(
                                rust_i18n::t!("app.workspace_review.next_file").to_string(),
                                &NextReviewFile,
                                Some("VTCodeDiffReview"),
                            )
                            .ghost()
                            .small()
                            .desktop_disabled(files.len() < 2)
                            .on_click(cx.listener(|view, _, _, cx| view.navigate_workspace_review_file(true, cx))),
                    ),
            );
        }
        if let Some(error) = &review.error {
            if review.snapshot.is_none() {
                return body
                    .child(review_message(
                        "workspace-review-unavailable",
                        rust_i18n::t!("app.workspace_review.git_review_unavailable").to_string(),
                        error.clone(),
                        IconName::GitCompare,
                    ))
                    .into_any_element();
            }
            body = body.child(warning_alert(
                "workspace-review-refresh-warning",
                rust_i18n::t!("app.workspace_review.refresh_failed", error = error).to_string(),
                &colors,
            ));
        }
        let Some(snapshot) = &review.snapshot else {
            return body
                .child(review_message(
                    "workspace-review-ready",
                    rust_i18n::t!("app.workspace_review.choose_what_to_review").to_string(),
                    rust_i18n::t!("app.workspace_review.choose_what_to_review_hint").to_string(),
                    IconName::GitCompare,
                ))
                .into_any_element();
        };
        if snapshot.patch.is_some() {
            body = body.child(
                div()
                    .px_4()
                    .pt_2()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(rust_i18n::t!(
                        "app.workspace_review.folder_value",
                        folder = if snapshot.scope.is_empty() {
                            "."
                        } else {
                            &snapshot.scope
                        }
                    )),
            );
        }
        body = body.child(
            div()
                .px_4()
                .py_2()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(snapshot.note.clone()),
        );
        match snapshot.patch.as_deref() {
            None => body
                .child(review_message(
                    "workspace-review-unavailable",
                    rust_i18n::t!("app.workspace_review.git_review_unavailable").to_string(),
                    rust_i18n::t!("app.workspace_review.repository_unavailable_hint").to_string(),
                    IconName::GitCompare,
                ))
                .into_any_element(),
            Some("") => body
                .child(review_message(
                    "workspace-review-clean",
                    if snapshot.truncated {
                        rust_i18n::t!("app.workspace_review.no_previewable_changes").to_string()
                    } else {
                        rust_i18n::t!("app.workspace_review.no_tracked_changes").to_string()
                    },
                    if snapshot.include_untracked {
                        rust_i18n::t!("app.workspace_review.no_previewable_changes_hint").to_string()
                    } else {
                        rust_i18n::t!("app.workspace_review.tracked_files_clean_hint").to_string()
                    },
                    IconName::Check,
                ))
                .into_any_element(),
            Some(patch) => {
                if let Some(viewer) = &review.viewer {
                    body.child(
                        div()
                            .id("workspace-review-diff")
                            .test_support()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .child(Diff::new(viewer).line_number(true).soft_wrap(true).text_color(diff_foreground)),
                    )
                    .into_any_element()
                } else {
                    body.child(div().px_4().py_2().text_sm().child(
                        if snapshot.truncated && snapshot.include_untracked {
                            rust_i18n::t!("app.workspace_review.preview_incomplete").to_string()
                        } else if snapshot.truncated {
                            rust_i18n::t!("app.workspace_review.preview_truncated").to_string()
                        } else {
                            rust_i18n::t!("app.workspace_review.structured_preview_unavailable").to_string()
                        },
                    ))
                    .child(
                        div()
                            .id("workspace-review-raw")
                            .test_support()
                            .role(gpui_kit::Role::Label)
                            .aria_label(patch.to_string())
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_4()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(cx.theme().mono_font_size)
                            .child(patch.to_string()),
                    )
                    .into_any_element()
                }
            }
        }
    }
}
