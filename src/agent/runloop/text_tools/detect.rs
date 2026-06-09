use memchr::{memchr, memmem};
use serde_json::Value;

use crate::agent::runloop::text_tools::canonical::{
    DIRECT_FUNCTION_ALIASES, TEXTUAL_TOOL_PREFIXES, canonicalize_tool_name,
    canonicalize_tool_result,
};
use crate::agent::runloop::text_tools::parse_args::parse_textual_arguments;
use crate::agent::runloop::text_tools::parse_bracketed::parse_bracketed_tool_call;
use crate::agent::runloop::text_tools::parse_channel::parse_channel_tool_call;
use crate::agent::runloop::text_tools::parse_dsml::parse_dsml_tool_call;
use crate::agent::runloop::text_tools::parse_structured::parse_rust_struct_tool_call;
use crate::agent::runloop::text_tools::parse_tagged::parse_tagged_tool_call;
use crate::agent::runloop::text_tools::parse_yaml::parse_yaml_tool_call;

const MAX_TEXTUAL_NESTING_DEPTH: usize = 256;

fn matching_open_delimiter(close: char) -> Option<char> {
    match close {
        ')' => Some('('),
        '}' => Some('{'),
        ']' => Some('['),
        _ => None,
    }
}

fn find_matching_paren_end_with_depth_limit(text: &str, args_start: usize) -> Option<usize> {
    let mut stack = Vec::with_capacity(8);
    stack.push('(');

    let mut in_string: Option<char> = None;
    let mut escaped = false;

    for (relative, ch) in text[args_start..].char_indices() {
        if let Some(delimiter) = in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == delimiter {
                in_string = None;
            }
            continue;
        }

        if ch == '"' || ch == '\'' {
            in_string = Some(ch);
            continue;
        }

        match ch {
            '(' | '{' | '[' => {
                stack.push(ch);
                if stack.len() > MAX_TEXTUAL_NESTING_DEPTH {
                    tracing::warn!(
                        nesting_depth = stack.len(),
                        max_nesting_depth = MAX_TEXTUAL_NESTING_DEPTH,
                        "Rejected textual tool call due to excessive delimiter nesting"
                    );
                    return None;
                }
            }
            ')' | '}' | ']' => {
                let expected = match matching_open_delimiter(ch) {
                    Some(value) => value,
                    None => {
                        tracing::debug!(
                            delimiter = %ch,
                            "Rejected textual tool call due to unsupported closing delimiter"
                        );
                        return None;
                    }
                };
                let current = match stack.pop() {
                    Some(value) => value,
                    None => {
                        tracing::debug!(
                            delimiter = %ch,
                            "Rejected textual tool call due to unmatched closing delimiter"
                        );
                        return None;
                    }
                };
                if current != expected {
                    tracing::debug!(
                        current_open = %current,
                        expected_open = %expected,
                        close = %ch,
                        "Rejected textual tool call due to mismatched delimiters"
                    );
                    return None;
                }
                if stack.is_empty() {
                    return Some(args_start + relative);
                }
            }
            _ => {}
        }
    }

    None
}

pub(crate) fn detect_textual_tool_call(text: &str) -> Option<(String, Value)> {
    // Try gpt-oss channel format first
    if let Some((name, args)) = parse_channel_tool_call(text)
        && let Some(result) = canonicalize_tool_result(name, args)
    {
        return Some(result);
    }

    // DeepSeek DSML v2 format: <||DSML||invoke name="...">
    if let Some((name, args)) = parse_dsml_tool_call(text)
        && let Some(result) = canonicalize_tool_result(name, args)
    {
        return Some(result);
    }

    if let Some((name, args)) = parse_tagged_tool_call(text)
        && let Some(result) = canonicalize_tool_result(name, args)
    {
        return Some(result);
    }

    if let Some((name, args)) = parse_rust_struct_tool_call(text)
        && let Some(result) = canonicalize_tool_result(name, args)
    {
        return Some(result);
    }

    if let Some((name, args)) = parse_yaml_tool_call(text)
        && let Some(result) = canonicalize_tool_result(name, args)
    {
        return Some(result);
    }

    if let Some((name, args)) = parse_bracketed_tool_call(text)
        && let Some(result) = canonicalize_tool_result(name, args)
    {
        return Some(result);
    }

    for prefix in TEXTUAL_TOOL_PREFIXES {
        let prefix_bytes = prefix.as_bytes();
        let text_bytes = text.as_bytes();
        let mut search_start = 0usize;

        while search_start < text_bytes.len() {
            if let Some(offset) = memmem::find(&text_bytes[search_start..], prefix_bytes) {
                let prefix_index = search_start + offset;
                let start = prefix_index + prefix.len();
                if start >= text.len() {
                    break;
                }
                let tail = &text[start..];
                let mut name_len = 0usize;
                for ch in tail.chars() {
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        name_len += ch.len_utf8();
                    } else {
                        break;
                    }
                }
                if name_len == 0 {
                    search_start = prefix_index + prefix.len();
                    continue;
                }

                let name = tail[..name_len].to_string();
                let after_name = &tail[name_len..];

                // Use memchr to search for the opening parenthesis
                let paren_pos = memchr(b'(', after_name.as_bytes());
                let paren_offset = if let Some(pos) = paren_pos {
                    pos
                } else {
                    search_start = start;
                    continue;
                };

                let args_start = start + name_len + paren_offset + 1;
                let Some(args_end) = find_matching_paren_end_with_depth_limit(text, args_start)
                else {
                    search_start = start;
                    continue;
                };
                let raw_args = &text[args_start..args_end];
                if let Some(args) = parse_textual_arguments(raw_args)
                    && let Some(canonical) = canonicalize_tool_name(&name)
                {
                    return Some((canonical, args));
                }

                search_start = prefix_index + prefix.len() + name_len;
            } else {
                break; // No more matches
            }
        }
    }

    if let Some(result) = detect_direct_function_alias(text) {
        return Some(result);
    }
    None
}

