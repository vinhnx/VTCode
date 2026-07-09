//! `search_dispatch action=outline` -- wraps `ast-grep outline` to produce a
//! cheap, token-efficient symbol map of a file or directory without requiring
//! a structural pattern.
//!
//! Coexists with `structural` (pattern-based rich records) and `grep` (text).
//! `outline` answers "what's here?"; `structural` answers "find pattern matches".
//!
//! Shells out to the resolved `ast-grep` binary (same path resolution as
//! `structural_search`). Always invokes `ast-grep outline --json=stream` and
//! shapes the per-file NDJSON records in Rust according to the requested
//! `view` (`digest` | `names` | `full`). The CLI's own `--view` flag is
//! text-only and is never passed.

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use tokio::process::Command;

use crate::tools::ast_grep_installer::AstGrepStatus;
use crate::tools::structural_search::stderr_or_stdout;
use crate::utils::path::resolve_workspace_path;

const SUPPORTED_ITEMS: &[&str] = &["auto", "structure", "exports", "imports", "all"];
const SUPPORTED_VIEWS: &[&str] = &["digest", "names", "full"];

/// Threshold for auto-downgrading `view: "full"` to `view: "names"` on
/// directory queries.  `full` view emits per-symbol records with
/// ranges/signatures/members — for a large directory this produces massive
/// output that gets spooled and truncated, forcing the agent to retry.
/// See checkpoint turn_586 (70-file directory, `view: "full"` → truncated
/// → retry → tools disabled → no final answer).
const LARGE_DIR_FULL_VIEW_THRESHOLD: usize = 20;

/// Cap on the number of entries emitted in the directory `summary.all_symbols`
/// array. Beyond this the array is truncated and `summary.truncated` /
/// `summary.visible_symbols` are set so the agent knows to narrow with `type`
/// or `match`, or outline a specific file, rather than relying on an
/// incomplete list — preventing the token-bloat → truncation → retry loop
/// that the `view` auto-downgrade already guards against.
const MAX_SUMMARY_SYMBOLS: usize = 200;

/// Hint emitted when `format` is passed to outline (it's a structural
/// scan-only field, not used by outline).
const HINT_FORMAT_IGNORED: &str = "Parameter `format` is not used by outline (it is a structural scan-only field). It was ignored.";

/// Hint emitted when `max_results` is passed to outline (outline doesn't
/// paginate; use `type` or `match` to filter instead).
const HINT_MAX_RESULTS_IGNORED: &str = "Parameter `max_results` is not used by outline. It was \
    ignored. Use `type` to filter symbol kinds or `match` to filter by name regex.";

/// Output shape applied in Rust after parsing the ast-grep JSON stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutlineView {
    Digest,
    Names,
    Full,
}

impl OutlineView {
    fn parse(value: Option<&str>) -> Result<Self> {
        match value.map(str::trim).filter(|s| !s.is_empty()) {
            None | Some("digest") => Ok(Self::Digest),
            Some("names") => Ok(Self::Names),
            Some("full") => Ok(Self::Full),
            Some(other) => bail!(
                "action='outline' `view` must be one of {} (got \"{other}\")",
                SUPPORTED_VIEWS.join(", "),
            ),
        }
    }
}

/// Maps to the `--items` CLI flag selecting which top-level symbols to include.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutlineItems {
    Auto,
    Structure,
    Exports,
    Imports,
    All,
}

impl OutlineItems {
    fn parse(value: Option<&str>) -> Result<Self> {
        match value.map(str::trim).filter(|s| !s.is_empty()) {
            None | Some("auto") => Ok(Self::Auto),
            Some("structure") => Ok(Self::Structure),
            Some("exports") => Ok(Self::Exports),
            Some("imports") => Ok(Self::Imports),
            Some("all") => Ok(Self::All),
            Some(other) => bail!(
                "action='outline' `items` must be one of {} (got \"{other}\")",
                SUPPORTED_ITEMS.join(", "),
            ),
        }
    }

    fn as_arg(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Structure => "structure",
            Self::Exports => "exports",
            Self::Imports => "imports",
            Self::All => "all",
        }
    }
}

#[derive(Debug)]
struct OutlineRequest {
    path: String,
    lang: Option<String>,
    /// Comma-joined symbol types for `--type`.
    type_filter: Option<String>,
    match_regex: Option<String>,
    items: OutlineItems,
    pub_members: bool,
    follow: bool,
    view: OutlineView,
}

