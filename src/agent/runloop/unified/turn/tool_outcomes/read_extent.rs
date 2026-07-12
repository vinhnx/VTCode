//! Canonical vocabulary and helpers for reasoning about a *read extent* — the
//! offset / limit / line-range / pagination fields that scope a file or search
//! read to a sub-range.
//!
//! # Why this module exists
//!
//! The "which argument keys narrow a read" concept was previously duplicated
//! across four sites (`helpers::READ_OFFSET_KEYS`,
//! `response_content::BOUNDED_READ_KEYS`, and `looping`'s
//! `READ_FILE_OFFSET_KEYS`/`READ_FILE_LIMIT_KEYS`). Those lists drifted — `o`,
//! `end_line`, `page_size_lines`, `encoding`, and `page`/`per_page` each
//! appeared in some copies but not others — and two of them carried
//! "keep aligned with …" comments admitting the fragility. This module is the
//! single source of truth; every consumer delegates here so the vocabulary can
//! never drift again.
//!
//! # Interface guard rails
//!
//! The public surface is intentionally small and value-oriented so the logic
//! is testable in isolation from the turn loop:
//! - [`OFFSET_KEYS`] / [`LIMIT_KEYS`] / [`PAGE_KEYS`]: the raw vocabulary.
//! - [`args_have_bounded_extent`]: did the model explicitly narrow the read?
//! - [`normalization_strip_keys`]: keys to strip for cross-turn read dedup.
//! - [`extent_covers`]: does a cached read's range cover a new query's range?

use serde_json::Value;

/// Keys that specify *where* a read starts (byte/line offset or start line).
pub(crate) const OFFSET_KEYS: &[&str] = &[
    "offset",
    "offset_lines",
    "offset_bytes",
    "o",
    "line_start",
    "start_line",
];

/// Keys that specify *how much* a read covers (limit, page size, or end line).
///
/// End-line keys (`line_end`, `end_line`) are absolute end positions rather
/// than counts, but they behave like limits for coverage purposes: a larger
/// end line covers a smaller one at the same offset.
pub(crate) const LIMIT_KEYS: &[&str] = &[
    "limit",
    "limit_lines",
    "page_size_lines",
    "max_lines",
    "chunk_lines",
    "line_end",
    "end_line",
];

/// Pagination keys that scope a read without mapping cleanly to offset/limit.
pub(crate) const PAGE_KEYS: &[&str] = &["page", "per_page"];

/// Keys that never change *what* is read (only how it is decoded) but were
/// historically stripped during read-signature normalization.
const NORMALIZE_ONLY_KEYS: &[&str] = &["encoding"];

/// Every key that scopes a read to a sub-range (offset ∪ limit ∪ page).
pub(crate) fn bounded_extent_keys() -> impl Iterator<Item = &'static str> {
    OFFSET_KEYS
        .iter()
        .chain(LIMIT_KEYS)
        .chain(PAGE_KEYS)
        .copied()
}

/// Keys stripped when normalizing a read's arguments for cross-turn dedup:
/// everything that scopes a read, plus decode-only keys like `encoding`.
///
/// Stripping these makes "the same file read with a different slice" hash to
/// the same signature, so redundant re-reads are recognized.
pub(crate) fn normalization_strip_keys() -> impl Iterator<Item = &'static str> {
    bounded_extent_keys().chain(NORMALIZE_ONLY_KEYS.iter().copied())
}

/// Returns `true` when `args` explicitly narrows the read to a sub-range via
/// any offset/limit/page key. A bounded read is a deliberate request for exact
/// content at a known location.
pub(crate) fn args_have_bounded_extent(args: &Value) -> bool {
    args.as_object()
        .is_some_and(|obj| bounded_extent_keys().any(|key| obj.contains_key(key)))
}

fn as_u64_lenient(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
}

/// First offset value present in `args` under any [`OFFSET_KEYS`] alias, or `0`.
pub(crate) fn extent_offset(args: &Value) -> u64 {
    OFFSET_KEYS
        .iter()
        .find_map(|key| args.get(*key).and_then(as_u64_lenient))
        .unwrap_or(0)
}