/// Check whether `text` contains raw pseudo-tool-call markup markers that
/// indicate tool-call intent without requiring successful parsing.
///
/// `detect_textual_tool_call` only returns `Some` when it can fully parse and
/// canonicalize a call. Malformed or partially-serialized markup (e.g. a bare
/// `<tool_call>` tag whose payload fails to parse) slips past it. This
/// function provides a lightweight pre-scan so the recovery guard can catch
/// those cases too.
pub(crate) fn contains_pseudo_tool_call_markers(text: &str) -> bool {
    const MARKERS: &[&str] = &[
        "<tool_call",
        "</tool_call>",
        "<function=",
        "<parameter=",
        "<invoke name=",
        "<minimax:tool_call>",
    ];
    let lowered = text.to_ascii_lowercase();
    MARKERS.iter().any(|marker| lowered.contains(marker))
}

pub(crate) fn strip_textual_tool_call_regions(text: &str) -> String {
    let mut regions = Vec::new();
    collect_channel_regions(text, &mut regions);
    collect_code_fence_regions(text, &mut regions);
    collect_enclosed_regions(
        text,
        "<minimax:tool_call>",
        "</minimax:tool_call>",
        &mut regions,
    );
    collect_enclosed_regions(text, "<invoke name=\"", "</invoke>", &mut regions);
    collect_enclosed_regions(text, "<tool_call>", "</tool_call>", &mut regions);
    // Strip <function=name>...</function> and <parameter=name>...</parameter>
    // blocks that models emit as pseudo-tool-call markup when tools are
    // unavailable. These may appear inside a <tool_call> wrapper (already
    // handled above) or bare when the wrapper is missing.
    // Unlike `collect_enclosed_regions`, these use `collect_pseudo_marker_regions`
    // because `detect_textual_tool_call` cannot parse them (they are intentionally
    // non-parseable), but they are clearly tool-call intent by tag name alone.
    // Use close prefixes (without the trailing ">") so that both
    // `</function>` and `</function=name>` (parameterised close tags) are
    // consumed in full; the scanner advances past any suffix to the next ">".
    collect_pseudo_marker_regions(text, "<function=", "</function", &mut regions);
    collect_pseudo_marker_regions(text, "<parameter=", "</parameter", &mut regions);
    collect_bracketed_regions(text, &mut regions);
    collect_function_call_regions(text, &mut regions);

    if regions.is_empty() {
        return text.to_string();
    }

    regions.sort_unstable_by_key(|(start, end)| (*start, *end));
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(regions.len());
    for (start, end) in regions {
        if start >= end || end > text.len() {
            continue;
        }
        if let Some((_, last_end)) = merged.last_mut()
            && start <= *last_end
        {
            *last_end = (*last_end).max(end);
            continue;
        }
        merged.push((start, end));
    }

    let mut stripped = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for (start, end) in merged {
        stripped.push_str(&text[cursor..start]);
        cursor = end;
    }
    stripped.push_str(&text[cursor..]);
    stripped
}

fn collect_channel_regions(text: &str, regions: &mut Vec<(usize, usize)>) {
    let mut search_start = 0usize;
    while let Some(relative_start) = text[search_start..].find("<|start|>") {
        let start = search_start + relative_start;
        let tail = &text[start..];
        let end = tail
            .find("<|call|>")
            .map(|idx| start + idx + "<|call|>".len())
            .or_else(|| {
                tail.find("<|end|>")
                    .map(|idx| start + idx + "<|end|>".len())
            })
            .or_else(|| {
                tail.find("<|return|>")
                    .map(|idx| start + idx + "<|return|>".len())
            })
            .unwrap_or(text.len());
        add_valid_region(text, start, end, regions);
        search_start = end.max(start + "<|start|>".len());
    }
}

