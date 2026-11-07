//! Inspector Tasks panel: typed `tracker.updated` checklist viewer.
//!
//! Renders the latest checklist snapshot (live `TrackerUpdated` events plus
//! session-load seeding) with CLI panel parity: resolved title, `N/M
//! complete` progress, and the compact item tree with status text styling.
//! Read-only on purpose: checklist mutation stays agent-owned through the
//! `task_tracker` tool, so rows carry no actions and need no keyboard focus
//! state. User-visible strings are English literals: new copy stays
//! untranslated unless full native-quality translations ship for every
//! locale, and no existing key covers task rows.
use super::*;
use vtcode_exec_events::tracker::{TrackerItemState, TrackerItemStatus, TrackerSnapshot};

/// Single-line task row height before UI-font scaling.
const TASKS_ROW_HEIGHT: f32 = 28.;

/// Deepest tree indent rendered; deeper rows clamp so a pathological
/// `index_path` cannot push content out of the pane.
const MAX_TASK_DEPTH: usize = 6;

/// Inline transcript tree rows before the overflow line, mirroring the CLI
/// transcript cap.
const TRACKER_INLINE_MAX_ROWS: usize = 30;

/// Leaf-aware current-task pick, mirroring the CLI panel: in-progress beats
/// pending beats blocked, leaves beat parents.
pub(in super::super) fn tracker_current_index(items: &[TrackerItemState]) -> Option<usize> {
    let is_leaf = |index_path: &str| {
        let prefix = format!("{index_path}.");
        !items.iter().any(|item| item.index_path.starts_with(&prefix))
    };
    for want_leaf in [true, false] {
        for status in [
            TrackerItemStatus::InProgress,
            TrackerItemStatus::Pending,
            TrackerItemStatus::Blocked,
        ] {
            if let Some((index, _)) = items
                .iter()
                .enumerate()
                .find(|(_, item)| item.status == status && (!want_leaf || is_leaf(item.index_path.as_str())))
            {
                return Some(index);
            }
        }
    }
    None
}

/// Tree depth from a dotted (`1.2`) or flat (`1`) index path.
fn tracker_task_depth(index_path: &str) -> usize {
    index_path.split('.').count().saturating_sub(1).min(MAX_TASK_DEPTH)
}

/// Lowercase status fragment for accessible row labels ("Write code, in
/// progress").
fn tracker_status_label(status: TrackerItemStatus) -> &'static str {
    match status {
        TrackerItemStatus::Pending => "pending",
        TrackerItemStatus::InProgress => "in progress",
        TrackerItemStatus::Completed => "completed",
        TrackerItemStatus::Blocked => "blocked",
        TrackerItemStatus::Unknown => "unknown",
    }
}

/// Resolved checklist title for panel surfaces, mirroring the CLI panel
/// fallback.
fn tracker_panel_title(snapshot: &TrackerSnapshot) -> String {
    vtcode_commons::tracker_title::resolve_tracker_title(
        Some(snapshot.title.as_str()),
        snapshot.items.first().map(|item| item.description.as_str()),
        "Task tracker",
    )
}

/// Status text color for one task row, mirroring the CLI tracker block:
/// completed reads dim, in-progress reads primary (the current row adds
/// semibold plus the marker), blocked reads warning, pending reads default.
fn task_row_color(status: TrackerItemStatus, colors: &gpui_kit::component::theme::ThemeColor) -> gpui_kit::Hsla {
    match status {
        TrackerItemStatus::Completed => colors.muted_foreground,
        TrackerItemStatus::Blocked => colors.warning,
        TrackerItemStatus::InProgress => colors.primary,
        TrackerItemStatus::Pending | TrackerItemStatus::Unknown => colors.foreground,
    }
}

