use super::files_panel::file_row_menu;
use super::*;
use gpui_kit::component::menu::ContextMenuExt;

impl DesktopView {
    pub(super) fn render_diff_item(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let diff_foreground =
            themes::diff_text_foreground(colors.background, colors.foreground, colors.success, colors.danger);
        let mono_font = cx.theme().mono_font_family.clone();
        let Some(diff) = item.diff.as_ref() else {
            return div().into_any_element();
        };
        let id = item.id.clone();
        let status_label = if diff.failed {
            rust_i18n::t!("app.transcript.patch_failed").to_string()
        } else if diff.incomplete {
            rust_i18n::t!("app.transcript.preview_incomplete").to_string()
        } else {
            rust_i18n::t!("app.transcript.applied").to_string()
        };
        let toggle_id = id.clone();
        let mut stats = div().flex().items_center().gap_2().text_sm().flex_shrink_0();
        if let Some(additions) = diff.additions {
            stats = stats.child(div().text_color(colors.success).child(format!("+{additions}")));
        }
        if let Some(deletions) = diff.deletions {
            stats = stats.child(div().text_color(colors.danger).child(format!("−{deletions}")));
        }

        // Trailing actions share one shrink-proof group (see panel_header):
        // the toggle and Changes jump must never overflow the card, so the
        // leading title truncates instead when the conversation narrows. Copy
        // patch lives in the expanded body beside the patch it copies.
        let actions = div()
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .child(
                desktop_ghost_button(format!("diff-{}", item.id))
                    .label(if item.expanded {
                        rust_i18n::t!("app.transcript.hide_diff").to_string()
                    } else {
                        rust_i18n::t!("app.transcript.show_diff").to_string()
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.transcript.toggle_diff(&toggle_id);
                        cx.notify();
                    })),
            )
            .child(Self::diff_changes_cta(&item.id, cx));