/// First limit value present in `args` under any [`LIMIT_KEYS`] alias, if any.
pub(crate) fn extent_limit(args: &Value) -> Option<u64> {
    LIMIT_KEYS
        .iter()
        .find_map(|key| args.get(*key).and_then(as_u64_lenient))
}

fn raw_flag(args: &Value) -> bool {
    args.get("raw").and_then(Value::as_bool).unwrap_or(false)
}

/// Returns `true` when a `cached` read's extent covers a `query`'s extent —
/// same raw mode, same offset, and a cached limit at least as large as the
/// query's (an unbounded cached limit covers any query).
///
/// Understands the full offset/limit alias vocabulary (not just literal
/// `offset`/`limit`), so reads scoped by `start_line`/`end_line` are compared
/// on their true ranges rather than silently treated as identical (which would
/// return the wrong slice's cached content).
pub(crate) fn extent_covers(cached: &Value, query: &Value) -> bool {
    if raw_flag(cached) != raw_flag(query) {
        return false;
    }
    if extent_offset(cached) != extent_offset(query) {
        return false;
    }
    match (extent_limit(cached), extent_limit(query)) {
        (Some(c), Some(q)) => c >= q,
        (None, None) => true,
        // A bounded cached read cannot be guaranteed to cover an unbounded
        // query, and vice versa.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bounded_extent_detects_all_alias_families() {
        assert!(args_have_bounded_extent(&json!({"offset": 10})));
        assert!(args_have_bounded_extent(&json!({"start_line": 1})));
        assert!(args_have_bounded_extent(&json!({"end_line": 40})));
        assert!(args_have_bounded_extent(&json!({"limit": 100})));
        assert!(args_have_bounded_extent(&json!({"page": 2})));
        assert!(args_have_bounded_extent(&json!({"o": 5})));
    }

    #[test]
    fn whole_file_read_is_not_bounded() {
        assert!(!args_have_bounded_extent(
            &json!({"action": "read", "path": "src/lib.rs"})
        ));
        assert!(!args_have_bounded_extent(
            &json!({"action": "read", "path": "src/lib.rs", "encoding": "utf8"})
        ));
    }

    #[test]
    fn normalization_strip_keys_includes_encoding_and_all_extent_keys() {
        let keys: Vec<&str> = normalization_strip_keys().collect();
        assert!(keys.contains(&"encoding"));
        assert!(keys.contains(&"offset"));
        assert!(keys.contains(&"limit"));
        assert!(keys.contains(&"page"));
        assert!(keys.contains(&"start_line"));
    }

    #[test]
    fn extent_offset_reads_aliases_and_defaults_zero() {
        assert_eq!(extent_offset(&json!({})), 0);
        assert_eq!(extent_offset(&json!({"offset": 42})), 42);
        assert_eq!(extent_offset(&json!({"start_line": 7})), 7);
        assert_eq!(extent_offset(&json!({"offset": "13"})), 13);
    }

    #[test]
    fn extent_covers_matches_wider_cached_limit() {
        let cached = json!({"offset": 0, "limit": 500});
        let query = json!({"offset": 0, "limit": 100});
        assert!(extent_covers(&cached, &query));
    }

    #[test]
    fn extent_covers_rejects_different_offset() {
        let cached = json!({"offset": 0, "limit": 500});
        let query = json!({"offset": 100, "limit": 100});
        assert!(!extent_covers(&cached, &query));
    }

    #[test]
    fn extent_covers_rejects_different_line_ranges() {
        // Regression for A1: line-range reads must be compared on their true
        // ranges, not collapsed into "identical" (which returned wrong content).
        let cached = json!({"start_line": 1, "end_line": 40});
        let query = json!({"start_line": 100, "end_line": 140});
        assert!(!extent_covers(&cached, &query));
    }

    #[test]
    fn extent_covers_rejects_raw_mode_mismatch() {
        let cached = json!({"offset": 0, "limit": 500});
        let query = json!({"offset": 0, "limit": 100, "raw": true});
        assert!(!extent_covers(&cached, &query));
    }

    #[test]
    fn extent_covers_unbounded_cached_covers_unbounded_query() {
        assert!(extent_covers(&json!({"offset": 0}), &json!({"offset": 0})));
        assert!(!extent_covers(
            &json!({"offset": 0}),
            &json!({"offset": 0, "limit": 100})
        ));
    }
}
