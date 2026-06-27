//! `unified_search action=outline` -- wraps `ast-grep outline` to produce a
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

use crate::tools::editing::patch::resolve_ast_grep_binary_path;
use crate::tools::structural_search::stderr_or_stdout;
use crate::utils::path::resolve_workspace_path;

const SUPPORTED_ITEMS: &[&str] = &["auto", "structure", "exports", "imports", "all"];
const SUPPORTED_VIEWS: &[&str] = &["digest", "names", "full"];

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

/// Entry point invoked by `execute_unified_search` for `action=outline`.
pub async fn execute_outline_search(workspace_root: &Path, args: Value) -> Result<Value> {
    let request = OutlineRequest::from_args(&args)?;
    let ast_grep = resolve_ast_grep_binary_path()
        .map_err(|reason| anyhow!("Outline requires ast-grep (`sg`). {reason}"))?;

    // Resolve the search path within the workspace. `resolve_workspace_path`
    // canonicalizes and enforces workspace containment, so a missing path
    // surfaces as a structured error rather than a panic.
    let resolved = resolve_workspace_path(workspace_root, Path::new(&request.path))
        .with_context(|| format!("Failed to resolve outline path: {}", request.path))?;
    let command_arg = command_path_arg(workspace_root, &resolved);

    let mut command = Command::new(&ast_grep);
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
    command.arg(&command_arg);

    let output = command
        .output()
        .await
        .context("failed to run `ast-grep outline`")?;

    if !output.status.success() {
        let detail = stderr_or_stdout(&output.stderr, &output.stdout);
        bail!("`ast-grep outline` failed: {detail}");
    }

    let files = parse_outline_stream(&output.stdout)?;
    shape_outline_result(request.view, files)
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
    #[serde(default, rename = "isPublic")]
    is_public: bool,
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
        "members": item.members.iter().map(|m| json!({
            "role": "member",
            "kind": m.symbol_type,
            "name": m.name,
            "signature": m.signature,
            "isPublic": m.is_public,
        })).collect::<Vec<_>>(),
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
