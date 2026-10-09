#![expect(
    clippy::string_slice,
    unused_results,
    reason = "Formatting uses ASCII delimiters and intentionally ignores infallible String mutation results."
)]

//! Unified formatting utilities for UI and logging

/// Case-insensitive ASCII substring search that does not allocate a lowercased
/// copy of either input.
///
/// Equivalent to
/// `haystack.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())` for all
/// inputs, because `[u8]::eq_ignore_ascii_case` is a length-preserving bytewise
/// fold and byte comparison only matches at the same offset on both sides. Bytes
/// `>= 0x80` are compared exactly (neither `to_ascii_lowercase` nor
/// `eq_ignore_ascii_case` folds them), so non-ASCII haystacks and needles behave
/// identically. An empty `needle` matches (as `str::contains` does).
#[inline]
pub fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let needle = needle.as_bytes();
    if needle.is_empty() {
        return true;
    }
    // `windows` yields nothing when the haystack is shorter than the needle, and
    // the empty-needle `windows(0)` panic is guarded above.
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

/// Format file size in human-readable form (KB, MB, GB, etc.)
pub fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.1}GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.1}MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.1}KB", size as f64 / KB as f64)
    } else {
        format!("{size}B")
    }
}

/// Format a duration in seconds as a compact human-readable unit.
///
/// Returns `"42s"`, `"5m"`, `"3h"`, or `"2d"`. Callers add their own suffix
/// (`" ago"`, `"expires in ..."`) so the same core serves past and future
/// durations. Do not fork this ladder per crate.
#[must_use]
pub fn humanize_duration_compact(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}d", seconds / 86400)
    }
}

/// Indent a block of text with the given prefix
pub fn indent_block(text: &str, indent: &str) -> String {
    if indent.is_empty() || text.is_empty() {
        return text.to_string();
    }
    let mut indented = String::with_capacity(text.len() + indent.len() * text.lines().count());
    for (idx, line) in text.split('\n').enumerate() {
        if idx > 0 {
            indented.push('\n');
        }
        if !line.is_empty() {
            indented.push_str(indent);
        }
        indented.push_str(line);
    }
    indented
}

/// Truncate text to a maximum length (in chars) with an optional ellipsis.
pub fn truncate_text(text: &str, max_len: usize, ellipsis: &str) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }

    let mut truncated = text.chars().take(max_len).collect::<String>();
    truncated.push_str(ellipsis);
    truncated
}

/// Truncate text to `max_len` chars, reserving room for `ellipsis` so the
/// returned string never exceeds `max_len` chars.
///
/// This differs from [`truncate_text`], which appends the ellipsis *after*
/// taking `max_len` chars (yielding up to `max_len + ellipsis.len()` chars).
/// Use this when the total rendered width must stay within a hard budget.
///
/// ```
/// # use vtcode_commons::formatting::truncate_within;
/// assert_eq!(truncate_within("hello world", 8, "..."), "hello...");
/// assert_eq!(truncate_within("hi", 8, "..."), "hi");
/// assert_eq!(truncate_within("hello", 3, "…"), "he…");
/// ```
pub fn truncate_within(text: &str, max_len: usize, ellipsis: &str) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    let keep = max_len.saturating_sub(ellipsis.chars().count());
    let mut truncated = text.chars().take(keep).collect::<String>();
    truncated.push_str(ellipsis);
    truncated
}

/// Truncate `text` to at most `max_len` chars, keeping a head and a tail joined by
/// a single `…` so context from both ends is preserved.
///
/// Control characters are replaced with spaces before truncation so the result is
/// safe to render in a terminal/TUI. When the text already fits it is returned
/// unchanged (after sanitization).
///
/// This is the canonical middle-truncation helper, shared so the same logic is not
/// re-implemented per crate.
pub fn truncate_middle(text: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let sanitized: String = text
        .chars()
        .map(|c| if matches!(c, '\n' | '\r' | '\t') { ' ' } else { c })
        .collect();
    let char_count = sanitized.chars().count();
    if char_count <= max_len {
        return sanitized;
    }
    if max_len <= 1 {
        return "…".to_string();
    }
    let head_len = max_len / 2;
    let tail_len = max_len.saturating_sub(head_len + 1);

    let head: String = sanitized.chars().take(head_len).collect();
    let mut result = String::with_capacity(head.len() + tail_len + 1);
    result.push_str(&head);
    result.push('…');
    if tail_len > 0 {
        let mut tail_rev: Vec<char> = sanitized.chars().rev().take(tail_len).collect();
        tail_rev.reverse();
        let tail: String = tail_rev.into_iter().collect();
        result.push_str(&tail);
    }
    result
}

/// Truncate a file path in the middle, preferring to break at path separators.
///
/// Keeps a head and a tail joined by `…`, choosing break points at `/` so the most
/// recognizable parts of the path (directories / file name) are preserved. This is
/// the path-aware sibling of [`truncate_middle`], shared so the same display logic
/// is not re-implemented per crate.
pub fn truncate_path_middle(path: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let char_count = path.chars().count();
    if char_count <= max_len {
        return path.to_string();
    }
    if max_len <= 1 {
        return "…".to_string();
    }

    // Work in char offsets throughout: byte offsets from `rfind`/`find` would
    // overshoot the budget on multi-byte paths.
    let chars: Vec<char> = path.chars().collect();
    let head_budget = max_len / 2;
    let tail_budget = max_len.saturating_sub(head_budget + 1);

    let head_break = chars.iter().take(head_budget).rposition(|c| *c == '/').unwrap_or(head_budget);

    let tail: Vec<char> = chars.iter().rev().take(tail_budget).rev().copied().collect();
    let tail_len = tail.iter().position(|c| *c == '/').map_or(tail_budget, |pos| tail_budget - pos);

    let head: String = chars.iter().take(head_break).collect();
    let tail: String = tail.iter().skip(tail_budget - tail_len).collect();

    format!("{head}…{tail}")
}

