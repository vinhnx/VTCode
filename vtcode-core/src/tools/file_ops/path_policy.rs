use super::FileOpsTool;
use crate::tools::jaro_winkler_similarity;
use anyhow::{Result, anyhow};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

const MAX_PATH_SUGGESTIONS: usize = 3;
const MAX_PATH_SUGGESTION_SCAN: usize = 20_000;
const MIN_PATH_SUGGESTION_SCORE: f32 = 0.78;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PathSuggestionKind {
    Any,
    File,
}

impl PathSuggestionKind {
    fn matches(self, entry: &DirEntry) -> bool {
        match self {
            Self::Any => true,
            Self::File => entry.file_type().is_file(),
        }
    }
}

fn normalize_path_for_suggestion(path: &str) -> String {
    path.replace('\\', "/")
        .trim_matches('/')
        .to_ascii_lowercase()
}

fn suggestion_basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn suggestion_score(requested_path: &str, candidate_path: &str) -> f32 {
    let requested_name = suggestion_basename(requested_path);
    let candidate_name = suggestion_basename(candidate_path);

    let full_score = jaro_winkler_similarity(requested_path, candidate_path);
    let name_score = if requested_name.is_empty() || candidate_name.is_empty() {
        0.0
    } else {
        jaro_winkler_similarity(requested_name, candidate_name)
    };

    let mut score = full_score.max(name_score * 0.85);

    if !requested_name.is_empty() && requested_name == candidate_name {
        score += 0.20;
    } else if !requested_name.is_empty()
        && (candidate_name.contains(requested_name) || requested_name.contains(candidate_name))
    {
        score += 0.06;
    }

    if candidate_path.ends_with(requested_path) || requested_path.ends_with(candidate_path) {
        score += 0.12;
    }

    score.min(1.0)
}

fn should_index_suggestion_entry(workspace_root: &Path, entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }

    let Ok(relative) = entry.path().strip_prefix(workspace_root) else {
        return true;
    };

    !relative.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".git" | "target" | "node_modules")
        )
    })
}

impl FileOpsTool {
    pub(super) fn canonical_workspace_root(&self) -> &PathBuf {
        &self.canonical_workspace_root
    }

    pub(super) fn workspace_relative_display(&self, path: &Path) -> String {
        if let Ok(relative) = path.strip_prefix(&self.workspace_root) {
            relative.to_string_lossy().into_owned()
        } else if let Ok(relative) = path.strip_prefix(self.canonical_workspace_root()) {
            relative.to_string_lossy().into_owned()
        } else {
            path.to_string_lossy().into_owned()
        }
    }

    pub(super) fn absolute_candidate(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_root.join(path)
        }
    }

    pub(super) async fn normalize_and_validate_user_path(&self, path: &str) -> Result<PathBuf> {
        self.normalize_and_validate_candidate(Path::new(path), path)
            .await
    }

    pub(super) async fn normalize_and_validate_candidate(
        &self,
        path: &Path,
        original_display: &str,
    ) -> Result<PathBuf> {
        use crate::utils::path::normalize_path;
        let absolute = self.absolute_candidate(path);
        let normalized = normalize_path(&absolute);
        let normalized_root = normalize_path(&self.workspace_root);
        let canonical = self.canonicalize_allow_missing(&normalized).await?;
        let canonical_root = normalize_path(self.canonical_workspace_root());

        let within_workspace = normalized.starts_with(&normalized_root)
            || normalized.starts_with(&canonical_root)
            || canonical.starts_with(&normalized_root)
            || canonical.starts_with(self.canonical_workspace_root());

        if !within_workspace {
            return Err(anyhow!(
                "Error: Path '{}' resolves outside the workspace.",
                original_display
            ));
        }

        Ok(canonical)
    }

    pub(super) async fn canonicalize_allow_missing(&self, normalized: &Path) -> Result<PathBuf> {
        crate::utils::path::canonicalize_allow_missing(normalized).await
    }

    pub(super) fn resolve_file_path(&self, path: &str) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        let requested = PathBuf::from(path);

        if requested.is_absolute() {
            paths.push(requested);
            return Ok(paths);
        }

        // Try exact path first
        paths.push(self.workspace_root.join(path));

        // If it's just a filename, try common directories that exist in most projects
        if !path.contains('/') && !path.contains('\\') {
            // Generic source directories found in most projects
            paths.push(self.workspace_root.join("src").join(path));
            paths.push(self.workspace_root.join("lib").join(path));
            paths.push(self.workspace_root.join("bin").join(path));
            paths.push(self.workspace_root.join("app").join(path));
            paths.push(self.workspace_root.join("source").join(path));
            paths.push(self.workspace_root.join("sources").join(path));
            paths.push(self.workspace_root.join("include").join(path));
            paths.push(self.workspace_root.join("docs").join(path));
            paths.push(self.workspace_root.join("doc").join(path));
            paths.push(self.workspace_root.join("examples").join(path));
            paths.push(self.workspace_root.join("example").join(path));
            paths.push(self.workspace_root.join("tests").join(path));
            paths.push(self.workspace_root.join("test").join(path));
        }

        // Try case-insensitive variants for filenames
        if !path.contains('/')
            && !path.contains('\\')
            && let Ok(entries) = std::fs::read_dir(&self.workspace_root)
        {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string()
                    && name.to_lowercase() == path.to_lowercase()
                {
                    paths.push(entry.path());
                }
            }
        }

        Ok(paths)
    }

    pub(super) fn missing_path_suggestion_suffix(
        &self,
        requested_path: &str,
        kind: PathSuggestionKind,
    ) -> String {
        let suggestions = self.suggest_workspace_paths(requested_path, kind);
        if suggestions.is_empty() {
            String::new()
        } else {
            format!(" Did you mean: {}?", suggestions.join(", "))
        }
    }

    pub(super) fn suggest_workspace_paths(
        &self,
        requested_path: &str,
        kind: PathSuggestionKind,
    ) -> Vec<String> {
        let requested_path = normalize_path_for_suggestion(requested_path);
        if requested_path.is_empty() || requested_path == "." {
            return Vec::new();
        }

        let mut scored_paths = Vec::with_capacity(MAX_PATH_SUGGESTIONS * 2);
        let mut scanned = 0usize;

        let walker = WalkDir::new(&self.workspace_root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| should_index_suggestion_entry(&self.workspace_root, entry));

        for entry in walker {
            let Ok(entry) = entry else {
                continue;
            };
            if entry.depth() == 0 || !kind.matches(&entry) {
                continue;
            }

            scanned += 1;
            if scanned > MAX_PATH_SUGGESTION_SCAN {
                break;
            }

            let display_path = self.workspace_relative_display(entry.path());
            let normalized_candidate = normalize_path_for_suggestion(&display_path);
            if normalized_candidate.is_empty() || normalized_candidate == requested_path {
                continue;
            }

            let score = suggestion_score(&requested_path, &normalized_candidate);
            if score < MIN_PATH_SUGGESTION_SCORE {
                continue;
            }

            scored_paths.push((score, display_path));
        }

        scored_paths.sort_by(|left, right| {
            right
                .0
                .partial_cmp(&left.0)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.1.cmp(&right.1))
        });
        scored_paths.dedup_by(|left, right| left.1 == right.1);

        scored_paths
            .into_iter()
            .take(MAX_PATH_SUGGESTIONS)
            .map(|(_, path)| path)
            .collect()
    }

    /// Public helper to normalize and validate a user-provided path against the workspace root.
    pub async fn normalize_user_path(&self, path: &str) -> Result<PathBuf> {
        self.normalize_and_validate_user_path(path).await
    }
}
