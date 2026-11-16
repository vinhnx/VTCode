use vtcode_commons::tool_headlines::strip_mcp_prefix;

use super::*;

impl DesktopView {
    /// A small, theme-colored dotted rule under an actionable filename.
    pub(super) fn filename_dotted_underline(color: gpui_kit::Hsla) -> impl IntoElement {
        gpui_kit::canvas(
            |_, _, _| (),
            move |bounds, (), window, _| {
                let width: f32 = bounds.size.width.into();
                for index in 0_u16..1024 {
                    let offset = f32::from(index) * 3.0;
                    if offset >= width {
                        break;
                    }
                    let x = bounds.origin.x + px(offset);
                    window.paint_quad(gpui_kit::fill(
                        gpui_kit::Bounds::new(point(x, bounds.origin.y), size(px(1.0), px(1.0))),
                        color,
                    ));
                }
            },
        )
        .absolute()
        .bottom_0()
        .left_0()
        .w_full()
        .h(px(1.0))
    }
    pub(super) fn render_activity_group(&self, range: std::ops::Range<usize>, cx: &mut Context<Self>) -> AnyElement {
        let Some(head) = self.transcript.get(range.start) else {
            return div().into_any_element();
        };
        let id = head.id.clone();
        let summary = head.activity_group_summary;
        let failed = summary.failed;
        let running = summary.running;
        let colors = cx.theme().colors;
        let mut counts = ToolCategoryCounts::default();
        for index in range.clone() {
            if let Some(item) = self.transcript.get(index)
                && let Some(category) = item.activity_category
            {
                counts.add(category);
            }
        }
        let mut label = describe_activity_verbs(&counts)
            .unwrap_or_else(|| rust_i18n::t!("app.transcript.activity_total", total = summary.total).to_string());
        if running > 0 {
            label.push_str(display::MIDDOT);
            label.push_str(rust_i18n::t!("app.transcript.activity_running", count = running).as_ref());
        }
        if failed > 0 {
            label.push_str(display::MIDDOT);
            label.push_str(rust_i18n::t!("app.transcript.activity_failed", count = failed).as_ref());
        }
        let toggle = desktop_button(format!("activity-group-{}", head.id))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .justify_start()
                    .gap_2()
                    .child(
                        div()
                            .id(format!("activity-group-label-{}", head.id))
                            .test_support()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .child(label.clone()),
                    )
                    .child(transcript_chevron_lane(head.activity_expanded)),
            )
            .accessibility_label(format!(
                "{} {label}",
                if head.activity_expanded {
                    rust_i18n::t!("app.transcript.collapse")
                } else {
                    rust_i18n::t!("app.transcript.expand")
                }
            ))
            .toggled(head.activity_expanded)
            .ghost()
            .small()
            .flex_1()
            .min_w_0()
            .justify_start()
            .text_color(themes::muted_text(&colors))
            .on_click(cx.listener(move |view, _, _, cx| {
                view.transcript.toggle_activity_group(&id);
                cx.notify();
            }));
        // The leading slot sits beside the toggle, never inside it: the
        // toggle keeps the flexible width and truncates first on narrow rows,
        // while the slot holds running status or, when idle, the single
        // inspector entry point. Status wins over the shortcut: a running
        // group always shows its spinner. One entry point only: Output and
        // Utilities stay one tab-switch away once the inspector is open, and
        // expanded tool rows keep their exact Output deep links, so per-tab
        // triplets on every group would repeat three destinations N times
        // for no new reachability.
        let leading = if running > 0 {
            transcript_icon_slot(
                div()
                    .id(format!("activity-group-running-indicator-{}", head.id))
                    .test_support()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Spinner::new().small().color(Self::transcript_tool_icon_color(colors))),
            )
            .into_any_element()
        } else {
            transcript_icon_slot(Self::activity_group_cta(&head.id, self.transcript_row_revealed(head.id.as_str()), cx))
                .into_any_element()
        };
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .child(leading)
            .child(toggle)
            .into_any_element()
    }

    /// Single transcript entry point into the inspector, landing on Activity:
    /// the tab that lists this group's rows with their per-row output actions.
    /// An icon-only ghost button in the leading slot: the accessible name and
    /// tooltip reuse the inspector tab vocabulary so both surfaces name the
    /// same destination. Recedes until the row is hovered or keyboard
    /// highlighted, keeping its space so the toggle never re-truncates. The
    /// xsmall icon box centers on the narrower slot lane; the bleed stays
    /// inside the row gaps.
    fn activity_group_cta(head_id: &str, revealed: bool, cx: &mut Context<Self>) -> impl IntoElement {
        desktop_button(format!("activity-group-open-activity-{head_id}"))
            .icon(IconName::Activity)
            .accessibility_label(InspectorTab::Activity.label())
            .tooltip(InspectorTab::Activity.label())
            .ghost()
            .xsmall()
            .when(!revealed, |button| button.invisible())
            .tab_stop(false)
            .on_click(cx.listener(move |view, _, _, cx| view.open_inspector_tab(InspectorTab::Activity, cx)))
    }

    /// Jump button beside an expanded tool row's copy action, opening that
    /// exact capture in Output. Same affordance as the Activity tab's per-row
    /// output action; callers gate on [`TranscriptItem::has_output`] so the
    /// CTA never lands on an unrelated fallback.
    pub(super) fn tool_output_cta(item: &TranscriptItem, cx: &mut Context<Self>) -> impl IntoElement {
        let output_id = item.id.clone();
        let output_title = item.title.to_string();
        desktop_button(format!("tool-open-output-{}", item.id))
            .icon(IconName::Terminal)
            .ghost()
            .xsmall()
            .accessibility_label(
                rust_i18n::t!("app.inspector.view_output_for", title = output_title.as_str()).to_string(),
            )
            .tooltip(rust_i18n::t!("app.inspector.view_captured_output").to_string())
            .on_click(cx.listener(move |view, _, window, cx| {
                view.select_activity_output(output_id.clone(), window, cx);
            }))
    }

    /// Tool-kind icon for a transcript callout, Codex-style: what ran, not
    /// whether it worked (book for reads, folder for listings, terminal for
    /// runs, magnifier for searches, pencil for edits, checklist for tracker
    /// updates). Icons follow the projected category; only tools whose icon
    /// differs from their category default are named (listings show a folder,
    /// PTY session tools show terminal work). Unknown names — including
    /// future tools — and nameless rows resolve by category, then to the
    /// neutral activity glyph, so a registry addition can never break the
    /// icon.
    pub(super) fn transcript_tool_kind_icon(tool_name: Option<&str>, category: Option<ToolCategory>) -> IconName {
        if let Some(name) = tool_name {
            match strip_mcp_prefix(name) {
                "list_files" => return IconName::Folder,
                "read_pty_session" | "send_pty_input" | "write_stdin" => return IconName::Terminal,
                _ => {}
            }
        }
        match category {
            Some(ToolCategory::Read) => IconName::BookOpen,
            Some(ToolCategory::Edit) => IconName::Pencil,
            Some(ToolCategory::Run) => IconName::Terminal,
            Some(ToolCategory::Search) => IconName::Search,
            Some(ToolCategory::Tracker) => IconName::ListChecks,
            Some(ToolCategory::Other) | None => IconName::Activity,
        }
    }

    /// Verb-first tool title callout, Codex-style: the action verb in the UI
    /// font, the command/object in mono, both dim. The verb never truncates;
    /// the remainder ellipsizes like the old single-span title. Single-word
    /// titles keep the flat shape. The accessible name stays on the owning
    /// button (full title plus status), so these spans carry test ids only.
    pub(super) fn transcript_tool_title(
        item_id: &str,
        title: &str,
        title_color: gpui_kit::Hsla,
        mono_family: gpui_kit::SharedString,
    ) -> AnyElement {
        let (verb, rest) = split_tool_title_verb(title);
        let title_row = div()
            .id(format!("tool-title-{item_id}"))
            .test_support()
            .min_w_0()
            .flex_1()
            .text_color(title_color);
        if rest.is_empty() {
            title_row.truncate().child(verb.to_string()).into_any_element()
        } else {
            title_row
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .id(format!("tool-title-verb-{item_id}"))
                        .test_support()
                        .flex_shrink_0()
                        .child(verb.to_string()),
                )
                .child(
                    div()
                        .id(format!("tool-title-rest-{item_id}"))
                        .test_support()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .font_family(mono_family)
                        .child(rest.to_string()),
                )
                .into_any_element()
        }
    }

    /// Jump button in a diff card header, opening Changes on the change-set's
    /// first previewable file, or the tab alone when nothing previews. The
    /// target resolves at click time from the live transcript (like the
    /// copy-patch action), so a rebuilt projection cannot strand it. Always
    /// visible: the diff card summarizes edits, so its way into the full
    /// change view must not hide behind hover.
    pub(super) fn diff_changes_cta(item_id: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let id = item_id.to_string();
        desktop_button(format!("diff-open-changes-{item_id}"))
            .label(rust_i18n::t!("app.inspector.tab_changes").to_string())
            .ghost()
            .xsmall()
            .tab_stop(false)
            .on_click(cx.listener(move |view, _, _, cx| {
                let open_path = view
                    .transcript
                    .items_newest_first()
                    .find(|candidate| candidate.id == id)
                    .and_then(|candidate| candidate.diff.as_ref())
                    .and_then(|diff| {
                        let previews = gpui_kit::component::diff::DiffFile::parse(&diff.patch).ok()?;
                        diff.files
                            .iter()
                            .find(|file| previews.iter().any(|preview| preview.path().as_ref() == file.path.as_str()))
                            .map(|file| file.path.clone())
                    });
                match open_path {
                    Some(path) if view.select_change_from_item(&id, &path, cx) => {}
                    _ => view.open_inspector_tab(InspectorTab::Changes, cx),
                }
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_icons_name_the_tool_not_the_outcome() {
        use ToolCategory as Category;
        let icon = DesktopView::transcript_tool_kind_icon;
        // One arm per tool family, each with an MCP-wrapped twin proving the
        // prefix strip: the twin must icon identically to the bare name.
        for (name, category, expected) in [
            ("read_file", Some(Category::Read), IconName::BookOpen),
            ("mcp__fs__read_file", Some(Category::Read), IconName::BookOpen),
            ("list_files", Some(Category::Read), IconName::Folder),
            ("mcp__fs__list_files", Some(Category::Read), IconName::Folder),
            ("web_fetch", Some(Category::Read), IconName::BookOpen),
            ("get_errors", Some(Category::Read), IconName::BookOpen),
            ("edit_file", Some(Category::Edit), IconName::Pencil),
            ("mcp__fs__edit_file", Some(Category::Edit), IconName::Pencil),
            ("apply_patch", Some(Category::Edit), IconName::Pencil),
            ("exec_command", Some(Category::Run), IconName::Terminal),
            ("mcp__sh__exec_command", Some(Category::Run), IconName::Terminal),
            ("shell", Some(Category::Run), IconName::Terminal),
            ("run_pty_cmd", Some(Category::Run), IconName::Terminal),
            ("create_pty_session", Some(Category::Run), IconName::Terminal),
            ("code_search", Some(Category::Search), IconName::Search),
            ("grep_file", Some(Category::Search), IconName::Search),
            ("task_tracker", Some(Category::Tracker), IconName::ListChecks),
        ] {
            assert_eq!(icon(Some(name), category), expected, "{name} icons by name");
        }
        // Asymmetric: PTY session tools icon as terminal work even though
        // `read_pty_session` categorizes as a read.
        assert_eq!(icon(Some("read_pty_session"), Some(Category::Read)), IconName::Terminal);
        assert_eq!(icon(Some("send_pty_input"), Some(Category::Other)), IconName::Terminal);
        assert_eq!(icon(Some("write_stdin"), Some(Category::Other)), IconName::Terminal);
        // Unknown names — including future tools — fall back to the
        // projected category, never to a broken or missing icon.
        assert_eq!(icon(Some("DiagramCommand"), Some(Category::Edit)), IconName::Pencil);
        assert_eq!(icon(Some("diagram_render"), Some(Category::Run)), IconName::Terminal);
        assert_eq!(icon(Some("whatever"), Some(Category::Other)), IconName::Activity);
        // Unified tools carry no name arm: the action-aware projected
        // category decides, so a read icons as a book and a run as terminal.
        assert_eq!(icon(Some("file_operation_internal"), Some(Category::Read)), IconName::BookOpen);
        assert_eq!(icon(Some("file_operation_internal"), Some(Category::Edit)), IconName::Pencil);
        assert_eq!(icon(Some("command_session_internal"), Some(Category::Run)), IconName::Terminal);
        // Asymmetric: exceptions are name-driven and need no category, while
        // ordinary names without a category stay neutral.
        assert_eq!(icon(Some("list_files"), None), IconName::Folder);
        assert_eq!(icon(Some("read_pty_session"), None), IconName::Terminal);
        assert_eq!(icon(Some("read_file"), None), IconName::Activity);
        assert_eq!(icon(Some("exec_command"), None), IconName::Activity);
        // Nameless rows (executions, outputs) icon purely by category.
        for (category, expected) in [
            (Some(Category::Read), IconName::BookOpen),
            (Some(Category::Edit), IconName::Pencil),
            (Some(Category::Run), IconName::Terminal),
            (Some(Category::Search), IconName::Search),
            (Some(Category::Tracker), IconName::ListChecks),
            (Some(Category::Other), IconName::Activity),
            (None, IconName::Activity),
        ] {
            assert_eq!(icon(None, category), expected, "{category:?} icons by category");
        }
    }
}
