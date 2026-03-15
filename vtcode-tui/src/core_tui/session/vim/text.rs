use super::types::{Motion, TextObjectSpec};

pub(super) fn next_char_boundary(content: &str, mut pos: usize) -> usize {
    if pos >= content.len() {
        return content.len();
    }
    pos += 1;
    while pos < content.len() && !content.is_char_boundary(pos) {
        pos += 1;
    }
    pos
}

pub(super) fn prev_char_boundary(content: &str, mut pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    pos -= 1;
    while pos > 0 && !content.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

pub(super) fn vim_current_line_bounds(content: &str, cursor: usize) -> (usize, usize) {
    let start = vim_line_start(content, cursor);
    let end = content[start..]
        .find('\n')
        .map(|idx| start + idx)
        .unwrap_or(content.len());
    (start, end)
}

pub(super) fn vim_line_start(content: &str, cursor: usize) -> usize {
    content[..cursor.min(content.len())]
        .rfind('\n')
        .map(|idx| idx + 1)
        .unwrap_or(0)
}

pub(super) fn vim_line_end(content: &str, cursor: usize) -> usize {
    let start = vim_line_start(content, cursor);
    content[start..]
        .find('\n')
        .map(|idx| start + idx)
        .unwrap_or(content.len())
}

pub(super) fn vim_line_first_non_ws(content: &str, cursor: usize) -> usize {
    let start = vim_line_start(content, cursor);
    let end = vim_line_end(content, cursor);
    content[start..end]
        .char_indices()
        .find_map(|(idx, ch)| (!ch.is_whitespace()).then_some(start + idx))
        .unwrap_or(start)
}

pub(super) fn vim_next_word_start(content: &str, cursor: usize) -> usize {
    if content.is_empty() {
        return 0;
    }
    let cursor = cursor.min(content.len());
    let mut chars = content[cursor..].char_indices();
    let Some((_, first_ch)) = chars.next() else {
        return content.len();
    };
    let mut seen_separator = !vim_is_word_char(first_ch);
    for (offset, ch) in content[cursor..].char_indices() {
        let is_word = vim_is_word_char(ch);
        if !seen_separator {
            if !is_word {
                seen_separator = true;
            }
            continue;
        }
        if is_word {
            return cursor + offset;
        }
    }
    content.len()
}

pub(super) fn vim_prev_word_start(content: &str, cursor: usize) -> usize {
    let mut pos = prev_char_boundary(content, cursor.min(content.len()));
    while pos > 0 {
        let ch = content[pos..].chars().next().unwrap_or('\0');
        if vim_is_word_char(ch) {
            while pos > 0 {
                let prev = prev_char_boundary(content, pos);
                let prev_ch = content[prev..].chars().next().unwrap_or('\0');
                if !vim_is_word_char(prev_ch) {
                    break;
                }
                pos = prev;
            }
            return pos;
        }
        pos = prev_char_boundary(content, pos);
    }
    0
}

pub(super) fn vim_end_word(content: &str, cursor: usize) -> usize {
    let cursor = cursor.min(content.len());
    let start = match content[cursor..].chars().next() {
        Some(ch) if vim_is_word_char(ch) => cursor,
        _ => vim_next_word_start(content, cursor),
    };
    if start >= content.len()
        || !content[start..]
            .chars()
            .next()
            .is_some_and(vim_is_word_char)
    {
        return content.len();
    }
    let mut last = start;
    for (offset, ch) in content[start..].char_indices() {
        if !vim_is_word_char(ch) {
            return last;
        }
        last = start + offset;
    }
    last
}

pub(super) fn vim_motion_range(
    content: &str,
    cursor: usize,
    motion: Motion,
) -> Option<(usize, usize)> {
    let target = match motion {
        Motion::WordForward => vim_next_word_start(content, cursor),
        Motion::EndWord => vim_end_word(content, cursor),
        Motion::WordBackward => vim_prev_word_start(content, cursor),
    };
    match motion {
        Motion::WordBackward => (target < cursor).then_some((target, cursor)),
        Motion::EndWord => {
            (target >= cursor).then_some((cursor, next_char_boundary(content, target)))
        }
        Motion::WordForward => (target > cursor).then_some((cursor, target)),
    }
}

pub(super) fn vim_current_line_full_range(content: &str, cursor: usize) -> (usize, usize) {
    let start = vim_line_start(content, cursor);
    let line_end = vim_line_end(content, cursor);
    let end = if line_end < content.len() {
        line_end + 1
    } else {
        line_end
    };
    (start, end)
}

pub(super) fn vim_is_linewise_range(content: &str, start: usize, end: usize) -> bool {
    start == vim_line_start(content, start)
        && (end == content.len() || content.get(end - 1..end) == Some("\n"))
}

pub(super) fn vim_find_char(
    content: &str,
    cursor: usize,
    ch: char,
    forward: bool,
    till: bool,
) -> Option<usize> {
    let (start, end) = vim_current_line_bounds(content, cursor);
    if forward {
        let search_start = next_char_boundary(content, cursor);
        content[search_start..end].find(ch).map(|offset| {
            let found = search_start + offset;
            if till {
                prev_char_boundary(content, found)
            } else {
                found
            }
        })
    } else {
        let slice = &content[start..cursor];
        slice.rfind(ch).map(|offset| {
            let found = start + offset;
            if till {
                next_char_boundary(content, found)
            } else {
                found
            }
        })
    }
}

pub(super) fn vim_text_object_range(
    content: &str,
    cursor: usize,
    object: TextObjectSpec,
) -> Option<(usize, usize)> {
    match object {
        TextObjectSpec::Word { around, big } => {
            let chars: Vec<(usize, char)> = content.char_indices().collect();
            let current_idx = chars
                .iter()
                .position(|(idx, _)| *idx == cursor)
                .unwrap_or_else(|| chars.len().saturating_sub(1));
            if chars.is_empty() {
                return None;
            }
            let classify = |ch: char| {
                if big {
                    !ch.is_whitespace()
                } else {
                    vim_is_word_char(ch)
                }
            };
            let mut start = current_idx;
            while start > 0 && classify(chars[start].1) {
                let prev = start - 1;
                if !classify(chars[prev].1) {
                    break;
                }
                start = prev;
            }
            let mut end = current_idx;
            while end + 1 < chars.len() && classify(chars[end].1) && classify(chars[end + 1].1) {
                end += 1;
            }
            let mut byte_start = chars[start].0;
            let mut byte_end = if end + 1 < chars.len() {
                chars[end + 1].0
            } else {
                content.len()
            };
            if around {
                while byte_start > 0 {
                    let prev = prev_char_boundary(content, byte_start);
                    let ch = content[prev..].chars().next().unwrap_or('\0');
                    if !ch.is_whitespace() {
                        break;
                    }
                    byte_start = prev;
                }
                while byte_end < content.len() {
                    let ch = content[byte_end..].chars().next().unwrap_or('\0');
                    if !ch.is_whitespace() {
                        break;
                    }
                    byte_end = next_char_boundary(content, byte_end);
                }
            }
            Some((byte_start, byte_end))
        }
        TextObjectSpec::Delimited {
            around,
            open,
            close,
        } => {
            let left = content[..cursor].rfind(open)?;
            let right = content[cursor..].find(close).map(|idx| cursor + idx)?;
            if left >= right {
                return None;
            }
            Some(if around {
                (left, next_char_boundary(content, right))
            } else {
                (next_char_boundary(content, left), right)
            })
        }
    }
}

fn vim_is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}
