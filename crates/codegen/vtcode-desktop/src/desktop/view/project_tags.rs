//! Project-tag filter UI and its sessions-in-context projection.
//!
//! The sidebar trigger opens a searchable popover; plain clicks select one
//! context and Shift-clicks combine contexts. Management lives in one modal
//! so rename/merge/delete and per-project edits stay on a single surface.
use std::borrow::Cow;

use super::*;
use gpui_kit::ClickEvent;
use gpui_kit::base::InteractiveElementExt;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::theme::ThemeColor;

use crate::desktop::project_tags::{MAX_TAGS_PER_WORKSPACE, normalize_tag};

/// One neutral tag chip. Selected tags use the filled accent token with the
/// design system's AA-safe label; resting tags use a recessed fill with the
/// shared soft-edge border. No per-tag colors: tags are labels, not status.
fn project_tag_chip(tag: &str, selected: bool, colors: &ThemeColor) -> Tag {
    let (fill, label, border) = if selected {
        (colors.primary, themes::filled_button_foreground(colors.primary), colors.primary)
    } else {
        (colors.secondary, colors.foreground, themes::soft_edge_color(colors))
    };
    // Kit's `small()` sets a 0.625rem label; pin our `text_xs` floor so the
    // chip never drops below the app's smallest supported text size.
    Tag::custom(fill, label, border)
        .small()
        .rounded_full()
        .max_w(gpui_kit::rems(8.))
        .text_xs()
        .child(div().truncate().child(tag.to_owned()))
}

