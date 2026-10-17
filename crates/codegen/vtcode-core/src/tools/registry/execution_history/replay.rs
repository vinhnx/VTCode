//! Successful-result replay, spool lifetimes, and read-range invalidation.
use super::*;
use crate::tools::continuation::read_chunk_progress_from_result;
use crate::tools::output_spooler::SpooledOutputReference;
use std::path::Path;

const READ_OFFSET_KEYS: &[&str] = &[
    "offset",
    "offset_lines",
    "offset_bytes",
    "byte_offset",
    "line_offset",
    "o",
    "line_start",
    "start_line",
];
const READ_LIMIT_KEYS: &[&str] = &[
    "limit",
    "limit_lines",
    "max_bytes",
    "page_size",
    "page_size_bytes",
    "byte_page_size",
    "page_size_lines",
    "line_page_size",
    "max_lines",
    "chunk_lines",
    "line_end",
    "end_line",
    "length",
];
const READ_EXTENT_KEYS: &[&str] = &[
    "offset",
    "offset_lines",
    "offset_bytes",
    "byte_offset",
    "line_offset",
    "o",
    "line_start",
    "start_line",
    "limit",
    "limit_lines",
    "max_bytes",
    "page_size",
    "page_size_bytes",
    "byte_page_size",
    "page_size_lines",
    "line_page_size",
    "max_lines",
    "chunk_lines",
    "line_end",
    "end_line",
    "length",
    "page",
    "per_page",
];

fn spool_reference_is_replayable(result: &Value, workspace_root: &Path) -> bool {
    if result.get("spool_path").is_none() {
        return true;
    }
    SpooledOutputReference::from_value(result)
        .is_some_and(|reference| reference.read_verified_completed(workspace_root).is_ok())
}

/// Whether a TTL replay requires the record to reference a spool file.
///
/// - `RequireSpool`: the caller only wants spool-backed payloads (PTY sessions,
///   large search outputs). Records without `spool_path` are skipped.
/// - `Any`: accept either an inline or spool-backed result, but always validate
///   that the spool file is still on disk when `spool_path` is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplayMode {
    RequireSpool,
    Any,
}