fn collect_code_fence_regions(text: &str, regions: &mut Vec<(usize, usize)>) {
    let mut search_start = 0usize;
    while let Some(relative_start) = text[search_start..].find("```") {
        let start = search_start + relative_start;
        let body_start = start + 3;
        let Some(relative_end) = text[body_start..].find("```") else {
            break;
        };
        let end = body_start + relative_end + 3;
        add_valid_region(text, start, end, regions);
        search_start = end;
    }
}

fn collect_enclosed_regions(
    text: &str,
    open_marker: &str,
    close_marker: &str,
    regions: &mut Vec<(usize, usize)>,
) {
    let mut search_start = 0usize;
    while let Some(relative_start) = text[search_start..].find(open_marker) {
        let start = search_start + relative_start;
        let content_start = start + open_marker.len();
        let end = text[content_start..]
            .find(close_marker)
            .map(|idx| content_start + idx + close_marker.len())
            .unwrap_or(text.len());
        add_valid_region(text, start, end, regions);
        search_start = end.max(content_start);
    }
}

/// Like `collect_enclosed_regions` but accepts regions that contain
/// pseudo-tool-call markers even when `detect_textual_tool_call` cannot
/// parse them. Used for `<function=…>` and `<parameter=…>` blocks whose
/// payload is intentionally not parseable by the full tool-call parser.
///
/// `close_prefix` is matched case-insensitively as a tag prefix (e.g.
/// `"</function"`). After finding the prefix the scanner advances past any
/// suffix bytes (e.g. `"=apply_patch"`) to the next `>`, so both
/// `</function>` and `</function=name>` are handled correctly.
fn collect_pseudo_marker_regions(
    text: &str,
    open_marker: &str,
    close_prefix: &str,
    regions: &mut Vec<(usize, usize)>,
) {
    let lowered = text.to_ascii_lowercase();
    let open_lower = open_marker.to_ascii_lowercase();
    let close_lower = close_prefix.to_ascii_lowercase();
    let mut search_start = 0usize;
    while let Some(relative_start) = lowered[search_start..].find(&open_lower) {
        let start = search_start + relative_start;
        let content_start = start + open_marker.len();
        let end = lowered[content_start..]
            .find(&close_lower)
            .map(|idx| {
                // Advance past the prefix bytes to the next '>' so that both
                // `</function>` and `</function=name>` are consumed in full.
                let prefix_end = content_start + idx + close_prefix.len();
                lowered[prefix_end..]
                    .find('>')
                    .map_or(text.len(), |extra| prefix_end + extra + 1)
            })
            .unwrap_or(text.len());
        if start < end && end <= text.len() {
            regions.push((start, end));
        }
        search_start = end.max(content_start);
    }
}

fn collect_bracketed_regions(text: &str, regions: &mut Vec<(usize, usize)>) {
    let mut search_start = 0usize;
    while let Some(relative_start) = text[search_start..].find("[tool: ") {
        let start = search_start + relative_start;
        let Some(header_end_relative) = text[start..].find(']') else {
            break;
        };
        let after_header = start + header_end_relative + 1;
        let args_start = after_header
            + text[after_header..]
                .chars()
                .take_while(|ch| ch.is_whitespace())
                .map(char::len_utf8)
                .sum::<usize>();
        let Some(open) = text[args_start..].chars().next() else {
            break;
        };
        let close = match open {
            '{' => '}',
            '(' => ')',
            _ => {
                search_start = after_header;
                continue;
            }
        };
        let Some(args_end) = find_matching_delimiter_end(text, args_start, open, close) else {
            search_start = after_header;
            continue;
        };
        add_valid_region(text, start, args_end + 1, regions);
        search_start = args_end + 1;
    }
}

fn collect_function_call_regions(text: &str, regions: &mut Vec<(usize, usize)>) {
    for prefix in TEXTUAL_TOOL_PREFIXES {
        collect_prefixed_function_regions(text, prefix, regions);
    }
    for alias in DIRECT_FUNCTION_ALIASES {
        collect_direct_function_regions(text, alias, regions);
    }
}

fn collect_prefixed_function_regions(text: &str, prefix: &str, regions: &mut Vec<(usize, usize)>) {
    let mut search_start = 0usize;
    while let Some(relative_start) = text[search_start..].find(prefix) {
        let start = search_start + relative_start;
        let after_prefix = start + prefix.len();
        let Some(paren_relative) = text[after_prefix..].find('(') else {
            break;
        };
        let args_start = after_prefix + paren_relative + 1;
        let Some(args_end) = find_matching_paren_end_with_depth_limit(text, args_start) else {
            search_start = after_prefix;
            continue;
        };
        let end = args_end + 1;
        let (region_start, region_end) = expand_wrapping_function_region(text, start, end);
        add_valid_region(text, region_start, region_end, regions);
        search_start = end;
    }
}