/// Truncate `value` to `max_chars` chars by keeping a head and a tail joined by
/// `marker`, preserving context from both ends of the text.
///
/// Returns `(text, was_truncated)`. When the budget is too small to fit the
/// marker plus meaningful context, falls back to a head-only prefix with a
/// ` [truncated]` suffix, respecting the `max_chars` budget.
///
/// ```
/// # use vtcode_commons::formatting::head_tail_truncate;
/// let (out, truncated) = head_tail_truncate("short", 64, " ... ");
/// assert_eq!(out, "short");
/// assert!(!truncated);
/// ```
pub fn head_tail_truncate(value: &str, max_chars: usize, marker: &str) -> (String, bool) {
    const SUFFIX: &str = " [truncated]";

    let total_chars = value.chars().count();
    if total_chars <= max_chars {
        return (value.to_string(), false);
    }

    let marker_chars = marker.chars().count();
    if max_chars <= marker_chars + 16 {
        let suffix_len = SUFFIX.chars().count();
        let truncated = if max_chars > suffix_len {
            let available = max_chars - suffix_len;
            let mut result = value.chars().take(available).collect::<String>();
            result.push_str(SUFFIX);
            result
        } else {
            value.chars().take(max_chars).collect::<String>()
        };
        return (truncated, true);
    }

    let available = max_chars.saturating_sub(marker_chars);
    let head_chars = (available * 2) / 3;
    let tail_chars = available.saturating_sub(head_chars);
    let head = value.chars().take(head_chars).collect::<String>();
    let tail = value.chars().skip(total_chars.saturating_sub(tail_chars)).collect::<String>();
    let mut truncated = String::with_capacity(max_chars + 20);
    truncated.push_str(&head);
    truncated.push_str(marker);
    truncated.push_str(&tail);
    (truncated, true)
}

/// Word-wrap `text` into lines, allowing `first_width` chars on the first line
/// and `continuation_width` chars on subsequent lines. Wrapping prefers
/// whitespace boundaries and is UTF-8 safe (widths count chars, not bytes).
///
/// Returns an empty vec for blank input. Words longer than the width are split
/// at the width boundary rather than overflowing.
///
/// ```
/// # use vtcode_commons::formatting::wrap_text_words;
/// let lines = wrap_text_words("the quick brown fox", 9, 9);
/// assert_eq!(lines, vec!["the quick", "brown fox"]);
/// assert!(wrap_text_words("   ", 5, 5).is_empty());
/// ```
pub fn wrap_text_words(text: &str, first_width: usize, continuation_width: usize) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut remaining = trimmed;
    let mut width = first_width.max(1);

    while remaining.chars().take(width + 1).count() > width {
        let split = split_at_word_boundary(remaining, width);
        let (head, tail) = remaining.split_at(split);
        let head = head.trim();
        if head.is_empty() {
            break;
        }
        result.push(head.to_string());
        remaining = tail.trim_start();
        if remaining.is_empty() {
            break;
        }
        width = continuation_width.max(1);
    }

    if !remaining.is_empty() {
        result.push(remaining.to_string());
    }
    result
}

fn split_at_word_boundary(input: &str, width: usize) -> usize {
    let mut last_space: Option<usize> = None;
    for (seen, (idx, ch)) in input.char_indices().enumerate() {
        if seen > width {
            break;
        }
        if ch.is_whitespace() {
            last_space = Some(idx);
        }
    }
    match last_space {
        Some(pos) => pos,
        None => byte_index_for_char_count(input, width),
    }
}

fn byte_index_for_char_count(input: &str, chars: usize) -> usize {
    if chars == 0 {
        return 0;
    }
    let mut seen = 0usize;
    for (idx, ch) in input.char_indices() {
        seen += 1;
        if seen == chars {
            return idx + ch.len_utf8();
        }
    }
    input.len()
}

