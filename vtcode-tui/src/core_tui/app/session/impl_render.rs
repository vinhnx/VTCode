use super::layout::{
    BottomPanelKind, resolve_bottom_panel_spec, split_input_and_bottom_panel_area,
};
use super::*;
use crate::config::constants::ui;
use crate::core_tui::session::render as core_render;
use crate::core_tui::session::{inline_list, list_panel, message_renderer};

impl Session {
    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let Some(viewport) = self.core.begin_frame(frame) else {
            return;
        };
        let metrics = self.core.measure_frame(viewport);
        let panel = resolve_bottom_panel_spec(
            self,
            viewport,
            metrics.header_height,
            metrics.input_core_height,
        );
        let layout = self
            .core
            .build_frame_layout(viewport, metrics, panel.height);
        self.core.set_modal_list_area(None);
        let transcript_area = layout.main_area;
        let (input_area, bottom_panel_area) =
            split_input_and_bottom_panel_area(layout.input_area, panel.height);
        self.core.set_bottom_panel_area(bottom_panel_area);
        self.core.render_base_frame(frame, &layout, transcript_area);
        self.core.render_input(frame, input_area);
        if let Some(panel_area) = bottom_panel_area {
            match panel.kind {
                BottomPanelKind::AgentPalette => {
                    render::render_agent_palette(self, frame, panel_area);
                }
                BottomPanelKind::FilePalette => {
                    render::render_file_palette(self, frame, panel_area);
                }
                BottomPanelKind::HistoryPicker => {
                    render::render_history_picker(self, frame, panel_area);
                }
                BottomPanelKind::SlashPalette => {
                    slash::render_slash_palette(self, frame, panel_area);
                }
                BottomPanelKind::TaskPanel => {
                    render_task_panel(self, frame, panel_area);
                }
                BottomPanelKind::LocalAgents => {
                    render::render_local_agents(self, frame, panel_area);
                }
                BottomPanelKind::None => {
                    frame.render_widget(Clear, panel_area);
                }
            }
        }

        if self.has_active_overlay() {
            core_render::render_modal(
                self,
                frame,
                core_render::floating_modal_area(layout.viewport),
            );
        }

        if self.diff_preview_state().is_some() {
            diff_preview::render_diff_preview(self, frame, layout.viewport);
        }
        self.core.finalize_mouse_selection(frame, layout.viewport);
    }

    #[allow(dead_code)]
    pub(crate) fn render_message_spans(&self, index: usize) -> Vec<Span<'static>> {
        let Some(line) = self.core.lines.get(index) else {
            return vec![Span::raw(String::new())];
        };
        message_renderer::render_message_spans(
            line,
            &self.core.theme,
            &self.core.labels,
            |kind| self.core.prefix_text(kind),
            |line| self.core.prefix_style(line),
            |kind| self.core.text_fallback(kind),
        )
    }
}

fn render_task_panel(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let rows = if session.task_panel_lines.is_empty() {
        vec![(
            inline_list::InlineListRow::single(
                ui::PLAN_STATUS_EMPTY.to_string().into(),
                session.core.header_secondary_style(),
            ),
            1,
        )]
    } else {
        session
            .task_panel_lines
            .iter()
            .map(|line| {
                (
                    inline_list::InlineListRow::single(
                        line.clone().into(),
                        session.core.header_secondary_style(),
                    ),
                    1,
                )
            })
            .collect()
    };
    let item_count = session.task_panel_lines.len();
    let sections = list_panel::SharedListPanelSections {
        header: vec![Line::from(vec![Span::styled(
            ui::PLAN_BLOCK_TITLE.to_string(),
            session.core.section_title_style(),
        )])],
        info: vec![Line::from(format!(
            "{} item{}",
            item_count,
            if item_count == 1 { "" } else { "s" }
        ))],
        search: None,
    };
    let styles = list_panel::SharedListPanelStyles {
        base_style: session.core.styles.default_style(),
        selected_style: Some(session.core.styles.modal_list_highlight_style()),
        text_style: session.core.header_secondary_style(),
    };
    let mut model = list_panel::StaticRowsListPanelModel {
        rows,
        selected: None,
        offset: 0,
        visible_rows: area.height as usize,
    };
    list_panel::render_shared_list_panel(frame, area, sections, styles, &mut model);
}
