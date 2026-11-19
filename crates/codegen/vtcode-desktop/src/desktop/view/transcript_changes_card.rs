//! Per-turn edited files and their actions below the final assistant response.

use super::files_panel::file_row_menu;
use super::turn_undo::{TurnUndoState, undo_turn_id};
use super::*;
use gpui_kit::component::menu::ContextMenuExt;

pub(super) fn render_turn_files_card(
    view: &DesktopView,
    item: &TranscriptItem,
    cx: &mut Context<DesktopView>,
) -> AnyElement {
    let colors = cx.theme().colors;
    let card_border = themes::hairline(colors.secondary, colors.foreground);
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
            desktop_ghost_button(format!("turn-undo-{turn_id}"))
                .label(rust_i18n::t!("app.transcript.turn_undo").to_string())
                .accessibility_label(rust_i18n::t!("app.transcript.turn_undo_action").to_string())
                .small()
                .desktop_disabled(pending)
                .on_click(cx.listener(move |view, _, _, cx| {
                    let _sent = view.request_turn_undo(&turn_id);
                    cx.notify();
                }))
                .into_any_element()
        }
    });

    let first_path = item.turn_files.first().map(|entry| entry.file.path.clone());
    let changes_cta = desktop_secondary_button(format!("turn-changes-{}", item.id))
        .label(rust_i18n::t!("app.transcript.turn_view_changes").to_string())
        .small()
        .on_click(cx.listener(move |view, _, _, cx| match first_path.clone() {
            Some(path) => view.open_change_or_tab(path, cx),
            None => view.open_inspector_tab(InspectorTab::Changes, cx),
        }));

    let title = if item.turn_files.len() == 1 {
        rust_i18n::t!("app.transcript.turn_edited_file_one").to_string()
    } else {
        rust_i18n::t!("app.transcript.turn_edited_files_many", n = item.turn_files.len()).to_string()
    };
    let header = div()
        // Match the small ghost buttons' content inset in the rows and footer.
        .px_2()
        .py_2()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(
            div()
                .min_w_0()
                .max_w_full()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .text_sm()
                .child(div().min_w_0().font_semibold().text_color(colors.foreground).child(title))
                .when_some(item.turn_file_totals, |summary, (added, deleted)| {
                    summary.child(change_counts(added, deleted, &colors))
                })
                .when(item.turn_files_incomplete, |summary| {
                    summary.child(
                        div()
                            .min_w_0()
                            .text_xs()
                            .text_color(muted)
                            .child(rust_i18n::t!("app.inspector.partial_diff").to_string()),
                    )
                }),
        )
        .child(
            div()
                .ml_auto()
                .min_w_0()
                .max_w_full()
                .flex()
                .flex_wrap()
                .justify_end()
                .items_center()
                .gap_2()
                .children(undo_control)
                .child(changes_cta),
        );

    let start = item.turn_files_offset.min(item.turn_files.len());
    let count = if start == 0 { 3 } else { 50 };
    let end = start.saturating_add(count).min(item.turn_files.len());
    let mut card = div()
        .id(format!("turn-files-card-{}", item.id))
        .test_support()
        .w_full()
        .min_w_0()
        .rounded_md()
        .border_1()
        .border_color(card_border)
        .bg(colors.secondary)
        .px_1()
        .pb_1()
        .flex()
        .flex_col()
        .gap_1()
        .child(header)
        .child(div().mx_2().h(px(1.)).bg(card_border));
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
                .context_menu(move |menu, _, _| file_row_menu(menu, &menu_path, false))
                .child(
                    desktop_ghost_button(format!("turn-file-{}-{path}", item.id))
                        .accessibility_label(path.clone())
                        .tooltip(path.clone())
                        .small()
                        .w_full()
                        .justify_start()
                        .min_w_0()
                        .on_click({
                            let path = path.clone();
                            cx.listener(move |view, _, _, cx| view.open_change_or_tab(path.clone(), cx))
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
                                        .when_some(directory, |row, directory| {
                                            row.child(div().min_w_0().truncate().text_color(muted).child(directory))
                                        })
                                        .child(
                                            div()
                                                .max_w_full()
                                                .flex_shrink_0()
                                                .truncate()
                                                .font_medium()
                                                .text_color(colors.foreground)
                                                .child(basename),
                                        ),
                                )
                                .when_some(file.counts, |row, (added, deleted)| {
                                    row.child(change_counts(added, deleted, &colors))
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
            desktop_ghost_button(format!("turn-files-toggle-{id}"))
                .accessibility_label(label.clone())
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(label)
                        .child(transcript_chevron_lane(expanded).text_color(muted)),
                )
                .small()
                .justify_start()
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.transcript.toggle_turn_files(&id);
                    cx.notify();
                })),
        );
    }
    card.into_any_element()
}

fn change_counts(added: u64, deleted: u64, colors: &gpui_kit::component::theme::ThemeColor) -> gpui_kit::Div {
    div()
        .flex()
        .flex_none()
        .gap_1()
        .child(div().text_color(colors.success).child(format!("+{added}")))
        .child(div().text_color(colors.danger).child(format!("−{deleted}")))
}
