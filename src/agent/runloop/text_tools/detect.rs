use memchr::{memchr, memmem};
use serde_json::Value;
use std::sync::OnceLock;

use crate::agent::runloop::text_tools::canonical::{
    DIRECT_FUNCTION_ALIASES, TEXTUAL_TOOL_PREFIXES, canonicalize_tool_result,
};
use crate::agent::runloop::text_tools::parse_args::{
    find_matching_delimiter, parse_textual_arguments,
};
use crate::agent::runloop::text_tools::parse_bracketed::BracketedToolParser;
use crate::agent::runloop::text_tools::parse_channel::ChannelToolParser;
use crate::agent::runloop::text_tools::parse_dsml::DsmlToolParser;
use crate::agent::runloop::text_tools::parse_structured::StructuredToolParser;
use crate::agent::runloop::text_tools::parse_tagged::TaggedToolParser;
use crate::agent::runloop::text_tools::parse_yaml::YamlToolParser;
use crate::agent::runloop::text_tools::parser::{
    ParseResult, ParsedToolCall, TextualToolParser, TextualToolParserRegistry,
};

const MAX_TEXTUAL_NESTING_DEPTH: usize = 256;

/// Creates the default parser registry with all textual tool parsers
/// registered in priority order.
static DEFAULT_PARSER_REGISTRY: OnceLock<TextualToolParserRegistry> = OnceLock::new();

fn default_parser_registry() -> &'static TextualToolParserRegistry {
    DEFAULT_PARSER_REGISTRY.get_or_init(|| {
        let mut registry = TextualToolParserRegistry::new();

        // Register parsers in priority order (same as hardcoded chain before)
        registry.register(Box::new(ChannelToolParser));
        registry.register(Box::new(DsmlToolParser));
        registry.register(Box::new(TaggedToolParser));
        registry.register(Box::new(StructuredToolParser));
        registry.register(Box::new(YamlToolParser));
        registry.register(Box::new(BracketedToolParser));
        registry.register(Box::new(PrefixedToolParser));
        registry.register(Box::new(DirectFunctionAliasParser));

        registry
    })
}

