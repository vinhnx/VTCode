use std::borrow::Cow;
use std::path::Path;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, ModifierKeyCode};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use regex::Regex;
use unicode_width::UnicodeWidthStr;
use vtcode_commons::{EditorTarget, parse_editor_target};

use super::super::types::InlineEvent;
use super::{
    Session,
    message::{RenderedTranscriptLink, TranscriptLine},
    wrapping,
};
use crate::ui::tui::types::InlineLinkTarget;
static NON_WHITESPACE_TOKEN_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\S+").expect("valid transcript token regex"));
const LINK_OPEN_THROTTLE_INTERVAL: Duration = Duration::from_millis(500);
static QUOTED_PATH_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"`(?:file://|~/|/|\./|\.\./|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[^`]+`|"(?:file://|~/|/|\./|\.\./|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[^"]+"|'(?:file://|~/|/|\./|\.\./|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[^']+'"#,
    )
    .expect("valid quoted transcript path regex")
});

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TranscriptFileLinkTarget {
    pub(crate) area: Rect,
    pub(crate) target: TranscriptLinkTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TranscriptLinkTarget {
    File(EditorTarget),
    Url(String),
}

#[derive(Clone, Debug)]
pub(crate) struct DetectedLinkMatch {
    start: usize,
    end: usize,
    target: TranscriptLinkTarget,
}

#[derive(Clone, Copy, Debug)]
struct StyledLinkMatch {
    start: usize,
    end: usize,
    hovered: bool,
}

#[derive(Clone, Copy, Debug)]
struct TranscriptLinkStyles {
    link: Style,
    hovered: Style,
}

pub(crate) enum TranscriptLinkClickAction {
    Open(InlineEvent),
    Consume,
    Ignore,
}

impl Session {
    pub(crate) fn clear_transcript_file_link_targets(&mut self) {
        self.transcript_file_link_targets.clear();
        self.hovered_transcript_file_link = None;
    }

    pub(crate) fn decorate_borrowed_cached_transcript_links(
        &mut self,
        lines: &[TranscriptLine],
        area: Rect,
    ) -> Vec<Line<'static>> {
        self.decorate_borrowed_transcript_links_impl(lines, area, false)
    }

    #[allow(dead_code)]
    pub(crate) fn decorate_borrowed_transcript_links(
        &mut self,
        lines: &[TranscriptLine],
        area: Rect,
    ) -> Vec<Line<'static>> {
        self.decorate_borrowed_transcript_links_impl(lines, area, true)
    }

    fn decorate_borrowed_transcript_links_impl(
        &mut self,
        lines: &[TranscriptLine],
        area: Rect,
        detect_raw_links: bool,
    ) -> Vec<Line<'static>> {
        if !detect_raw_links && lines.iter().all(|line| line.explicit_links.is_empty()) {
            self.transcript_file_link_targets.clear();
            self.hovered_transcript_file_link = None;
            return lines.iter().map(|line| line.line.clone()).collect();
        }

        let workspace_root = self.workspace_root.as_deref();
        let link_style = self
            .styles
            .transcript_link_style()
            .add_modifier(Modifier::UNDERLINED);
        let styles = TranscriptLinkStyles {
            link: link_style,
            hovered: link_style.add_modifier(Modifier::BOLD),
        };
        let mut targets = Vec::new();
        let mut decorated = Vec::with_capacity(lines.len());

        for (row_idx, transcript_line) in lines.iter().enumerate() {
            decorated.push(decorate_transcript_line(
                transcript_line.line.clone(),
                &transcript_line.explicit_links,
                row_idx,
                area,
                workspace_root,
                self.last_mouse_position,
                detect_raw_links,
                styles,
                &mut targets,
            ));
        }

        self.transcript_file_link_targets = targets;
        self.hovered_transcript_file_link = self.mouse_hovered_transcript_file_link_index();

        decorated
    }

    #[allow(dead_code)]
    pub(crate) fn decorate_visible_transcript_links(
        &mut self,
        lines: Vec<TranscriptLine>,
        area: Rect,
    ) -> Vec<Line<'static>> {
        self.decorate_visible_transcript_links_impl(lines, area, true)
    }

    pub(crate) fn decorate_visible_cached_transcript_links(
        &mut self,
        lines: Vec<TranscriptLine>,
        area: Rect,
    ) -> Vec<Line<'static>> {
        self.decorate_visible_transcript_links_impl(lines, area, false)
    }

    fn decorate_visible_transcript_links_impl(
        &mut self,
        lines: Vec<TranscriptLine>,
        area: Rect,
        detect_raw_links: bool,
    ) -> Vec<Line<'static>> {
        if !detect_raw_links && lines.iter().all(|line| line.explicit_links.is_empty()) {
            self.transcript_file_link_targets.clear();
            self.hovered_transcript_file_link = None;
            return lines.into_iter().map(|line| line.line).collect();
        }

        let workspace_root = self.workspace_root.as_deref();
        let link_style = self
            .styles
            .transcript_link_style()
            .add_modifier(Modifier::UNDERLINED);
        let styles = TranscriptLinkStyles {
            link: link_style,
            hovered: link_style.add_modifier(Modifier::BOLD),
        };
        let mut targets = Vec::new();
        let mut decorated = Vec::with_capacity(lines.len());

        for (row_idx, transcript_line) in lines.into_iter().enumerate() {
            let TranscriptLine {
                line,
                explicit_links,
            } = transcript_line;
            decorated.push(decorate_transcript_line(
                line,
                &explicit_links,
                row_idx,
                area,
                workspace_root,
                self.last_mouse_position,
                detect_raw_links,
                styles,
                &mut targets,
            ));
        }

        self.transcript_file_link_targets = targets;
        self.hovered_transcript_file_link = self.mouse_hovered_transcript_file_link_index();

        decorated
    }

    pub(crate) fn update_transcript_file_link_hover(&mut self, column: u16, row: u16) -> bool {
        let previous_position = self.last_mouse_position;
        let previous_modal_hover = previous_position.and_then(|(previous_column, previous_row)| {
            self.modal_link_target_index_at(previous_column, previous_row)
        });
        self.last_mouse_position = Some((column, row));

        let hovered = self.mouse_hovered_transcript_file_link_index();
        let transcript_hover_changed = hovered != self.hovered_transcript_file_link;
        let modal_hover_changed =
            previous_modal_hover != self.modal_link_target_index_at(column, row);

        self.hovered_transcript_file_link = hovered;
        transcript_hover_changed || modal_hover_changed
    }

    pub(crate) fn transcript_file_link_click_action(
        &self,
        column: u16,
        row: u16,
        modifiers: KeyModifiers,
    ) -> TranscriptLinkClickAction {
        let modifiers = self.effective_link_click_modifiers(modifiers);
        self.link_click_action(
            self.transcript_link_target_index_at(column, row)
                .and_then(|index| self.transcript_file_link_targets.get(index)),
            modifiers,
        )
    }

    pub(crate) fn modal_link_click_action(
        &self,
        column: u16,
        row: u16,
        modifiers: KeyModifiers,
    ) -> TranscriptLinkClickAction {
        let modifiers = self.effective_link_click_modifiers(modifiers);
        self.link_click_action(
            self.modal_link_target_index_at(column, row)
                .and_then(|index| self.modal_link_targets.get(index)),
            modifiers,
        )
    }

    pub(crate) fn transcript_file_link_double_click_action(
        &self,
        column: u16,
        row: u16,
    ) -> TranscriptLinkClickAction {
        self.link_open_action(
            self.transcript_link_target_index_at(column, row)
                .and_then(|index| self.transcript_file_link_targets.get(index)),
        )
    }

    pub(crate) fn modal_link_double_click_action(
        &self,
        column: u16,
        row: u16,
    ) -> TranscriptLinkClickAction {
        self.link_open_action(
            self.modal_link_target_index_at(column, row)
                .and_then(|index| self.modal_link_targets.get(index)),
        )
    }

    pub(crate) fn throttle_link_click_action(
        &mut self,
        action: TranscriptLinkClickAction,
    ) -> TranscriptLinkClickAction {
        let TranscriptLinkClickAction::Open(outbound) = action else {
            return action;
        };

        let Some(key) = link_action_throttle_key(&outbound).map(str::to_owned) else {
            return TranscriptLinkClickAction::Open(outbound);
        };
        let now = Instant::now();
        if self
            .last_link_open
            .as_ref()
            .is_some_and(|(last_key, last_at)| {
                last_key == &key
                    && now.saturating_duration_since(*last_at) <= LINK_OPEN_THROTTLE_INTERVAL
            })
        {
            return TranscriptLinkClickAction::Consume;
        }

        self.last_link_open = Some((key, now));
        TranscriptLinkClickAction::Open(outbound)
    }

    pub(crate) fn queue_link_click_action(&mut self, action: TranscriptLinkClickAction) -> bool {
        match action {
            TranscriptLinkClickAction::Open(outbound) => {
                self.pending_link_open = link_action_throttle_key(&outbound).map(str::to_owned);
                false
            }
            TranscriptLinkClickAction::Consume => true,
            TranscriptLinkClickAction::Ignore => false,
        }
    }

    pub(crate) fn pending_link_click_action(
        &mut self,
        action: TranscriptLinkClickAction,
    ) -> TranscriptLinkClickAction {
        let Some(pending_key) = self.pending_link_open.clone() else {
            return TranscriptLinkClickAction::Ignore;
        };
        let TranscriptLinkClickAction::Open(outbound) = action else {
            return TranscriptLinkClickAction::Ignore;
        };

        if link_action_throttle_key(&outbound) != Some(pending_key.as_str()) {
            return TranscriptLinkClickAction::Ignore;
        }

        self.pending_link_open = None;
        self.throttle_link_click_action(TranscriptLinkClickAction::Open(outbound))
    }

    pub(crate) fn clear_pending_link_click(&mut self) {
        self.pending_link_open = None;
    }

    pub(crate) fn update_held_key_modifiers(&mut self, key: &KeyEvent) {
        let Some(modifier) = modifier_key_flag(key.code) else {
            return;
        };

        match key.kind {
            KeyEventKind::Press | KeyEventKind::Repeat => self.held_key_modifiers.insert(modifier),
            KeyEventKind::Release => self.held_key_modifiers.remove(modifier),
        }
    }

    pub(crate) fn clear_held_key_modifiers(&mut self) {
        self.held_key_modifiers = KeyModifiers::empty();
    }

    fn mouse_hovered_transcript_file_link_index(&self) -> Option<usize> {
        let (column, row) = self.last_mouse_position?;
        self.transcript_link_target_index_at(column, row)
    }

    fn transcript_link_target_index_at(&self, column: u16, row: u16) -> Option<usize> {
        self.transcript_file_link_targets
            .iter()
            .position(|target| point_in_rect(target.area, column, row))
    }

    fn modal_link_target_index_at(&self, column: u16, row: u16) -> Option<usize> {
        self.modal_link_targets
            .iter()
            .position(|target| point_in_rect(target.area, column, row))
    }

    fn effective_link_click_modifiers(&self, mouse_modifiers: KeyModifiers) -> KeyModifiers {
        mouse_modifiers | self.held_key_modifiers
    }

    fn link_click_action(
        &self,
        target: Option<&TranscriptFileLinkTarget>,
        modifiers: KeyModifiers,
    ) -> TranscriptLinkClickAction {
        let Some(target) = target else {
            return TranscriptLinkClickAction::Ignore;
        };

        if should_consume_transcript_link_click(modifiers) {
            return TranscriptLinkClickAction::Consume;
        }

        self.link_open_action(Some(target))
    }

    fn link_open_action(
        &self,
        target: Option<&TranscriptFileLinkTarget>,
    ) -> TranscriptLinkClickAction {
        let Some(target) = target else {
            return TranscriptLinkClickAction::Ignore;
        };

        TranscriptLinkClickAction::Open(match &target.target {
            TranscriptLinkTarget::File(target) => {
                InlineEvent::OpenFileInEditor(target.canonical_string())
            }
            TranscriptLinkTarget::Url(url) => InlineEvent::OpenUrl(url.clone()),
        })
    }
}

