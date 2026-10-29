use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use glob::{Pattern, glob};
use hashbrown::HashSet;
use tokio::io;
use tracing::warn;
use vtcode_commons::VtCodePaths;
use vtcode_commons::canonicalize;
use vtcode_commons::canonicalize_async;
use vtcode_commons::walk::build_walker_single_threaded;

use crate::utils::file_utils::canonicalize_with_context;

use super::matching::{
    RulePattern, contains_glob_meta, parse_rule_frontmatter, pattern_matches_candidate, rule_specificity,
};
use super::types::{
    InstructionDiscoveryOptions, InstructionScope, InstructionSource, InstructionSourceKind, MatchCandidate,
};
use super::{AGENTS_FILENAME, AGENTS_OVERRIDE_FILENAME, CLAUDE_FILENAME, GLOBAL_CONFIG_DIRECTORY, RULES_DIRECTORY};

#[derive(Debug, Clone)]
struct RuleDescriptor {
    patterns: Vec<String>,
    specificity: usize,
}

#[derive(Debug, Clone)]
struct MatchContext {
    candidates: Vec<MatchCandidate>,
}

impl MatchContext {
    async fn new(project_root: &Path, match_paths: &[PathBuf]) -> Self {
        let mut seen = HashSet::new();
        let mut candidates = Vec::with_capacity(match_paths.len());
        let canonical_root = canonicalize_async(project_root).await.ok();

        for raw_path in match_paths {
            let candidate = if raw_path.is_absolute() {
                raw_path.to_path_buf()
            } else {
                project_root.join(raw_path)
            };

            let normalized = canonicalize_async(&candidate).await.unwrap_or_else(|_| candidate.clone());
            let relative = normalized
                .strip_prefix(project_root)
                .ok()
                .or_else(|| canonical_root.as_ref().and_then(|root| normalized.strip_prefix(root).ok()))
                .or_else(|| candidate.strip_prefix(project_root).ok())
                .or_else(|| canonical_root.as_ref().and_then(|root| candidate.strip_prefix(root).ok()));
            let Some(relative) = relative else {
                continue;
            };

            let relative = relative.display().to_string();
            if relative.is_empty() {
                continue;
            }

            let is_dir = tokio::fs::metadata(&normalized).await.is_ok_and(|metadata| metadata.is_dir());
            let key = format!("{relative}:{is_dir}");
            if seen.insert(key) {
                candidates.push(MatchCandidate { relative_path: relative, is_dir });
            }
        }

        candidates.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Self { candidates }
    }

