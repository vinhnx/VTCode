use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::tools::ast_grep_language::AstGrepLanguage;
use crate::tools::code_search::CodeSearchFilters;
use crate::tools::tree_sitter_runtime::usage_node_kind_allowlist;
use crate::types::CompactStr;
use vtcode_commons::canonicalize;
use vtcode_commons::exclusions::is_sensitive_file;
use vtcode_commons::walk::{build_default_walker, is_excluded_dir};

#[derive(Debug)]
pub struct ResolvedSearchScope {
    pub workspace_root: PathBuf,
    pub requested_path: PathBuf,
    pub requested_is_file: bool,
    pub allowed_files: HashSet<PathBuf>,
}

#[derive(Debug, Default)]
pub struct UsageScopeSupport {
    pub has_supported_files: bool,
    pub has_unsupported_files: bool,
    pub requested_unsupported_file_types: bool,
}

pub fn requested_languages(filters: &CodeSearchFilters) -> Vec<AstGrepLanguage> {
    filters
        .file_types
        .iter()
        .filter_map(|value| AstGrepLanguage::from_user_value(value))
        .collect()
}

pub fn usage_scope_support(scope: &ResolvedSearchScope, languages: &[AstGrepLanguage]) -> UsageScopeSupport {
    let mut support = UsageScopeSupport {
        requested_unsupported_file_types: languages
            .iter()
            .any(|language| usage_node_kind_allowlist(*language).is_none()),
        ..UsageScopeSupport::default()
    };
    for path in &scope.allowed_files {
        let Some(language) = AstGrepLanguage::from_path(path) else {
            continue;
        };
        if !languages.is_empty() && !languages.contains(&language) {
            continue;
        }
        if usage_node_kind_allowlist(language).is_some() {
            support.has_supported_files = true;
        } else {
            support.has_unsupported_files = true;
        }
    }
    support
}

pub fn has_complete_supported_inventory(
    scope: &ResolvedSearchScope,
    languages: &[AstGrepLanguage],
    stream_complete: bool,
    inventories: &HashMap<PathBuf, super::DeclarationInventory>,
) -> bool {
    scope.allowed_files.iter().any(|path| {
        let Some(language) = AstGrepLanguage::from_path(path) else {
            return false;
        };
        if (!languages.is_empty() && !languages.contains(&language)) || usage_node_kind_allowlist(language).is_none() {
            return false;
        }
        inventories.get(path).map_or(stream_complete, |inventory| inventory.complete)
    })
}

pub fn resolve_scope(workspace_root: &Path, requested: &str) -> Result<ResolvedSearchScope> {
    let requested_path = Path::new(requested);
    if requested_path
        .components()
        .any(|component| component == std::path::Component::ParentDir)
    {
        bail!("code_search path must not contain '..' traversal");
    }
    let workspace_root = canonicalize(workspace_root)
        .with_context(|| format!("failed to canonicalise workspace root {}", workspace_root.display()))?;
    let candidate = if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        workspace_root.join(requested_path)
    };
    let requested_path = canonicalize(&candidate)
        .with_context(|| format!("failed to resolve code_search path {}", candidate.display()))?;
    if !requested_path.starts_with(&workspace_root) {
        bail!("code_search path escapes the workspace");
    }
    if requested_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_sensitive_file)
    {
        bail!("code_search path is sensitive");
    }

    let requested_is_file = requested_path.is_file();
    let walk_root = if requested_is_file {
        requested_path.parent().unwrap_or(&workspace_root).to_path_buf()
    } else {
        requested_path.clone()
    };
    let mut builder = build_default_walker(&walk_root);
    let filter_walk_root = walk_root.clone();
    let filter_requested_path = requested_path.clone();
    builder.filter_entry(move |entry| {
        !is_excluded_dir(entry)
            && (!requested_is_file || entry.path() == filter_walk_root || entry.path() == filter_requested_path)
    });
    let mut requested_available = requested_path == workspace_root;
    let mut allowed_files = HashSet::new();
    for entry in builder.build().filter_map(std::result::Result::ok) {
        if entry.file_type().is_some_and(|file_type| file_type.is_symlink()) {
            continue;
        }
        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()).is_some_and(is_sensitive_file) {
            continue;
        }
        let Ok(canonical) = canonicalize(path) else {
            continue;
        };
        if !canonical.starts_with(&workspace_root) {
            continue;
        }
        if canonical == requested_path {
            requested_available = true;
        }
        if entry.file_type().is_some_and(|file_type| file_type.is_file()) {
            allowed_files.insert(canonical);
        }
    }
    if !requested_available {
        bail!("code_search path is ignored or unavailable");
    }

    Ok(ResolvedSearchScope {
        workspace_root,
        requested_path,
        requested_is_file,
        allowed_files,
    })
}

pub fn workspace_relative(scope: &ResolvedSearchScope, path: &Path) -> Option<CompactStr> {
    let relative = path.strip_prefix(&scope.workspace_root).ok()?;
    let value = if relative.as_os_str().is_empty() {
        ".".to_string()
    } else {
        relative.to_string_lossy().replace('\\', "/")
    };
    Some(CompactStr::from(value))
}

pub fn accepted_candidate_path(
    scope: &ResolvedSearchScope,
    base: &Path,
    candidate: &Path,
    languages: &[AstGrepLanguage],
) -> Option<(PathBuf, CompactStr)> {
    let path = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base.join(candidate)
    };
    let canonical = canonicalize(path).ok()?;
    if !scope.allowed_files.contains(&canonical)
        || !canonical.starts_with(&scope.workspace_root)
        || (scope.requested_is_file && canonical != scope.requested_path)
        || (!scope.requested_is_file && !canonical.starts_with(&scope.requested_path))
        || canonical
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is_sensitive_file)
    {
        return None;
    }
    let language_matches = languages.is_empty()
        || AstGrepLanguage::from_path(&canonical).is_some_and(|language| languages.contains(&language));
    if !language_matches {
        return None;
    }
    let relative = workspace_relative(scope, &canonical)?;
    Some((canonical, relative))
}

pub fn response_path(scope: &ResolvedSearchScope) -> CompactStr {
    workspace_relative(scope, &scope.requested_path).unwrap_or_else(|| CompactStr::from("."))
}
