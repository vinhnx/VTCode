//! Turn files-changes card: the per-turn edited-files summary appended to
//! the final assistant response. Icon badge + title/totals header, per-file
//! rows that open the in-app preview, per-turn undo, and the show-more
//! expander past three rows.

use super::files_panel::file_row_menu;
use super::turn_undo::{TurnUndoState, undo_turn_id};
use super::*;
use gpui_kit::component::menu::ContextMenuExt;

/// Per-turn edited-files card under the final assistant response. The header
/// pairs one icon badge with the "Edited N files" title and the turn's
/// additive totals beside the Undo / View changes actions; each file row
/// splits a dimmed directory from its basename and opens the in-app preview
/// on click with the shared file menu on right-click; past three rows the
/// expander pages through the rest (3 then 50, `Transcript::toggle_turn_files`).
pub(super) fn render_turn_files_card(
    view: &DesktopView,
    item: &TranscriptItem,
    cx: &mut Context<DesktopView>,
) -> AnyElement {
    let colors = cx.theme().colors;
    let card_border = themes::hairline(colors.secondary, colors.foreground);
    let (badge_surface, badge_border) = themes::tone_chip_surface(colors.foreground, &colors);
    let muted = themes::muted_text(&colors);

    let undo_control = undo_turn_id(item).map(|turn_id| {
        let turn_id = turn_id.to_owned();
        if view.turn_undos.get(turn_id.as_str()) == Some(&TurnUndoState::Undone) {
            div()
                .text_sm()
                .text_color(muted)
                .child(rust_i18n::t!("app.transcript.turn_undone").to_string())
                .into_any_element()
        } else {
            let pending = view.turn_undos.contains_key(turn_id.as_str());
            desktop_button(format!("turn-undo-{turn_id}"))
                .label(rust_i18n::t!("app.transcript.turn_undo").to_string())
                .accessibility_label(rust_i18n::t!("app.transcript.turn_undo_action").to_string())
                .ghost()
                .small()
                .disabled(pending)
                .on_click(cx.listener(move |view, _, _, cx| {
                    let _sent = view.request_turn_undo(&turn_id);
                    cx.notify();
                }))
                .into_any_element()
        }
    });

    let first_path = item.turn_files.first().map(|entry| entry.file.path.clone());
    let changes_cta = desktop_default_button(format!("turn-changes-{}", item.id))
        .label(rust_i18n::t!("app.transcript.turn_view_changes").to_string())
        .small()
        .on_click(cx.listener(move |view, _, _, cx| {
            if let Some(path) = first_path.as_ref().filter(|path| view.changes.has_preview(path)) {
                view.select_change(path.clone(), cx);
            } else {
                view.open_inspector_tab(InspectorTab::Changes, cx);
            }
        }));

    let title = if item.turn_files.len() == 1 {
        rust_i18n::t!("app.transcript.turn_edited_file_one").to_string()
    } else {
        rust_i18n::t!("app.transcript.turn_edited_files_many", n = item.turn_files.len()).to_string()
    };
    let header = div()
        .px_3()
        .py_1()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(
                    div()
                        .size_6()
                        .flex_none()
                        .rounded_sm()
                        .border_1()
                        .border_color(badge_border)
                        .bg(badge_surface)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(IconName::GitCompare).small().text_color(muted)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(
                            div()
                                .text_sm()
                                .font_medium()
                                .text_color(colors.foreground)
                                .truncate()
                                .child(title),
                        )
                        .when(item.turn_files_incomplete, |column| {
                            column.child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(rust_i18n::t!("app.inspector.partial_diff").to_string()),
                            )
                        })
                        .when_some(item.turn_file_totals, |column, (added, deleted)| {
                            column.child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .text_sm()
                                    .child(div().text_color(colors.success).child(format!("+{added}")))
                                    .child(div().text_color(colors.danger).child(format!("−{deleted}"))),
                            )
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap_2()
                .children(undo_control)
                .child(changes_cta),
        );

    let start = item.turn_files_offset.min(item.turn_files.len());
    let count = if start == 0 { 3 } else { 50 };
    let end = start.saturating_add(count).min(item.turn_files.len());
    let mut card = div()
        .w_full()
        .rounded_md()
        .border_1()
        .border_color(card_border)
        .bg(colors.secondary)
        .py_1()
        .flex()
        .flex_col()
        .gap_1()
        .child(header);
    for file in item.turn_files.iter().skip(start).take(count) {
        let path = file.file.path.clone();
        let (directory, basename) = match path.rsplit_once('/') {
            Some((directory, base)) => (Some(format!("{directory}/")), base.to_string()),
            None => (None, path.clone()),
        };
        let menu_path = path.clone();
        card = card.child(
            div()
                .id(format!("turn-file-card-{}-{path}", item.id))
                .test_support()
                .w_full()
                .px_3()
                .context_menu(move |menu, _, _| file_row_menu(menu, &menu_path, false))
                .child(
                    desktop_button(format!("turn-file-{}-{path}", item.id))
                        .accessibility_label(path.clone())
                        .tooltip(path.clone())
                        .ghost()
                        .small()
                        .w_full()
                        .justify_start()
                        .min_w_0()
                        .on_click({
                            let path = path.clone();
                            cx.listener(move |view, _, window, cx| {
                                view.open_file_preview_overlay(path.clone(), window, cx)
                            })
                        })
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_sm()
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .flex()
                                        .truncate()
                                        .when_some(directory, |row, directory| {
                                            row.child(div().min_w_0().truncate().text_color(muted).child(directory))
                                        })
                                        .child(
                                            div()
                                                .max_w(px(240.))
                                                .flex_shrink_0()
                                                .truncate()
                                                .font_medium()
                                                .text_color(colors.foreground)
                                                .child(basename.clone()),
                                        ),
                                )
                                .when_some(file.counts, |row, (added, deleted)| {
                                    row.child(
                                        div()
                                            .flex()
                                            .flex_shrink_0()
                                            .gap_1()
                                            .child(div().text_color(colors.success).child(format!("+{added}")))
                                            .child(div().text_color(colors.danger).child(format!("−{deleted}"))),
                                    )
                                }),
                        ),
                ),
        );
    }
    if item.turn_files.len() > 3 {
        let id = item.id.clone();
        let expanded = end == item.turn_files.len();
        let label = if expanded {
            rust_i18n::t!("app.transcript.collapse").to_string()
        } else {
            rust_i18n::t!("app.configuration.show_more", remaining = item.turn_files.len() - end).to_string()
        };
        card = card.child(
            div().px_2().child(
                desktop_button(format!("turn-files-toggle-{id}"))
                    .label(label)
                    .child(transcript_chevron_lane(expanded))
                    .ghost()
                    .small()
                    .justify_start()
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.transcript.toggle_turn_files(&id);
                        cx.notify();
                    })),
            ),
        );
    }
    card.into_any_element()
}
