//! CSS-variable inlining for native SVG rasterizers.
//!
//! Pele emits `var(--pele-*)` references that browsers resolve at view time.
//! Consumers that rasterize without a CSS engine (the desktop inline previews)
//! substitute the references up front against their own theme map with
//! [`inline_css_vars`].

use std::collections::{HashMap, HashSet};

/// Substitute `var(--name[, fallback])` inside tag markup only (never in text
/// nodes, where a hostile label could carry a literal `var(`). Theme wins;
/// document `--*` definitions resolve as aliases with a fixpoint cap.
/// Returns the rewritten SVG plus the count of references left unresolved
/// (unknown names without fallbacks, kept literally).
pub fn inline_css_vars(svg: &str, theme: &HashMap<String, String>) -> (String, usize) {
    let defs = collect_custom_props(svg);
    let mut unresolved = 0;
    let mut out = String::with_capacity(svg.len());
    for (in_tag, segment) in split_markup(svg) {
        if in_tag {
            out.push_str(&substitute_vars(segment, theme, &defs, &mut unresolved));
        } else {
            out.push_str(segment);
        }
    }
    (out, unresolved)
}

/// Split SVG into `(in_tag, segment)` runs. Quote-aware: `<`/`>` inside
/// single or double quotes never toggle the mode, so attribute values like
/// `title="a>b"` cannot desynchronize the scanner. Every cut sits on an
/// ASCII delimiter just matched, so `get` only fails if the offset math
/// drifts; breaking early then truncates the scan instead of panicking, and
/// the downstream SVG parse fails closed on the partial document.
fn split_markup(svg: &str) -> Vec<(bool, &str)> {
    let bytes = svg.as_bytes();
    let mut segments = Vec::new();
    let mut index = 0;
    let mut segment_start = 0;
    let mut in_tag = false;
    let mut quote: Option<u8> = None;
    while let Some(byte) = bytes.get(index).copied() {
        if in_tag {
            match byte {
                b'\'' | b'"' if quote.is_none() => quote = Some(byte),
                q if Some(q) == quote => quote = None,
                b'>' if quote.is_none() => {
                    let Some(segment) = svg.get(segment_start..=index) else {
                        break;
                    };
                    segments.push((true, segment));
                    index += 1;
                    segment_start = index;
                    in_tag = false;
                    continue;
                }
                _ => {}
            }
        } else if byte == b'<' {
            if segment_start < index {
                let Some(segment) = svg.get(segment_start..index) else {
                    break;
                };
                segments.push((false, segment));
            }
            segment_start = index;
            in_tag = true;
        }
        index += 1;
    }
    if segment_start < bytes.len()
        && let Some(rest) = svg.get(segment_start..)
    {
        segments.push((in_tag, rest));
    }
    segments
}

/// Collect `--name: value;` definitions (paren-aware) from tag markup only.
/// Text nodes are excluded: a hostile label containing `--x: y;` must not
/// poison the definition map.
fn collect_custom_props(svg: &str) -> HashMap<String, String> {
    let mut defs = HashMap::new();
    for (in_tag, segment) in split_markup(svg) {
        if in_tag {
            collect_from_tag(segment, &mut defs);
        }
    }
    defs
}

fn collect_from_tag(tag: &str, defs: &mut HashMap<String, String>) {
    let bytes = tag.as_bytes();
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes.get(index..index + 2) != Some(b"--".as_slice()) {
            index += 1;
            continue;
        }
        let name_start = index;
        let mut cursor = index + 2;
        while let Some(byte) = bytes.get(cursor)
            && (byte.is_ascii_alphanumeric() || *byte == b'-' || *byte == b'_')
        {
            cursor += 1;
        }
        let (Some(name), is_definition) = (tag.get(name_start..cursor), bytes.get(cursor) == Some(&b':')) else {
            // Truly unreachable: both cuts sit on ASCII just matched. Skip
            // forward instead of panicking so malformed input degrades.
            index += 1;
            continue;
        };
        if !is_definition {
            index += 1;
            continue;
        }
        cursor += 1;
        let value_start = cursor;
        let mut depth = 0;
        while let Some(byte) = bytes.get(cursor).copied() {
            match byte {
                b'(' => depth += 1,
                // An unbalanced `)` in malformed input is kept
                // literally instead of underflowing the counter.
                b')' if depth > 0 => depth -= 1,
                b';' if depth == 0 => break,
                _ => {}
            }
            cursor += 1;
        }
        if let Some(value) = tag.get(value_start..cursor) {
            let _previous = defs.insert(name.to_string(), value.trim().to_string());
        }
        // `cursor` always advanced past the name and colon, so this makes
        // progress every iteration.
        index = cursor;
    }
}

/// Maximum alias-chase rounds. Pele emits one alias layer (`--_*` over
/// `--pele-*`); the cap only binds adversarial cycles, which then surface
/// as unresolved instead of hanging the worker.
const INLINE_ROUNDS: usize = 10;

/// One substitution round over tag text, repeated to a fixpoint cap.
fn substitute_vars(
    text: &str,
    theme: &HashMap<String, String>,
    defs: &HashMap<String, String>,
    unresolved: &mut usize,
) -> String {
    // Names, not occurrences: a reference unresolvable in one round stays
    // unresolvable (the theme and definitions never change mid-fixpoint), so
    // recounting it every round would inflate the total.
    let mut unseen = HashSet::new();
    let mut current = text.to_string();
    for _ in 0..INLINE_ROUNDS {
        if !current.contains("var(") {
            break;
        }
        let mut next = String::with_capacity(current.len());
        let mut rest = current.as_str();
        let mut changed = false;
        while let Some(start) = rest.find("var(") {
            // Both cuts sit on the `find` match (ASCII), so `get` only fails
            // on impossible offset drift; stopping the round then keeps the
            // remaining text literal instead of panicking.
            let (Some(before), Some(after)) =
                (rest.get(..start), rest.get(start..).and_then(|tail| tail.strip_prefix("var")))
            else {
                break;
            };
            next.push_str(before);
            match parse_var_call(after) {
                Some((name, fallback, len)) => {
                    let replacement = theme.get(name).or_else(|| defs.get(name)).map(String::as_str).or(fallback);
                    match replacement {
                        Some(value) => {
                            next.push_str(value);
                            changed = true;
                        }
                        None => {
                            if unseen.insert(name.to_string()) {
                                *unresolved += 1;
                            }
                            next.push_str("var");
                            if let Some(call) = after.get(..len) {
                                next.push_str(call);
                            }
                        }
                    }
                    let Some(tail) = after.get(len..) else {
                        break;
                    };
                    rest = tail;
                }
                None => {
                    next.push_str("var");
                    rest = after;
                }
            }
        }
        next.push_str(rest);
        current = next;
        if !changed {
            break;
        }
    }
    current
}

/// Parse `(name[, fallback])` with paren matching; returns the name, the
/// optional fallback, and the consumed length. Commas and parens nest, so
/// only a top-level comma splits and only the matching close ends the call.
fn parse_var_call(after_var: &str) -> Option<(&str, Option<&str>, usize)> {
    let inner = after_var.strip_prefix('(')?;
    let mut depth = 1;
    let mut split = None;
    for (offset, byte) in inner.bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    let name = inner.get(..split.unwrap_or(offset))?;
                    let fallback = match split {
                        Some(comma) => Some(inner.get(comma + 1..offset)?.trim()),
                        None => None,
                    };
                    return Some((name.trim(), fallback, offset + 2));
                }
            }
            b',' if depth == 1 && split.is_none() => split = Some(offset),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests;
