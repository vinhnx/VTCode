use std::env;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use ratatui::crossterm::event::KeyModifiers;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use regex::Regex;
use unicode_width::UnicodeWidthStr;

use super::super::types::InlineEvent;
use super::Session;

static NON_WHITESPACE_TOKEN_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\S+").expect("valid transcript token regex"));
static QUOTED_PATH_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"`(?:file://|~/|/|\./|\.\./|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[^`]+`|"(?:file://|~/|/|\./|\.\./|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[^"]+"|'(?:file://|~/|/|\./|\.\./|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[^']+'"#,
    )
    .expect("valid quoted transcript path regex")
});

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TranscriptFileLinkTarget {
    pub(crate) area: Rect,
    pub(crate) file_path: PathBuf,
}

#[derive(Clone, Debug)]
struct FileLinkMatch {
    start: usize,
    end: usize,
    path: PathBuf,
}

#[derive(Clone, Copy, Debug)]
struct StyledFileLinkMatch {
    start: usize,
    end: usize,
    hovered: bool,
}

impl Session {
    pub(crate) fn clear_transcript_file_link_targets(&mut self) {
        self.transcript_file_link_targets.clear();
        self.hovered_transcript_file_link = None;
    }

    pub(crate) fn decorate_visible_transcript_links(
        &mut self,
        lines: Vec<Line<'static>>,
        area: Rect,
    ) -> Vec<Line<'static>> {
        let workspace_root = self.workspace_root.as_deref();
        let link_style = self
            .styles
            .accent_style()
            .add_modifier(Modifier::UNDERLINED);
        let hovered_style = link_style.add_modifier(Modifier::BOLD);
        let mut targets = Vec::new();
        let mut decorated = Vec::with_capacity(lines.len());

        for (row_idx, line) in lines.into_iter().enumerate() {
            let text: String = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect();
            let matches = detect_transcript_file_link_matches(&text, workspace_root);
            if matches.is_empty() {
                decorated.push(line);
                continue;
            }

            let mut styled_matches = Vec::with_capacity(matches.len());
            for link_match in matches {
                let start_col = UnicodeWidthStr::width(&text[..link_match.start]);
                let width = UnicodeWidthStr::width(&text[link_match.start..link_match.end]);
                if width == 0 {
                    continue;
                }

                let target_area = Rect::new(
                    area.x.saturating_add(start_col as u16),
                    area.y.saturating_add(row_idx as u16),
                    width as u16,
                    1,
                );
                let hovered = self
                    .last_mouse_position
                    .is_some_and(|(column, row)| point_in_rect(target_area, column, row));

                targets.push(TranscriptFileLinkTarget {
                    area: target_area,
                    file_path: link_match.path.clone(),
                });
                styled_matches.push(StyledFileLinkMatch {
                    start: link_match.start,
                    end: link_match.end,
                    hovered,
                });
            }

            if styled_matches.is_empty() {
                decorated.push(line);
            } else {
                decorated.push(style_transcript_file_link_line(
                    line,
                    &styled_matches,
                    link_style,
                    hovered_style,
                ));
            }
        }

        self.transcript_file_link_targets = targets;
        self.hovered_transcript_file_link = self.mouse_hovered_transcript_file_link_index();

        decorated
    }

    pub(crate) fn update_transcript_file_link_hover(&mut self, column: u16, row: u16) -> bool {
        self.last_mouse_position = Some((column, row));
        let hovered = self.mouse_hovered_transcript_file_link_index();
        if hovered == self.hovered_transcript_file_link {
            return false;
        }

        self.hovered_transcript_file_link = hovered;
        true
    }

    pub(crate) fn transcript_file_link_event(
        &self,
        column: u16,
        row: u16,
        modifiers: KeyModifiers,
    ) -> Option<InlineEvent> {
        if !is_open_file_modifier_click(modifiers) {
            return None;
        }

        let target = self
            .transcript_link_target_index_at(column, row)
            .and_then(|index| self.transcript_file_link_targets.get(index))?;

        Some(InlineEvent::OpenFileInEditor(
            target.file_path.display().to_string(),
        ))
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
}

fn point_in_rect(area: Rect, column: u16, row: u16) -> bool {
    row >= area.y
        && row < area.y.saturating_add(area.height)
        && column >= area.x
        && column < area.x.saturating_add(area.width)
}