fn decorate_transcript_line(
    line: Line<'static>,
    explicit_links: &[RenderedTranscriptLink],
    row_idx: usize,
    area: Rect,
    workspace_root: Option<&Path>,
    last_mouse_position: Option<(u16, u16)>,
    detect_raw_links: bool,
    styles: TranscriptLinkStyles,
    targets: &mut Vec<TranscriptFileLinkTarget>,
) -> Line<'static> {
    let mut styled_matches = Vec::new();
    append_explicit_link_matches(
        targets,
        &mut styled_matches,
        explicit_links,
        row_idx,
        area,
        workspace_root,
        last_mouse_position,
    );

    if detect_raw_links && may_contain_link_candidate(&line, explicit_links) {
        let text = transcript_line_text(&line);
        let matches = detect_transcript_link_matches(&text, workspace_root);
        styled_matches.reserve(matches.len());
        for DetectedLinkMatch { start, end, target } in matches {
            if styled_matches
                .iter()
                .any(|existing| start < existing.end && end > existing.start)
            {
                continue;
            }

            let start_col = UnicodeWidthStr::width(&text[..start]);
            let width = UnicodeWidthStr::width(&text[start..end]);
            if width == 0 {
                continue;
            }

            let target_area = Rect::new(
                area.x.saturating_add(start_col as u16),
                area.y.saturating_add(row_idx as u16),
                width as u16,
                1,
            );
            let hovered = last_mouse_position
                .is_some_and(|(column, row)| point_in_rect(target_area, column, row));

            targets.push(TranscriptFileLinkTarget {
                area: target_area,
                target,
            });
            styled_matches.push(StyledLinkMatch {
                start,
                end,
                hovered,
            });
        }
    }

    if styled_matches.is_empty() {
        line
    } else {
        style_transcript_file_link_line(line, &styled_matches, styles.link, styles.hovered)
    }
}

