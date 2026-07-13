use serde_json::{Value, json};
use std::time::Duration;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::tools::tool_intent;

use crate::agent::runloop::unified::tool_reads::{is_read_file_style_call, read_file_path_arg};

pub(super) use crate::agent::runloop::unified::tool_reads::spool_chunk_read_path;

// Read-offset / read-limit field aliases are the canonical vocabulary owned by
// `tool_outcomes::read_extent` — the single source of truth shared with the
// cross-turn dedup normalizer and the summarization gate. The family-cap slice
// suffix distinguishes `off=` from `lim=`, so it consumes the two lists
// separately. Previously each site kept its own drifting copy; delegating here
// guarantees they can never diverge (which used to falsely collapse paginated
// reads into one family key and deadlock the agent on large files).
use crate::agent::runloop::unified::turn::tool_outcomes::read_extent::{LIMIT_KEYS, OFFSET_KEYS};

fn compact_loop_key_part(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect()
}

fn compact_loop_text(value: &str, max_chars: usize) -> String {
    let collapsed = value.split_whitespace().fold(String::new(), |mut acc, s| {
        if !acc.is_empty() {
            acc.push(' ');
        }
        acc.push_str(s);
        acc
    });
    compact_loop_key_part(&collapsed, max_chars)
}

fn normalize_shell_command_text(value: &str, max_chars: usize) -> String {
    compact_loop_text(
        &value
            .chars()
            .filter(|ch| !matches!(ch, '\'' | '"'))
            .collect::<String>(),
        max_chars,
    )
}

fn normalized_shell_command_arg(args: &Value, max_chars: usize) -> Option<String> {
    vtcode_core::tools::command_args::command_text(args)
        .ok()
        .flatten()
        .map(|command| normalize_shell_command_text(&command, max_chars))
        .filter(|command| !command.is_empty())
}

fn search_dispatch_globs_arg(args: &Value) -> Option<String> {
    let globs = args.get("globs")?;
    match globs {
        Value::String(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(compact_loop_text(trimmed, 120))
            }
        }
        Value::Array(items) => {
            let joined = items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join(",");
            if joined.is_empty() {
                None
            } else {
                Some(compact_loop_text(&joined, 120))
            }
        }
        _ => None,
    }
}

fn first_arg_value_by_keys<'a>(args: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| args.get(*key))
}

fn has_any_arg_by_keys(args: &Value, keys: &[&str]) -> bool {
    first_arg_value_by_keys(args, keys).is_some()
}

fn read_file_has_offset_arg(args: &Value) -> bool {
    has_any_arg_by_keys(args, OFFSET_KEYS)
}

fn read_file_offset_value(args: &Value) -> Option<usize> {
    first_arg_value_by_keys(args, OFFSET_KEYS).and_then(|value| {
        value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .or_else(|| value.as_str().and_then(|s| s.parse::<usize>().ok()))
    })
}

fn read_file_has_limit_arg(args: &Value) -> bool {
    has_any_arg_by_keys(args, LIMIT_KEYS)
}

fn read_file_limit_value(args: &Value) -> Option<usize> {
    first_arg_value_by_keys(args, LIMIT_KEYS).and_then(|value| {
        value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .or_else(|| value.as_str().and_then(|s| s.parse::<usize>().ok()))
    })
}

/// Read-only flag. `unified_file`/`read_file` honor `raw` (bypass LLM
/// summarization). Two reads of the same path + same slice but different `raw`
/// modes return *different* payloads, so they must not be treated as the same
/// family call. `true`/`false`/absent are all distinct suffixes.
fn read_file_raw_flag(args: &Value) -> Option<bool> {
    args.get("raw").and_then(Value::as_bool)
}

