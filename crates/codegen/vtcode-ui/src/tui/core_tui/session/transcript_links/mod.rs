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

mod detect;

pub(crate) use detect::detect_rendered_transcript_links;
use detect::{detect_transcript_link_matches, resolve_transcript_file_target, transcript_link_target_string};

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
        relative_start = wrapped_text.floor_char_boundary(relative_start);
    }
    if !wrapped_text.is_char_boundary(relative_end) {
        relative_end = wrapped_text.ceil_char_boundary(relative_end);
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

#[cfg(test)]
mod tests;