fn point_in_rect(area: Rect, column: u16, row: u16) -> bool {
    row >= area.y
        && row < area.y.saturating_add(area.height)
        && column >= area.x
        && column < area.x.saturating_add(area.width)
}

fn link_action_throttle_key(event: &InlineEvent) -> Option<&str> {
    match event {
        InlineEvent::OpenFileInEditor(path) | InlineEvent::OpenUrl(path) => Some(path.as_str()),
        _ => None,
    }
}

fn should_consume_transcript_link_click(modifiers: KeyModifiers) -> bool {
    // On macOS, Ctrl+Click is a secondary-click gesture. If a terminal forwards it
    // to the TUI as Left+Control, consume it on transcript links so it does not
    // open the link or fall through into transcript selection/expansion handling.
    cfg!(target_os = "macos")
        && modifiers.contains(KeyModifiers::CONTROL)
        && !modifiers.contains(KeyModifiers::SUPER)
        && !modifiers.contains(KeyModifiers::META)
}

fn modifier_key_flag(code: KeyCode) -> Option<KeyModifiers> {
    match code {
        KeyCode::Modifier(ModifierKeyCode::LeftShift | ModifierKeyCode::RightShift) => {
            Some(KeyModifiers::SHIFT)
        }
        KeyCode::Modifier(ModifierKeyCode::LeftControl | ModifierKeyCode::RightControl) => {
            Some(KeyModifiers::CONTROL)
        }
        KeyCode::Modifier(ModifierKeyCode::LeftAlt | ModifierKeyCode::RightAlt) => {
            Some(KeyModifiers::ALT)
        }
        KeyCode::Modifier(ModifierKeyCode::LeftSuper | ModifierKeyCode::RightSuper) => {
            Some(KeyModifiers::SUPER)
        }
        KeyCode::Modifier(ModifierKeyCode::LeftMeta | ModifierKeyCode::RightMeta) => {
            Some(KeyModifiers::META)
        }
        _ => None,
    }
}