    fn matches_any(&self, patterns: &[String]) -> Result<bool> {
        const MAX_RULE_PATTERN_BYTES: usize = 4096;
        for raw_pattern in patterns {
            let trimmed = raw_pattern.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.len() > MAX_RULE_PATTERN_BYTES {
                anyhow::bail!("instruction rule path pattern exceeds {MAX_RULE_PATTERN_BYTES} bytes");
            }

            let Ok(pattern) = RulePattern::new(trimmed) else {
                warn!("Ignoring invalid instruction rule path pattern `{trimmed}`");
                continue;
            };

            if self
                .candidates
                .iter()
                .any(|candidate| pattern_matches_candidate(&pattern, candidate))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[derive(Debug, Clone)]
struct ExclusionMatcher {
    patterns: Vec<Pattern>,
}

impl ExclusionMatcher {
    async fn compile(project_root: &Path, home_dir: Option<&Path>, raw_patterns: &[String]) -> Result<Self> {
        let mut patterns = Vec::with_capacity(raw_patterns.len());
        for raw in raw_patterns {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let resolved = resolve_pattern(trimmed, project_root, home_dir).await?;
            let pattern = Pattern::new(&resolved)
                .with_context(|| format!("Failed to compile instruction exclude pattern `{trimmed}`"))?;
            patterns.push(pattern);
        }

        Ok(Self { patterns })
    }

    fn matches(&self, path: &Path) -> bool {
        self.patterns.iter().any(|pattern| {
            pattern.matches_path(path)
                || pattern.matches_path_with(
                    path,
                    glob::MatchOptions {
                        case_sensitive: true,
                        require_literal_separator: false,
                        require_literal_leading_dot: false,
                    },
                )
        })
    }
}

pub async fn discover_instruction_sources(options: &InstructionDiscoveryOptions<'_>) -> Result<Vec<InstructionSource>> {
    let mut sources = Vec::with_capacity(16);
    let mut seen_paths = HashSet::new();
    let excludes = ExclusionMatcher::compile(options.project_root, options.home_dir, options.exclude_patterns).await?;
    let match_context = MatchContext::new(options.project_root, options.match_paths).await;

    if let Some(home) = options.home_dir {
        for candidate in user_instruction_candidates(home, options.fallback_filenames) {
            if let Some(path) = normalize_instruction_candidate(&candidate, &excludes).await?
                && seen_paths.insert(path.clone())
            {
                sources.push(InstructionSource {
                    path,
                    scope: InstructionScope::User,
                    kind: InstructionSourceKind::Agents,
                    matched: false,
                });
            }
        }

        let (user_unconditional_rules, user_matched_rules) =
            discover_rule_sources(user_rules_roots(home), InstructionScope::User, &match_context, &excludes, None)
                .await?;
        for source in user_unconditional_rules.into_iter().chain(user_matched_rules) {
            if seen_paths.insert(source.path.clone()) {
                sources.push(source);
            }
        }
    }

    let extra_paths =
        expand_instruction_patterns(options.project_root, options.home_dir, options.extra_patterns, &excludes).await?;
    for path in extra_paths {
        if seen_paths.insert(path.clone()) {
            sources.push(InstructionSource {
                path,
                scope: InstructionScope::Custom,
                kind: InstructionSourceKind::Extra,
                matched: false,
            });
        }
    }

    let root = canonicalize_async(options.project_root)
        .await
        .with_context(|| format!("Failed to canonicalize project root {}", options.project_root.display()))?;
    let mut cursor = canonicalize_async(options.current_dir)
        .await
        .with_context(|| format!("Failed to canonicalize working directory {}", options.current_dir.display()))?;
    if !cursor.starts_with(&root) {
        cursor = root.clone();
    }

    let mut workspace_levels = Vec::with_capacity(4);
    loop {
        let chosen_paths =
            select_workspace_instruction_candidates(&cursor, &root, options.fallback_filenames, &excludes).await?;

        let mut level_sources = Vec::with_capacity(chosen_paths.len());
        for path in chosen_paths {
            if seen_paths.insert(path.clone()) {
                level_sources.push(InstructionSource {
                    path,
                    scope: InstructionScope::Workspace,
                    kind: InstructionSourceKind::Agents,
                    matched: false,
                });
            }
        }
        if !level_sources.is_empty() {
            workspace_levels.push(level_sources);
        }

        if cursor == root {
            break;
        }

        cursor = cursor
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("Reached filesystem root before encountering project root"))?;
    }

    workspace_levels.reverse();
    sources.extend(workspace_levels.into_iter().flatten());

    let (workspace_unconditional_rules, workspace_matched_rules) = discover_rule_sources(
        vec![root.join(RULES_DIRECTORY)],
        InstructionScope::Workspace,
        &match_context,
        &excludes,
        Some(&root),
    )
    .await?;
    for source in workspace_unconditional_rules.into_iter().chain(workspace_matched_rules) {
        if seen_paths.insert(source.path.clone()) {
            sources.push(source);
        }
    }

    Ok(sources)
}

fn user_instruction_candidates(home: &Path, fallback_filenames: &[String]) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for root in user_global_instruction_roots(home) {
        candidates.extend(instruction_candidates_for_dir(&root, fallback_filenames));
    }
    candidates
}

fn user_rules_roots(home: &Path) -> Vec<PathBuf> {
    user_global_instruction_roots(home)
        .into_iter()
        .map(|root| root.join("rules"))
        .collect()
}

pub(super) fn user_global_instruction_roots(home: &Path) -> Vec<PathBuf> {
    let canonical_config = VtCodePaths::resolve()
        .ok()
        .filter(|_paths| dirs::home_dir().as_deref() == Some(home))
        .map(|paths| paths.config_dir().to_path_buf())
        .unwrap_or_else(|| home.join(GLOBAL_CONFIG_DIRECTORY));
    let legacy = VtCodePaths::resolve()
        .ok()
        .filter(|_paths| dirs::home_dir().as_deref() == Some(home))
        .map(|paths| paths.legacy_dir().to_path_buf())
        .unwrap_or_else(|| home.join(".vtcode"));

    let mut roots = vec![
        home.to_path_buf(),
        legacy,
        home.join(GLOBAL_CONFIG_DIRECTORY),
        canonical_config,
    ];
    roots.dedup();
    roots
}

fn instruction_candidates_for_dir(dir: &Path, fallback_filenames: &[String]) -> Vec<PathBuf> {
    let mut candidates = Vec::with_capacity(2 + fallback_filenames.len());
    candidates.push(dir.join(AGENTS_OVERRIDE_FILENAME));
    candidates.push(dir.join(AGENTS_FILENAME));
    for name in fallback_filenames {
        let trimmed = name.trim();
        if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case(AGENTS_FILENAME)
            || trimmed.eq_ignore_ascii_case(AGENTS_OVERRIDE_FILENAME)
        {
            continue;
        }
        candidates.push(dir.join(trimmed));
    }
    candidates
}

async fn select_workspace_instruction_candidate(
    dir: &Path,
    workspace_root: &Path,
    fallback_filenames: &[String],
    excludes: &ExclusionMatcher,
) -> Result<Option<PathBuf>> {
    for candidate in instruction_candidates_for_dir(dir, fallback_filenames) {
        if let Some(path) = normalize_instruction_candidate(&candidate, excludes).await?
            && path.starts_with(workspace_root)
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

async fn select_workspace_instruction_candidates(
    dir: &Path,
    workspace_root: &Path,
    fallback_filenames: &[String],
    excludes: &ExclusionMatcher,
) -> Result<Vec<PathBuf>> {
    let mut selected = Vec::with_capacity(2);
    if let Some(path) =
        select_workspace_instruction_candidate(dir, workspace_root, fallback_filenames, excludes).await?
    {
        selected.push(path);
    }

    if let Some(path) = normalize_instruction_candidate(&dir.join(CLAUDE_FILENAME), excludes).await?
        && path.starts_with(workspace_root)
        && !selected.iter().any(|existing_path| existing_path == &path)
    {
        selected.push(path);
    }

    Ok(selected)
}

async fn normalize_instruction_candidate(candidate: &Path, excludes: &ExclusionMatcher) -> Result<Option<PathBuf>> {
    if !instruction_exists(candidate).await? {
        return Ok(None);
    }

    let canonical = canonicalize_async(candidate)
        .await
        .with_context(|| format!("Failed to canonicalize instruction candidate {}", candidate.display()))?;
    if excludes.matches(&canonical) {
        return Ok(None);
    }

    Ok(Some(canonical))
}

async fn discover_rule_sources(
    rule_roots: Vec<PathBuf>,
    scope: InstructionScope,
    match_context: &MatchContext,
    excludes: &ExclusionMatcher,
    workspace_root: Option<&Path>,
) -> Result<(Vec<InstructionSource>, Vec<InstructionSource>)> {
    // Directory walking performs blocking filesystem I/O (readdir, stat, canonicalize).
    // Offload the traversal to the blocking thread pool so the async runtime
    // stays responsive while we discover rule files on disk.
    let discovered = {
        let excludes = excludes.clone();
        let workspace_root = workspace_root.map(Path::to_path_buf);
        tokio::task::spawn_blocking(move || {
            let mut paths = Vec::new();
            let mut seen = HashSet::new();

            for root in &rule_roots {
                if !root.exists() {
                    continue;
                }
                if workspace_root.as_ref().is_some_and(|workspace_root| {
                    canonicalize(root).map_or(true, |path| !path.starts_with(workspace_root))
                }) {
                    continue;
                }

                let mut builder = build_walker_single_threaded(root);
                builder.follow_links(true).sort_by_file_name(|a, b| a.cmp(b));
                if let Some(workspace_root) = &workspace_root {
                    let workspace_root = workspace_root.clone();
                    builder.filter_entry(move |entry| {
                        canonicalize(entry.path()).is_ok_and(|path| path.starts_with(&workspace_root))
                    });
                }
                for entry in builder.build().filter_map(std::result::Result::ok) {
                    if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                        continue;
                    }

                    if entry.path().extension().and_then(|value| value.to_str()) != Some("md") {
                        continue;
                    }

                    if entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.eq_ignore_ascii_case("README.md"))
                    {
                        continue;
                    }

                    let path = canonicalize_with_context(entry.path(), "instruction rule")?;
                    if workspace_root.as_ref().is_some_and(|root| !path.starts_with(root))
                        || excludes.matches(&path)
                        || !seen.insert(path.clone())
                    {
                        continue;
                    }

                    paths.push(path);
                }
            }

            Ok::<_, anyhow::Error>(paths)
        })
        .await
        .context("Rule discovery task panicked")??
    };

    // Process discovered paths asynchronously: read frontmatter and match patterns.
    let mut unconditional = Vec::new();
    let mut matched = Vec::new();

    for path in discovered {
        let descriptor = read_rule_descriptor(&path).await?;
        let is_matched = if descriptor.patterns.is_empty() {
            false
        } else {
            match_context
                .matches_any(&descriptor.patterns)
                .with_context(|| format!("Failed to match instruction rule {}", path.display()))?
        };
        let source = InstructionSource {
            path,
            scope: scope.clone(),
            kind: InstructionSourceKind::Rule,
            matched: is_matched,
        };

        if descriptor.patterns.is_empty() {
            unconditional.push(source);
        } else if is_matched {
            matched.push((descriptor.specificity, source));
        }
    }

    unconditional.sort_by(|left, right| left.path.cmp(&right.path));
    matched.sort_by(|(left_specificity, left), (right_specificity, right)| {
        left_specificity.cmp(right_specificity).then(left.path.cmp(&right.path))
    });

    Ok((unconditional, matched.into_iter().map(|(_, source)| source).collect()))
}

async fn read_rule_descriptor(path: &Path) -> Result<RuleDescriptor> {
    let contents = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read instruction rule {}", path.display()))?;
    let frontmatter = parse_rule_frontmatter(&contents, path)?;
    let specificity = frontmatter
        .paths
        .iter()
        .map(|pattern| rule_specificity(pattern))
        .max()
        .unwrap_or(0);

    Ok(RuleDescriptor { patterns: frontmatter.paths, specificity })
}

async fn expand_instruction_patterns(
    project_root: &Path,
    home_dir: Option<&Path>,
    patterns: &[String],
    excludes: &ExclusionMatcher,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::with_capacity(patterns.len());
    let mut seen = HashSet::new();

    for pattern in patterns {
        let resolved = resolve_pattern(pattern, project_root, home_dir).await?;
        let glob_matches: Vec<PathBuf> = glob(&resolved)
            .with_context(|| format!("Failed to expand instruction pattern `{pattern}`"))?
            .filter_map(|entry| match entry {
                Ok(path) => Some(path),
                Err(err) => {
                    warn!("Ignoring malformed instruction path for pattern `{pattern}`: {err}");
                    None
                }
            })
            .collect();

        let mut matches = Vec::with_capacity(glob_matches.len());
        for path in glob_matches {
            match normalize_instruction_candidate(&path, excludes).await {
                Ok(Some(canonical)) if seen.insert(canonical.clone()) => matches.push(canonical),
                Ok(Some(_)) | Ok(None) => {}
                Err(err) => {
                    warn!("Failed to inspect potential instruction `{}`: {err:#}", path.display());
                }
            }
        }

        if matches.is_empty() {
            warn!("Instruction pattern `{pattern}` did not match any files");
        } else {
            matches.sort();
            paths.extend(matches);
        }
    }

    Ok(paths)
}

async fn resolve_pattern(pattern: &str, project_root: &Path, home_dir: Option<&Path>) -> Result<String> {
    if let Some(stripped) = pattern.strip_prefix("~/") {
        let home = home_dir
            .ok_or_else(|| anyhow!("Cannot expand `~` in instruction pattern `{pattern}` without a home directory"))?;
        let resolved = home.join(stripped);
        if !contains_glob_meta(stripped) && tokio::fs::try_exists(&resolved).await.unwrap_or(false) {
            return Ok(canonicalize_async(&resolved)
                .await
                .with_context(|| format!("Failed to canonicalize instruction pattern {}", resolved.display()))?
                .to_string_lossy()
                .into_owned());
        }
        return Ok(resolved.to_string_lossy().into_owned());
    }

    let candidate = Path::new(pattern);
    let full_path = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        project_root.join(candidate)
    };

    if !contains_glob_meta(pattern) && tokio::fs::try_exists(&full_path).await.unwrap_or(false) {
        return Ok(canonicalize_async(&full_path)
            .await
            .with_context(|| format!("Failed to canonicalize instruction pattern {}", full_path.display()))?
            .to_string_lossy()
            .into_owned());
    }

    Ok(full_path.to_string_lossy().into_owned())
}

async fn instruction_exists(path: &Path) -> Result<bool> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => Ok(metadata.file_type().is_file() || metadata.file_type().is_symlink()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("Failed to inspect instruction candidate {}", path.display())),
    }
}