fn collect_direct_function_regions(text: &str, alias: &str, regions: &mut Vec<(usize, usize)>) {
    let lowered = text.to_ascii_lowercase();
    let alias_lower = alias.to_ascii_lowercase();
    let mut search_start = 0usize;
    while let Some(relative_start) = lowered[search_start..].find(&alias_lower) {
        let start = search_start + relative_start;
        let alias_end = start + alias_lower.len();
        if start > 0
            && let Some(prev) = lowered[..start].chars().next_back()
            && (prev.is_ascii_alphanumeric() || prev == '_')
        {
            search_start = alias_end;
            continue;
        }
        let mut paren_index = None;
        for (relative, ch) in text[alias_end..].char_indices() {
            if ch.is_whitespace() {
                continue;
            }
            if ch == '(' {
                paren_index = Some(alias_end + relative);
            }
            break;
        }
        let Some(paren_pos) = paren_index else {
            search_start = alias_end;
            continue;
        };
        let Some(args_end) = find_matching_paren_end_with_depth_limit(text, paren_pos + 1) else {
            search_start = alias_end;
            continue;
        };
        let end = args_end + 1;
        let (region_start, region_end) = expand_wrapping_function_region(text, start, end);
        add_valid_region(text, region_start, region_end, regions);
        search_start = end;
    }
}

fn expand_wrapping_function_region(text: &str, start: usize, end: usize) -> (usize, usize) {
    let before = text[..start].trim_end();
    if !before.ends_with('(') {
        return (start, end);
    }

    let open_paren = before.len() - 1;
    let mut wrapper_start = open_paren;
    for (index, ch) in text[..open_paren].char_indices().rev() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
            wrapper_start = index;
        } else {
            break;
        }
    }
    if wrapper_start == open_paren {
        return (start, end);
    }

    let after_inner_whitespace = text[end..]
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .map(char::len_utf8)
        .sum::<usize>();
    let close_start = end + after_inner_whitespace;
    if text[close_start..].starts_with(')') {
        (wrapper_start, close_start + 1)
    } else {
        (start, end)
    }
}

fn find_matching_delimiter_end(
    text: &str,
    open_index: usize,
    open: char,
    close: char,
) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string: Option<char> = None;
    let mut escaped = false;

    for (relative, ch) in text[open_index..].char_indices() {
        if let Some(delimiter) = in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == delimiter {
                in_string = None;
            }
            continue;
        }

        if ch == '"' || ch == '\'' {
            in_string = Some(ch);
            continue;
        }
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(open_index + relative);
            }
        }
    }
    None
}

fn add_valid_region(text: &str, start: usize, end: usize, regions: &mut Vec<(usize, usize)>) {
    if start >= end || end > text.len() {
        return;
    }
    if detect_textual_tool_call(&text[start..end]).is_some() {
        regions.push((start, end));
    }
}

fn detect_direct_function_alias(text: &str) -> Option<(String, Value)> {
    let lowered = text.to_ascii_lowercase();

    for alias in DIRECT_FUNCTION_ALIASES {
        let alias_lower = alias.to_ascii_lowercase();
        let alias_bytes = alias_lower.as_bytes();
        let lowered_bytes = lowered.as_bytes();
        let mut search_start = 0usize;

        while search_start < lowered_bytes.len() {
            if let Some(offset) = memmem::find(&lowered_bytes[search_start..], alias_bytes) {
                let start = search_start + offset;
                let end = start + alias_lower.len();

                if start > 0
                    && let Some(prev) = lowered[..start].chars().next_back()
                    && (prev.is_ascii_alphanumeric() || prev == '_')
                {
                    search_start = end;
                    continue;
                }

                let mut paren_index: Option<usize> = None;
                let iter = text[end..].char_indices();
                for (relative, ch) in iter {
                    if ch.is_whitespace() {
                        continue;
                    }
                    if ch == '(' {
                        paren_index = Some(end + relative);
                    }
                    break;
                }

                let Some(paren_pos) = paren_index else {
                    search_start = end;
                    continue;
                };

                let args_start = paren_pos + 1;
                let Some(end_pos) = find_matching_paren_end_with_depth_limit(text, args_start)
                else {
                    search_start = end;
                    continue;
                };

                let raw_args = &text[args_start..end_pos];
                if let Some(args) = parse_textual_arguments(raw_args)
                    && let Some(result) = canonicalize_tool_result(alias.to_string(), args)
                {
                    return Some(result);
                }

                search_start = end;
            } else {
                break; // No more matches
            }
        }
    }

    None
}