fn append_explicit_link_matches(
    targets: &mut Vec<TranscriptFileLinkTarget>,
    styled_matches: &mut Vec<StyledLinkMatch>,
    explicit_links: &[RenderedTranscriptLink],
    row_idx: usize,
    area: Rect,
    workspace_root: Option<&Path>,
    last_mouse_position: Option<(u16, u16)>,
) {
    for explicit in explicit_links {
        let target_area = Rect::new(
            area.x.saturating_add(explicit.start_col as u16),
            area.y.saturating_add(row_idx as u16),
            explicit.width as u16,
            1,
        );
        let hovered = last_mouse_position
            .is_some_and(|(column, row)| point_in_rect(target_area, column, row));
        let target = match &explicit.target {
            InlineLinkTarget::Url(url) => resolve_transcript_file_target(url, workspace_root)
                .map(TranscriptLinkTarget::File)
                .unwrap_or_else(|| TranscriptLinkTarget::Url(url.clone())),
        };
        targets.push(TranscriptFileLinkTarget {
            area: target_area,
            target,
        });
        styled_matches.push(StyledLinkMatch {
            start: explicit.start,
            end: explicit.end,
            hovered,
        });
    }
}

fn may_contain_link_candidate(line: &Line<'_>, explicit_links: &[RenderedTranscriptLink]) -> bool {
    !explicit_links.is_empty()
        || line
            .spans
            .iter()
            .any(|span| may_contain_link_candidate_text(span.content.as_ref()))
}

