//! Review source collection and cached block construction.

use super::*;

pub(crate) fn collect_review_sources(session: &Session) -> Vec<ReviewSource> {
    let core_len = session.core.lines.len();
    let review_revisions = session.core.review_message_revisions();
    let mut anchored_blocks = HashMap::<usize, Vec<usize>>::with_capacity(session.tool_output_blocks.len());
    let mut positioned_orphans = Vec::<(usize, usize)>::new();

    for (block_index, block) in session.tool_output_blocks.iter().enumerate() {
        let Some(anchor) = block.anchor_line else {
            if let Some(recorded_at_line) = block.recorded_at_line {
                positioned_orphans.push((recorded_at_line, block_index));
            }
            continue;
        };
        // The app session sets this line from a per-call identity marker (or
        // from the live PTY header). Never reverse-match the rendered command
        // text here: identical commands are valid consecutive calls, and rich
        // wrapping can legitimately change their visible text.
        anchored_blocks.entry(anchor).or_default().push(block_index);
    }
    positioned_orphans.sort_unstable();

    let mut used_blocks = HashSet::with_capacity(session.tool_output_blocks.len());
    let mut sources = Vec::with_capacity(core_len + session.tool_output_blocks.len());
    let mut index = 0usize;
    let mut next_positioned_orphan = 0usize;
    while index < core_len {
        while let Some(&(recorded_at_line, block_index)) = positioned_orphans.get(next_positioned_orphan)
            && recorded_at_line <= index
        {
            let block = &session.tool_output_blocks[block_index];
            sources.push(ReviewSource {
                key: ReviewBlockKey::OrphanTool(block_index),
                revision: block.id,
                kind: ReviewSourceKind::Tool(block_index),
            });
            used_blocks.insert(block_index);
            next_positioned_orphan += 1;
        }

        if let Some(block_indices) = anchored_blocks.get(&index) {
            for &block_index in block_indices {
                let block = &session.tool_output_blocks[block_index];
                sources.push(ReviewSource {
                    key: ReviewBlockKey::Tool(block.id),
                    revision: block.id,
                    kind: ReviewSourceKind::Tool(block_index),
                });
                used_blocks.insert(block_index);
            }
            index = tool_output_body_end(session, index, &anchored_blocks);
        } else {
            sources.push(ReviewSource {
                key: ReviewBlockKey::Core(index),
                revision: review_revisions[index],
                kind: ReviewSourceKind::Core(index),
            });
            index += 1;
        }
    }

    for &(_, block_index) in positioned_orphans.iter().skip(next_positioned_orphan) {
        let block = &session.tool_output_blocks[block_index];
        sources.push(ReviewSource {
            key: ReviewBlockKey::OrphanTool(block_index),
            revision: block.id,
            kind: ReviewSourceKind::Tool(block_index),
        });
        used_blocks.insert(block_index);
    }

    for (block_index, block) in session.tool_output_blocks.iter().enumerate() {
        if used_blocks.contains(&block_index) {
            continue;
        }
        sources.push(ReviewSource {
            key: ReviewBlockKey::OrphanTool(block_index),
            revision: block.id,
            kind: ReviewSourceKind::Tool(block_index),
        });
    }

    sources
}

pub(crate) fn rendered_message_text(session: &Session, index: usize) -> String {
    let Some(line) = session.core.lines.get(index) else {
        return String::new();
    };
    if let Some(activity) = session.compact_activity_for_line(index) {
        return activity.display_text();
    }
    session
        .core
        .render_message_spans_for_line(line)
        .into_iter()
        .map(|span| strip_ansi_codes(span.content.as_ref()).into_owned())
        .collect()
}

pub(crate) fn tool_output_body_end(
    session: &Session,
    anchor: usize,
    anchored_blocks: &HashMap<usize, Vec<usize>>,
) -> usize {
    let Some(anchor_line) = session.core.lines.get(anchor) else {
        return anchor.saturating_add(1);
    };
    let anchor_kind = anchor_line.kind;
    let mut end = anchor.saturating_add(1);
    while let Some(line) = session.core.lines.get(end) {
        // A following PTY/Tool line may be the next command's live output,
        // not detail belonging to this summary. Identity anchors are the
        // unambiguous boundary; text and message kind alone are not.
        if anchored_blocks.contains_key(&end) {
            break;
        }
        let belongs_to_tool = match anchor_kind {
            InlineMessageKind::Pty => line.kind == InlineMessageKind::Pty,
            InlineMessageKind::Tool => matches!(line.kind, InlineMessageKind::Tool | InlineMessageKind::Pty),
            InlineMessageKind::Info => {
                if !matches!(line.kind, InlineMessageKind::Tool | InlineMessageKind::Pty) {
                    // Detail text is only needed for Info-followed-by-Info;
                    // render lazily so a full-transcript pass stays O(n)
                    // without per-line ANSI stripping.
                    if line.kind != InlineMessageKind::Info {
                        break;
                    }
                    let text = rendered_message_text(session, end);
                    if !(text.starts_with("  ") || text.starts_with("    ")) {
                        break;
                    }
                }
                true
            }
            _ => false,
        };
        if !belongs_to_tool {
            break;
        }
        end += 1;
    }
    end
}