impl DesktopView {
    /// Sessions in the active context. The unfiltered case (the common one)
    /// borrows the raw session list; only an active filter allocates a projection.
    pub(super) fn context_sessions(&self) -> Cow<'_, [SessionSummary]> {
        let state = &self.preferences.workspace_tags;
        let active = state.effective_active();
        if active.is_empty() {
            return Cow::Borrowed(&self.sessions);
        }
        Cow::Owned(
            self.sessions
                .iter()
                .filter(|session| state.matches_active(&session.workspace, &active))
                .cloned()
                .collect(),
        )
    }

    /// Recent workspaces visible under the active tag filter, most recent first.
    pub(super) fn visible_recent_workspaces(&self) -> Vec<String> {
        self.preferences
            .workspace_tags
            .visible_workspaces(&self.preferences.recent_workspaces)
    }

    /// Trigger label: empty, one tag, or the first tag plus a count.
    pub(super) fn tag_active_label(&self) -> String {
        self.preferences.workspace_tags.active_label()
    }

    /// Mark a tag mutation as persisted and refresh the dependent projection.
    fn commit_tag_change(&mut self, changed: bool, cx: &mut Context<Self>) {
        if !changed {
            return;
        }
        // Every mutation can remove the last project carrying a tag; keep the
        // active set free of stale entries so the unfiltered path stays cheap.
        let _ = self.preferences.workspace_tags.prune_active();
        self.preferences.save();
        cx.notify();
    }

    /// Whether a tag pick may move an untouched new-thread page into the
    /// selected context. Workspace switches restart the local service, so a
    /// conversation, draft, attachment, or retained submission blocks it.
    fn new_thread_page_untouched(&self, cx: &Context<Self>) -> bool {
        self.selected_session_id.is_none()
            && self.composer.read(cx).value().trim().is_empty()
            && self.attachments.is_empty()
            && self.retained_submission.is_none()
            && self.workspace_switch_idle()
    }

    /// Select or combine a tag context. A plain click picks one tag (or clears
    /// it when already selected); Shift+click toggles it in the existing set.
    pub(super) fn toggle_tag_filter(&mut self, tag: &str, combine: bool, window: &mut Window, cx: &mut Context<Self>) {
        let state = &mut self.preferences.workspace_tags;
        let active = &mut state.active;
        let position = active.iter().position(|existing| existing == tag);
        let changed = if combine {
            match position {
                Some(index) => {
                    let _ = active.remove(index);
                    true
                }
                None => {
                    active.push(tag.to_owned());
                    true
                }
            }
        } else {
            let next = if active.as_slice() == [tag] {
                Vec::new()
            } else {
                vec![tag.to_owned()]
            };
            if *active != next {
                *active = next;
                true
            } else {
                false
            }
        };
        let pruned = state.prune_active();
        self.commit_tag_change(changed || pruned, cx);

        // Move an untouched landing page into the new context; never disturb
        // an open conversation or a draft (see `new_thread_page_untouched`).
        if !self.preferences.workspace_tags.active.is_empty()
            && self.new_thread_page_untouched(cx)
            && let Some(path) = self.visible_recent_workspaces().into_iter().next()
            && self
                .workspace
                .as_ref()
                .is_none_or(|current| current.display().to_string() != path)
        {
            self.select_workspace_from_ui(PathBuf::from(path), window, cx);
        }
    }

    pub(super) fn clear_tag_filter(&mut self, cx: &mut Context<Self>) {
        self.preferences.workspace_tags.active.clear();
        self.commit_tag_change(true, cx);
    }

    pub(super) fn open_tag_manager(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.disarm_double_escape();
        self.tag_filter_open = false;
        self.tag_manager_open = true;
        self.tag_error = None;
        self.tag_manager_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_tag_manager(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.disarm_double_escape();
        self.tag_manager_open = false;
        self.tag_project_editor = None;
        self.tag_rename_target = None;
        self.tag_delete_target = None;
        self.tag_error = None;
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(super) fn add_project_tag(&mut self, workspace: &str, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.tag_project_input.read(cx).value().to_string();
        let Some(tag) = normalize_tag(&raw) else {
            self.tag_error = Some("Use 1-32 lowercase characters, without commas".to_owned());
            cx.notify();
            return;
        };
        let state = &self.preferences.workspace_tags;
        let current = state.workspaces.get(workspace);
        if current.is_some_and(|tags| tags.contains(&tag)) {
            self.tag_error = Some(format!("\"{tag}\" is already on this project"));
            cx.notify();
            return;
        }
        if current.is_some_and(|tags| tags.len() >= MAX_TAGS_PER_WORKSPACE) {
            self.tag_error = Some(format!("At the {MAX_TAGS_PER_WORKSPACE}-tag limit"));
            cx.notify();
            return;
        }
        let changed = self.preferences.workspace_tags.add_tag(workspace, &tag);
        if changed {
            self.tag_project_input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.tag_error = None;
        self.commit_tag_change(changed, cx);
    }

    pub(super) fn remove_project_tag(&mut self, workspace: &str, tag: &str, cx: &mut Context<Self>) {
        let changed = self.preferences.workspace_tags.remove_tag(workspace, tag);
        self.commit_tag_change(changed, cx);
    }

    pub(super) fn start_tag_rename(&mut self, tag: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.tag_rename_target = Some(tag.to_owned());
        self.tag_delete_target = None;
        self.tag_error = None;
        self.tag_rename_input.update(cx, |input, cx| input.set_value(tag, window, cx));
        cx.notify();
    }

    pub(super) fn confirm_tag_rename(&mut self, cx: &mut Context<Self>) {
        let Some(from) = self.tag_rename_target.clone() else {
            return;
        };
        let to = self.tag_rename_input.read(cx).value().to_string();
        let Some(normalized) = normalize_tag(&to) else {
            self.tag_error = Some("Use 1-32 lowercase characters, without commas".to_owned());
            cx.notify();
            return;
        };
        let changed = self.preferences.workspace_tags.rename_tag(&from, &normalized);
        self.tag_rename_target = None;
        self.tag_error = None;
        self.commit_tag_change(changed, cx);
    }

    pub(super) fn confirm_tag_delete(&mut self, cx: &mut Context<Self>) {
        let Some(tag) = self.tag_delete_target.clone() else {
            return;
        };
        let changed = self.preferences.workspace_tags.delete_tag(&tag);
        self.tag_delete_target = None;
        self.commit_tag_change(changed, cx);
    }

    /// Sidebar trigger plus its searchable tag popover.
    pub(super) fn render_tag_filter(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let label = self.tag_active_label();
        let active = !label.is_empty();
        let trigger = desktop_ghost_button("project-tag-filter")
            .icon(IconName::Tags)
            .when(active, |button| button.child(project_tag_chip(&label, true, &colors)))
            .small()
            .accessibility_label(if active {
                format!("Filter projects by tag: {label}")
            } else {
                "Filter projects by tag".to_owned()
            })
            .tooltip("Filter projects by tag")
            .toggled(self.tag_filter_open || active);

        let open_view = cx.entity().downgrade();
        let content_view = cx.entity().downgrade();
        let search = self.tag_search.clone();
        Popover::new("project-tag-filter-popover")
            .anchor(Anchor::TopLeft)
            .offset(px(6.))
            .w(px(300.))
            .max_h(px(380.))
            .open(self.tag_filter_open)
            .on_open_change(move |open, window, cx| {
                let _updated = open_view.update(cx, |view, cx| {
                    // Kit does not announce prop-driven closes, so reset on
                    // both transitions: a programmatic close must not leave a
                    // stale query for the next open.
                    view.tag_search.update(cx, |input, cx| input.set_value("", window, cx));
                    view.tag_filter_open = *open;
                    cx.notify();
                });
            })
            .trigger(trigger)
            .content(move |_popover, _window, cx| {
                let colors = cx.theme().colors;
                let weak = content_view.clone();
                let query = search.read(cx).value().to_string().to_lowercase();
                let (tags, project_names, active_tags) = weak
                    .read_with(cx, |view, _| {
                        let state = &view.preferences.workspace_tags;
                        let all = state.all_tags();
                        let matches_query = |tag: &str| {
                            query.is_empty()
                                || tag.contains(&query)
                                || state.workspaces.iter().any(|(path, tags)| {
                                    path.to_lowercase().contains(&query) && tags.iter().any(|entry| entry == tag)
                                })
                        };
                        let tags = all.into_iter().filter(|(tag, _)| matches_query(tag)).collect::<Vec<_>>();
                        let project_names = view.preferences.recent_workspaces.len();
                        (tags, project_names, view.preferences.workspace_tags.active.clone())
                    })
                    .ok()
                    .unwrap_or((Vec::new(), 0, Vec::new()));

                let mut content = div()
                    .id("tag-filter-list")
                    .w_full()
                    .max_h(px(340.))
                    .lock_scroll_axis()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(Input::new(&search).small().w_full())
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("{project_names} projects")),
                    );
                if tags.is_empty() {
                    content = content.child(div().px_2().py_2().text_sm().text_color(colors.muted_foreground).child(
                        if query.is_empty() {
                            "No tags yet. Manage tags to add one."
                        } else {
                            "No tags match your search."
                        },
                    ));
                }
                for (tag, count) in tags {
                    let selected = active_tags.iter().any(|active| active == &tag);
                    let tag_for_click = tag.clone();
                    let click_weak = weak.clone();
                    content = content.child(
                        desktop_ghost_button(format!("tag-filter-{tag}"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .w_full()
                                    .child(project_tag_chip(&tag, selected, &colors))
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .child(count.to_string()),
                                    ),
                            )
                            .accessibility_label(format!("{tag}, {count} projects"))
                            .w_full()
                            .justify_start()
                            .toggled(selected)
                            .on_click(move |event: &ClickEvent, window, cx| {
                                let _updated = click_weak.update(cx, |view, cx| {
                                    view.toggle_tag_filter(&tag_for_click, event.modifiers().shift, window, cx);
                                });
                            }),
                    );
                }
                if !active_tags.is_empty() {
                    let clear_weak = weak.clone();
                    content = content.child(
                        desktop_ghost_button("tag-filter-clear")
                            .icon(IconName::X)
                            .label("Clear filter")
                            .w_full()
                            .justify_start()
                            .on_click(move |_, _, cx| {
                                let _updated = clear_weak.update(cx, |view, cx| view.clear_tag_filter(cx));
                            }),
                    );
                }
                let manage_weak = weak.clone();
                content = content.child(
                    desktop_ghost_button("tag-manage-open")
                        .icon(IconName::Settings2)
                        .label("Manage tags")
                        .w_full()
                        .justify_start()
                        .on_click(move |_, window, cx| {
                            let _updated = manage_weak.update(cx, |view, cx| view.open_tag_manager(window, cx));
                        }),
                );
                content.into_any_element()
            })
            .into_any_element()
    }

    /// Full-screen management surface for tags and per-project tag edits.
    pub(super) fn render_tag_manager(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let weak = cx.entity().downgrade();
        let state = &self.preferences.workspace_tags;
        let tags = state.all_tags();
        let projects = self.preferences.recent_workspaces.clone();
        let active = state.active.clone();

        let hairline = || div().h(px(1.)).w_full().bg(themes::hairline(colors.popover, colors.foreground));

        let mut body = div()
            .id("tag-manager-body")
            .flex()
            .flex_col()
            .gap_3()
            .max_h(px(520.))
            .lock_scroll_axis()
            .overflow_y_scroll();

        body = body.child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child("A tag exists while a project carries it. Renaming onto an existing tag merges them."),
        );

        if let Some(error) = &self.tag_error {
            body = body.child(accessible_error("tag-manager-error", error.clone(), &colors));
        }

        // Tag list: rename and delete.
        for (tag, count) in &tags {
            let is_renaming = self.tag_rename_target.as_deref() == Some(tag.as_str());
            let is_deleting = self.tag_delete_target.as_deref() == Some(tag.as_str());
            let selected = active.contains(tag);
            let row_weak = weak.clone();
            let row_tag = tag.clone();
            let rename_tag = tag.clone();
            let delete_tag = tag.clone();
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(project_tag_chip(tag, selected, &colors))
                            .child(div().flex_1())
                            .child(div().text_xs().text_color(colors.muted_foreground).child(format!("{count}")))
                            .when(selected, |row| row.child(div().text_xs().text_color(colors.primary).child("active")))
                            .child(
                                desktop_ghost_button(format!("tag-manager-rename-{tag}"))
                                    .icon(IconName::Pencil)
                                    .accessibility_label(format!("Rename tag {tag}"))
                                    .tooltip(format!("Rename {tag}"))
                                    .on_click(move |_, window, cx| {
                                        let _updated = row_weak.update(cx, |view, cx| {
                                            view.start_tag_rename(&rename_tag, window, cx);
                                        });
                                    }),
                            )
                            .child({
                                let delete_weak = weak.clone();
                                desktop_ghost_button(format!("tag-manager-delete-{tag}"))
                                    .icon(IconName::Trash)
                                    .accessibility_label(format!("Delete tag {tag}"))
                                    .tooltip(format!("Delete {tag}"))
                                    .on_click(move |_, _, cx| {
                                        let _updated = delete_weak.update(cx, |view, cx| {
                                            view.tag_delete_target = Some(delete_tag.clone());
                                            view.tag_rename_target = None;
                                            view.tag_error = None;
                                            cx.notify();
                                        });
                                    })
                            }),
                    )
                    .when(is_renaming, |column| {
                        column.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(Input::new(&self.tag_rename_input).small().flex_1())
                                .child({
                                    let save_weak = weak.clone();
                                    desktop_primary_button("tag-manager-rename-save")
                                        .label("Rename")
                                        .small()
                                        .on_click(move |_, _, cx| {
                                            let _updated = save_weak.update(cx, |view, cx| view.confirm_tag_rename(cx));
                                        })
                                })
                                .child({
                                    let cancel_weak = weak.clone();
                                    desktop_ghost_button("tag-manager-rename-cancel")
                                        .label("Cancel")
                                        .small()
                                        .on_click(move |_, _, cx| {
                                            let _updated = cancel_weak.update(cx, |view, cx| {
                                                view.tag_rename_target = None;
                                                cx.notify();
                                            });
                                        })
                                }),
                        )
                    })
                    .when(is_deleting, |column| {
                        column.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .text_sm()
                                        .child(format!("Delete \"{row_tag}\" from {count} projects?")),
                                )
                                .child({
                                    let confirm_weak = weak.clone();
                                    desktop_danger_button("tag-manager-delete-confirm")
                                        .label("Delete")
                                        .small()
                                        .on_click(move |_, _, cx| {
                                            let _updated =
                                                confirm_weak.update(cx, |view, cx| view.confirm_tag_delete(cx));
                                        })
                                })
                                .child({
                                    let cancel_weak = weak.clone();
                                    desktop_ghost_button("tag-manager-delete-cancel")
                                        .label("Cancel")
                                        .small()
                                        .on_click(move |_, _, cx| {
                                            let _updated = cancel_weak.update(cx, |view, cx| {
                                                view.tag_delete_target = None;
                                                cx.notify();
                                            });
                                        })
                                }),
                        )
                    }),
            );
        }
        body = body
            .child(hairline())
            .child(div().text_xs().text_color(colors.muted_foreground).child("Projects"));

        for path in projects {
            let tags = state.workspaces.get(&path).cloned().unwrap_or_default();
            let expanded = self.tag_project_editor.as_deref() == Some(path.as_str());
            let editor_weak = weak.clone();
            let editor_path = path.clone();
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(Icon::new(IconName::Folder).small().text_color(colors.muted))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .truncate()
                                    .child(workspace_short_name(&path).to_string()),
                            )
                            .child({
                                let mut chips = div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .overflow_hidden()
                                    .max_w(gpui_kit::rems(17.5));
                                for tag in tags.iter().take(3) {
                                    chips = chips.child(project_tag_chip(tag, active.contains(tag), &colors));
                                }
                                if tags.len() > 3 {
                                    chips = chips.child(
                                        Tag::custom(
                                            colors.secondary,
                                            colors.muted_foreground,
                                            themes::soft_edge_color(&colors),
                                        )
                                        .small()
                                        .rounded_full()
                                        .text_xs()
                                        .child(format!("+{}", tags.len() - 3)),
                                    );
                                }
                                chips
                            })
                            .child(
                                desktop_ghost_button(opaque_path_element_id("tag-manager-edit", &path))
                                    .icon(IconName::Pencil)
                                    .accessibility_label(format!("Edit tags for {path}"))
                                    .tooltip("Edit tags")
                                    .on_click(move |_, window, cx| {
                                        let _updated = editor_weak.update(cx, |view, cx| {
                                            view.tag_project_editor = Some(editor_path.clone());
                                            view.tag_project_input
                                                .update(cx, |input, cx| input.set_value("", window, cx));
                                            view.tag_delete_target = None;
                                            view.tag_rename_target = None;
                                            view.tag_error = None;
                                            cx.notify();
                                        });
                                    }),
                            ),
                    )
                    .when(expanded, |_column| {
                        let mut editor = div().flex().flex_col().gap_2();
                        for tag in &tags {
                            let remove_weak = weak.clone();
                            let remove_path = path.clone();
                            let remove_tag = tag.clone();
                            let remove_id = format!("{}-{tag}", opaque_path_element_id("tag-manager-remove", &path));
                            editor = editor.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(project_tag_chip(tag, active.contains(tag), &colors))
                                    .child(div().flex_1())
                                    .child(
                                        desktop_ghost_button(remove_id)
                                            .icon(IconName::X)
                                            .accessibility_label(format!("Remove tag {tag}"))
                                            .on_click(move |_, _, cx| {
                                                let _updated = remove_weak.update(cx, |view, cx| {
                                                    view.remove_project_tag(&remove_path, &remove_tag, cx);
                                                });
                                            }),
                                    ),
                            );
                        }
                        let add_weak = weak.clone();
                        let add_path = path.clone();
                        let at_cap = tags.len() >= MAX_TAGS_PER_WORKSPACE;
                        editor
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(Input::new(&self.tag_project_input).small().flex_1())
                                    .child(
                                        desktop_secondary_button("tag-manager-add")
                                            .label("Add tag")
                                            .small()
                                            .desktop_disabled(at_cap)
                                            .on_click(move |_, window, cx| {
                                                let _updated = add_weak.update(cx, |view, cx| {
                                                    view.add_project_tag(&add_path, window, cx);
                                                });
                                            }),
                                    ),
                            )
                            .when(at_cap, |editor| {
                                editor.child(
                                    div()
                                        .text_xs()
                                        .text_color(colors.muted_foreground)
                                        .child(format!("At the {MAX_TAGS_PER_WORKSPACE}-tag limit")),
                                )
                            })
                    }),
            );
        }

        let close_weak = weak.clone();
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .bg(modal_scrim(&colors))
            .child(
                div()
                    .id("tag-manager-dialog")
                    .test_support()
                    .track_focus(&self.tag_manager_focus)
                    .focus_trap("tag-manager-dialog", &self.tag_manager_focus)
                    .on_mouse_down_out(move |_, window, cx| {
                        let _updated = close_weak.update(cx, |view, cx| view.close_tag_manager(window, cx));
                    })
                    .role(gpui_kit::Role::Dialog)
                    .accessibility_id("tag-manager-dialog")
                    .aria_label("Manage tags")
                    .w(px(560.))
                    .max_w_full()
                    .max_h_full()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .rounded_md()
                    .bg(colors.popover)
                    .border_1()
                    .border_color(themes::soft_edge_color(&colors))
                    .shadow_md()
                    .p_5()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_base().font_semibold().child("Manage tags"))
                            .child({
                                let close_weak = weak.clone();
                                desktop_dismiss_button("tag-manager-dismiss", "Close tags").on_click(
                                    move |_, window, cx| {
                                        let _updated =
                                            close_weak.update(cx, |view, cx| view.close_tag_manager(window, cx));
                                    },
                                )
                            }),
                    )
                    .child(body),
            )
            .into_any_element()
    }
}