fn may_contain_link_candidate_text(text: &str) -> bool {
    text.contains("://")
        || text.contains('/')
        || text.contains('\\')
        || text.contains("~/")
        || text.contains("./")
        || text.contains("../")
}

pub(crate) fn transcript_line_text<'a>(line: &'a Line<'a>) -> Cow<'a, str> {
    match line.spans.as_slice() {
        [] => Cow::Borrowed(""),
        [span] => Cow::Borrowed(span.content.as_ref()),
        spans => {
            let capacity = spans.iter().map(|span| span.content.len()).sum();
            let mut text = String::with_capacity(capacity);
            for span in spans {
                text.push_str(span.content.as_ref());
            }
            Cow::Owned(text)
        }
    }
}

fn style_transcript_file_link_line(
    line: Line<'static>,
    matches: &[StyledLinkMatch],
    link_style: Style,
    hovered_style: Style,
) -> Line<'static> {
    let line_style = line.style;
    let line_alignment = line.alignment;
    let mut spans = Vec::new();
    let mut global_offset = 0usize;
    let mut match_idx = 0usize;

    for span in line.spans {
        let content = span.content.into_owned();
        if content.is_empty() {
            continue;
        }

        let span_start = global_offset;
        let span_end = span_start + content.len();
        while match_idx < matches.len() && matches[match_idx].end <= span_start {
            match_idx += 1;
        }

        let mut current_match_idx = match_idx;
        let mut local_offset = 0usize;
        while current_match_idx < matches.len() {
            let link_match = matches[current_match_idx];
            if link_match.start >= span_end {
                break;
            }

            let overlap_start = link_match.start.max(span_start);
            let overlap_end = link_match.end.min(span_end);
            let relative_start = overlap_start.saturating_sub(span_start);
            let relative_end = overlap_end.saturating_sub(span_start);

            if relative_start > local_offset {
                spans.push(Span::styled(
                    content[local_offset..relative_start].to_string(),
                    span.style,
                ));
            }

            if relative_end > relative_start {
                let style = if link_match.hovered {
                    hovered_style
                } else {
                    link_style
                };
                spans.push(Span::styled(
                    content[relative_start..relative_end].to_string(),
                    span.style.patch(style),
                ));
            }

            local_offset = relative_end;
            if link_match.end <= span_end {
                current_match_idx += 1;
            } else {
                break;
            }
        }

        if local_offset < content.len() {
            spans.push(Span::styled(
                content[local_offset..].to_string(),
                span.style,
            ));
        }

        global_offset = span_end;
        match_idx = current_match_idx;
    }

    let mut styled = Line::from(spans);
    styled.style = line_style;
    styled.alignment = line_alignment;
    styled
}