impl OutlineRequest {
    fn from_args(args: &Value) -> Result<Self> {
        let obj = args
            .as_object()
            .ok_or_else(|| anyhow!("action='outline' expects an arguments object"))?;

        let path = get_string_field(obj, "path")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| ".".to_string());

        let lang = get_string_field(obj, "lang")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let type_filter = get_string_or_array_field(obj, "type")?;
        let match_regex = get_string_field(obj, "match")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let items = OutlineItems::parse(get_string_field(obj, "items"))?;
        let pub_members = get_bool_field(obj, "pub_members").unwrap_or(false);
        let follow = get_bool_field(obj, "follow").unwrap_or(false);
        let view = OutlineView::parse(get_string_field(obj, "view"))?;

        Ok(Self {
            path,
            lang,
            type_filter,
            match_regex,
            items,
            pub_members,
            follow,
            view,
        })
    }
}

fn get_string_field<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    let hyphenated = key.replace('_', "-");
    obj.get(key)
        .and_then(|v| v.as_str())
        .or_else(|| obj.get(&hyphenated).and_then(|v| v.as_str()))
}

fn get_bool_field(obj: &Map<String, Value>, key: &str) -> Option<bool> {
    obj.get(key)
        .and_then(|v| v.as_bool())
        .or_else(|| obj.get(&key.replace('_', "-")).and_then(|v| v.as_bool()))
}

/// Accept `type` as a string ("function") or array (["function","struct"]) and
/// join into the comma-separated form ast-grep's `--type` flag expects.
fn get_string_or_array_field(obj: &Map<String, Value>, key: &str) -> Result<Option<String>> {
    let raw = obj.get(key).or_else(|| obj.get(&key.replace('_', "-")));
    let Some(value) = raw else {
        return Ok(None);
    };
    match value {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        Value::Array(items) => {
            let joined: Vec<&str> = items
                .iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            if joined.is_empty() {
                Ok(None)
            } else {
                Ok(Some(joined.join(",")))
            }
        }
        _ => bail!("action='outline' `{key}` must be a string or array of strings"),
    }
}

/// Whether the caller explicitly passed a non-empty `view` parameter.
/// Matches `OutlineView::parse`'s empty-string handling: `view: ""` is
/// treated as "not explicit" so directory auto-tuning still applies.
fn view_is_explicit(args: &Value) -> bool {
    args.as_object()
        .and_then(|obj| obj.get("view"))
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
}

/// Resolve the outline search path within the workspace, with a tolerant
/// fallback: when the path ends with a source extension (`.rs`, `.py`, etc.)
/// but doesn't exist as a file, retry without the extension — the agent often
/// passes `path: "foo/bar.rs"` when `foo/bar` is actually a directory
/// (checkpoint turn_595/turn_597).
fn resolve_outline_path(workspace_root: &Path, request_path: &str) -> Result<std::path::PathBuf> {
    resolve_workspace_path(workspace_root, Path::new(request_path))
        .or_else(|_| {
            if has_source_extension(request_path) {
                let stem = strip_extension(request_path);
                resolve_workspace_path(workspace_root, Path::new(&stem))
            } else {
                Err(anyhow!("Failed to resolve outline path: {request_path}"))
            }
        })
        .with_context(|| format!("Failed to resolve outline path: {request_path}"))
}