/// Split `text` into shell-like words, keeping quoted spans atomic.
///
/// Whitespace inside single (`'…'`) or double (`"…"`) quotes never separates
/// words, and a backslash escapes the next character outside single quotes
/// (like the TUI tokenizer in `pty_stream/segments.rs`, except an escaped
/// space stays atomic here so `foo\ bar` wraps as one word). Quote
/// characters and backslashes are kept verbatim so single-spaced words rejoin
/// losslessly with single spaces. An unclosed quote runs to the end of input.
fn split_shell_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for ch in text.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' && !in_single {
            current.push(ch);
            escaped = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            current.push(ch);
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            current.push(ch);
            continue;
        }
        if ch.is_whitespace() && !in_single && !in_double {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// Shared `• Ran` header wrap widths so every tool-call command surface stays
/// in sync: 62 chars for the first line (`• Ran ` prefix), 58 for
/// continuations (`  │ ` prefix). All surfaces wrap the full command with
/// [`wrap_shell_command_with_continuations`] (explicit `\`, no `…`); TUI
/// reflow owns any residual viewport overflow.
pub const RAN_COMMAND_FIRST_WIDTH: usize = 62;
/// Continuation-line budget for [`RAN_COMMAND_FIRST_WIDTH`] headers.
pub const RAN_COMMAND_CONTINUATION_WIDTH: usize = 58;

/// Word-wrap a shell `command` into lines, allowing `first_width` chars on
/// the first line and `continuation_width` chars on subsequent lines.
///
/// Unlike [`wrap_text_words`], breaks happen only at unquoted whitespace, so
/// quoted patterns containing spaces (e.g. `grep -rn "a b|c" docs`) stay on
/// one line when they fit instead of splitting mid-quote and reading as
/// broken shell. Words longer than the active width are hard-split at the
/// width boundary rather than overflowing. Widths count chars, not bytes.
///
/// Returns an empty vec for blank input. Unquoted whitespace runs collapse to
/// a single space (the transcript pipeline already normalizes via
/// `collapse_whitespace`), so joining short-word wraps with single spaces
/// reproduces single-spaced input; hard-split overlong tokens are the
/// exception (their chunks gain separators when joined).
///
/// ```
/// # use vtcode_commons::formatting::wrap_shell_command;
/// let lines = wrap_shell_command("grep -rn \"a b\" docs | grep -v x", 20, 20);
/// assert_eq!(lines, vec!["grep -rn \"a b\" docs", "| grep -v x"]);
/// assert!(wrap_shell_command("   ", 5, 5).is_empty());
/// ```
pub fn wrap_shell_command(text: &str, first_width: usize, continuation_width: usize) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let words = split_shell_words(trimmed);
    if words.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::with_capacity(words.len());
    let mut current = String::new();
    let mut width = first_width.max(1);
    for word in words {
        let word_len = word.chars().count();
        if word_len > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                width = continuation_width.max(1);
            }
            current = push_split_word(&mut lines, &word, width, continuation_width);
            width = continuation_width.max(1);
            continue;
        }
        let current_len = current.chars().count();
        let need = if current_len == 0 {
            word_len
        } else {
            current_len + 1 + word_len
        };
        if need <= width {
            if current_len > 0 {
                current.push(' ');
            }
            current.push_str(&word);
        } else {
            lines.push(std::mem::take(&mut current));
            width = continuation_width.max(1);
            if word_len > width {
                // The word fit the wider first line but not the narrower
                // continuation: hard-split so no emitted row exceeds its
                // budget (e.g. a 60-char token with 62/58 widths).
                current = push_split_word(&mut lines, &word, width, continuation_width);
            } else {
                current = word;
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Word-wrap a shell `command` into display lines with explicit `\`
/// continuations, so multi-line tool-call headers read as valid shell.
///
/// Operator-aware: when the command does not fit on one line it is first
/// split at top-level shell list separators (`&&`, `||`, `|`, `|&`, `;`,
/// `;;`, `&`) — the separator set shared with tree-sitter-bash `list` nodes
/// and the TUI `is_command_separator` highlighters (verified with
/// `ast-grep --lang bash`; redirections like `>`, `>>`, `2>` never split).
/// Each operator chunk starts on a fresh display line with the operator kept
/// trailing (e.g. `... && \\`), then overlong chunks wrap further with
/// [`wrap_shell_command`]. Finally `" \\"` is appended to every line except
/// the last. Commands that fit on one line are returned unchanged: no forced
/// operator splits, no trailing `\\`.
///
/// Joining a multi-line result with a single space does NOT reproduce the
/// source (use [`wrap_shell_command`] when lossless rejoining is required).
/// Widths count chars, not bytes, matching [`wrap_shell_command`].
///
/// ```
/// # use vtcode_commons::formatting::wrap_shell_command_with_continuations;
/// let lines = wrap_shell_command_with_continuations("echo a b c d", 7, 7);
/// assert_eq!(lines, vec!["echo a \\", "b c d"]);
/// assert_eq!(wrap_shell_command_with_continuations("git status", 62, 58), vec!["git status"]);
/// assert!(wrap_shell_command_with_continuations("   ", 5, 5).is_empty());
/// // Short chains stay on one line; overlong chains break at operators:
/// assert_eq!(
///     wrap_shell_command_with_continuations("echo a && echo b", 62, 58),
///     vec!["echo a && echo b"]
/// );
/// let chained = wrap_shell_command_with_continuations("echo a && echo b", 10, 10);
/// assert_eq!(chained, vec!["echo a && \\", "echo b"]);
/// ```
pub fn wrap_shell_command_with_continuations(text: &str, first_width: usize, continuation_width: usize) -> Vec<String> {
    let lines = wrap_shell_command_lines(text, first_width, continuation_width);
    let total = lines.len();
    if total <= 1 {
        return lines;
    }
    lines
        .into_iter()
        .enumerate()
        .map(|(index, mut line)| {
            // Last line ends the command; earlier lines continue with `\`.
            if index + 1 < total {
                line.push_str(" \\");
            }
            line
        })
        .collect()
}

/// Operator-aware word-wrap of a shell `command` without continuation
/// markers.
///
/// Same breaks as [`wrap_shell_command_with_continuations`] but without the
/// trailing `" \\"` suffixes, for renderers that style the marker
/// separately (ANSI/TUI highlighting must not feed the marker to the bash
/// grammar). Prefer this over [`wrap_shell_command`] for `• Ran` headers so
/// every surface breaks identically.
pub fn wrap_shell_command_lines(text: &str, first_width: usize, continuation_width: usize) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    // Fast path: fits on one line → return unchanged (no forced operator
    // splits, so short `a && b` chains stay on a single row).
    let single = wrap_shell_command(trimmed, first_width.max(1), continuation_width.max(1));
    if single.len() <= 1 {
        return single;
    }
    // Split into operator chunks first so `&&`/`||`/`|`/`;` boundaries start
    // a fresh display line; fall back to one chunk when no top-level
    // separator is present.
    let chunks = split_shell_operator_chunks(trimmed);
    let mut lines: Vec<String> = Vec::new();
    for (chunk_idx, chunk) in chunks.iter().enumerate() {
        let width = if lines.is_empty() {
            first_width.max(1)
        } else {
            continuation_width.max(1)
        };
        // First chunk may use the wider first-line budget; every later chunk
        // (and every wrapped row within a chunk) uses the continuation width.
        let wrapped = if chunk_idx == 0 {
            wrap_shell_command(chunk, first_width.max(1), continuation_width.max(1))
        } else {
            wrap_shell_command(chunk, width, continuation_width.max(1))
        };
        if wrapped.is_empty() {
            continue;
        }
        lines.extend(wrapped);
    }
    lines
}

/// Whether `word` is a top-level shell list separator that should end a
/// display chunk.
///
/// Covers the `&&`/`||`/`|`/`;`/`&` list separators shared with
/// tree-sitter-bash `list` nodes and the TUI `is_command_separator`
/// highlighters, plus `|&` (pipe stdout+stderr) and the case-terminator
/// forms (`;;`, `;&`, `;;&`). Redirections (`>`, `>>`, `2>`, `<`) are intentionally absent:
/// they belong to the same simple command and must not force a new line.
/// A trailing `;` attached without whitespace (e.g. `hi;`) also ends a chunk.
fn is_shell_list_separator(word: &str) -> bool {
    matches!(word, "&&" | "||" | "|" | "|&" | ";" | ";;" | ";&" | ";;&" | "&")
        || (word.len() > 1 && word.ends_with(';') && !word.ends_with(";;"))
}

/// Split `command` into operator chunks at top-level list separators.
///
/// Words come from [`split_shell_words`], so separators inside single/double
/// quotes or backslash-escaped never split (e.g. the `||` inside
/// `"a||b"` stays atomic). The separator word is kept trailing on its chunk
/// (`git add a &&` + `git commit`), so each display line after the first
/// starts with a fresh command rather than a dangling operator.
fn split_shell_operator_chunks(command: &str) -> Vec<String> {
    let words = split_shell_words(command);
    if words.is_empty() {
        return Vec::new();
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in words {
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(&word);
        if is_shell_list_separator(&word) {
            chunks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Push hard-split chunks of an overlong `word` that exceeds `width`,
/// returning the trailing remainder (which fits `width`) as the new
/// in-progress line. Subsequent chunks use `continuation_width`.
fn push_split_word(lines: &mut Vec<String>, word: &str, width: usize, continuation_width: usize) -> String {
    let mut width = width.max(1);
    let mut rest = word;
    while rest.chars().count() > width {
        let idx = byte_index_for_char_count(rest, width);
        let (head, tail) = rest.split_at(idx);
        lines.push(head.to_string());
        rest = tail;
        width = continuation_width.max(1);
    }
    rest.to_string()
}

/// Format an `f64` float for display, collapsing decimal artifacts.
///
/// Config floats backed by `f32` (temperatures, thresholds) widen to long
/// `f64` tails (`0.7` becomes `0.699999988079071`) once serialized to
/// `toml::Value`, and `±0.1` stepper presses accumulate similar tails
/// (`0.8` becomes `0.799999988079071`). When the value round-trips exactly
/// through `f32`, the shorter `f32` form is displayed; otherwise a value
/// within a tiny epsilon of a 1- or 2-decimal grid shows the grid form.
/// Genuine precision is never rounded. Display-only: the stored value is
/// untouched.
///
/// ```
/// # use vtcode_commons::formatting::format_float_display;
/// assert_eq!(format_float_display(0.699999988079071), "0.7");
/// assert_eq!(format_float_display(0.799999988079071), "0.8");
/// assert_eq!(format_float_display(0.3333333333333333), "0.3333333333333333");
/// ```
#[allow(
    clippy::cast_possible_truncation,
    clippy::float_cmp,
    reason = "Narrowing `f64` to `f32` and comparing exactly is the detection mechanism: only exact round-trips collapse."
)]
pub fn format_float_display(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let narrowed = value as f32;
    if (narrowed as f64) == value {
        return narrowed.to_string();
    }
    // Stepper/grid artifacts differ from the grid form by ~1e-8 or less;
    // anything at or beyond 1e-6 off-grid keeps full precision.
    const GRID_EPSILON: f64 = 1e-7;
    for precision in [1i32, 2i32] {
        let factor = 10f64.powi(precision);
        let snapped = (value * factor).round() / factor;
        if (snapped - value).abs() < GRID_EPSILON {
            return snapped.to_string();
        }
    }
    value.to_string()
}

/// Borrow the longest prefix of `text` that is at most `max_bytes` bytes,
/// rounded down to the nearest UTF-8 char boundary.
///
/// This is the canonical byte-bounded prefix used by provider previews, PTY
/// capture limits, and bounded WebMCP prompts; do not fork the boundary loop.
///
/// ```
/// # use vtcode_commons::formatting::truncate_utf8_prefix;
/// assert_eq!(truncate_utf8_prefix("hello world", 5), "hello");
/// assert_eq!(truncate_utf8_prefix("hi", 10), "hi");
/// // Never splits a multi-byte char: 4 bytes lands mid-`日`, so only 2 survive.
/// assert_eq!(truncate_utf8_prefix("AB日", 4), "AB");
/// ```
#[inline]
pub fn truncate_utf8_prefix(text: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Truncate a string so that the retained prefix is at most `max_bytes` bytes,
/// rounded down to the nearest UTF-8 char boundary.  Returns the truncated
/// prefix with `suffix` appended, or the original string when it already fits.
pub fn truncate_byte_budget(text: &str, max_bytes: usize, suffix: &str) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    format!("{}{suffix}", truncate_utf8_prefix(text, max_bytes))
}

/// Whether `line` opens or closes a fenced markdown code block.
///
/// Matches CommonMark-style ```` ``` ```` / `~~~` fences with up to three
/// leading spaces. Shared so plan-markup stripping in the binary and markdown
/// rendering in `vtcode-ui` cannot drift.
///
/// ```
/// # use vtcode_commons::formatting::is_markdown_fence_delimiter;
/// assert!(is_markdown_fence_delimiter("```text"));
/// assert!(is_markdown_fence_delimiter("   ~~~"));
/// assert!(!is_markdown_fence_delimiter("    ```"));
/// assert!(!is_markdown_fence_delimiter("`inline`"));
/// ```
#[inline]
pub fn is_markdown_fence_delimiter(line: &str) -> bool {
    let indent = line.len() - line.trim_start().len();
    if indent > 3 {
        return false;
    }
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// Lowercase a leading capitalized plain word so `text` reads as a clause
/// continuing a sentence (for example after a colon). Acronyms and
/// identifiers (a second uppercase letter, a digit, or punctuation after the
/// first letter) and the pronoun "I" keep their case.
///
/// ```
/// # use vtcode_commons::formatting::lowercase_leading_word;
/// assert_eq!(lowercase_leading_word("Tool calls were rejected"), "tool calls were rejected");
/// assert_eq!(lowercase_leading_word("MCP server failed"), "MCP server failed");
/// assert_eq!(lowercase_leading_word("I cannot help"), "I cannot help");
/// assert_eq!(lowercase_leading_word("A"), "a");
/// ```
pub fn lowercase_leading_word(text: &str) -> String {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let second = chars.clone().next();
    let plain_word =
        first.is_uppercase() && first != 'I' && second.is_none_or(|c| c.is_lowercase() || c.is_whitespace());
    if plain_word {
        let mut lowered: String = first.to_lowercase().collect();
        lowered.push_str(chars.as_str());
        lowered
    } else {
        text.to_string()
    }
}

/// Collapse consecutive whitespace into single spaces, trimming leading/trailing.
///
/// ```
/// # use vtcode_commons::formatting::collapse_whitespace;
/// assert_eq!(collapse_whitespace("  hello   world  "), "hello world");
/// assert_eq!(collapse_whitespace(""), "");
/// ```
#[inline]
pub fn collapse_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            pending_space = true;
        } else {
            if pending_space && !result.is_empty() {
                result.push(' ');
            }
            result.push(ch);
            pending_space = false;
        }
    }
    result
}

/// Strip an optional leading `word` (case-insensitive) that is separated from the
/// rest by whitespace, returning the trimmed remainder, or `input` unchanged
/// when the prefix is absent or not word-boundary separated.
///
/// Canonical home for shell/file operand prefixes such as `on`/`from`.
///
/// ```
/// # use vtcode_commons::formatting::strip_optional_word_prefix;
/// assert_eq!(strip_optional_word_prefix("on src/main.rs", "on"), "src/main.rs");
/// assert_eq!(strip_optional_word_prefix("From here", "from"), "here");
/// // No whitespace boundary: left untouched.
/// assert_eq!(strip_optional_word_prefix("onto x", "on"), "onto x");
/// ```
#[inline]
pub fn strip_optional_word_prefix<'a>(input: &'a str, word: &str) -> &'a str {
    let Some(prefix) = input.get(..word.len()) else {
        return input;
    };
    if !prefix.eq_ignore_ascii_case(word) {
        return input;
    }
    let remainder = &input[word.len()..];
    if remainder.chars().next().is_some_and(char::is_whitespace) {
        remainder.trim_start()
    } else {
        input
    }
}

