//! Read-only Git review; snapshots never enter the agent transcript.

use gpui_kit::assets::IconName;
use gpui_kit::base::{Disableable, StyledExt};
use gpui_kit::component::Sizable;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::diff::{Diff, DiffFile, DiffMode, DiffState};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClipboardItem, Context, Entity, TestSupportExt, Window, div, px};
use serde_json::json;
use vtcode_desktop_protocol::{self as protocol, WorkspaceReviewResult};

use super::{
    DesktopButtonExt, DesktopView, RefreshWorkspace, desktop_button, empty_state, skeleton_line, uuid_like_id,
    warning_alert,
};

pub(super) struct WorkspaceReviewState {
    pub(super) request_id: String,
    pub(super) scope_input: Entity<InputState>,
    workspace: String,
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
    title: &'static str,
    description: impl Into<gpui_kit::SharedString>,
    icon: IconName,
) -> AnyElement {
    let description = description.into();
    div()
        .id(id)
        .test_support()
        .role(gpui_kit::Role::Status)
        .aria_label(title)
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
            self.fail_workspace_review("The local service request queue is unavailable.".into());
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

    fn navigate_workspace_review_file(&mut self, forward: bool, cx: &mut Context<Self>) {
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
        let patch = review.snapshot.as_ref().and_then(|snapshot| snapshot.patch.clone());
        let has_viewer = review.viewer.is_some();
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .px_4()
                .py_2()
                .child(div().text_sm().child("Folder (optional)"))
                .child(
                    Input::new(&review.scope_input)
                        .accessibility_id("workspace-review-scope")
                        .aria_label("Review folder (optional)")
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
                    .label("Include untracked text files")
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
                .border_b_1()
                .border_color(colors.border)
                .child(
                    desktop_button("refresh-workspace-review")
                        .tooltip_with_action("Refresh working tree review", &RefreshWorkspace, Some("VTCodeDesktop"))
                        .label(if review.pending {
                            "Refreshing…"
                        } else if review.snapshot.is_none() {
                            "Load changes"
                        } else {
                            "Refresh"
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
                        .aria_label("Diff layout")
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            desktop_button("workspace-review-unified")
                                .label("Unified")
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
                                .label("Split")
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
                        .label("Copy preview")
                        .icon(IconName::Copy)
                        .ghost()
                        .small()
                        .desktop_disabled(patch.as_ref().is_none_or(String::is_empty))
                        .on_click(move |_, window, app| {
                            if let Some(patch) = &patch {
                                app.write_to_clipboard(ClipboardItem::new_string(patch.clone()));
                                super::super::notifications::copied_to_clipboard(window, app);
                            }
                        }),
                ),
        );
        if review.pending && review.snapshot.is_none() {
            let loading = div()
                .id("workspace-review-loading")
                .test_support()
                .role(gpui_kit::Role::Status)
                .aria_label("Reading Git changes")
                .aria_description("Capturing a bounded read-only snapshot.")
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap_4()
                .p_4()
                .child(div().font_medium().child("Reading Git changes…"))
                .child(
                    div()
                        .text_sm()
                        .text_color(colors.muted_foreground)
                        .child("Capturing a bounded read-only snapshot."),
                )
                .child(
                    div()
                        .w_full()
                        .rounded_lg()
                        .border_1()
                        .border_color(colors.border)
                        .bg(colors.secondary)
                        .p_4()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(skeleton_line("workspace-review-placeholder-file", px(180.), cx))
                        .child(skeleton_line("workspace-review-placeholder-hunk", px(120.), cx))
                        .child(div().h(px(1.)).w_full().bg(colors.border.opacity(0.7)))
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
                    .aria_label("Refreshing Git changes")
                    .aria_description("Showing the last successful snapshot until this refresh finishes.")
                    .px_4()
                    .py_2()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child("Refreshing Git changes · showing the last successful snapshot"),
            );
        }
        if let Some(viewer) = &review.viewer {
            let files = viewer.read(cx).files();
            let position = files
                .iter()
                .position(|file| Some(file.path().as_str()) == review.selected_file.as_deref());
            let label = format!(
                "File {} of {} · {}",
                position.map_or(0, |index| index + 1),
                files.len(),
                review.selected_file.as_deref().unwrap_or("")
            );
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .py_2()
                    .child(
                        desktop_button("workspace-review-previous-file")
                            .label("Previous file")
                            .ghost()
                            .small()
                            .desktop_disabled(files.len() < 2)
                            .on_click(cx.listener(|view, _, _, cx| view.navigate_workspace_review_file(false, cx))),
                    )
                    .child(
                        div()
                            .id("workspace-review-current-file")
                            .test_support()
                            .role(gpui_kit::Role::Status)
                            .aria_label(label.clone())
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .truncate()
                            .child(label),
                    )
                    .child(
                        desktop_button("workspace-review-next-file")
                            .label("Next file")
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
                        "Git review unavailable",
                        error.clone(),
                        IconName::GitCompare,
                    ))
                    .into_any_element();
            }
            body = body.child(warning_alert(
                "workspace-review-refresh-warning",
                format!("Refresh failed. Showing the last successful snapshot. {error}"),
                &colors,
            ));
        }
        let Some(snapshot) = &review.snapshot else {
            return body.child(review_message("workspace-review-ready", "Choose what to review", "Leave Folder empty for the whole workspace, or enter a relative directory. Then choose Load changes.", IconName::GitCompare)).into_any_element();
        };
        if snapshot.patch.is_some() {
            body = body.child(div().px_4().pt_2().text_xs().text_color(colors.muted_foreground).child(format!(
                "Folder: {}",
                if snapshot.scope.is_empty() {
                    "."
                } else {
                    &snapshot.scope
                }
            )));
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
                    "Git review unavailable",
                    "Refresh after Git and a valid repository are available for this workspace.",
                    IconName::GitCompare,
                ))
                .into_any_element(),
            Some("") => body
                .child(review_message(
                    "workspace-review-clean",
                    if snapshot.truncated {
                        "No previewable changes"
                    } else {
                        "No tracked changes"
                    },
                    if snapshot.include_untracked {
                        "No previewable changes in this snapshot. See the capture note for omitted files."
                    } else {
                        "Tracked files match the captured Git baseline. Untracked files are excluded."
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
                    body.child(div().px_4().py_2().text_sm().child(if snapshot.truncated && snapshot.include_untracked {
                        "Preview incomplete. See the capture note for omitted files or limits. Copy includes only the captured preview."
                    } else if snapshot.truncated {
                        "Preview truncated at the review limit. Choose a narrower folder for a complete review. Copy includes only this preview."
                    } else {
                        "Structured preview unavailable for this patch. Showing the captured text."
                    }))
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
