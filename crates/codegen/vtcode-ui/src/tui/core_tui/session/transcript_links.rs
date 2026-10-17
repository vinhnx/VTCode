use crate::tui::utils::transcript::{
    NON_WHITESPACE_TOKEN_PATTERN, QUOTED_PATH_PATTERN, has_explicit_path_syntax, trim_transcript_token_bounds,
};
use std::borrow::Cow;
use std::path::Path;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, ModifierKeyCode};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;
use vtcode_commons::{EditorTarget, parse_editor_target};

use super::super::types::InlineEvent;
use super::{
    Session,
    message::{RenderedTranscriptLink, TranscriptLine},
    wrapping,
};
use crate::tui::ui::tui::types::InlineLinkTarget;
const LINK_OPEN_THROTTLE_INTERVAL: Duration = Duration::from_millis(500);

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

    fn transcript_link_styles(&self) -> TranscriptLinkStyles {
        let link = self.styles.transcript_link_style().add_modifier(Modifier::UNDERLINED);
        TranscriptLinkStyles { link, hovered: link.add_modifier(Modifier::BOLD) }
    }

    fn reset_transcript_link_targets(&mut self) {
        self.transcript_file_link_targets.clear();
        self.hovered_transcript_file_link = None;
    }

    fn store_transcript_link_targets(&mut self, targets: Vec<TranscriptFileLinkTarget>) {
        self.transcript_file_link_targets = targets;
        self.hovered_transcript_file_link = self.mouse_hovered_transcript_file_link_index();
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "Single shared row helper keeps borrowed/visible paths in sync."
    )]
    fn decorate_one_transcript_row(
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
        decorate_transcript_line(
            line,
            explicit_links,
            row_idx,
            area,
            workspace_root,
            last_mouse_position,
            detect_raw_links,
            styles,
            targets,
        )
    }

    #[cfg_attr(feature = "profiling", hotpath::measure)]
    pub(crate) fn decorate_borrowed_cached_transcript_links(
        &mut self,
        lines: &[TranscriptLine],
        area: Rect,
    ) -> Vec<Line<'static>> {
        self.decorate_borrowed_transcript_links_impl(lines, area, false)
    }

    fn decorate_borrowed_transcript_links(&mut self, lines: &[TranscriptLine], area: Rect) -> Vec<Line<'static>> {
        self.decorate_borrowed_transcript_links_impl(lines, area, true)
    }

    #[cfg_attr(feature = "profiling", hotpath::measure)]
    fn decorate_borrowed_transcript_links_impl(
        &mut self,
        lines: &[TranscriptLine],
        area: Rect,
        detect_raw_links: bool,
    ) -> Vec<Line<'static>> {
        if !detect_raw_links && lines.iter().all(|line| line.explicit_links.is_empty()) {
            self.reset_transcript_link_targets();
            return lines.iter().map(|line| line.line.clone()).collect();
        }

        let styles = self.transcript_link_styles();
        let workspace_root = self.workspace_root.as_deref();
        let last_mouse_position = self.last_mouse_position;
        let mut targets = Vec::new();
        let mut decorated = Vec::with_capacity(lines.len());

        for (row_idx, transcript_line) in lines.iter().enumerate() {
            decorated.push(Self::decorate_one_transcript_row(
                transcript_line.line.clone(),
                &transcript_line.explicit_links,
                row_idx,
                area,
                workspace_root,
                last_mouse_position,
                detect_raw_links,
                styles,
                &mut targets,
            ));
        }

        self.store_transcript_link_targets(targets);

        decorated
    }

    pub(crate) fn decorate_visible_transcript_links(
        &mut self,
        lines: Vec<TranscriptLine>,
        area: Rect,
    ) -> Vec<Line<'static>> {
        self.decorate_visible_transcript_links_impl(lines, area, true)
    }

    #[cfg_attr(feature = "profiling", hotpath::measure)]
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
            self.reset_transcript_link_targets();
            return lines.into_iter().map(|line| line.line).collect();
        }

        let styles = self.transcript_link_styles();
        let workspace_root = self.workspace_root.as_deref();
        let last_mouse_position = self.last_mouse_position;
        let mut targets = Vec::new();
        let mut decorated = Vec::with_capacity(lines.len());

        for (row_idx, transcript_line) in lines.into_iter().enumerate() {
            let TranscriptLine { line, explicit_links } = transcript_line;
            decorated.push(Self::decorate_one_transcript_row(
                line,
                &explicit_links,
                row_idx,
                area,
                workspace_root,
                last_mouse_position,
                detect_raw_links,
                styles,
                &mut targets,
            ));
        }

        self.store_transcript_link_targets(targets);

        decorated
    }

    pub(crate) fn is_hovering_link(&self) -> bool {
        self.hovered_transcript_file_link.is_some()
            || self
                .last_mouse_position
                .and_then(|(col, row)| self.modal_link_target_index_at(col, row))
                .is_some()
    }

    pub(crate) fn is_selecting_text(&self) -> bool {
        self.mouse_selection.is_selecting || self.mouse_selection.has_selection
    }

    pub(crate) fn update_transcript_file_link_hover(&mut self, column: u16, row: u16) -> bool {
        let previous_position = self.last_mouse_position;
        let previous_modal_hover = previous_position
            .and_then(|(previous_column, previous_row)| self.modal_link_target_index_at(previous_column, previous_row));
        self.last_mouse_position = Some((column, row));

        let hovered = self.mouse_hovered_transcript_file_link_index();
        let transcript_hover_changed = hovered != self.hovered_transcript_file_link;
        let modal_hover_changed = previous_modal_hover != self.modal_link_target_index_at(column, row);

        self.hovered_transcript_file_link = hovered;
        transcript_hover_changed || modal_hover_changed
    }

    pub(crate) fn clear_transcript_file_link_hover(&mut self) -> bool {
        let changed = self.hovered_transcript_file_link.is_some();
        self.hovered_transcript_file_link = None;
        changed
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

    pub(crate) fn transcript_file_link_double_click_action(&self, column: u16, row: u16) -> TranscriptLinkClickAction {
        self.link_open_action(
            self.transcript_link_target_index_at(column, row)
                .and_then(|index| self.transcript_file_link_targets.get(index)),
        )
    }

    pub(crate) fn modal_link_double_click_action(&self, column: u16, row: u16) -> TranscriptLinkClickAction {
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
        if self.last_link_open.as_ref().is_some_and(|(last_key, last_at)| {
            last_key == &key && now.saturating_duration_since(*last_at) <= LINK_OPEN_THROTTLE_INTERVAL
        }) {
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

    pub(crate) fn pending_link_click_action(&mut self, action: TranscriptLinkClickAction) -> TranscriptLinkClickAction {
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

    fn link_open_action(&self, target: Option<&TranscriptFileLinkTarget>) -> TranscriptLinkClickAction {
        let Some(target) = target else {
            return TranscriptLinkClickAction::Ignore;
        };

        TranscriptLinkClickAction::Open(match &target.target {
            TranscriptLinkTarget::File(target) => InlineEvent::OpenFileInEditor(target.canonical_string()),
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
            let hovered = last_mouse_position.is_some_and(|(column, row)| point_in_rect(target_area, column, row));

            targets.push(TranscriptFileLinkTarget { area: target_area, target });
            styled_matches.push(StyledLinkMatch { start, end, hovered });
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
        KeyCode::Modifier(ModifierKeyCode::LeftShift | ModifierKeyCode::RightShift) => Some(KeyModifiers::SHIFT),
        KeyCode::Modifier(ModifierKeyCode::LeftControl | ModifierKeyCode::RightControl) => Some(KeyModifiers::CONTROL),
        KeyCode::Modifier(ModifierKeyCode::LeftAlt | ModifierKeyCode::RightAlt) => Some(KeyModifiers::ALT),
        KeyCode::Modifier(ModifierKeyCode::LeftSuper | ModifierKeyCode::RightSuper) => Some(KeyModifiers::SUPER),
        KeyCode::Modifier(ModifierKeyCode::LeftMeta | ModifierKeyCode::RightMeta) => Some(KeyModifiers::META),
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
        let hovered = last_mouse_position.is_some_and(|(column, row)| point_in_rect(target_area, column, row));
        let target = match &explicit.target {
            InlineLinkTarget::Url(url) => resolve_transcript_file_target(url, workspace_root)
                .map(TranscriptLinkTarget::File)
                .unwrap_or_else(|| TranscriptLinkTarget::Url(url.clone())),
        };
        targets.push(TranscriptFileLinkTarget { area: target_area, target });
        styled_matches.push(StyledLinkMatch { start: explicit.start, end: explicit.end, hovered });
    }
}

fn may_contain_link_candidate(line: &Line<'_>, explicit_links: &[RenderedTranscriptLink]) -> bool {
    !explicit_links.is_empty()
        || line
            .spans
            .iter()
            .any(|span| may_contain_link_candidate_text(span.content.as_ref()))
}

pub(super) fn may_contain_link_candidate_text(text: &str) -> bool {
    if text.contains("://") || text.contains('/') || text.contains('\\') {
        return true;
    }
    // Hotpath: filename tokens require '.' — skip the whitespace split for
    // plain prose without dot/slash/backslash (tui_hotpath Agent rows).
    // "://" contains '/', so the slash check above already covers it.
    if !text.contains('.') {
        return false;
    }
    // Bare workspace filenames (`Cargo.toml`) resolve without a path separator.
    // Only treat `name.ext` tokens as candidates — not sentence periods (`end.`).
    text.split_whitespace().any(looks_like_filename_token)
}

fn looks_like_filename_token(token: &str) -> bool {
    // Sentence punctuation trails filenames: `Cargo.toml.` `test.rs,`
    let token = token.trim_end_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.');
    let token = token.trim_end_matches('.');
    let Some((stem, ext)) = token.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty() && !ext.is_empty() && ext.len() <= 10 && ext.chars().all(|c| c.is_ascii_alphanumeric())
}

/// A wrapped display row anchored in the original unwrapped text.
///
/// Dimension key: `start`/`end` are byte offsets into the original text,
/// `indent` counts leading wrapped-row bytes (hanging indent) that are
/// absent from the original.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LocatedRow {
    /// Byte offset where the row body starts in the original text.
    start: usize,
    /// Byte offset where the row body ends in the original text.
    end: usize,
    /// Leading row bytes (hanging indent) absent from the original text.
    indent: usize,
}

/// Locate a wrapped display row inside the original unwrapped text.
///
/// Word-boundary wrapping is not lossless: the break whitespace is dropped
/// and bullet-aware wrapping prepends a hanging indent (e.g. `  ` under
/// `• `) that never existed in the source. Accumulating row byte lengths
/// therefore drifts a little further with every wrap, projecting link ranges
/// onto the wrong columns so underlines spill into neighboring words.
/// Instead, anchor each row by searching for its body text from the current
/// cursor, which transparently skips dropped break whitespace and added
/// indents.
///
/// Returns `None` when the row body cannot be found (defensive only: the
/// wrappers preserve mid-row text, so this means the row carries no
/// mappable content and callers should leave it undecorated).
fn locate_wrapped_row(original_text: &str, cursor: usize, wrapped_text: &str) -> Option<LocatedRow> {
    if wrapped_text.is_empty() {
        return Some(LocatedRow { start: cursor, end: cursor, indent: 0 });
    }
    let rest = original_text.get(cursor..)?;
    // Fast path: the row continues exactly where the cursor is (no wrapping
    // transformation at this boundary).
    if rest.starts_with(wrapped_text) {
        return Some(LocatedRow {
            start: cursor,
            end: cursor + wrapped_text.len(),
            indent: 0,
        });
    }
    // General path: strip the wrapper-added hanging indent, then locate the
    // body text. Leading content indent that genuinely exists in the source
    // is harmless here: link matches never start inside whitespace, and the
    // indent offset is added back when translating to row space.
    let body = wrapped_text.trim_start_matches([' ', '\t']);
    let indent = wrapped_text.len() - body.len();
    if body.is_empty() {
        return Some(LocatedRow { start: cursor, end: cursor, indent });
    }
    let offset = rest.find(body)?;
    let row_start = cursor + offset;
    Some(LocatedRow {
        start: row_start,
        end: row_start + body.len(),
        indent,
    })
}

/// Translate an original-space link range onto a located wrapped row.
///
/// Returns row-space byte bounds (char-boundary fixed), or `None` when the
/// match does not overlap the row.
fn project_match_onto_located_row(
    link_start: usize,
    link_end: usize,
    located: &LocatedRow,
    wrapped_text: &str,
) -> Option<(usize, usize)> {
    let overlap_start = link_start.max(located.start);
    let overlap_end = link_end.min(located.end);
    if overlap_start >= overlap_end {
        return None;
    }

    let mut relative_start = overlap_start - located.start + located.indent;
    let mut relative_end = overlap_end - located.start + located.indent;
    if !wrapped_text.is_char_boundary(relative_start) {
        relative_start = floor_char_boundary_impl(wrapped_text, relative_start);
    }
    if !wrapped_text.is_char_boundary(relative_end) {
        relative_end = ceil_char_boundary_impl(wrapped_text, relative_end);
    }

    (relative_start < relative_end && relative_end <= wrapped_text.len()).then_some((relative_start, relative_end))
}

/// Emit one callback per detected link overlapping a located row, with
/// row-space byte bounds. Centralizes the overlap/width filtering so both
/// decoration sites stay consistent.
fn each_projected_row_link(
    matches: &[DetectedLinkMatch],
    located: &LocatedRow,
    wrapped_text: &str,
    mut emit: impl FnMut(usize, usize, &DetectedLinkMatch),
) {
    for link_match in matches {
        let Some((relative_start, relative_end)) =
            project_match_onto_located_row(link_match.start, link_match.end, located, wrapped_text)
        else {
            continue;
        };
        if UnicodeWidthStr::width(&wrapped_text[relative_start..relative_end]) == 0 {
            continue;
        }
        emit(relative_start, relative_end, link_match);
    }
}

fn floor_char_boundary_impl(s: &str, i: usize) -> usize {
    let mut idx = i.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn ceil_char_boundary_impl(s: &str, i: usize) -> usize {
    let mut idx = i.min(s.len());
    while idx < s.len() && !s.is_char_boundary(idx) {
        idx += 1;
    }
    idx
}

#[inline]
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
                spans.push(Span::styled(content[local_offset..relative_start].to_string(), span.style));
            }

            if relative_end > relative_start {
                let style = if link_match.hovered { hovered_style } else { link_style };
                spans.push(Span::styled(content[relative_start..relative_end].to_string(), span.style.patch(style)));
            }

            local_offset = relative_end;
            if link_match.end <= span_end {
                current_match_idx += 1;
            } else {
                break;
            }
        }

        if local_offset < content.len() {
            spans.push(Span::styled(content[local_offset..].to_string(), span.style));
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

        let original_text = transcript_line_text(&line).into_owned();
        let matches = detect_transcript_link_matches(&original_text, workspace_root);
        let line_width: usize = UnicodeWidthStr::width(original_text.as_str());
        let wrapped_lines = if line_width <= usize::from(area.width).max(1) {
            vec![line]
        } else {
            wrapping::wrap_line_preserving_urls(line, area.width.max(1) as usize)
        };
        // Rows are anchored by searching for their body text (see
        // `locate_wrapped_row`): wrapping drops break whitespace and may
        // prepend a hanging indent, so byte lengths do not accumulate.
        let mut cursor = 0usize;

        for wrapped_line in wrapped_lines {
            if row_idx >= usize::from(area.height) {
                break;
            }

            let wrapped_text = transcript_line_text(&wrapped_line);
            let mut styled_matches = Vec::new();
            if let Some(located) = locate_wrapped_row(&original_text, cursor, &wrapped_text) {
                cursor = located.end;
                each_projected_row_link(
                    &matches,
                    &located,
                    &wrapped_text,
                    |relative_start, relative_end, link_match| {
                        let start_col = UnicodeWidthStr::width(&wrapped_text[..relative_start]);
                        let width = UnicodeWidthStr::width(&wrapped_text[relative_start..relative_end]);

                        let target_area = Rect::new(
                            area.x.saturating_add(start_col as u16),
                            area.y.saturating_add(row_idx as u16),
                            width as u16,
                            1,
                        );
                        let hovered =
                            last_mouse_position.is_some_and(|(column, row)| point_in_rect(target_area, column, row));
                        targets.push(TranscriptFileLinkTarget {
                            area: target_area,
                            target: link_match.target.clone(),
                        });
                        styled_matches.push(StyledLinkMatch { start: relative_start, end: relative_end, hovered });
                    },
                );
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
    // Fast reject: plain prose never needs link projection. Skipping this was
    // a per-row String + regex cost on every reflow (hotpath).
    if !may_contain_link_candidate_text(original_text) {
        return vec![Vec::new(); wrapped_lines.len()];
    }
    let matches = detect_transcript_link_matches(original_text, workspace_root);
    let mut projected = Vec::with_capacity(wrapped_lines.len());
    // Rows are anchored by searching for their body text (see
    // `locate_wrapped_row`): wrapping drops break whitespace and may prepend
    // a hanging indent, so byte lengths do not accumulate.
    let mut cursor = 0usize;

    for wrapped_line in wrapped_lines {
        let wrapped_text = transcript_line_text(wrapped_line);
        let mut line_links = Vec::new();
        if let Some(located) = locate_wrapped_row(original_text, cursor, &wrapped_text) {
            cursor = located.end;
            each_projected_row_link(&matches, &located, &wrapped_text, |relative_start, relative_end, link_match| {
                let start_col = UnicodeWidthStr::width(&wrapped_text[..relative_start]);
                let width = UnicodeWidthStr::width(&wrapped_text[relative_start..relative_end]);

                line_links.push(RenderedTranscriptLink {
                    start: relative_start,
                    end: relative_end,
                    start_col,
                    width,
                    target: InlineLinkTarget::Url(transcript_link_target_string(&link_match.target)),
                });
            });
        }

        projected.push(line_links);
    }

    projected
}

#[cfg_attr(feature = "profiling", hotpath::measure)]
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

fn detect_transcript_link_matches(text: &str, workspace_root: Option<&Path>) -> Vec<DetectedLinkMatch> {
    let mut matches = Vec::new();

    for quoted_match in QUOTED_PATH_PATTERN.find_iter(text) {
        if let Some(link_match) =
            build_transcript_link_match(text, quoted_match.start(), quoted_match.end(), workspace_root)
        {
            matches.push(link_match);
        }
    }

    for token_match in NON_WHITESPACE_TOKEN_PATTERN.find_iter(text) {
        if matches
            .iter()
            .any(|existing| token_match.start() < existing.end && token_match.end() > existing.start)
        {
            continue;
        }

        if let Some(link_match) =
            build_transcript_link_match(text, token_match.start(), token_match.end(), workspace_root)
        {
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
        .or_else(|| resolve_transcript_file_target(candidate, workspace_root).map(TranscriptLinkTarget::File))?;

    Some(DetectedLinkMatch { start, end, target })
}

fn resolve_transcript_file_target(token: &str, workspace_root: Option<&Path>) -> Option<EditorTarget> {
    let token = token.trim();
    // Location parsing cannot make an ordinary word into a path candidate.
    // Percent escapes can introduce path syntax, so they must still be decoded.
    if !token.bytes().any(|byte| matches!(byte, b'.' | b'/' | b'\\' | b'%')) {
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
    // Explicit prefixes, drive letters, and separators are shared with the
    // markdown detector; the extension fallback below favors recall because
    // `path.is_file()` is the final filter.
    if has_explicit_path_syntax(token) {
        return true;
    }
    // Has a file-like extension: non-empty stem + dot + 1-12 alphanumeric ext.
    // Called after suffix stripping so `a.rs` (not `a.rs:10`) is the input.
    // `path.is_file()` is the final filter, so favor recall here.
    token.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty() && !ext.is_empty() && ext.len() <= 12 && ext.chars().all(|c| c.is_ascii_alphanumeric())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use std::fs;
    use std::path::PathBuf;

    use crate::tui::core_tui::types::InlineTheme;

    #[test]
    fn path_candidate_shortcut_preserves_decoded_paths_and_locations() {
        struct Case {
            raw: &'static str,
            path: &'static str,
            location: Option<&'static str>,
        }
        let root = tempfile::tempdir().expect("link workspace");
        fs::create_dir(root.path().join("src")).unwrap();
        for path in ["src/main.rs", "Cargo.toml", "東京.rs", "Cargo"] {
            fs::write(root.path().join(path), "fixture").unwrap();
        }
        let cases = [
            Case {
                raw: "src%2Fmain%2Ers:12:3",
                path: "src/main.rs",
                location: Some(":12:3"),
            },
            Case {
                raw: "Cargo%2Etoml(5,2)",
                path: "Cargo.toml",
                location: Some(":5:2"),
            },
            Case {
                raw: "./Cargo.toml:7",
                path: "Cargo.toml",
                location: Some(":7"),
            },
            Case {
                raw: "東京.rs:4",
                path: "東京.rs",
                location: Some(":4"),
            },
            Case {
                raw: "src/main.rs",
                path: "src/main.rs",
                location: None,
            },
        ];
        for case in cases {
            let target = resolve_transcript_file_target(case.raw, Some(root.path())).expect("file link");
            assert_eq!(target.path(), root.path().join(case.path), "{}", case.raw);
            assert_eq!(target.location_suffix(), case.location, "{}", case.raw);
        }
        for word in ["ordinary", "3999:", "inspect", "or", "Cargo", "Cargo(5,2)"] {
            assert!(resolve_transcript_file_target(word, Some(root.path())).is_none(), "{word}");
        }
    }

    #[test]
    fn path_candidate_shortcut_keeps_filesystem_checks_live() {
        let root = tempfile::tempdir().expect("link workspace");
        let path = root.path().join("appearing.rs");
        assert!(resolve_transcript_file_target("appearing.rs:9", Some(root.path())).is_none());
        fs::write(&path, "created after detection").unwrap();
        let target = resolve_transcript_file_target("appearing.rs:9", Some(root.path())).expect("new file");
        assert_eq!(target.path(), path);
        assert_eq!(target.location_suffix(), Some(":9"));
        fs::remove_file(&path).unwrap();
        assert!(resolve_transcript_file_target("appearing.rs:9", Some(root.path())).is_none());
        fs::create_dir(&path).unwrap();
        assert!(resolve_transcript_file_target("appearing.rs:9", Some(root.path())).is_none());
    }

    #[test]
    fn filename_candidates_ignore_sentence_punctuation() {
        assert!(may_contain_link_candidate_text("see Cargo.toml."));
        assert!(may_contain_link_candidate_text("open test.rs, then continue"));
        assert!(!may_contain_link_candidate_text("the end."));
        assert!(!may_contain_link_candidate_text("plain prose without names"));
        assert!(may_contain_link_candidate_text("src/main.rs"));
    }

    /// Walk up from `CARGO_MANIFEST_DIR` to the directory whose `Cargo.toml`
    /// contains `[workspace]`. Hardcoding `parent()` breaks when crates are
    /// nested (e.g. `crates/codegen/<crate>` — three levels deep, not one).
    fn find_workspace_root() -> PathBuf {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut current = manifest.as_path();
        while let Some(parent) = current.parent() {
            let cargo_toml = parent.join("Cargo.toml");
            if cargo_toml.is_file() && fs::read_to_string(&cargo_toml).is_ok_and(|c| c.contains("[workspace]")) {
                return parent.to_path_buf();
            }
            current = parent;
        }
        panic!("could not find workspace root from CARGO_MANIFEST_DIR");
    }

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
    fn borrowed_and_visible_decoration_agree_on_mixed_links() {
        use crate::tui::ui::tui::types::InlineLinkTarget;

        // Asymmetric rows: explicit link first, raw URL last, plain middle —
        // row index feeds target areas, so order matters for the oracle.
        let root = tempfile::tempdir().expect("link workspace");
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/app.rs"), "fixture").unwrap();
        fs::write(root.path().join("Cargo.toml"), "fixture").unwrap();
        let area = Rect::new(0, 0, 60, 5);
        let explicit_text = "see explicit docs";
        let explicit_target = "https://example.com/explicit";
        let explicit_start = explicit_text.find("explicit").expect("explicit offset");
        let lines = vec![
            TranscriptLine {
                line: Line::from(explicit_text),
                explicit_links: vec![RenderedTranscriptLink {
                    start: explicit_start,
                    end: explicit_start + "explicit".len(),
                    start_col: explicit_start,
                    width: "explicit".len(),
                    target: InlineLinkTarget::Url(explicit_target.to_string()),
                }],
            },
            TranscriptLine {
                line: Line::from("plain text only"),
                explicit_links: Vec::new(),
            },
            TranscriptLine {
                line: Line::from("open src/app.rs and https://example.com/raw"),
                explicit_links: Vec::new(),
            },
        ];

        let mut borrowed_session = Session::new(InlineTheme::default(), None, 60);
        borrowed_session.set_workspace_root(Some(root.path().to_path_buf()));
        let borrowed = borrowed_session.decorate_borrowed_transcript_links(&lines, area);
        let borrowed_targets = borrowed_session.transcript_file_link_targets.clone();

        let mut owned_session = Session::new(InlineTheme::default(), None, 60);
        owned_session.set_workspace_root(Some(root.path().to_path_buf()));
        let owned = owned_session.decorate_visible_transcript_links(lines.clone(), area);
        let owned_targets = owned_session.transcript_file_link_targets.clone();

        assert_eq!(borrowed, owned);
        assert_eq!(borrowed_targets, owned_targets);
        // Independently derived: explicit URL + raw file + raw URL = 3 targets
        // across rows 0 and 2; plain row 1 contributes none.
        assert_eq!(borrowed_targets.len(), 3);
        assert_eq!(borrowed_targets[0].area.y, area.y);
        assert_eq!(borrowed_targets[1].area.y, area.y + 2);
        assert_eq!(borrowed_targets[2].area.y, area.y + 2);

        // Cached (no raw detection) fast path: explicit survives, raw links vanish.
        let mut cached_borrowed = Session::new(InlineTheme::default(), None, 60);
        cached_borrowed.set_workspace_root(Some(root.path().to_path_buf()));
        let cached_borrowed_lines = cached_borrowed.decorate_borrowed_cached_transcript_links(&lines, area);
        let mut cached_owned = Session::new(InlineTheme::default(), None, 60);
        cached_owned.set_workspace_root(Some(root.path().to_path_buf()));
        let cached_owned_lines = cached_owned.decorate_visible_cached_transcript_links(lines, area);
        assert_eq!(cached_borrowed_lines, cached_owned_lines);
        assert_eq!(cached_borrowed.transcript_file_link_targets, cached_owned.transcript_file_link_targets);
        assert_eq!(cached_borrowed.transcript_file_link_targets.len(), 1);
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

    #[test]
    fn project_detected_links_preserves_char_boundaries_for_unicode_wraps() {
        let original_text = "crates/codegen/vtcode-core/src/tool_policy.rs — those files";
        let wrapped_lines = vec![
            Line::from("crates/codegen/vtcode-core/src/tool_policy.rs —"),
            Line::from(" those files"),
        ];

        let projected =
            project_detected_links_onto_wrapped_lines(&wrapped_lines, original_text, Some(&find_workspace_root()));

        assert_eq!(projected.len(), 2);
        assert_eq!(projected[0].len(), 1);
        assert_eq!(projected[0][0].start, 0);
        assert!(projected[0][0].end <= "crates/codegen/vtcode-core/src/tool_policy.rs —".len());
        assert!("crates/codegen/vtcode-core/src/tool_policy.rs —".is_char_boundary(projected[0][0].end));
    }

    #[test]
    fn projected_links_align_with_wrapped_bullet_rows() {
        let root = find_workspace_root();
        // Long bullet item: the real wrapper wraps it across several display
        // rows with a hanging indent and drops the break whitespace, so naive
        // byte-length accumulation drifts and the underline spills into
        // neighboring words. Links sit on different rows so accumulating
        // drift would corrupt the later one more.
        let original = "• Verified the implementation across all of the modules in Cargo.toml and then confirmed the behavior in Cargo.lock, and continued checking the remaining pieces afterwards";
        let wrapped = wrapping::wrap_line_preserving_urls(Line::from(original), 60);
        assert!(wrapped.len() > 2, "expected multiple wrapped rows, got {}", wrapped.len());

        let projected = project_detected_links_onto_wrapped_lines(&wrapped, original, Some(&root));
        assert_eq!(projected.len(), wrapped.len());

        let mut found = Vec::new();
        for (row, links) in wrapped.iter().zip(projected.iter()) {
            let row_text = transcript_line_text(row);
            for link in links {
                let slice = &row_text.as_ref()[link.start..link.end];
                assert!(
                    slice == "Cargo.toml" || slice == "Cargo.lock",
                    "link underline spilled outside the path: {slice:?} in {row_text:?}"
                );
                assert_eq!(link.start_col, UnicodeWidthStr::width(&row_text.as_ref()[..link.start]));
                assert_eq!(link.width, UnicodeWidthStr::width(slice));
                found.push(slice.to_string());
            }
        }
        found.sort();
        assert_eq!(found, vec!["Cargo.lock".to_string(), "Cargo.toml".to_string()]);
    }
}