/// Trim surrounding whitespace, matched wrapping quotes (`"`/`'`), and trailing
/// sentence punctuation (`. , ; ! ?`), repeating until the value is stable.
///
/// ```
/// # use vtcode_commons::formatting::trim_wrapping_quotes_and_punctuation;
/// assert_eq!(trim_wrapping_quotes_and_punctuation("'src/main.rs',"), "src/main.rs");
/// assert_eq!(trim_wrapping_quotes_and_punctuation("  plain  "), "plain");
/// ```
#[inline]
pub fn trim_wrapping_quotes_and_punctuation(target: &str) -> &str {
    let mut normalized = target.trim();
    loop {
        let previous = normalized;
        normalized = normalized.trim();
        normalized = normalized.strip_prefix('"').unwrap_or(normalized);
        normalized = normalized.strip_suffix('"').unwrap_or(normalized);
        normalized = normalized.strip_prefix('\'').unwrap_or(normalized);
        normalized = normalized.strip_suffix('\'').unwrap_or(normalized);
        normalized = normalized.trim_end_matches(['.', ',', ';', '!', '?']).trim();
        if normalized == previous {
            return normalized;
        }
    }
}

/// Clean reasoning text by trimming trailing whitespace on each line and
/// removing blank lines.
///
/// ```
/// # use vtcode_commons::formatting::clean_reasoning_text;
/// assert_eq!(clean_reasoning_text("line1\n\n\nline2\n"), "line1\nline2");
/// assert_eq!(clean_reasoning_text(""), "");
/// ```
pub fn clean_reasoning_text(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Compact reasoning text for on-screen display.
///
/// Unlike [`clean_reasoning_text`], which removes *all* blank lines, this
/// collapses runs of two or more blank/whitespace-only lines into a single
/// blank line so paragraph structure is preserved while "blank-line spam"
/// from the model is removed. Leading/trailing whitespace on every line is
/// trimmed and leading/trailing blank lines of the whole block are dropped.
///
/// ```
/// # use vtcode_commons::formatting::compact_reasoning_text;
/// assert_eq!(compact_reasoning_text("line1\n\n\n\nline2\n"), "line1\n\nline2");
/// assert_eq!(compact_reasoning_text("  a  \n\n\n  b  \n"), "a\n\nb");
/// assert_eq!(compact_reasoning_text("\n\n\n"), "");
/// assert_eq!(compact_reasoning_text(""), "");
/// ```
pub fn compact_reasoning_text(text: &str) -> String {
    let mut out: Vec<&str> = Vec::with_capacity(text.lines().count());
    let mut prev_blank = false;
    for line in text.lines() {
        let trimmed = line.trim();
        let is_blank = trimmed.is_empty();
        if is_blank {
            if prev_blank {
                continue;
            }
            out.push("");
            prev_blank = true;
        } else {
            out.push(trimmed);
            prev_blank = false;
        }
    }
    while out.first().is_some_and(|l| l.trim().is_empty()) {
        out.remove(0);
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_duration_compact_ladder() {
        assert_eq!(humanize_duration_compact(0), "0s");
        assert_eq!(humanize_duration_compact(42), "42s");
        assert_eq!(humanize_duration_compact(59), "59s");
        assert_eq!(humanize_duration_compact(60), "1m");
        assert_eq!(humanize_duration_compact(300), "5m");
        assert_eq!(humanize_duration_compact(3599), "59m");
        assert_eq!(humanize_duration_compact(3600), "1h");
        assert_eq!(humanize_duration_compact(3 * 3600), "3h");
        assert_eq!(humanize_duration_compact(86399), "23h");
        assert_eq!(humanize_duration_compact(86400), "1d");
        assert_eq!(humanize_duration_compact(2 * 86400), "2d");
    }

    #[test]
    fn format_float_display_collapses_f32_widening() {
        // `0.7f32` widened to `f64` is the exact artifact seen in the settings
        // value column; asymmetric counterpart must keep full precision.
        assert_eq!(format_float_display(f64::from(0.7f32)), "0.7");
        assert_eq!(format_float_display(f64::from(0.3f32)), "0.3");
        assert_eq!(format_float_display(f64::from(-0.7f32)), "-0.7");
        assert_eq!(format_float_display(0.3333333333333333), "0.3333333333333333");
    }

    #[test]
    fn format_float_display_snaps_stepper_artifacts_to_grid() {
        // One `+0.1` press on the widened `0.7` default lands at
        // `0.799999988079071`; the classic `0.1 + 0.2` sum snaps the same way.
        // Asymmetric counterpart keeps genuine off-grid precision.
        assert_eq!(format_float_display(f64::from(0.7f32) + 0.1), "0.8");
        assert_eq!(format_float_display(0.799999988079071), "0.8");
        assert_eq!(format_float_display(0.1 + 0.2), "0.3");
        assert_eq!(format_float_display(-0.799999988079071), "-0.8");
        assert_eq!(format_float_display(0.123456789), "0.123456789");
    }

    #[test]
    fn format_float_display_keeps_plain_floats() {
        assert_eq!(format_float_display(1.0), "1");
        assert_eq!(format_float_display(0.0), "0");
        assert_eq!(format_float_display(0.05), "0.05");
        assert_eq!(format_float_display(0.75), "0.75");
    }

    #[test]
    fn truncate_byte_budget_ascii() {
        assert_eq!(truncate_byte_budget("hello world", 5, "..."), "hello...");
        assert_eq!(truncate_byte_budget("hi", 10, "..."), "hi");
    }

    #[test]
    fn contains_ignore_ascii_case_matches_str_contains_semantics() {
        // Mixed case on both sides.
        assert!(contains_ignore_ascii_case("MiniMax-M2.5", "minimax-m2.5"));
        assert!(contains_ignore_ascii_case("minimax-m2.5", "MINIMAX"));
        // Uppercase needle only (no lowercase form provided).
        assert!(contains_ignore_ascii_case("openai/gpt-5", "GPT-5"));
        // Empty needle matches, as `str::contains("")` does.
        assert!(contains_ignore_ascii_case("anything", ""));
        // Needle longer than the haystack.
        assert!(!contains_ignore_ascii_case("glm", "glm-4.5"));
        // No match.
        assert!(!contains_ignore_ascii_case("openai/gpt-5", "minimax"));
    }

    #[test]
    fn contains_ignore_ascii_case_matches_lowercased_contains_for_unicode() {
        // ASCII needle inside a non-ASCII haystack: `to_ascii_lowercase` leaves
        // multi-byte bytes untouched, so results must agree exactly.
        for (haystack, needle) in [
            ("café GLM-5", "glm-5"),
            ("日本語 <THINK", "<think"),
            ("straße", "STRASSE"),
            ("naïve", "naïve"),
        ] {
            assert_eq!(
                contains_ignore_ascii_case(haystack, needle),
                haystack.to_ascii_lowercase().contains(&needle.to_ascii_lowercase()),
                "mismatch for haystack={haystack:?} needle={needle:?}"
            );
        }
    }

    #[test]
    fn truncate_byte_budget_cjk_no_panic() {
        // 'こ' = 3 bytes, 'ん' = 3 bytes → "こんにちは" = 15 bytes
        let jp = "こんにちは";
        // Cutting at 5 bytes lands inside 'ん' (bytes 3..6); must round down to 3.
        assert_eq!(truncate_byte_budget(jp, 5, "…"), "こ…");
        // Cutting at 6 lands on boundary
        assert_eq!(truncate_byte_budget(jp, 6, "…"), "こん…");
    }

    #[test]
    fn truncate_byte_budget_mixed_ascii_cjk() {
        let mixed = "AB日本語CD";
        // A=1, B=1, 日=3, 本=3, 語=3, C=1, D=1 → 13 bytes total
        assert_eq!(truncate_byte_budget(mixed, 4, ".."), "AB.."); // mid-日 rounds to 2
        assert_eq!(truncate_byte_budget(mixed, 5, ".."), "AB日.."); // 2+3=5 exact
    }

    #[test]
    fn truncate_byte_budget_emoji() {
        let emoji = "👋🌍"; // 4 bytes each = 8 bytes
        assert_eq!(truncate_byte_budget(emoji, 5, "!"), "👋!");
    }

    #[test]
    fn truncate_byte_budget_zero() {
        assert_eq!(truncate_byte_budget("abc", 0, "..."), "...");
    }

    #[test]
    fn compact_reasoning_text_collapses_blank_runs() {
        assert_eq!(compact_reasoning_text("line1\n\n\n\nline2\n"), "line1\n\nline2");
        assert_eq!(compact_reasoning_text("a\n\n\n\n\n\nb"), "a\n\nb");
    }

    #[test]
    fn compact_reasoning_text_preserves_single_paragraph_breaks() {
        assert_eq!(compact_reasoning_text("para one\n\npara two\n"), "para one\n\npara two");
    }

    #[test]
    fn compact_reasoning_text_trims_trailing_whitespace() {
        assert_eq!(compact_reasoning_text("  a  \n\n\n  b  \n"), "a\n\nb");
    }

    #[test]
    fn compact_reasoning_text_strips_leading_trailing_blanks() {
        assert_eq!(compact_reasoning_text("\n\n\nmid\n\n\n"), "mid");
        assert_eq!(compact_reasoning_text("\n\n\n"), "");
        assert_eq!(compact_reasoning_text(""), "");
    }

    #[test]
    fn wrap_text_words_basic_and_continuation_width() {
        assert_eq!(wrap_text_words("the quick brown fox", 9, 9), vec!["the quick", "brown fox"]);
        // First line wider than continuation lines.
        assert_eq!(wrap_text_words("alpha beta gamma delta", 11, 5), vec!["alpha beta", "gamma", "delta"]);
    }

    #[test]
    fn wrap_text_words_blank_and_unicode() {
        assert!(wrap_text_words("   ", 5, 5).is_empty());
        // Must not panic on multi-byte chars and counts chars, not bytes.
        let wrapped = wrap_text_words("あいう えお かきく", 3, 3);
        assert_eq!(wrapped, vec!["あいう", "えお", "かきく"]);
    }

    #[test]
    fn wrap_shell_command_keeps_screenshot_pipeline_in_full() {
        // Screenshot 2026-09-24 16:37: the quoted grep pattern holds spaces
        // and pipes but must never split mid-quote; every pipe segment must
        // survive with no `…`, and rejoining restores the source exactly.
        let command = "grep -rn \"@vinhnx/vtcode|npm install -g||npx @vinhnx\" docs | grep -v node_modules | grep -v package-lock | grep -v \"\\.backup\"";
        let wrapped = wrap_shell_command(command, 62, 58);
        assert_eq!(
            wrapped,
            vec![
                "grep -rn \"@vinhnx/vtcode|npm install -g||npx @vinhnx\" docs |",
                "grep -v node_modules | grep -v package-lock | grep -v",
                "\"\\.backup\"",
            ]
        );
        assert!(wrapped.iter().all(|line| !line.contains('…')));
        assert_eq!(wrapped.join(" "), command);
        assert_eq!(wrapped.join("\n").matches('|').count(), 6);
    }

    #[test]
    fn wrap_shell_command_breaks_only_at_unquoted_spaces() {
        // The quoted span is atomic: `echo` overflows alone rather than the
        // pattern splitting across lines.
        assert_eq!(wrap_shell_command("echo \"a b c\" d", 10, 10), vec!["echo", "\"a b c\" d"]);
        assert_eq!(wrap_shell_command("   ", 5, 5), Vec::<String>::new());
    }

    #[test]
    fn wrap_shell_command_splits_overlong_token_at_width() {
        assert_eq!(wrap_shell_command("abcdefghij", 4, 4), vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn wrap_shell_command_with_continuations_marks_wrapped_lines() {
        // Multi-line headers read as valid shell: every line except the last
        // ends with ` \`, single-line commands stay bare, blanks stay empty.
        assert_eq!(wrap_shell_command_with_continuations("echo a b c d", 7, 7), vec!["echo a \\", "b c d"]);
        assert_eq!(wrap_shell_command_with_continuations("git status --short", 62, 58), vec!["git status --short"]);
        assert!(wrap_shell_command_with_continuations("   ", 5, 5).is_empty());
        // Chained git commands (issue screenshot) keep every segment with no
        // `…` and mark each wrapped row as a continuation.
        let command = "git add a b && git commit -m \"msg\" && git status --short";
        let wrapped = wrap_shell_command_with_continuations(command, 20, 20);
        assert!(wrapped.len() > 1, "expected wrapping: {wrapped:?}");
        assert!(wrapped[..wrapped.len() - 1].iter().all(|line| line.ends_with(" \\")));
        assert!(!wrapped.last().unwrap().ends_with(" \\"));
        assert!(wrapped.iter().all(|line| !line.contains('…')));
        // Overlong chains break at operators with `&&` trailing, while
        // quoted `||` never splits.
        assert_eq!(wrap_shell_command_with_continuations("echo a && echo b", 10, 10), vec!["echo a && \\", "echo b"]);
        assert_eq!(wrap_shell_command_with_continuations("a | b | c", 7, 5), vec!["a | \\", "b | \\", "c"]);
        assert_eq!(
            wrap_shell_command_with_continuations("grep -rn \"a||b\" docs", 62, 58),
            vec!["grep -rn \"a||b\" docs"]
        );
        // Short chains that fit stay on one line (no forced splits).
        assert_eq!(wrap_shell_command_with_continuations("echo a && echo b", 62, 58), vec!["echo a && echo b"]);
        assert_eq!(wrap_shell_command_with_continuations("a | b | c", 62, 58), vec!["a | b | c"]);
        // The marker-free helper breaks identically minus suffixes.
        assert_eq!(wrap_shell_command_lines("echo a && echo b", 10, 10), vec!["echo a &&", "echo b"]);
        assert_eq!(wrap_shell_command_lines("git status", 62, 58), vec!["git status"]);
    }

    #[test]
    fn wrap_shell_command_narrow_continuation_never_overflows() {
        // A token that fits the wider first line but not the narrower
        // continuation must hard-split after the line break instead of
        // emitting an over-budget row (62/58 production widths).
        let token = "b".repeat(60);
        let command = format!("aa {token} cc");
        let wrapped = wrap_shell_command(&command, 62, 58);
        assert!(wrapped.len() >= 3, "expected a split continuation: {wrapped:?}");
        assert!(wrapped[0].chars().count() <= 62, "first line budget: {wrapped:?}");
        for line in wrapped.iter().skip(1) {
            assert!(line.chars().count() <= 58, "continuation budget: {wrapped:?}");
        }
        // Same shape with a tiny budget for a fast unit check.
        let wrapped = wrap_shell_command("aa bbbbbbbb cc", 10, 5);
        assert_eq!(wrapped, vec!["aa", "bbbbb", "bbb", "cc"]);
    }

    #[test]
    fn truncate_within_reserves_ellipsis_budget() {
        // Matches former runner::orchestration::truncate_chars behavior.
        assert_eq!(truncate_within("hello world", 8, "..."), "hello...");
        assert_eq!(truncate_within("hi", 8, "..."), "hi");
        // Single-char ellipsis reserves exactly one char (former snapshots /
        // session_archive behavior).
        assert_eq!(truncate_within("abcdef", 4, "…"), "abc…");
    }

    #[test]
    fn truncate_within_counts_chars() {
        let jp = "あいうえお"; // 5 chars
        assert_eq!(truncate_within(jp, 5, "…"), jp);
        assert_eq!(truncate_within(jp, 3, "…"), "あい…");
    }

    #[test]
    fn head_tail_truncate_keeps_both_ends() {
        let value = "0123456789".repeat(10); // 100 chars
        let (out, truncated) = head_tail_truncate(&value, 40, " ... [truncated] ... ");
        assert!(truncated);
        assert!(out.chars().count() <= 40);
        assert!(out.starts_with("012"));
        assert!(out.contains("[truncated]"));
        assert!(out.ends_with('9'));
    }

    #[test]
    fn head_tail_truncate_passes_through_when_short() {
        let (out, truncated) = head_tail_truncate("short", 64, " ... ");
        assert_eq!(out, "short");
        assert!(!truncated);
    }

    #[test]
    fn head_tail_truncate_small_budget_falls_back_to_prefix() {
        let marker = " ... [truncated] ... ";
        // max_chars <= marker_chars + 16 triggers the prefix fallback.
        // When max_chars (5) <= suffix_len (12), return just the prefix without suffix.
        let (out, truncated) = head_tail_truncate("abcdefghij", 5, marker);
        assert!(truncated);
        assert_eq!(out, "abcde");

        // When max_chars allows room for suffix, include it in the fallback branch.
        // Use max_chars=17 which is <= 21+16=37 (triggers fallback).
        let long_text = "abcdefghijklmnopqrstuvwxyz";
        let (out2, truncated2) = head_tail_truncate(long_text, 17, marker);
        assert!(truncated2);
        assert_eq!(out2, "abcde [truncated]");
        assert_eq!(out2.chars().count(), 17);
    }

    #[test]
    fn truncate_text_counts_chars_not_bytes() {
        let jp = "あいうえお"; // 5 chars, 15 bytes
        assert_eq!(truncate_text(jp, 3, "…"), "あいう…");
        assert_eq!(truncate_text(jp, 5, "…"), "あいうえお");
    }

    #[test]
    fn truncate_middle_keeps_both_ends() {
        assert_eq!(truncate_middle("short", 80), "short");
        assert_eq!(truncate_middle("abcdefghij", 5), "ab…ij");
        assert_eq!(truncate_middle("a b c", 80), "a b c");
        // Zero/one-char budgets.
        assert_eq!(truncate_middle("abc", 0), "");
        assert_eq!(truncate_middle("abc", 1), "…");
        // Control characters are sanitized to spaces before truncating.
        assert_eq!(truncate_middle("a\nb\tc", 80), "a b c");
    }

    #[test]
    fn truncate_path_middle_breaks_at_separator() {
        assert_eq!(truncate_path_middle("src/lib.rs", 80), "src/lib.rs");
        assert_eq!(truncate_path_middle("foo/bar/baz/qux", 12), "foo…/qux");
        assert_eq!(truncate_path_middle("abc", 0), "");
    }

    #[test]
    fn truncate_path_middle_counts_chars_not_bytes() {
        // Found by generative_tests: byte offsets from rfind/find overshot the budget.
        let out = truncate_path_middle("日本/日本語/日本語日本語/xyz", 6);
        assert!(out.chars().count() <= 6, "{out:?}");
        assert_eq!(truncate_path_middle("日本/日本語/xyz", 8), "日本…xyz");
    }
}

#[cfg(test)]
mod generative_tests;