pub(crate) fn decorate_detected_link_lines(
    lines: Vec<Line<'static>>,
    area: Rect,
    workspace_root: Option<&Path>,
    last_mouse_position: Option<(u16, u16)>,
    link_style: Style,
    hovered_style: Style,
) -> (Vec<Line<'static>>, Vec<TranscriptFileLinkTarget>) {
    let mut targets = Vec::new();
    let mut decorated = Vec::new();
    let mut row_idx = 0usize;

    for line in lines {
        if row_idx >= usize::from(area.height) {
            break;
        }

        let original_text = transcript_line_text(&line);
        let matches = detect_transcript_link_matches(&original_text, workspace_root);
        let wrapped_lines = wrapping::wrap_line_preserving_urls(line, area.width.max(1) as usize);
        let mut original_offset = 0usize;

        for wrapped_line in wrapped_lines {
            if row_idx >= usize::from(area.height) {
                break;
            }

            let wrapped_text = transcript_line_text(&wrapped_line);
            let wrapped_start = original_offset;
            let wrapped_end = wrapped_start + wrapped_text.len();
            original_offset = wrapped_end;

            let mut styled_matches = Vec::new();
            for DetectedLinkMatch { start, end, target } in &matches {
                let local_start = (*start).max(wrapped_start);
                let local_end = (*end).min(wrapped_end);
                if local_start >= local_end {
                    continue;
                }

                let relative_start = local_start - wrapped_start;
                let relative_end = local_end - wrapped_start;
                let start_col = UnicodeWidthStr::width(&wrapped_text[..relative_start]);
                let width = UnicodeWidthStr::width(&wrapped_text[relative_start..relative_end]);
                if width == 0 {
                    continue;
                }

                let target_area = Rect::new(
                    area.x.saturating_add(start_col as u16),
                    area.y.saturating_add(row_idx as u16),
                    width as u16,
                    1,
                );
                let hovered = last_mouse_position
                    .is_some_and(|(column, row)| point_in_rect(target_area, column, row));
                targets.push(TranscriptFileLinkTarget {
                    area: target_area,
                    target: target.clone(),
                });
                styled_matches.push(StyledLinkMatch {
                    start: relative_start,
                    end: relative_end,
                    hovered,
                });
            }

            if styled_matches.is_empty() {
                decorated.push(wrapped_line);
            } else {
                decorated.push(style_transcript_file_link_line(
                    wrapped_line,
                    &styled_matches,
                    link_style,
                    hovered_style,
                ));
            }
            row_idx += 1;
        }
    }

    (decorated, targets)
}