pub(crate) fn build_cached_block(session: &Session, source: ReviewSource, width: u16) -> CachedToolOutputBlock {
    match source.kind {
        ReviewSourceKind::Core(index) => {
            if let Some(activity) = session.compact_activity_for_line(index) {
                let lines = wrap_output_line(&activity.display_text(), usize::from(width.max(1)));
                let style = ratatui_style_from_inline(
                    &session.core.styles.accent_inline_style().bold(),
                    session.core.theme.foreground,
                );
                let rich_lines = lines.iter().map(|line| Line::styled(line.clone(), style)).collect::<Vec<_>>();
                return CachedToolOutputBlock {
                    key: source.key,
                    revision: source.revision,
                    lines,
                    rich_lines,
                    evidence_links: Vec::new(),
                    lowered_lines: None,
                };
            }
            let mut rich_lines = session
                .core
                .reflow_message_lines_for_review(index, width)
                .into_iter()
                .map(|line| line.line)
                .collect::<Vec<_>>();
            if rich_lines.is_empty() {
                rich_lines.push(Line::default());
            }
            let lines = rich_lines.iter().map(line_text).collect::<Vec<_>>();
            CachedToolOutputBlock {
                key: source.key,
                revision: source.revision,
                lines,
                rich_lines,
                evidence_links: Vec::new(),
                lowered_lines: None,
            }
        }
        ReviewSourceKind::Tool(index) => {
            let block = &session.tool_output_blocks[index];
            let rows = collect_tool_output_rows(block, width);
            let lines = rows.lines;
            let rich_lines = lines
                .iter()
                .map(|line| Line::styled(line.clone(), tool_output_line_style(session, line)))
                .collect();
            CachedToolOutputBlock {
                key: source.key,
                revision: source.revision,
                lines,
                rich_lines,
                evidence_links: rows.evidence_links,
                lowered_lines: None,
            }
        }
    }
}

pub(crate) fn line_text(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| strip_ansi_codes(span.content.as_ref()).into_owned())
        .collect()
}

pub(crate) fn tool_output_line_style(session: &Session, line: &str) -> Style {
    let trimmed = line.trim_start();
    if trimmed.starts_with("• ") {
        return session.core.styles.accent_style().add_modifier(Modifier::BOLD);
    }

    let lowercase = trimmed.to_ascii_lowercase();
    let kind = if lowercase.contains("run error") || lowercase.contains("exit code") {
        InlineMessageKind::Error
    } else if lowercase.contains("warning") {
        InlineMessageKind::Warning
    } else {
        InlineMessageKind::Pty
    };
    let mut style = session.core.styles.default_style();
    if let Some(color) = session.core.text_fallback(kind) {
        style = style.fg(ratatui_color_from_ansi(color));
    }
    if kind == InlineMessageKind::Pty {
        style = style.add_modifier(Modifier::DIM);
    }
    style
}

struct ToolOutputRows {
    lines: Vec<String>,
    evidence_links: Vec<Option<std::sync::Arc<str>>>,
}

pub(crate) fn evidence_target(line: &str) -> Option<std::sync::Arc<str>> {
    let (_, suffix) = line.split_once("[evidence](vtcode-evidence:")?;
    let (reference, _) = suffix.split_once(')')?;
    let mut parts = reference.split(':');
    let session = parts.next()?;
    if session.is_empty()
        || session.len() > 256
        || !session
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return None;
    }
    parts.next()?.parse::<u64>().ok()?;
    let digest = parts.next()?;
    if parts.next().is_some() || digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("vtcode-evidence:{reference}").into())
}

fn collect_tool_output_rows(block: &ToolOutputBlock, width: u16) -> ToolOutputRows {
    let max_width = usize::from(width.max(1));
    let mut lines = Vec::new();
    let mut evidence_links = Vec::new();
    for line in &block.lines {
        let clean = strip_ansi_codes(line);
        let target = evidence_target(&clean);
        let wrapped = wrap_output_line(&clean, max_width);
        evidence_links.extend(std::iter::repeat_n(target, wrapped.len()));
        lines.extend(wrapped);
    }
    if lines.is_empty() {
        lines.push(String::new());
        evidence_links.push(None);
    }
    ToolOutputRows { lines, evidence_links }
}

pub(crate) fn wrap_output_line(line: &str, width: usize) -> Vec<String> {
    // Plain-text hard wrap for ANSI-free review/export lines. Kept separate
    // from `text_utils::wrap_line` (styled, word-boundary wrapping) on purpose:
    // unifying them would change wrapping behavior for tool output.
    if line.is_empty() {
        return vec![String::new()];
    }

    let mut wrapped = Vec::new();
    let mut current = String::new();
    let mut current_width: usize = 0;
    for ch in line.chars() {
        let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if !current.is_empty() && current_width.saturating_add(char_width) > width {
            wrapped.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(ch);
        current_width = current_width.saturating_add(char_width);
    }
    if !current.is_empty() {
        wrapped.push(current);
    }
    wrapped
}