/// Build a slice descriptor for a read-file call so the family-cap can
/// distinguish paginated reads of the same path.
///
/// The cap exists to stop true retry loops (same path + same slice, repeated
/// verbatim). When the model paginates — same path, different `offset`/`limit`
/// — or flips `raw` to bypass summarization, those are *different* logical
/// reads, not retries. Without this suffix, four reads of one large file with
/// four different `offset`/`limit` pairs all collapse into one family key and
/// trip the cap at 4, even though no slice was read twice. That forces a
/// tool-free recovery pass that produces a garbage final answer.
///
/// The suffix includes only fields that change *what* is read:
/// - `offset`/`offset_lines`/`offset_bytes` -> `off=<n>`
/// - `limit`/`page_size_lines`/`max_lines`/`chunk_lines` -> `lim=<n>`
/// - `raw` -> `raw=<bool>`
///
/// Fields that are absent contribute nothing, so a bare default read
/// (`{path}` with no offset/limit/raw) still produces the same key it did
/// before — only paginated/raw-flipped reads gain suffixes.
fn read_file_slice_suffix(args: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(offset) = read_file_offset_value(args) {
        parts.push(format!("off={offset}"));
    }
    if let Some(limit) = read_file_limit_value(args) {
        parts.push(format!("lim={limit}"));
    }
    if let Some(raw) = read_file_raw_flag(args) {
        parts.push(format!("raw={raw}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("::{}", parts.join("::"))
    }
}

pub(super) fn shell_run_signature(canonical_tool_name: &str, args: &Value) -> Option<String> {
    if !tool_intent::is_command_run_tool_call(canonical_tool_name, args) {
        return None;
    }

    let command = normalized_shell_command_arg(args, 200)?;
    Some(format!("{}::{}", tool_names::UNIFIED_EXEC, command))
}

pub(super) fn maybe_apply_spool_read_offset_hint(
    tool_registry: &mut ToolRegistry,
    canonical_tool_name: &str,
    args: &Value,
) -> Value {
    if !is_read_file_style_call(canonical_tool_name, args) {
        return args.clone();
    }

    let Some(path) = spool_chunk_read_path(canonical_tool_name, args) else {
        return args.clone();
    };

    let Some((next_offset, chunk_limit)) =
        tool_registry.find_recent_read_file_spool_progress(path, Duration::from_secs(180))
    else {
        return args.clone();
    };

    let requested_offset = read_file_offset_value(args);
    let should_advance_offset = match requested_offset {
        Some(existing) => existing < next_offset,
        None => true,
    };
    let should_fill_offset = !read_file_has_offset_arg(args);

    let mut adjusted = args.clone();
    let keep_existing_limit = read_file_has_limit_arg(&adjusted);
    if let Some(obj) = adjusted.as_object_mut() {
        if should_fill_offset || should_advance_offset {
            obj.insert("offset".to_string(), json!(next_offset));
        }
        if !keep_existing_limit {
            obj.insert("limit".to_string(), json!(chunk_limit));
        }
        if should_fill_offset || should_advance_offset || !keep_existing_limit {
            tracing::debug!(
                tool = canonical_tool_name,
                path = path,
                requested_offset = requested_offset.unwrap_or(0),
                next_offset,
                chunk_limit,
                "Applied spool read continuation hint to avoid repeated identical chunk reads"
            );
        }
    }
    adjusted
}

pub(super) fn task_tracker_create_signature(tool_name: &str, args: &Value) -> Option<String> {
    if tool_name != tool_names::TASK_TRACKER {
        return None;
    }

    let action = args.get("action").and_then(Value::as_str)?;
    if action != "create" {
        return None;
    }

    #[derive(serde::Serialize)]
    struct TaskTrackerCreateSignature<'a> {
        title: Option<&'a Value>,
        items: Option<&'a Value>,
        notes: Option<&'a Value>,
    }

    let payload = TaskTrackerCreateSignature {
        title: args.get("title"),
        items: args.get("items"),
        notes: args.get("notes"),
    };
    let payload_str = serde_json::to_string(&payload).ok()?;
    let mut signature = String::with_capacity("task_tracker::create::".len() + payload_str.len());
    signature.push_str("task_tracker::create::");
    signature.push_str(&payload_str);

    Some(signature)
}
pub(crate) fn low_signal_family_key(canonical_tool_name: &str, args: &Value) -> Option<String> {
    match canonical_tool_name {
        tool_names::READ_FILE => read_file_path_arg(args).map(|path| {
            format!(
                "{canonical_tool_name}::{}{}",
                compact_loop_key_part(path, 120),
                read_file_slice_suffix(args),
            )
        }),
        tool_names::UNIFIED_FILE => {
            let action = tool_intent::file_operation_action(args).unwrap_or("read");
            if !action.eq_ignore_ascii_case("read") {
                return None;
            }
            read_file_path_arg(args).map(|path| {
                format!(
                    "{canonical_tool_name}::read::{}{}",
                    compact_loop_key_part(path, 120),
                    read_file_slice_suffix(args),
                )
            })
        }
        tool_names::UNIFIED_EXEC => normalized_shell_command_arg(args, 160)
            .map(|command| format!("{canonical_tool_name}::run::{command}")),
        tool_names::UNIFIED_SEARCH => {
            let normalized = tool_intent::normalize_search_dispatch_args(args);
            let action = tool_intent::search_dispatch_action(&normalized).unwrap_or("grep");
            let mut key = format!("{canonical_tool_name}::{action}");
            // Include pattern for grep/structural so different searches on the same
            // path are tracked separately (avoids false-positive family cap violations).
            if matches!(action, "grep" | "structural") {
                if let Some(pattern) = normalized
                    .get("pattern")
                    .and_then(Value::as_str)
                    .map(|p| compact_loop_text(p, 80))
                    .filter(|p| !p.is_empty())
                {
                    key.push_str("::pat=");
                    key.push_str(&pattern);
                }
            }
            if let Some(globs) = search_dispatch_globs_arg(&normalized) {
                key.push_str("::globs=");
                key.push_str(&globs);
            } else {
                let path = normalized
                    .get("path")
                    .and_then(Value::as_str)
                    .map(|value| compact_loop_key_part(value, 120))
                    .unwrap_or_else(|| ".".to_string());
                key.push_str("::");
                key.push_str(&path);
            }
            Some(key)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        read_file_has_limit_arg, read_file_has_offset_arg, read_file_limit_value,
        read_file_offset_value, read_file_raw_flag, read_file_slice_suffix,
    };
    use crate::agent::runloop::unified::turn::tool_outcomes::handlers::low_signal_family_key;
    use serde_json::json;
    use vtcode_core::config::constants::tools as tool_names;

    #[test]
    fn read_file_offset_value_accepts_alias_keys() {
        assert_eq!(read_file_offset_value(&json!({"offset": 7})), Some(7));
        assert_eq!(
            read_file_offset_value(&json!({"offset_lines": "8"})),
            Some(8)
        );
        assert_eq!(read_file_offset_value(&json!({"offset_bytes": 9})), Some(9));
    }

    #[test]
    fn read_file_has_offset_arg_accepts_alias_keys() {
        assert!(read_file_has_offset_arg(&json!({"offset_lines": 1})));
        assert!(read_file_has_offset_arg(&json!({"offset_bytes": 1})));
        assert!(!read_file_has_offset_arg(&json!({"path": "src/main.rs"})));
    }

    #[test]
    fn read_file_has_limit_arg_accepts_alias_keys() {
        assert!(read_file_has_limit_arg(&json!({"limit": 10})));
        assert!(read_file_has_limit_arg(&json!({"page_size_lines": 10})));
        assert!(read_file_has_limit_arg(&json!({"max_lines": 10})));
        assert!(read_file_has_limit_arg(&json!({"chunk_lines": 10})));
        assert!(!read_file_has_limit_arg(&json!({"path": "src/main.rs"})));
    }

    #[test]
    fn read_file_limit_value_accepts_alias_keys() {
        assert_eq!(read_file_limit_value(&json!({"limit": 10})), Some(10));
        assert_eq!(
            read_file_limit_value(&json!({"page_size_lines": "20"})),
            Some(20)
        );
        assert_eq!(read_file_limit_value(&json!({"max_lines": 5})), Some(5));
        assert_eq!(read_file_limit_value(&json!({"chunk_lines": 3})), Some(3));
        assert_eq!(read_file_limit_value(&json!({"path": "src/main.rs"})), None);
    }

    #[test]
    fn read_file_raw_flag_reads_optional_bool() {
        assert_eq!(read_file_raw_flag(&json!({"raw": true})), Some(true));
        assert_eq!(read_file_raw_flag(&json!({"raw": false})), Some(false));
        assert_eq!(read_file_raw_flag(&json!({"path": "x"})), None);
    }

    #[test]
    fn read_file_slice_suffix_is_empty_when_unpaginated() {
        // A bare read with no offset/limit/raw must keep the legacy key
        // unchanged so true retry loops (same path, no slice) still collide.
        assert_eq!(read_file_slice_suffix(&json!({"path": "src/lib.rs"})), "");
    }

    #[test]
    fn read_file_slice_suffix_distinguishes_offsets() {
        let off0 = read_file_slice_suffix(&json!({"path": "x", "offset": 0}));
        let off80 = read_file_slice_suffix(&json!({"path": "x", "offset": 80}));
        assert_eq!(off0, "::off=0");
        assert_eq!(off80, "::off=80");
        assert_ne!(off0, off80);
    }

    #[test]
    fn read_file_slice_suffix_distinguishes_limits() {
        let lim100 = read_file_slice_suffix(&json!({"path": "x", "limit": 100}));
        let lim200 = read_file_slice_suffix(&json!({"path": "x", "limit": 200}));
        assert_eq!(lim100, "::lim=100");
        assert_eq!(lim200, "::lim=200");
        assert_ne!(lim100, lim200);
    }

    #[test]
    fn read_file_slice_suffix_distinguishes_raw_flag() {
        let no_raw = read_file_slice_suffix(&json!({"path": "x"}));
        let raw_true = read_file_slice_suffix(&json!({"path": "x", "raw": true}));
        let raw_false = read_file_slice_suffix(&json!({"path": "x", "raw": false}));
        assert_eq!(no_raw, "");
        assert_eq!(raw_true, "::raw=true");
        assert_eq!(raw_false, "::raw=false");
        assert_ne!(raw_true, raw_false);
        assert_ne!(raw_true, no_raw);
    }

    #[test]
    fn read_file_slice_suffix_combines_all_present_fields() {
        let suffix = read_file_slice_suffix(&json!({
            "path": "x",
            "offset": 80,
            "limit": 200,
            "raw": true
        }));
        assert_eq!(suffix, "::off=80::lim=200::raw=true");
    }

    #[test]
    fn low_signal_family_key_distinguishes_paginated_reads_of_same_path() {
        // Reproduces turn_613: four reads of the same file with different
        // offset/limit/raw must produce four distinct family keys, so the
        // per-turn family cap does not trip on legitimate pagination.
        let base = low_signal_family_key(
            tool_names::UNIFIED_FILE,
            &json!({"action": "read", "path": "src/cli/update.rs"}),
        );
        let off81 = low_signal_family_key(
            tool_names::UNIFIED_FILE,
            &json!({"action": "read", "path": "src/cli/update.rs", "offset": 81, "limit": 229}),
        );
        let off80 = low_signal_family_key(
            tool_names::UNIFIED_FILE,
            &json!({"action": "read", "path": "src/cli/update.rs", "offset": 80, "limit": 200}),
        );
        let raw = low_signal_family_key(
            tool_names::UNIFIED_FILE,
            &json!({
                "action": "read",
                "path": "src/cli/update.rs",
                "offset": 80,
                "limit": 200,
                "raw": true
            }),
        );

        let keys = [base.clone(), off81.clone(), off80.clone(), raw.clone()];
        let unique: std::collections::HashSet<_> = keys.iter().cloned().collect();
        assert_eq!(
            unique.len(),
            4,
            "paginated reads must have distinct family keys, got: {keys:?}"
        );

        // Sanity: the bare read keeps the legacy key (no slice suffix).
        assert_eq!(
            base.as_deref(),
            Some("unified_file::read::src/cli/update.rs")
        );
        assert!(off81.unwrap().ends_with("::off=81::lim=229"));
        assert!(off80.unwrap().ends_with("::off=80::lim=200"));
        assert!(raw.unwrap().ends_with("::off=80::lim=200::raw=true"));
    }

    #[test]
    fn low_signal_family_key_collides_for_identical_slice_retry() {
        // True retry loop: same path + same slice must still collide so the
        // cap can stop it. This is the guard's reason for existing.
        let first = low_signal_family_key(
            tool_names::UNIFIED_FILE,
            &json!({
                "action": "read",
                "path": "src/lib.rs",
                "offset": 0,
                "limit": 100
            }),
        );
        let second = low_signal_family_key(
            tool_names::UNIFIED_FILE,
            &json!({
                "action": "read",
                "path": "src/lib.rs",
                "offset": 0,
                "limit": 100
            }),
        );
        assert_eq!(
            first, second,
            "identical slice retries must share a family key"
        );
    }

    #[test]
    fn low_signal_family_key_read_file_distinguishes_paginated_reads() {
        // read_file (not unified_file) must also be slice-aware.
        let off0 = low_signal_family_key(
            tool_names::READ_FILE,
            &json!({"path": "src/lib.rs", "offset": 0}),
        );
        let off80 = low_signal_family_key(
            tool_names::READ_FILE,
            &json!({"path": "src/lib.rs", "offset": 80}),
        );
        assert_ne!(off0, off80, "different offsets must produce different keys");
        assert!(off0.unwrap().ends_with("::off=0"));
        assert!(off80.unwrap().ends_with("::off=80"));
    }
}