fn style_transcript_file_link_line(
    line: Line<'static>,
    matches: &[StyledFileLinkMatch],
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

fn detect_transcript_file_link_matches(
    text: &str,
    workspace_root: Option<&Path>,
) -> Vec<FileLinkMatch> {
    let mut matches = Vec::new();

    for quoted_match in QUOTED_PATH_PATTERN.find_iter(text) {
        if let Some(link_match) = build_transcript_file_link_match(
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

        if let Some(link_match) = build_transcript_file_link_match(
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

fn build_transcript_file_link_match(
    text: &str,
    token_start: usize,
    token_end: usize,
    workspace_root: Option<&Path>,
) -> Option<FileLinkMatch> {
    let token = &text[token_start..token_end];
    let (trimmed_start, trimmed_end) = trim_transcript_token_bounds(token);
    if trimmed_start >= trimmed_end {
        return None;
    }

    let start = token_start + trimmed_start;
    let end = token_start + trimmed_end;
    let candidate = &text[start..end];
    let path = resolve_transcript_file_path(candidate, workspace_root)?;

    Some(FileLinkMatch { start, end, path })
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
            if ch == ')' && has_location_paren_suffix(&token[start..end]) {
                break;
            }
            end -= ch.len_utf8();
        } else {
            break;
        }
    }

    (start, end)
}

/// Check if token ends with a parenthesized location suffix like `(10)` or `(10,5)`.
fn has_location_paren_suffix(token: &str) -> bool {
    let Some(paren_start) = token.rfind('(') else {
        return false;
    };
    let after = &token[paren_start + 1..];
    after
        .strip_suffix(')')
        .is_some_and(|inner| !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit() || c == ','))
}

fn resolve_transcript_file_path(token: &str, workspace_root: Option<&Path>) -> Option<PathBuf> {
    let token = token.trim();
    if token.is_empty() || !looks_like_transcript_path(token) {
        return None;
    }

    let raw_path = strip_location_suffix(strip_file_scheme(token)).trim_end_matches(':');
    if raw_path.is_empty() {
        return None;
    }

    // Normalize Windows backslashes to forward slashes on Unix for cross-platform output
    #[cfg(not(target_os = "windows"))]
    let raw_path = &raw_path.replace('\\', "/");

    let path = expand_home_relative_path(raw_path)
        .or_else(|| {
            Path::new(raw_path)
                .is_absolute()
                .then(|| PathBuf::from(raw_path))
        })
        .or_else(|| workspace_root.map(|root| root.join(raw_path)))?;

    path.is_file().then_some(path)
}

fn strip_file_scheme(token: &str) -> &str {
    token.strip_prefix("file://").unwrap_or(token)
}

fn strip_location_suffix(token: &str) -> &str {
    let without_fragment = token.split('#').next().unwrap_or(token);
    let mut base = without_fragment;

    // Strip parenthesized location suffix like (10,5) or (10)
    if let Some(paren_start) = base.rfind('(') {
        let after = &base[paren_start + 1..];
        if after
            .strip_suffix(')')
            .is_some_and(|inner| !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit() || c == ','))
        {
            base = &base[..paren_start];
        }
    }

    // Strip colon-separated line:col suffix like :10:5 or :10
    for _ in 0..2 {
        let Some(colon_idx) = base.rfind(':') else {
            break;
        };
        let suffix = &base[colon_idx + 1..];
        if suffix.is_empty() || !suffix.chars().all(|ch| ch.is_ascii_digit()) {
            break;
        }
        base = &base[..colon_idx];
    }

    base
}

fn expand_home_relative_path(path: &str) -> Option<PathBuf> {
    let remainder = path.strip_prefix("~/")?;
    let home = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(remainder))
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
        // Has a file-like extension (dot followed by 1-12 alphanumeric chars at end),
        // but reject short tokens that look like abbreviations (e.g., "e.g.", "i.e.")
        || (token.len() > 4
            && token
                .rsplit_once('.')
                .is_some_and(|(_, ext)| !ext.is_empty() && ext.len() <= 12 && ext.chars().all(|c| c.is_ascii_alphanumeric())))
}

fn is_open_file_modifier_click(modifiers: KeyModifiers) -> bool {
    #[cfg(target_os = "macos")]
    {
        // Command key: crossterm reports as SUPER or META depending on terminal.
        // Some terminals (e.g. iTerm2, Alacritty) also emit CONTROL for Cmd+Click
        // mouse events, so accept CONTROL as fallback for mouse-only contexts.
        modifiers.contains(KeyModifiers::SUPER)
            || modifiers.contains(KeyModifiers::META)
            || modifiers.contains(KeyModifiers::CONTROL)
    }

    #[cfg(not(target_os = "macos"))]
    {
        modifiers.contains(KeyModifiers::CONTROL)
    }
}