pub(crate) fn project_detected_links_onto_wrapped_lines(
    wrapped_lines: &[Line<'static>],
    original_text: &str,
    workspace_root: Option<&Path>,
) -> Vec<Vec<RenderedTranscriptLink>> {
    let matches = detect_transcript_link_matches(original_text, workspace_root);
    let mut projected = Vec::with_capacity(wrapped_lines.len());
    let mut original_offset = 0usize;

    for wrapped_line in wrapped_lines {
        let wrapped_text = transcript_line_text(wrapped_line);
        let wrapped_start = original_offset;
        let wrapped_end = wrapped_start + wrapped_text.len();
        original_offset = wrapped_end;

        let mut line_links = Vec::new();
        for DetectedLinkMatch { start, end, target } in &matches {
            let local_start = (*start).max(wrapped_start);
            let local_end = (*end).min(wrapped_end);
            if local_start >= local_end {
                continue;
            }

            let relative_start = local_start - wrapped_start;
            let relative_end = local_end - wrapped_start;
            let start_col = UnicodeWidthStr::width(&wrapped_text[..relative_start]);
            let width = UnicodeWidthStr::width(&wrapped_text[relative_start..relative_end]);
            if width == 0 {
                continue;
            }

            line_links.push(RenderedTranscriptLink {
                start: relative_start,
                end: relative_end,
                start_col,
                width,
                target: InlineLinkTarget::Url(transcript_link_target_string(target)),
            });
        }

        projected.push(line_links);
    }

    projected
}

pub(crate) fn detect_rendered_transcript_links(
    line: &Line<'_>,
    workspace_root: Option<&Path>,
) -> Vec<RenderedTranscriptLink> {
    if !may_contain_link_candidate(line, &[]) {
        return Vec::new();
    }

    let text = transcript_line_text(line);
    detect_transcript_link_matches(&text, workspace_root)
        .into_iter()
        .filter_map(|DetectedLinkMatch { start, end, target }| {
            let width = UnicodeWidthStr::width(&text[start..end]);
            if width == 0 {
                return None;
            }

            Some(RenderedTranscriptLink {
                start,
                end,
                start_col: UnicodeWidthStr::width(&text[..start]),
                width,
                target: InlineLinkTarget::Url(transcript_link_target_string(&target)),
            })
        })
        .collect()
}

pub(crate) fn detect_transcript_link_matches(
    text: &str,
    workspace_root: Option<&Path>,
) -> Vec<DetectedLinkMatch> {
    let mut matches = Vec::new();

    for quoted_match in QUOTED_PATH_PATTERN.find_iter(text) {
        if let Some(link_match) = build_transcript_link_match(
            text,
            quoted_match.start(),
            quoted_match.end(),
            workspace_root,
        ) {
            matches.push(link_match);
        }
    }

    for token_match in NON_WHITESPACE_TOKEN_PATTERN.find_iter(text) {
        if matches.iter().any(|existing| {
            token_match.start() < existing.end && token_match.end() > existing.start
        }) {
            continue;
        }

        if let Some(link_match) = build_transcript_link_match(
            text,
            token_match.start(),
            token_match.end(),
            workspace_root,
        ) {
            matches.push(link_match);
        }
    }

    matches.sort_by_key(|link_match| link_match.start);
    matches.dedup_by(|left, right| left.start == right.start && left.end == right.end);

    matches
}

fn transcript_link_target_string(target: &TranscriptLinkTarget) -> String {
    match target {
        TranscriptLinkTarget::File(target) => target.canonical_string(),
        TranscriptLinkTarget::Url(url) => url.clone(),
    }
}

fn build_transcript_link_match(
    text: &str,
    token_start: usize,
    token_end: usize,
    workspace_root: Option<&Path>,
) -> Option<DetectedLinkMatch> {
    let token = &text[token_start..token_end];
    let (trimmed_start, trimmed_end) = trim_transcript_token_bounds(token);
    if trimmed_start >= trimmed_end {
        return None;
    }

    let start = token_start + trimmed_start;
    let end = token_start + trimmed_end;
    let candidate = &text[start..end];
    let target = resolve_transcript_url(candidate)
        .map(TranscriptLinkTarget::Url)
        .or_else(|| {
            resolve_transcript_file_target(candidate, workspace_root)
                .map(TranscriptLinkTarget::File)
        })?;

    Some(DetectedLinkMatch { start, end, target })
}

fn trim_transcript_token_bounds(token: &str) -> (usize, usize) {
    let mut start = 0usize;
    let mut end = token.len();

    while start < end {
        let Some(ch) = token[start..end].chars().next() else {
            break;
        };
        if matches!(ch, '(' | '[' | '{' | '<' | '"' | '\'' | '`') {
            start += ch.len_utf8();
        } else {
            break;
        }
    }

    while start < end {
        let Some(ch) = token[start..end].chars().next_back() else {
            break;
        };
        if matches!(
            ch,
            ')' | ']' | '}' | '>' | '"' | '\'' | '`' | ',' | ';' | '.' | '!' | '?'
        ) {
            // Preserve trailing ')' when it looks like a location suffix e.g. file.rs(10,5)
            if ch == ')' && location_paren_suffix_start(&token[start..end]).is_some() {
                break;
            }
            end -= ch.len_utf8();
        } else {
            break;
        }
    }

    (start, end)
}

/// Return the byte offset of a trailing parenthesized location suffix like `(10)` or `(10,5)`.
fn location_paren_suffix_start(token: &str) -> Option<usize> {
    let paren_start = token.rfind('(')?;
    let inner = token[paren_start + 1..].strip_suffix(')')?;
    // Accept `(digits)` or `(digits,digits)` — reject empty or malformed like `(,)` `(10,,5)`
    let valid = !inner.is_empty()
        && !inner.starts_with(',')
        && !inner.ends_with(',')
        && !inner.contains(",,")
        && inner.chars().all(|c| c.is_ascii_digit() || c == ',');
    valid.then_some(paren_start)
}

fn resolve_transcript_file_target(
    token: &str,
    workspace_root: Option<&Path>,
) -> Option<EditorTarget> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    let parsed = parse_editor_target(token)?;
    let stripped = parsed.path().to_string_lossy();
    if stripped.is_empty() || !looks_like_transcript_path(&stripped) {
        return None;
    }

    let resolved = if parsed.path().is_absolute() {
        parsed
    } else {
        let root = workspace_root?;
        parsed.with_resolved_path(root)
    };

    resolved.path().is_file().then_some(resolved)
}