pub(crate) fn detect_textual_tool_call(text: &str) -> Option<(String, Value)> {
    let registry = default_parser_registry();

    if let Some((parsed, should_validate)) = registry.try_parse(text) {
        canonicalize_tool_result(parsed.name, parsed.args, should_validate)
    } else {
        None
    }
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
    let registry = default_parser_registry();
    let mut regions = registry.consumed_spans(text);

    // Pseudo-marker regions are not parseable as tool calls but still indicate
    // tool-call intent, so they are stripped separately.
    collect_enclosed_regions(
        text,
        "<minimax:tool_call>",
        "</minimax:tool_call>",
        &mut regions,
    );
    collect_pseudo_marker_regions(text, "<tool_call>", "</tool_call", &mut regions);
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
        if start < end && end <= text.len() {
            regions.push((start, end));
        }
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

fn collect_prefixed_function_regions(text: &str, prefix: &str, regions: &mut Vec<(usize, usize)>) {
    let mut search_start = 0usize;
    while let Some(relative_start) = text[search_start..].find(prefix) {
        let start = search_start + relative_start;
        let after_prefix = start + prefix.len();
        let Some(paren_relative) = text[after_prefix..].find('(') else {
            break;
        };
        let paren_index = after_prefix + paren_relative;
        // Use shared delimiter matcher (pass paren index, not args_start)
        let Some(args_end) =
            find_matching_delimiter(text, paren_index, '(', ')', MAX_TEXTUAL_NESTING_DEPTH)
        else {
            tracing::debug!(
                prefix = %prefix,
                "Rejected prefixed function call due to unmatched parentheses"
            );
            search_start = after_prefix;
            continue;
        };
        let end = args_end + 1;
        let (region_start, region_end) = expand_wrapping_function_region(text, start, end);
        if region_start < region_end && region_end <= text.len() {
            regions.push((region_start, region_end));
        }
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
        // Use shared delimiter matcher (pass paren index, not args_start)
        let Some(args_end) =
            find_matching_delimiter(text, paren_pos, '(', ')', MAX_TEXTUAL_NESTING_DEPTH)
        else {
            tracing::debug!(
                alias = %alias,
                "Rejected direct function call due to unmatched parentheses"
            );
            search_start = alias_end;
            continue;
        };
        let end = args_end + 1;
        let (region_start, region_end) = expand_wrapping_function_region(text, start, end);
        if region_start < region_end && region_end <= text.len() {
            regions.push((region_start, region_end));
        }
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

/// Parser for prefixed tool calls (e.g., "default_api.tool_name(...)").
struct PrefixedToolParser;

impl TextualToolParser for PrefixedToolParser {
    fn name(&self) -> &'static str {
        "prefixed"
    }

    fn try_parse(&self, text: &str) -> ParseResult {
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

                    let paren_pos = memchr(b'(', after_name.as_bytes());
                    let paren_offset = if let Some(pos) = paren_pos {
                        pos
                    } else {
                        tracing::debug!(
                            parser = "prefixed",
                            reason = "no opening parenthesis",
                            "Rejected prefixed tool call"
                        );
                        search_start = start;
                        continue;
                    };

                    let paren_index = start + name_len + paren_offset;
                    // Use shared delimiter matcher (pass paren index, not args_start)
                    let Some(args_end) = find_matching_delimiter(
                        text,
                        paren_index,
                        '(',
                        ')',
                        MAX_TEXTUAL_NESTING_DEPTH,
                    ) else {
                        tracing::debug!(
                            parser = "prefixed",
                            reason = "unmatched parentheses",
                            "Rejected prefixed tool call"
                        );
                        search_start = start;
                        continue;
                    };
                    let args_start = paren_index + 1;
                    let raw_args = &text[args_start..args_end];
                    if let Some(args) = parse_textual_arguments(raw_args) {
                        // Return raw name/args; canonicalization happens in detect_textual_tool_call
                        return ParseResult::Success(ParsedToolCall { name, args });
                    }

                    search_start = prefix_index + prefix.len() + name_len;
                } else {
                    break;
                }
            }
        }
        tracing::debug!(
            parser = "prefixed",
            reason = "no matching prefix pattern found",
            "Rejected textual tool call"
        );
        ParseResult::Reject("no matching prefix pattern found")
    }

    fn should_validate_tool_name(&self) -> bool {
        false // Skip validation for prefixed tools (backward compatibility)
    }

    fn find_consumed_spans(&self, text: &str) -> Vec<(usize, usize)> {
        let mut regions = Vec::new();
        for prefix in TEXTUAL_TOOL_PREFIXES {
            collect_prefixed_function_regions(text, prefix, &mut regions);
        }
        regions
    }
}

/// Parser for direct function aliases (e.g., "run(...)", "terminal_cmd(...)").
struct DirectFunctionAliasParser;

impl TextualToolParser for DirectFunctionAliasParser {
    fn name(&self) -> &'static str {
        "direct_alias"
    }

    fn try_parse(&self, text: &str) -> ParseResult {
        detect_direct_function_alias(text)
    }

    fn find_consumed_spans(&self, text: &str) -> Vec<(usize, usize)> {
        let mut regions = Vec::new();
        for alias in DIRECT_FUNCTION_ALIASES {
            collect_direct_function_regions(text, alias, &mut regions);
        }
        regions
    }
}

fn detect_direct_function_alias(text: &str) -> ParseResult {
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
                    tracing::debug!(
                        parser = "direct_alias",
                        alias = %alias,
                        reason = "no opening parenthesis",
                        "Rejected direct function alias"
                    );
                    search_start = end;
                    continue;
                };

                // Use shared delimiter matcher (pass paren index, not args_start)
                let Some(end_pos) =
                    find_matching_delimiter(text, paren_pos, '(', ')', MAX_TEXTUAL_NESTING_DEPTH)
                else {
                    tracing::debug!(
                        parser = "direct_alias",
                        alias = %alias,
                        reason = "unmatched parentheses",
                        "Rejected direct function alias"
                    );
                    search_start = end;
                    continue;
                };

                let args_start = paren_pos + 1;
                let raw_args = &text[args_start..end_pos];
                if let Some(args) = parse_textual_arguments(raw_args) {
                    // Return raw name/args; canonicalization happens in detect_textual_tool_call
                    return ParseResult::Success(ParsedToolCall {
                        name: alias.to_string(),
                        args,
                    });
                }

                search_start = end;
            } else {
                break; // No more matches
            }
        }
    }

    tracing::debug!(
        parser = "direct_alias",
        reason = "no matching alias pattern found",
        "Rejected textual tool call"
    );
    ParseResult::Reject("no matching alias pattern found")
}