impl DesktopView {
    pub(in super::super) fn render_tasks_tab(
        &mut self,
        colors: &gpui_kit::component::theme::ThemeColor,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = inspector_tab_body()
            .id("tasks-list")
            .test_support()
            .overflow_hidden()
            .gap_2()
            .p_3();
        let snapshot = self.tracker_snapshot.clone().filter(|snapshot| !snapshot.items.is_empty());
        let Some(snapshot) = snapshot else {
            return body
                .child(empty_state("No tasks yet", "Task tracker updates will appear here.", IconName::ListChecks))
                .into_any_element();
        };
        let title = tracker_panel_title(&snapshot);
        let progress = format!("{}/{} complete", snapshot.completed, snapshot.total);
        let current = tracker_current_index(&snapshot.items);
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .child(div().flex_1().min_w_0().truncate().child(row_title(title, colors)))
                .child(
                    div()
                        .id("tasks-progress")
                        .test_support()
                        .role(gpui_kit::Role::Label)
                        .aria_label(progress.clone())
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(progress),
                ),
        );
        let row_height = scale_row_height_for_ui_font(TASKS_ROW_HEIGHT, cx.theme().font_size);
        let item_sizes = self.tasks_row_sizes.get(snapshot.items.len(), row_height);
        let rows: Rc<Vec<TrackerItemState>> = Rc::new(snapshot.items);
        let row_colors = *colors;
        body.child(
            div()
                .id("tasks-rows")
                .test_support()
                .role(gpui_kit::Role::List)
                .aria_label("Checklist tasks".to_string())
                .relative()
                .w_full()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .child(v_virtual_list(cx.entity(), "tasks-virtual-list", item_sizes, move |_, visible_range, _, _| {
                    visible_range
                        .map(|index| {
                            rows.get(index).map_or_else(
                                || div().into_any_element(),
                                |item| render_task_row(item, Some(index) == current, &row_colors),
                            )
                        })
                        .collect()
                })),
        )
        .into_any_element()
    }

    /// Inline task block for `task_tracker` transcript rows, replaying the
    /// newest snapshot (derived views never show stale checklists). Collapsed
    /// rows show the header plus the current task; expanded rows show the
    /// bounded tree with an overflow line. Inner ids scope under `row_id` so
    /// sibling tracker rows never collide. Returns `None` without a snapshot.
    pub(in super::super) fn render_tracker_inline_block(
        &self,
        row_id: &str,
        expanded: bool,
        colors: &gpui_kit::component::theme::ThemeColor,
    ) -> Option<AnyElement> {
        let snapshot = self.tracker_snapshot.as_ref()?;
        if snapshot.items.is_empty() {
            return None;
        }
        let title = vtcode_commons::tracker_title::resolve_tracker_title(
            Some(snapshot.title.as_str()),
            snapshot.items.first().map(|item| item.description.as_str()),
            "Tasks",
        );
        let header = format!("{} {}/{}", title, snapshot.completed, snapshot.total);
        let current = tracker_current_index(&snapshot.items);
        let mut block = div()
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .child(div().text_xs().font_medium().text_color(colors.muted_foreground).child(header));
        if expanded {
            let shown = snapshot.items.len().min(TRACKER_INLINE_MAX_ROWS);
            block = block.children(snapshot.items.iter().take(shown).enumerate().map(|(index, item)| {
                div()
                    .id(format!("tracker-inline-{row_id}-row-{index}"))
                    .test_support()
                    .child(render_task_line(item, Some(index) == current, colors))
            }));
            if snapshot.items.len() > shown {
                block = block.child(
                    div()
                        .id(format!("tracker-inline-{row_id}-overflow"))
                        .test_support()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(format!("… {} more", snapshot.items.len() - shown)),
                );
            }
        } else if let Some(index) = current
            && let Some(item) = snapshot.items.get(index)
        {
            block = block.child(render_task_line(item, true, colors));
        }
        Some(block.into_any_element())
    }
}

/// One virtualized task row with depth indent, current marker, and status
/// text styling. Returns `AnyElement`: render helpers called in loops must
/// not leak the RPIT lifetime.
fn render_task_row(
    item: &TrackerItemState,
    is_current: bool,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> AnyElement {
    let depth = tracker_task_depth(item.index_path.as_str());
    let label = format!("{}, {}", item.description, tracker_status_label(item.status));
    let mut row = div()
        .id(format!("task-row-{}", item.index_path))
        .test_support()
        .role(gpui_kit::Role::ListItem)
        .aria_label(label)
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .w_full()
        .min_w_0()
        .when(is_current, |row| row.child(div().flex_shrink_0().text_color(colors.primary).child("»")))
        .child(render_task_line(item, is_current, colors))
        .into_any_element();
    for _ in 0..depth {
        row = div().pl_4().child(row).into_any_element();
    }
    row
}

/// Styled task description line shared by the panel rows and the inline
/// transcript block. Completed rows read dim italic struck-through; the
/// current row reads semibold primary; other rows read by status color.
fn render_task_line(
    item: &TrackerItemState,
    is_current: bool,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> AnyElement {
    div()
        .min_w_0()
        .flex_1()
        .truncate()
        .text_sm()
        .text_color(task_row_color(item.status, colors))
        .when(item.status == TrackerItemStatus::Completed, |line| line.italic().line_through())
        .when(is_current, |line| line.font_semibold())
        .child(item.description.clone())
        .into_any_element()
}