fn resolve_transcript_url(token: &str) -> Option<String> {
    let trimmed = token.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn looks_like_transcript_path(token: &str) -> bool {
    // Explicit path prefixes
    token.starts_with("./")
        || token.starts_with("../")
        || token.starts_with('/')
        || token.starts_with("~/")
        || token.starts_with("file://")
        // Windows drive letter (e.g. C:\ or C:/)
        || (token.len() >= 3
            && token.as_bytes()[0].is_ascii_alphabetic()
            && token.as_bytes()[1] == b':'
            && matches!(token.as_bytes()[2], b'\\' | b'/'))
        // Contains path separator
        || token.contains('/')
        || token.contains('\\')
        // Has a file-like extension: non-empty stem + dot + 1-12 alphanumeric ext.
        // Called after suffix stripping so `a.rs` (not `a.rs:10`) is the input.
        // `path.is_file()` is the final filter, so favor recall here.
        || token.rsplit_once('.').is_some_and(|(stem, ext)| {
            !stem.is_empty()
                && !ext.is_empty()
                && ext.len() <= 12
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    use crate::core_tui::types::InlineTheme;

    #[test]
    fn plain_text_prefilter_skips_link_detection() {
        assert!(!may_contain_link_candidate_text("plain text only"));
        assert!(may_contain_link_candidate_text("see src/main.rs"));
        assert!(may_contain_link_candidate_text("https://example.com"));
    }

    #[test]
    fn borrowed_decoration_keeps_plain_text_undecorated() {
        let mut session = Session::new(InlineTheme::default(), None, 12);
        let lines = vec![TranscriptLine {
            line: Line::from("plain text only"),
            explicit_links: Vec::new(),
        }];

        let decorated = session.decorate_borrowed_transcript_links(&lines, Rect::new(0, 0, 20, 2));

        assert_eq!(decorated, vec![Line::from("plain text only")]);
        assert!(session.transcript_file_link_targets.is_empty());
    }

    #[test]
    fn transcript_line_text_borrows_single_span_content() {
        let line = Line::from("plain text only");

        let text = transcript_line_text(&line);

        assert!(matches!(text, Cow::Borrowed("plain text only")));
    }

    #[test]
    fn transcript_line_text_allocates_once_for_multi_span_content() {
        let line = Line::from(vec![Span::raw("plain "), Span::raw("text only")]);

        let text = transcript_line_text(&line);

        assert!(matches!(text, Cow::Owned(ref content) if content == "plain text only"));
    }
}