/// Collect hints for unrecognized parameters so the agent gets feedback
/// instead of silently dropped fields. Returns hints for `format`,
/// `max_results`, and any grep/structural-only filtering params.
fn collect_outline_hints(args: &Value) -> Vec<String> {
    let mut hints: Vec<String> = Vec::new();
    let Some(obj) = args.as_object() else {
        return hints;
    };
    if obj.get("format").is_some() {
        hints.push(HINT_FORMAT_IGNORED.to_string());
    }
    if obj.get("max_results").is_some() {
        hints.push(HINT_MAX_RESULTS_IGNORED.to_string());
    }
    // Grep/structural-only filtering params that outline does not consume.
    // Emit one combined hint listing every offender so the agent gets
    // actionable feedback instead of silently-dropped fields.
    let ignored_grep_params: Vec<&str> = [
        "glob_pattern",
        "glob",
        "globs",
        "case_sensitive",
        "literal",
        "context_lines",
        "files_with_matches",
        "type_pattern",
        "max_file_size",
    ]
    .into_iter()
    .filter(|p| obj.get(*p).is_some())
    .collect();
    if !ignored_grep_params.is_empty() {
        hints.push(format!(
            "Parameters {} are grep/structural fields and are not used by outline. They \
             were ignored. Use `type` to filter symbol kinds, `match` to filter by name \
             regex, or `items` to select structure/exports/imports.",
            ignored_grep_params
                .iter()
                .map(|p| format!("`{p}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    hints
}

/// Build the `ast-grep outline --json=stream` command from the resolved
/// request. Pure construction — does not execute.
fn build_outline_command(
    ast_grep: &Path,
    workspace_root: &Path,
    request: &OutlineRequest,
    command_arg: &str,
) -> Command {
    let mut command = Command::new(ast_grep);
    command.current_dir(workspace_root).arg("outline");
    command.arg("--json=stream");
    if let Some(lang) = request.lang.as_deref().filter(|s| !s.trim().is_empty()) {
        command.arg("--lang").arg(lang);
    }
    if let Some(types) = request.type_filter.as_deref() {
        command.arg("--type").arg(types);
    }
    if let Some(regex) = request.match_regex.as_deref() {
        command.arg("--match").arg(regex);
    }
    command.arg("--items").arg(request.items.as_arg());
    if request.pub_members {
        command.arg("--pub-members");
    }
    if request.follow {
        command.arg("--follow");
    }
    command.arg(command_arg);
    command
}

/// Entry point invoked by `execute_search_dispatch` for `action=outline`.
pub async fn execute_outline_search(workspace_root: &Path, args: Value) -> Result<Value> {
    let mut request = OutlineRequest::from_args(&args)?;
    let ast_grep = AstGrepStatus::resolve_or_install()
        .await
        .map_err(|reason| anyhow!("Outline requires ast-grep (`sg`). {reason}"))?;

    let resolved = resolve_outline_path(workspace_root, &request.path)?;

    let mut hints = collect_outline_hints(&args);

    // Auto-tune the output for directory queries: when the user asks for an
    // outline of a directory, default to `view=names` (less verbose than
    // `digest`, no member lists) and emit a top-level `summary` block that
    // gives the model the symbol counts it usually wants when answering
    // "what's in this directory?" in a single tool call.
    let is_directory = resolved.is_dir();
    let was_view_explicit = view_is_explicit(&args);
    if is_directory && !was_view_explicit {
        request.view = OutlineView::Names;
    }

    let command_arg = command_path_arg(workspace_root, &resolved);
    let mut command = build_outline_command(&ast_grep, workspace_root, &request, &command_arg);

    let output = command
        .output()
        .await
        .context("failed to run `ast-grep outline`")?;

    if !output.status.success() {
        let detail = stderr_or_stdout(&output.stderr, &output.stdout);
        bail!("`ast-grep outline` failed: {detail}");
    }

    let files = parse_outline_stream(&output.stdout)?;

    // Auto-downgrade `view: "full"` to `view: "names"` for large directories.
    // `full` view emits per-symbol records with ranges/signatures/members —
    // for a 70-file directory this produces massive output that gets spooled
    // and truncated, forcing the agent to retry (checkpoint turn_586: 70-file
    // directory, `view: "full"` → truncated → retry → tools disabled → no
    // final answer).
    let file_count = files.len();
    let was_view_full_explicit =
        is_directory && was_view_explicit && request.view == OutlineView::Full;
    if was_view_full_explicit && file_count > LARGE_DIR_FULL_VIEW_THRESHOLD {
        hints.push(format!(
            "Auto-downgraded `view: \"full\"` to `view: \"names\"` because the directory has \
             {file_count} files (threshold: {LARGE_DIR_FULL_VIEW_THRESHOLD}). `view: \"full\"` \
             produces per-symbol records that are too large for directories. Use `view: \"full\"` \
             on individual files, or `view: \"names\"`/`\"digest\"` for directories."
        ));
        request.view = OutlineView::Names;
    }

    // Compute the directory summary from typed data before `shape_outline_result`
    // consumes `files`. The summary is view-independent (always walks
    // `file.items`), so the auto-downgrade above doesn't affect it.
    let summary = if is_directory {
        Some(compute_directory_summary(&files, &request.path))
    } else {
        None
    };

    let mut result = shape_outline_result(request.view, files)?;

    if let Some(summary) = summary {
        if let Some(obj) = result.as_object_mut() {
            obj.insert("summary".to_string(), summary);
        }
    }

    // Attach collected hints to the result so the agent gets feedback about
    // silently-dropped or auto-corrected parameters.
    if !hints.is_empty() {
        if let Some(obj) = result.as_object_mut() {
            obj.insert("hints".to_string(), json!(hints));
        }
    }

    Ok(result)
}

/// Compute a directory-level `summary` block from the parsed ast-grep records.
///
/// Pure function of the typed `&[OutlineFile]` slice — no re-parsing of shaped
/// JSON, no branching on view. This makes it independently testable and
/// ensures `by_kind`, `all_symbols[].kind`, and `total_symbols` are all
/// derived from a single source of truth (fixing the previous inconsistency
/// where `all_symbols[].kind` could be empty while `by_kind` counted it under
/// `"item"`).
fn compute_directory_summary(files: &[OutlineFile], request_path: &str) -> Value {
    let mut by_lang: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut all_symbols: Vec<Value> = Vec::new();
    let mut total_items = 0usize;

    for file in files {
        *by_lang
            .entry(if file.lang.is_empty() {
                "unknown".to_string()
            } else {
                file.lang.clone()
            })
            .or_default() += 1;

        for item in &file.items {
            if item.name.is_empty() {
                continue;
            }
            let kind_key = if item.symbol_type.is_empty() {
                "item"
            } else {
                &item.symbol_type
            };
            if all_symbols.len() < MAX_SUMMARY_SYMBOLS {
                all_symbols.push(json!({
                    "path": file.path,
                    "lang": file.lang,
                    "kind": kind_key,
                    "name": item.name,
                }));
            }
            *by_kind.entry(kind_key.to_string()).or_default() += 1;
            total_items += 1;
        }
    }

    let truncated = total_items > all_symbols.len();
    let visible_symbols = all_symbols.len();
    let next_action = if truncated {
        format!(
            "The directory has {total_items} symbols but only the first {visible_symbols} are \
             shown in `summary.all_symbols` (cap: {MAX_SUMMARY_SYMBOLS}). Narrow with `type` to \
             filter symbol kinds or `match` to filter by name, or outline a specific \
             file/sub-directory. Synthesize your final answer from the `summary` counts and the \
             `files` arrays above."
        )
    } else {
        "The directory outline is complete. Synthesize your final answer from the `summary.all_symbols` and `files` arrays above — no further tool calls needed for an overview.".to_string()
    };

    let mut summary = json!({
        "path": request_path,
        "is_directory": true,
        "file_count": files.len(),
        "total_symbols": total_items,
        "by_lang": by_lang,
        "by_kind": by_kind,
        "all_symbols": all_symbols,
        "next_action": next_action,
    });

    if truncated {
        if let Some(summary_obj) = summary.as_object_mut() {
            summary_obj.insert("truncated".to_string(), json!(true));
            summary_obj.insert("visible_symbols".to_string(), json!(visible_symbols));
        }
    }

    summary
}

/// Check if a path string ends with a known source file extension. Used by
/// the tolerant path fallback to detect when the agent passed a file path
/// (e.g. `registry.rs`) when the target is actually a directory (`registry/`).
fn has_source_extension(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    const EXTENSIONS: &[&str] = &[
        ".rs", ".py", ".js", ".ts", ".tsx", ".jsx", ".go", ".java", ".c", ".cpp", ".h", ".hpp",
        ".rb", ".php", ".swift", ".kt", ".scala", ".lua", ".sh", ".bash", ".zsh", ".cs", ".dart",
        ".r", ".hs", ".clj", ".cljs", ".edn", ".ex", ".exs", ".ml", ".m", ".mm", ".zig", ".nim",
        ".v", ".erl", ".jl", ".graphql", ".vue", ".svelte", ".gd", ".elm", ".f90", ".f", ".pas",
        ".pl", ".pm", ".tcl", ".sql", ".proto",
    ];
    EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

/// Strip the final extension from a path string. `"foo/bar.rs"` → `"foo/bar"`.
fn strip_extension(path: &str) -> String {
    match path.rfind('.') {
        Some(dot_pos) if dot_pos > path.rfind('/').unwrap_or(0) => path[..dot_pos].to_string(),
        _ => path.to_string(),
    }
}

/// Build the path argument passed to ast-grep. Use the workspace-relative form
/// when possible so the emitted `path` field is relative and readable.
fn command_path_arg(workspace_root: &Path, resolved: &Path) -> String {
    let workspace_canonical =
        std::fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
    if let Ok(relative) = resolved.strip_prefix(&workspace_canonical) {
        if relative.as_os_str().is_empty() {
            ".".to_string()
        } else {
            relative.to_string_lossy().replace('\\', "/")
        }
    } else {
        resolved.to_string_lossy().to_string()
    }
}

// ---------------------------------------------------------------------------
// Tolerant deserialization of the ast-grep outline JSON stream.
//
// The outline JSON is an alpha preview surface in ast-grep 0.44.0. Unknown
// keys may appear in future versions, so every struct uses `#[serde(default)]`
// and none set `deny_unknown_fields`. Missing fields degrade gracefully
// instead of failing the whole call.
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Deserialize)]
struct OutlineFile {
    #[serde(default)]
    path: String,
    #[serde(default, rename = "language")]
    lang: String,
    #[serde(default)]
    items: Vec<OutlineItem>,
}

/// Source range as reported by ast-grep outline. All line/column values are
/// zero-based in the raw stream. We expose the raw range plus a derived
/// 1-based `lineRange` in the `full` view so callers can feed the lines
/// straight to `file_operation` `read` (`offset_lines` is 1-based, inclusive).
///
/// Forward-compat tolerant: `#[serde(default)]` and no `deny_unknown_fields`,
/// so unknown keys from future ast-grep versions are ignored instead of
/// failing the whole call.
#[derive(Debug, Default, Clone, Deserialize)]
struct OutlineRange {
    #[serde(default, rename = "byteOffset")]
    byte_offset: OutlineByteOffset,
    #[serde(default)]
    start: OutlineLineColumn,
    #[serde(default)]
    end: OutlineLineColumn,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct OutlineByteOffset {
    #[serde(default)]
    start: u64,
    #[serde(default)]
    end: u64,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct OutlineLineColumn {
    #[serde(default)]
    line: u64,
    #[serde(default)]
    column: u64,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct OutlineItem {
    #[serde(default)]
    role: String,
    #[serde(default)]
    #[serde(rename = "symbolType")]
    symbol_type: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    signature: String,
    #[serde(default, rename = "astKind")]
    ast_kind: String,
    #[serde(default, rename = "isImport")]
    is_import: bool,
    #[serde(default, rename = "isExported")]
    is_exported: bool,
    /// Source range of the item. Parsed from the stream and re-emitted in the
    /// `full` view; omitted (gracefully) from `digest`/`names`.
    #[serde(default)]
    range: Option<OutlineRange>,
    #[serde(default)]
    members: Vec<OutlineMember>,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct OutlineMember {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "symbolType")]
    symbol_type: String,
    #[serde(default)]
    signature: String,
    #[serde(default, rename = "astKind")]
    ast_kind: String,
    #[serde(default, rename = "isPublic")]
    is_public: bool,
    #[serde(default)]
    range: Option<OutlineRange>,
}

/// Build the raw `range` object for the `full` view: the zero-based ast-grep
/// native shape (`byteOffset`, `start`, `end`). Returns `null` when ast-grep
/// omitted the range (e.g. for some member kinds).
fn range_value(range: &Option<OutlineRange>) -> Value {
    match range {
        Some(r) => json!({
            "byteOffset": {"start": r.byte_offset.start, "end": r.byte_offset.end},
            "start": {"line": r.start.line, "column": r.start.column},
            "end": {"line": r.end.line, "column": r.end.column},
        }),
        None => Value::Null,
    }
}

/// Derived 1-based inclusive line range (`{start, end}`) suitable for
/// `file_operation` `read` pagination: `offset_lines = lineRange.start`,
/// `page_size_lines = lineRange.end - lineRange.start + 1`. Returns `null`
/// when the raw range is absent. `saturating_add` guards against pathological
/// inputs (and satisfies the `-D warnings` arithmetic lint).
fn line_range_value(range: &Option<OutlineRange>) -> Value {
    match range {
        Some(r) => json!({
            "start": r.start.line.saturating_add(1),
            "end": r.end.line.saturating_add(1),
        }),
        None => Value::Null,
    }
}

fn parse_outline_stream(stdout: &[u8]) -> Result<Vec<OutlineFile>> {
    let stdout = String::from_utf8_lossy(stdout);
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<OutlineFile>(line).with_context(|| {
                format!("failed to parse ast-grep outline JSON stream line: {line}")
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Output shaping by `view`.
// ---------------------------------------------------------------------------

/// A grouped symbol kind for the `digest`/`names` views.
#[derive(Debug, Clone)]
struct OutlineGroup {
    kind: String,
    names: Vec<String>,
    members: Vec<String>,
}

fn shape_outline_result(view: OutlineView, files: Vec<OutlineFile>) -> Result<Value> {
    match view {
        OutlineView::Full => Ok(json!({
            "view": "full",
            "files": files.iter().map(full_file_record).collect::<Vec<_>>(),
        })),
        OutlineView::Digest | OutlineView::Names => {
            let include_members = view == OutlineView::Digest;
            Ok(json!({
                "view": view.as_str(),
                "files": files
                    .iter()
                    .map(|file| grouped_file_record(file, include_members))
                    .collect::<Vec<_>>(),
            }))
        }
    }
}

impl OutlineView {
    fn as_str(self) -> &'static str {
        match self {
            Self::Digest => "digest",
            Self::Names => "names",
            Self::Full => "full",
        }
    }
}

/// `full` view: passthrough of the parsed ast-grep records (re-serialized to
/// drop unknown fields and normalize the shape we expose to callers).
fn full_file_record(file: &OutlineFile) -> Value {
    json!({
        "path": file.path,
        "lang": file.lang,
        "items": file.items.iter().map(full_item_record).collect::<Vec<_>>(),
    })
}

fn full_item_record(item: &OutlineItem) -> Value {
    json!({
        "role": item.role,
        "kind": item.symbol_type,
        "name": item.name,
        "signature": item.signature,
        "astKind": item.ast_kind,
        "isImport": item.is_import,
        "isExported": item.is_exported,
        "range": range_value(&item.range),
        "lineRange": line_range_value(&item.range),
        "members": item.members.iter().map(full_member_record).collect::<Vec<_>>(),
    })
}

/// `full` view member record: includes `astKind`, the raw zero-based `range`,
/// and the derived 1-based `lineRange` so the agent can locate a member
/// (method/field/enum variant) precisely within its parent item and feed the
/// lines straight to `file_operation` `read` pagination.
fn full_member_record(member: &OutlineMember) -> Value {
    json!({
        "role": "member",
        "kind": member.symbol_type,
        "name": member.name,
        "signature": member.signature,
        "astKind": member.ast_kind,
        "isPublic": member.is_public,
        "range": range_value(&member.range),
        "lineRange": line_range_value(&member.range),
    })
}

/// `digest`/`names` view: group top-level items by `symbolType`, collecting
/// their names and (for `digest`) the flat list of all member names.
fn grouped_file_record(file: &OutlineFile, include_members: bool) -> Value {
    let mut groups: BTreeMap<String, OutlineGroup> = BTreeMap::new();
    for item in &file.items {
        let group = groups
            .entry(item.symbol_type.clone())
            .or_insert_with(|| OutlineGroup {
                kind: item.symbol_type.clone(),
                names: Vec::new(),
                members: Vec::new(),
            });
        if !item.name.is_empty() {
            group.names.push(item.name.clone());
        }
        if include_members {
            for member in &item.members {
                if !member.name.is_empty() {
                    group.members.push(member.name.clone());
                }
            }
        }
    }

    let groups_value: Vec<Value> = groups
        .into_values()
        .map(|g| {
            if include_members {
                json!({
                    "kind": g.kind,
                    "names": g.names,
                    "members": g.members,
                })
            } else {
                json!({
                    "kind": g.kind,
                    "names": g.names,
                })
            }
        })
        .collect();

    json!({
        "path": file.path,
        "lang": file.lang,
        "groups": groups_value,
    })
}

#[cfg(test)]
mod tests;
