//! Shared prose and planning exemptions for mutation accounting.

use std::path::{Component, Path, PathBuf};

/// Staging root for workspace-less plan artifacts (`$TMPDIR/vtcode-plans`).
/// Single source of truth shared by mutation accounting, planning admission,
/// and plan-state staging, so the exemption always matches the real temp dir
/// (e.g. `/var/folders/...` on macOS) instead of a `/tmp` literal.
pub fn tmp_plans_dir() -> PathBuf {
    std::env::temp_dir().join("vtcode-plans")
}

fn tmp_plans_root_normalized() -> String {
    tmp_plans_dir().to_string_lossy().replace('\\', "/")
}

fn path_targets_plan_artifact(path: &str) -> bool {
    let normalized = path.trim().replace('\\', "/");
    // This helper has no workspace to resolve traversal or symlinks against.
    // Parent traversal cannot establish that the target is a planning artifact.
    if Path::new(&normalized).components().any(|part| part == Component::ParentDir) {
        return false;
    }
    if normalized == ".vtcode/plans"
        || normalized.starts_with(".vtcode/plans/")
        || normalized.contains("/.vtcode/plans/")
    {
        return true;
    }
    let tmp_root = tmp_plans_root_normalized();
    normalized == tmp_root
        || normalized.starts_with(&format!("{tmp_root}/"))
        || normalized.contains(&format!("{tmp_root}/"))
}

fn path_is_docs_only(path: &str) -> bool {
    let normalized = path.trim().replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    let file_name = lower.rsplit('/').next().unwrap_or_default();
    if matches!(file_name, "cmakelists.txt" | "requirements.txt") {
        return false;
    }
    // Well-known prose filenames count only with a docs extension or no
    // extension at all, so `README.py` and `docs/script.py` stay code edits.
    let stem = file_name.split('.').next().unwrap_or_default();
    if matches!(stem, "readme" | "changelog" | "license") {
        let ext = file_name.rsplit('.').next().unwrap_or_default();
        if ext == stem || matches!(ext, "md" | "mdx" | "txt" | "rst") {
            return true;
        }
    }
    matches!(file_name.rsplit('.').next(), Some("md" | "mdx" | "txt" | "rst"))
}

fn file_mutation_paths(name: &str, args: &serde_json::Value) -> Vec<PathBuf> {
    let mut paths = crate::tools::apply_patch::mutation_target_paths(name, args);
    // Include the camelCase alias used by file adapters as well as canonical keys.
    if let Some(extra) = args.get("filePath").and_then(|value| value.as_str()) {
        let trimmed = extra.trim();
        if !trimmed.is_empty() {
            paths.push(PathBuf::from(trimmed));
        }
    }
    paths
}

/// Docs-only writes are low-risk prose edits. They neither trip nor clear the
/// anti-blind gate: while pending they stay allowed (warn-only), and they never
/// increment `consecutive_mutations`. Mixed docs+code patches, empty path sets,
/// and exec-tool mutations fail closed as code edits.
pub fn is_docs_only_write(name: &str, args: &serde_json::Value) -> bool {
    use crate::config::constants::tools as tool_names;
    use crate::tools::names::canonical_tool_name;
    use crate::tools::tool_intent::file_operation_action;

    let canonical = canonical_tool_name(name);
    let is_file_write = matches!(
        canonical,
        tool_names::APPLY_PATCH
            | tool_names::WRITE_FILE
            | tool_names::EDIT_FILE
            | tool_names::CREATE_FILE
            | tool_names::SEARCH_REPLACE
            | tool_names::DELETE_FILE
            | tool_names::MOVE_FILE
            | tool_names::COPY_FILE
            | tool_names::UNIFIED_FILE
    );
    if !is_file_write {
        return false;
    }
    if canonical == tool_names::UNIFIED_FILE
        && file_operation_action(args)
            .map(|action| action.eq_ignore_ascii_case("read"))
            .unwrap_or(false)
    {
        return false;
    }
    let paths = file_mutation_paths(canonical, args);
    if paths.is_empty() {
        return false;
    }
    paths.iter().all(|path| path.to_str().is_some_and(path_is_docs_only))
}

pub fn is_plan_artifact_write(name: &str, args: &serde_json::Value) -> bool {
    use crate::config::constants::tools as tool_names;
    use crate::tools::names::canonical_tool_name;
    use crate::tools::tool_intent::file_operation_action;

    let canonical = canonical_tool_name(name);
    if canonical == tool_names::TASK_TRACKER {
        return true;
    }
    if !matches!(
        canonical,
        tool_names::UNIFIED_FILE
            | tool_names::WRITE_FILE
            | tool_names::EDIT_FILE
            | tool_names::CREATE_FILE
            | tool_names::SEARCH_REPLACE
    ) || (canonical == tool_names::UNIFIED_FILE
        && file_operation_action(args).is_some_and(|action| action.eq_ignore_ascii_case("read")))
    {
        return false;
    }
    let paths = file_mutation_paths(canonical, args);
    !paths.is_empty() && paths.iter().all(|path| path.to_str().is_some_and(path_targets_plan_artifact))
}
