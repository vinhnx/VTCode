use ratatui::layout::{Constraint, Layout, Rect};

use crate::config::constants::ui;

use super::{Session, list_panel, render, slash};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BottomPanelKind {
    None,
    InlineModal,
    FilePalette,
    HistoryPicker,
    SlashPalette,
    TaskPanel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BottomPanelSpec {
    pub(super) kind: BottomPanelKind,
    pub(super) height: u16,
}

pub(super) fn resolve_bottom_panel_spec(
    session: &mut Session,
    viewport: Rect,
    header_height: u16,
    input_reserved_height: u16,
) -> BottomPanelSpec {
    let max_panel_height = viewport
        .height
        .saturating_sub(header_height)
        .saturating_sub(input_reserved_height)
        .saturating_sub(1);
    if max_panel_height == 0 || viewport.width == 0 {
        return BottomPanelSpec {
            kind: BottomPanelKind::None,
            height: 0,
        };
    }

    if session.inline_lists_visible() {
        let split_context = SplitContext {
            width: viewport.width,
            max_panel_height,
        };
        if modal_eligible_for_inline_bottom(session) {
            if let Some(panel) = panel_from_split(
                session,
                split_context,
                BottomPanelKind::InlineModal,
                split_inline_modal_area_probe,
            ) {
                return panel;
            }
        } else if session.file_palette_active {
            if let Some(panel) = panel_from_split(
                session,
                split_context,
                BottomPanelKind::FilePalette,
                render::split_inline_file_palette_area,
            ) {
                return panel;
            }
        } else if session.history_picker_state.active {
            if let Some(panel) = panel_from_split(
                session,
                split_context,
                BottomPanelKind::HistoryPicker,
                render::split_inline_history_picker_area,
            ) {
                return panel;
            }
        } else if !session.slash_palette.is_empty()
            && let Some(panel) = panel_from_split(
                session,
                split_context,
                BottomPanelKind::SlashPalette,
                slash::split_inline_slash_area,
            )
        {
            return panel;
        }
    }

    if session.show_task_panel
        && let Some(panel) = panel_from_split(
            session,
            SplitContext {
                width: viewport.width,
                max_panel_height,
            },
            BottomPanelKind::TaskPanel,
            split_inline_task_panel_area,
        )
    {
        return panel;
    }

    BottomPanelSpec {
        kind: BottomPanelKind::None,
        height: 0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SplitContext {
    width: u16,
    max_panel_height: u16,
}

fn panel_from_split(
    session: &mut Session,
    ctx: SplitContext,
    kind: BottomPanelKind,
    split_fn: fn(&mut Session, Rect) -> (Rect, Option<Rect>),
) -> Option<BottomPanelSpec> {
    let height = probe_panel_height(session, ctx, split_fn);
    if height == 0 {
        None
    } else {
        Some(BottomPanelSpec {
            kind,
            height: normalize_panel_height(height, ctx.max_panel_height),
        })
    }
}

fn normalize_panel_height(raw_height: u16, max_panel_height: u16) -> u16 {
    if raw_height == 0 || max_panel_height == 0 {
        return 0;
    }

    let min_floor = ui::INLINE_LIST_PANEL_MIN_HEIGHT
        .min(max_panel_height)
        .max(1);
    raw_height.max(min_floor).min(max_panel_height)
}

fn modal_eligible_for_inline_bottom(session: &Session) -> bool {
    session.wizard_overlay().is_some()
        || session
            .modal_state()
            .is_some_and(|modal| modal.list.is_some())
}

fn split_inline_modal_area_probe(session: &mut Session, area: Rect) -> (Rect, Option<Rect>) {
    render::split_inline_modal_area(session, area)
}

fn split_inline_task_panel_area(session: &mut Session, area: Rect) -> (Rect, Option<Rect>) {
    let visible_lines = session.task_panel_lines.len().max(1);
    let desired_list_rows =
        list_panel::rows_to_u16(visible_lines.min(ui::INLINE_LIST_MAX_ROWS_MULTILINE));
    let fixed_rows = list_panel::fixed_section_rows(1, 1, false);
    list_panel::split_bottom_list_panel(area, fixed_rows, desired_list_rows)
}

fn probe_panel_height(
    session: &mut Session,
    ctx: SplitContext,
    split_fn: fn(&mut Session, Rect) -> (Rect, Option<Rect>),
) -> u16 {
    if ctx.width == 0 || ctx.max_panel_height == 0 {
        return 0;
    }

    let probe_area = Rect::new(0, 0, ctx.width, ctx.max_panel_height.saturating_add(1));
    let (_, panel_area) = split_fn(session, probe_area);
    panel_area.map(|area| area.height).unwrap_or(0)
}

pub(super) fn split_input_and_bottom_panel_area(
    area: Rect,
    panel_height: u16,
) -> (Rect, Option<Rect>) {
    if area.height == 0 || panel_height == 0 || area.height <= 1 {
        return (area, None);
    }

    let resolved_panel = panel_height.min(area.height.saturating_sub(1));
    if resolved_panel == 0 {
        return (area, None);
    }

    let input_height = area.height.saturating_sub(resolved_panel);
    let chunks = Layout::vertical([
        Constraint::Length(input_height.max(1)),
        Constraint::Length(resolved_panel),
    ])
    .split(area);
    (chunks[0], Some(chunks[1]))
}