        let header = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .px_3()
            .py_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .flex_1()
                    .child(div().font_medium().min_w_0().truncate().child(item.title.clone()))
                    .child(stats)
                    .child(
                        div()
                            .text_sm()
                            .flex_shrink_0()
                            .text_color(if diff.failed {
                                colors.danger
                            } else {
                                themes::muted_text(&colors)
                            })
                            .child(status_label),
                    ),
            )
            .child(actions);

        // One card surface for header, file rows, and patch: the quiet fill
        // and hairline border match the turn files card, so every change
        // summary shares one geometry and the rows never read as orphans.
        let mut panel = div()
            .relative()
            .w_full()
            .max_w(px(self.content_width()))
            .rounded_md()
            .border_1()
            .border_color(themes::hairline(colors.secondary, colors.foreground))
            .bg(colors.secondary)
            .py_1()
            .flex()
            .flex_col()
            .gap_1()
            .child(header);

        let mut body = div().w_full().flex().flex_col().px_3().py_1().gap_1();
        for file in &diff.files {
            let (kind_label, kind_color) = match file.kind {
                DiffFileKind::Add => ("added", colors.success),
                DiffFileKind::Delete => ("deleted", colors.danger),
                DiffFileKind::Update => ("modified", colors.muted_foreground),
            };
            let path = file.path.clone();
            let (directory, basename) = match path.rsplit_once('/') {
                Some((directory, base)) => (Some(format!("{directory}/")), base.to_string()),
                None => (None, path.clone()),
            };
            let menu_path = path.clone();
            body = body.child(
                div()
                    .id(format!("diff-file-card-{}-{path}", item.id))
                    .test_support()
                    .context_menu(move |menu, _, _| file_row_menu(menu, &menu_path, false))
                    .child(
                        desktop_ghost_button(format!("diff-file-{}-{path}", item.id))
                            .accessibility_label(path.clone())
                            .tooltip(path.clone())
                            .small()
                            .w_full()
                            .justify_start()
                            .min_w_0()
                            .on_click({
                                let toggle_id = item.id.clone();
                                cx.listener(move |view, _, _, cx| {
                                    // File rows reuse the Show/Hide diff
                                    // toggle: the card expands inline.
                                    view.transcript.toggle_diff(&toggle_id);
                                    cx.notify();
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
                                    .font_family(mono_font.clone())
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .flex()
                                            .truncate()
                                            .when_some(directory, |row, directory| {
                                                row.child(
                                                    div()
                                                        .min_w_0()
                                                        .truncate()
                                                        .text_color(themes::muted_text(&colors))
                                                        .child(directory),
                                                )
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
                                    .child(div().text_xs().flex_shrink_0().text_color(kind_color).child(kind_label)),
                            ),
                    ),
            );
        }
        if diff.incomplete {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(rust_i18n::t!("app.transcript.previews_unavailable").to_string()),
            );
        }

        if item.expanded && !diff.patch.is_empty() {
            // Export sits with its object, mirroring the tool-row actions
            // above their output block; the header keeps only the toggle and
            // the Changes jump.
            let copy_id = id.clone();
            body = body.child(
                div().flex().justify_end().child(
                    desktop_ghost_button(format!("copy-patch-{}", item.id))
                        .label(rust_i18n::t!("app.transcript.copy_patch").to_string())
                        .xsmall()
                        .on_click(cx.listener(move |view, _, window, cx| {
                            let Some(patch) = view
                                .transcript
                                .items_newest_first()
                                .find(|candidate| candidate.id == copy_id)
                                .and_then(|candidate| candidate.diff.as_ref())
                                .map(|diff| diff.patch.clone())
                                .filter(|patch| !patch.is_empty())
                            else {
                                return;
                            };
                            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(patch));
                            super::super::notifications::copied_to_clipboard(window, cx);
                        })),
                ),
            );
            let mut lines = ClassifiedDiffLines::new(&diff.patch);
            let mut diff_view = div()
                .id(format!("diff-lines-{}", item.id))
                .test_support()
                .w_full()
                .flex()
                .flex_col()
                .rounded_md()
                .overflow_hidden()
                .overflow_x_scroll()
                .bg(colors.background);
            for line in &mut lines {
                let (line_color, line_bg, marker_color) = match line.kind {
                    DiffLineKind::Added => (
                        diff_foreground,
                        themes::diff_line_background(colors.background, diff_foreground, colors.success),
                        Some(("+", "+", colors.success)),
                    ),
                    DiffLineKind::Removed => (
                        diff_foreground,
                        themes::diff_line_background(colors.background, diff_foreground, colors.danger),
                        Some(("-", "−", colors.danger)),
                    ),
                    // Hunk markers orient multi-hunk patches but earn no band:
                    // muted text on the base surface, like context lines.
                    DiffLineKind::Hunk => (colors.muted_foreground, colors.background, None),
                    DiffLineKind::Header | DiffLineKind::Context => (colors.foreground, colors.background, None),
                };
                let mut row = div()
                    .w_full()
                    .px_2()
                    .text_xs()
                    .font_family(mono_font.clone())
                    .whitespace_nowrap()
                    .bg(line_bg);
                if let Some((source_marker, marker, marker_color)) = marker_color {
                    let content = line.text.strip_prefix(source_marker).unwrap_or_default();
                    row = row
                        .flex()
                        .child(
                            div()
                                .w_4()
                                .flex_shrink_0()
                                .font_semibold()
                                .text_color(marker_color)
                                .child(marker),
                        )
                        .child(div().text_color(line_color).child(content.to_string()));
                } else {
                    row = row.text_color(line_color).child(line.text.to_string());
                }
                diff_view = diff_view.child(row);
            }
            if lines.truncated {
                diff_view = diff_view.child(
                    div()
                        .w_full()
                        .px_2()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(rust_i18n::t!("app.transcript.diff_truncated").to_string()),
                );
            }
            // The body mounts on expand, so the shared fade replays on every
            // toggle; reduced motion renders the end state statically.
            body = body.child(diff_view.fade_in(format!("diff-lines-{}-fade", item.id), cx.theme()));
        }
        panel = panel.child(body);
        panel.into_any_element()
    }
}
