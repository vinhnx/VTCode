use memchr::memmem;
use serde_json::Value;

use crate::agent::runloop::text_tools::canonical::{
    DIRECT_FUNCTION_ALIASES,
    TEXTUAL_TOOL_PREFIXES,
    canonicalize_tool_name,
    canonicalize_tool_result,
};
use crate::agent::runloop::text_tools::parse_args::parse_textual_arguments;
use crate::agent::runloop::text_tools::parse_bracketed::parse_bracketed_tool_call;
use crate::agent::runloop::text_tools::parse_channel::parse_channel_tool_call;
use crate::agent::runloop::text_tools::parse_structured::parse_rust_struct_tool_call;
use crate::agent::runloop::text_tools::parse_tagged::parse_tagged_tool_call;
use crate::agent::runloop::text_tools::parse_yaml::parse_yaml_tool_call;

pub(crate) fn detect_textual_tool_call(text: &str) -> Option<(String, Value)> {
    // Try gpt-oss channel format first
    if let Some((name, args)) = parse_channel_tool_call(text)
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
                let paren_pos = memmem::find(after_name.as_bytes(), b"(");
                let paren_offset = if let Some(pos) = paren_pos {
                    pos
                } else {
                    search_start = start;
                    continue;
                };

                let args_start = start + name_len + paren_offset + 1;
                let mut depth = 1i32;
                let mut end: Option<usize> = None;
                for (rel_idx, ch) in text[args_start..].char_indices() {
                    match ch {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(args_start + rel_idx);
                                break;
                            }
                        }
                        _ => {}
                    }
                }

                let args_end = end?;
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
                let mut depth = 1i32;
                let mut args_end: Option<usize> = None;
                for (relative, ch) in text[args_start..].char_indices() {
                    match ch {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                args_end = Some(args_start + relative);
                                break;
                            }
                        }
                        _ => {}
                    }
                }

                let Some(end_pos) = args_end else {
                    search_start = end;
                    continue;
                };

                let raw_args = &text[args_start..end_pos];
                if let Some(args) = parse_textual_arguments(raw_args)
                    && let Some(canonical) = canonicalize_tool_name(alias)
                {
                    return Some((canonical, args));
                }

                search_start = end;
            } else {
                break; // No more matches
            }
        }
    }

    None
}