fn read_file_path_from_args(args: &Value) -> Option<&str> {
    let obj = args.as_object()?;
    for key in PATH_ALIAS_KEYS {
        if let Some(path) = obj.get(key).and_then(|v| v.as_str()) {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn normalize_path_for_match(path: &str) -> String {
    path.trim().replace('\\', "/").trim_start_matches("./").to_string()
}

fn to_absolute_path(path: &str) -> Option<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let raw = Path::new(trimmed);
    if raw.is_absolute() {
        return Some(raw.to_path_buf());
    }
    env::current_dir().ok().map(|cwd| cwd.join(raw))
}

fn paths_match(record_path: &str, expected_path: &str) -> bool {
    let lhs = normalize_path_for_match(record_path);
    let rhs = normalize_path_for_match(expected_path);
    if lhs == rhs {
        return true;
    }
    if lhs.ends_with(&format!("/{rhs}")) || rhs.ends_with(&format!("/{lhs}")) {
        return true;
    }

    match (to_absolute_path(record_path), to_absolute_path(expected_path)) {
        (Some(abs_lhs), Some(abs_rhs)) => abs_lhs == abs_rhs,
        _ => false,
    }
}

fn is_read_file_style_record(record: &ToolExecutionRecord) -> bool {
    if is_read_file_tool_name(&record.tool_name) {
        return true;
    }

    if !is_file_operation_tool_name(&record.tool_name) {
        return false;
    }

    tool_intent::file_operation_action_is(&record.args, "read")
}

/// Read-target identity for cross-turn reuse matching: what was read and how
/// the read was scoped. `verbatim` distinguishes raw file-line output (bare
/// `sed -n '<range>p'`) from transformed output (piped stages, awk programs);
/// a verbatim record can only serve a verbatim query with an identical slice,
/// and a transformed record can only serve an identically-transformed one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ExecTargetIdentity {
    path: String,
    start_line: usize,
    end_line: usize,
    verbatim: bool,
}

impl ExecTargetIdentity {
    /// Identity of a command's bounded slice read, when the command parses as
    /// one. Returns `None` for mutating or dynamic shapes.
    fn from_args(args: &Value) -> Option<Self> {
        let target = crate::tools::parse_exec_read_target(args)?;
        Some(Self {
            path: target.path,
            start_line: target.start_line,
            end_line: target.end_line,
            verbatim: target.verbatim,
        })
    }

    /// Serve-ability: a served read must be the same positioned slice with the
    /// same transform class. Superset coverage is deliberately out of scope
    /// for command reads — the command text, not a structured offset, scopes
    /// the read, so nothing here proves a wider command covers a narrower one.
    fn serves(&self, query: &Self) -> bool {
        self.path == query.path
            && self.start_line == query.start_line
            && self.end_line == query.end_line
            && self.verbatim == query.verbatim
    }
}

impl ToolExecutionHistory {
    /// Find the most recent spooled output for a tool call with identical args.
    pub fn find_recent_spooled_result(&self, tool_name: &str, args: &Value, max_age: Duration) -> Option<Value> {
        self.find_recent_matching(tool_name, args, max_age, ReplayMode::RequireSpool)
    }

    /// Find the most recent successful output for a tool call with identical args.
    pub fn find_recent_successful_result(&self, tool_name: &str, args: &Value, max_age: Duration) -> Option<Value> {
        self.find_recent_matching(tool_name, args, max_age, ReplayMode::Any)
    }

    /// Find the most recent successful output for a read-only tool call that
    /// targets the same file path and compatible read shape. This enables
    /// cross-turn dedup only when the cached read covers the new request.
    ///
    /// For command-session tools (`exec_command`, `unified_exec`), only a
    /// read-only-classified command that parses as a bounded slice read
    /// (`sed -n '<range>p' <file>`, `awk '<NR program>' <file>`) participates;
    /// the served read must be the identical slice with the same transform
    /// class (verbatim vs piped). Mutating commands and dynamic shapes never
    /// reuse.
    ///
    /// Returns `None` for non-read-only tools or when no matching path can be
    /// extracted from the args.
    pub fn find_recent_successful_by_read_target(
        &self,
        tool_name: &str,
        query_args: &Value,
        max_age: Duration,
    ) -> Option<Value> {
        if matches!(tool_name, tools::EXEC_COMMAND | tools::UNIFIED_EXEC) {
            return self.find_recent_successful_by_exec_slice(tool_name, query_args, max_age);
        }
        let query_path = Self::extract_read_target(tool_name, query_args)?;
        self.find_recent_matching_with_predicate(tool_name, max_age, ReplayMode::Any, |record| {
            let record_path = Self::extract_read_target(tool_name, &record.args)?;
            if record_path != query_path {
                return None;
            }
            // `code_search` includes every effective search filter and its
            // result limit in `extract_read_target`, so an equal target is
            // already an exact read-shape match. Other read tools use the
            // generic extent check below to allow cached supersets.
            if tool_name != tools::CODE_SEARCH && !Self::read_extent_matches(&record.args, query_args) {
                return None;
            }
            Some(())
        })
    }

    /// Exec-slice variant of [`Self::find_recent_successful_by_read_target`]:
    /// cross-turn reuse for bounded shell reads. Validated by the shared
    /// read-only classification plus per-record intent re-check, so a mutating
    /// command recorded under the same tool name can never be served.
    fn find_recent_successful_by_exec_slice(
        &self,
        tool_name: &str,
        query_args: &Value,
        max_age: Duration,
    ) -> Option<Value> {
        let query_identity = ExecTargetIdentity::from_args(query_args)?;
        // Fail closed: the query itself must be read-only, not just slice-shaped.
        if tool_intent::classify_tool_intent(tool_name, query_args).mutating {
            return None;
        }
        self.find_recent_matching_with_predicate(tool_name, max_age, ReplayMode::Any, |record| {
            let record_identity = ExecTargetIdentity::from_args(&record.args)?;
            if !record_identity.serves(&query_identity) {
                return None;
            }
            // The parsed shape is identical, but the recorded command may
            // still have been mutating (e.g. `sed -i '1,2p;d' file` masks as
            // a slice). A record is servable only when the registry recorded
            // it as a non-mutating call.
            if tool_intent::classify_tool_intent(tool_name, &record.args).mutating {
                return None;
            }
            Some(())
        })
    }

    /// Single source of truth for "find a recent successful record for this
    /// tool call, honoring the spool path lifetime semantics". Replaces the
    /// three near-identical loops that previously diverged on whether spool
    /// was required and how its existence was checked.
    fn find_recent_matching(
        &self,
        tool_name: &str,
        args: &Value,
        max_age: Duration,
        mode: ReplayMode,
    ) -> Option<Value> {
        self.find_recent_matching_with_predicate(tool_name, max_age, mode, |record| {
            (record.args == *args).then_some(())
        })
    }

    fn find_recent_matching_with_predicate(
        &self,
        tool_name: &str,
        max_age: Duration,
        mode: ReplayMode,
        mut matches: impl FnMut(&ToolExecutionRecord) -> Option<()>,
    ) -> Option<Value> {
        let records = self.records.read().unwrap_or_else(|e| e.into_inner());
        let now = SystemTime::now();
        let mut later_mutated_paths = Vec::new();
        let mut later_pathless_mutation = false;

        for record in records.iter().rev() {
            if record.success && tool_intent::classify_tool_intent(&record.tool_name, &record.args).mutating {
                let mutation_paths = crate::tools::mutation_target_paths(&record.tool_name, &record.args);
                later_pathless_mutation |= mutation_paths.is_empty();
                later_mutated_paths.extend(mutation_paths);
            }
            if record.tool_name != tool_name || !record.success {
                continue;
            }
            if matches(record).is_none() {
                continue;
            }

            let age_ok = match now.duration_since(record.timestamp) {
                Ok(age) => age <= max_age,
                Err(_) => false,
            };
            if !age_ok {
                continue;
            }

            if record.tool_name == tools::CODE_SEARCH
                && (later_pathless_mutation
                    || later_mutated_paths.iter().any(|mutated_path| {
                        crate::tools::code_search::scope_contains_mutated_path(
                            &record.args,
                            mutated_path,
                            self.workspace_root.as_ref(),
                        )
                    }))
            {
                continue;
            }

            let Ok(result) = &record.result else {
                continue;
            };

            if result.get("spool_path").is_some() {
                if !spool_reference_is_replayable(result, &self.workspace_root) {
                    continue;
                }
            } else if mode == ReplayMode::RequireSpool {
                continue;
            }

            return Some(result.clone());
        }
        None
    }

    /// Invalidate cache records whose read target overlaps the mutated path.
    /// Used when a write tool modifies a file: only the records that could
    /// contain stale content for that file are dropped, instead of wiping
    /// every cached read-only result.
    ///
    /// Covers both structured file reads (`read_file`, `unified_file`) and
    /// bounded shell slice reads (`sed -n '<range>p' <file>`), because both
    /// participate in cross-turn reuse and both go stale after a mutation of
    /// their target.
    pub fn invalidate_for_path(&self, target_path: &str) {
        let Ok(mut records) = self.records.write() else {
            return;
        };
        records.retain(|record| !Self::record_reads_path(record, target_path).unwrap_or(false));
    }

    /// Return `Some(true)` when `record` is a read of `target_path`
    /// (structured file read or bounded shell slice), `Some(false)` when it is
    /// a read of a different path, and `None` when it is not a path-scoped read
    /// at all. Shared by the path-scoped invalidators so the two read families
    /// cannot drift.
    fn record_reads_path(record: &ToolExecutionRecord, target_path: &str) -> Option<bool> {
        if matches!(record.tool_name.as_str(), tools::READ_FILE | tools::UNIFIED_FILE) {
            let record_path = Self::extract_read_target(&record.tool_name, &record.args)?;
            return Some(record_path == target_path);
        }
        if matches!(record.tool_name.as_str(), tools::EXEC_COMMAND | tools::UNIFIED_EXEC) {
            let identity = ExecTargetIdentity::from_args(&record.args)?;
            return Some(identity.path == target_path);
        }
        None
    }

    /// Whether `record` is any path-scoped read (structured file read or
    /// bounded shell slice) that a pathless mutation must conservatively drop.
    /// Structured file reads are always path-scoped; command records only count
    /// when they parse as a slice read.
    fn record_is_path_scoped_read(record: &ToolExecutionRecord) -> bool {
        if matches!(record.tool_name.as_str(), tools::READ_FILE | tools::UNIFIED_FILE) {
            return true;
        }
        matches!(record.tool_name.as_str(), tools::EXEC_COMMAND | tools::UNIFIED_EXEC)
            && ExecTargetIdentity::from_args(&record.args).is_some()
    }

    /// Conservatively drop every cached read-only result.
    ///
    /// Used when a mutating shell command produces no identifiable target path
    /// (e.g. `sed -i`), so we cannot know which files may now be stale. A
    /// pathless mutation could have touched anything, so no read record can be
    /// trusted afterward. Covers structured file reads and bounded shell slice
    /// reads; other command records (e.g. `cargo build`) are not reads and
    /// survive.
    pub fn invalidate_all_reads(&self) {
        let Ok(mut records) = self.records.write() else {
            return;
        };
        records.retain(|record| !Self::record_is_path_scoped_read(record));
    }

    /// Check whether the cached record's read shape covers the new query's shape.
    ///
    /// Non-range arguments must match exactly. Range aliases are normalized
    /// only after their values are validated, and the cached range must cover
    /// the query range. This prevents replaying a different slice, encoding,
    /// pagination page, or read mode (issue #680).
    fn read_extent_matches(cached_args: &Value, query_args: &Value) -> bool {
        let Some(cached_shape) = Self::read_shape_without_extent(cached_args) else {
            return false;
        };
        let Some(query_shape) = Self::read_shape_without_extent(query_args) else {
            return false;
        };
        if cached_shape != query_shape {
            return false;
        }

        let Ok(cached_offset) = read_extent_value(cached_args, READ_OFFSET_KEYS) else {
            return false;
        };
        let Ok(query_offset) = read_extent_value(query_args, READ_OFFSET_KEYS) else {
            return false;
        };
        if !compatible_extent_values(cached_offset, query_offset, true) {
            return false;
        }

        let Ok(cached_limit) = read_extent_value(cached_args, READ_LIMIT_KEYS) else {
            return false;
        };
        let Ok(query_limit) = read_extent_value(query_args, READ_LIMIT_KEYS) else {
            return false;
        };
        if !compatible_extent_values(cached_limit, query_limit, false) {
            return false;
        }

        let Ok(cached_page) = read_page_extent(cached_args) else {
            return false;
        };
        let Ok(query_page) = read_page_extent(query_args) else {
            return false;
        };
        cached_page == query_page
    }

    fn read_shape_without_extent(args: &Value) -> Option<Value> {
        let mut object = args.as_object()?.clone();
        for key in PATH_ALIAS_KEYS.iter().chain(READ_EXTENT_KEYS.iter()) {
            object.remove(*key);
        }
        Some(Value::Object(object))
    }

    /// Extract the read target from tool args for path-based matching.
    /// Returns `None` for non-read-only tools or when no path is found.
    ///
    /// For search tools, the key includes the normalised query identity so
    /// different searches on the same directory are not treated as duplicates.
    fn extract_read_target(tool_name: &str, args: &Value) -> Option<String> {
        let obj = args.as_object()?;
        let is_read = match tool_name {
            tools::READ_FILE | tools::GREP_FILE | tools::LIST_FILES | tools::CODE_SEARCH => true,
            tools::UNIFIED_FILE => {
                matches!(obj.get("action").and_then(Value::as_str), Some("read"))
            }
            _ => false,
        };
        if !is_read {
            return None;
        }
        if tool_name == tools::CODE_SEARCH {
            return crate::tools::normalised_code_search_identity(args);
        }
        let path = Self::extract_path_from_args(obj)?;
        if tool_name == tools::GREP_FILE {
            let pattern = obj.get("pattern").and_then(Value::as_str).unwrap_or("");
            return Some(format!("{path}::{pattern}"));
        }
        Some(path)
    }

    fn extract_path_from_args(obj: &serde_json::Map<String, Value>) -> Option<String> {
        for key in PATH_ALIAS_KEYS {
            if let Some(path) = obj.get(key).and_then(Value::as_str) {
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
        None
    }
    ///
    /// Supports both `read_file` and `file_operation` read action records.
    ///
    /// Returns `(next_offset, chunk_limit)` when the recent call indicates more chunks are
    /// available (`spool_chunked=true`, `has_more=true`).
    pub fn find_recent_read_file_spool_progress(&self, path: &str, max_age: Duration) -> Option<(usize, usize)> {
        let records = self.records.read().unwrap_or_else(|e| e.into_inner());
        let now = SystemTime::now();
        let expected_path = path.trim();

        for record in records.iter().rev() {
            if !record.success || !is_read_file_style_record(record) {
                continue;
            }

            let Some(record_path) = read_file_path_from_args(&record.args) else {
                continue;
            };
            if !paths_match(record_path, expected_path) {
                continue;
            }

            let age_ok = match now.duration_since(record.timestamp) {
                Ok(age) => age <= max_age,
                Err(_) => false,
            };
            if !age_ok {
                continue;
            }

            let Ok(result) = &record.result else {
                continue;
            };
            let chunked = result.get("spool_chunked").and_then(|v| v.as_bool()).unwrap_or(false);
            let has_more = result.get("has_more").and_then(|v| v.as_bool()).unwrap_or(false);
            if !(chunked && has_more) {
                continue;
            }

            if let Some(progress) = read_chunk_progress_from_result(result) {
                return Some(progress);
            }
        }
        None
    }
}

fn read_extent_value(args: &Value, keys: &[&'static str]) -> Result<Option<(&'static str, u64)>, ()> {
    let object = args.as_object().ok_or(())?;
    let mut found = None;
    for key in keys {
        let Some(value) = object.get(*key) else {
            continue;
        };
        let value = value
            .as_u64()
            .or_else(|| value.as_str().and_then(|value| value.trim().parse::<u64>().ok()))
            .ok_or(())?;
        if found.is_some() {
            return Err(());
        }
        found = Some((*key, value));
    }
    Ok(found)
}

fn compatible_extent_values(
    cached: Option<(&'static str, u64)>,
    query: Option<(&'static str, u64)>,
    default_zero_is_compatible: bool,
) -> bool {
    match (cached, query) {
        (Some((cached_key, cached_value)), Some((query_key, query_value))) => {
            cached_key == query_key
                && if default_zero_is_compatible {
                    cached_value == query_value
                } else {
                    cached_value >= query_value
                }
        }
        (None, None) => true,
        (Some((_, value)), None) if default_zero_is_compatible => value == 0,
        (None, Some((_, value))) if default_zero_is_compatible => value == 0,
        _ => false,
    }
}

fn read_page_extent(args: &Value) -> Result<(Option<u64>, Option<u64>), ()> {
    let page = read_extent_value(args, &["page"])?.map(|(_, value)| value);
    let per_page = read_extent_value(args, &["per_page"])?.map(|(_, value)| value);
    Ok((page, per_page))
}
