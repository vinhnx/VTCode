use hashbrown::HashSet;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use tokio::io;

use crate::utils::file_utils::canonicalize_with_context;
use anyhow::{Context, Result, anyhow};
use glob::{Pattern, glob};
use tracing::warn;
use vtcode_commons::VtCodePaths;
use vtcode_commons::canonicalize;
use vtcode_commons::canonicalize_async;
use vtcode_commons::walk::build_walker_single_threaded;

const AGENTS_FILENAME: &str = "AGENTS.md";
const AGENTS_OVERRIDE_FILENAME: &str = "AGENTS.override.md";
const CLAUDE_FILENAME: &str = "CLAUDE.md";
const GLOBAL_CONFIG_DIRECTORY: &str = ".config/vtcode";
const RULES_DIRECTORY: &str = ".vtcode/rules";
const IMPORT_PROBE_NAME: &str = "__vtcode_instruction_probe__";

mod labels;
pub use labels::{format_instruction_path, instruction_scope_label, instruction_source_label};
mod matching;
use matching::{
    RulePattern, allowed_import_roots, collect_imports, contains_glob_meta, is_fence_line, parse_rule_frontmatter,
    pattern_matches_candidate, resolve_import_path, rule_specificity, strip_html_comments, strip_rule_frontmatter,
};

/// Defines the scope from which an instruction source originates.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum InstructionScope {
    /// User-level instructions from the canonical config root or a compatibility root.
    User,
    /// Workspace-level instructions from the project root or subdirectories.
    Workspace,
    /// Custom instructions from explicit glob patterns.
    Custom,
}

/// The kind of instruction file discovered on disk.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstructionSourceKind {
    /// An `AGENTS.md` or `CLAUDE.md` file.
    Agents,
    /// A rule file from `.vtcode/rules/` with optional path matching.
    Rule,
    /// An extra instruction file matched by a custom glob pattern.
    Extra,
}

/// A discovered instruction file with its path, scope, and kind metadata.
#[derive(Debug, Clone, Serialize)]
pub struct InstructionSource {
    /// Canonical path to the instruction file on disk.
    pub path: PathBuf,
    /// Scope from which this source was discovered.
    pub scope: InstructionScope,
    /// Kind of instruction file (agents, rule, or extra).
    pub kind: InstructionSourceKind,
    /// Whether a conditional rule matched the current file context.
    pub matched: bool,
}

/// A single instruction segment pairing a source with its expanded text content.
#[derive(Debug, Clone, Serialize)]
pub struct InstructionSegment {
    /// Metadata about where this segment was loaded from.
    pub source: InstructionSource,
    /// The instruction text after frontmatter stripping and import expansion.
    pub contents: String,
}

/// A complete bundle of instruction segments ready for prompt assembly.
#[derive(Debug, Clone, Serialize)]
pub struct InstructionBundle {
    /// Ordered list of instruction segments (lowest to highest precedence).
    pub segments: Vec<InstructionSegment>,
    /// Whether the bundle was truncated due to the byte budget.
    pub truncated: bool,
    /// Total bytes read from disk across all segments.
    pub bytes_read: usize,
}

impl InstructionBundle {
    /// Returns `true` if the bundle contains no instruction segments.
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Concatenates all segment contents into a single string separated by blank lines.
    pub fn combined_text(&self) -> String {
        let capacity = self
            .segments
            .iter()
            .map(|segment| segment.contents.len())
            .sum::<usize>()
            .saturating_add(self.segments.len().saturating_sub(1) * 2);
        let mut output = String::with_capacity(capacity);
        for (index, segment) in self.segments.iter().enumerate() {
            if index > 0 {
                output.push_str("\n\n");
            }

            output.push_str(&segment.contents);
        }
        output
    }

    /// Extracts up to `limit` key-point highlights from the bundle's segments.
    pub fn highlights(&self, limit: usize) -> Vec<String> {
        extract_instruction_highlights(&self.segments, limit)
    }
}

#[derive(Debug, Clone)]
pub struct InstructionDiscoveryOptions<'a> {
    pub current_dir: &'a Path,
    pub project_root: &'a Path,
    pub home_dir: Option<&'a Path>,
    pub extra_patterns: &'a [String],
    pub fallback_filenames: &'a [String],
    pub exclude_patterns: &'a [String],
    pub match_paths: &'a [PathBuf],
    pub import_max_depth: usize,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct RuleFrontmatter {
    #[serde(default)]
    paths: Vec<String>,
}

#[derive(Debug, Clone)]
struct RuleDescriptor {
    patterns: Vec<String>,
    specificity: usize,
}

#[derive(Debug, Clone)]
pub(super) struct MatchCandidate {
    relative_path: String,
    is_dir: bool,
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

pub fn extract_instruction_highlights(segments: &[InstructionSegment], limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }

    let mut highlights = Vec::with_capacity(limit);
    for segment in segments {
        let mut found_bullet = false;
        for line in segment.contents.lines() {
            if highlights.len() >= limit {
                break;
            }

            let trimmed = line.trim();
            if trimmed.starts_with('-') {
                let highlight = trimmed.trim_start_matches('-').trim();
                if !highlight.is_empty() {
                    highlights.push(highlight.to_string());
                    found_bullet = true;
                }
            }
        }

        if highlights.len() >= limit {
            break;
        }

        if found_bullet {
            continue;
        }

        for line in segment.contents.lines() {
            if highlights.len() >= limit {
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let fallback = trimmed.trim_start_matches('#').trim();
            if !fallback.is_empty() {
                highlights.push(fallback.to_string());
                break;
            }
        }

        if highlights.len() >= limit {
            break;
        }
    }

    highlights
}

pub fn render_instruction_markdown(
    title: &str,
    segments: &[InstructionSegment],
    truncated: bool,
    project_root: &Path,
    home_dir: Option<&Path>,
    truncation_note: &str,
) -> String {
    let combined_len = segments.iter().map(|segment| segment.contents.len()).sum::<usize>();
    let mut section = String::with_capacity(combined_len.saturating_add(512));
    let _ = writeln!(section, "## {title}\n");

    // Full segment bodies are inlined below, so a "Key points" highlight list
    // is a pure echo of content the model already receives (AGENTS.md alone
    // re-printed ~1.2 KB of the first bullets). Skip it. The map and
    // precedence sentence only pay for themselves with multiple sources.
    let show_map = segments.len() > 1;
    if show_map {
        section.push_str(
            "Instructions are listed from lowest to highest precedence. When conflicts exist, defer to the later entries.\n\n",
        );
        section.push_str("### Instruction map\n");
        for (index, segment) in segments.iter().enumerate() {
            let _ = writeln!(
                section,
                "- {}. {} ({})",
                index + 1,
                format_instruction_path(&segment.source.path, project_root, home_dir),
                instruction_source_label(&segment.source),
            );
        }
    }

    if !segments.is_empty() {
        for (index, segment) in segments.iter().enumerate() {
            let _ = writeln!(
                section,
                "\n### {}. {} ({})\n",
                index + 1,
                format_instruction_path(&segment.source.path, project_root, home_dir),
                instruction_source_label(&segment.source),
            );
            section.push_str(segment.contents.trim());
            section.push('\n');
        }
    }

    if truncated && !truncation_note.is_empty() {
        let _ = writeln!(section, "\n_{truncation_note}_");
    }

    section.push('\n');
    section
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

pub async fn read_instruction_bundle(
    options: &InstructionDiscoveryOptions<'_>,
    max_bytes: usize,
) -> Result<Option<InstructionBundle>> {
    if max_bytes == 0 {
        return Ok(None);
    }

    let sources = discover_instruction_sources(options).await?;
    if sources.is_empty() {
        return Ok(None);
    }

    let allowed_import_roots = allowed_import_roots(options.project_root, options.home_dir).await?;
    let workspace_import_root = canonicalize_async(options.project_root)
        .await
        .with_context(|| format!("Failed to canonicalize project root {}", options.project_root.display()))?;
    let import_max_depth = options.import_max_depth.max(1);

    // expand_instruction_contents performs blocking filesystem I/O (read_to_string,
    // canonicalize) for each instruction file and its recursive imports. Offload
    // the entire expansion loop to the blocking thread pool.
    let (segments, truncated, bytes_read) = tokio::task::spawn_blocking(move || {
        let mut remaining = max_bytes;
        let mut segments = Vec::with_capacity(sources.len());
        let mut truncated = false;
        let mut bytes_read = 0usize;
        let mut seen_imports = HashSet::new();

        for source in sources {
            if remaining == 0 {
                truncated = true;
                break;
            }

            let import_policy = ImportPolicy {
                workspace_root: &workspace_import_root,
                allowed_roots: if source.scope == InstructionScope::User
                    && !source.path.starts_with(&workspace_import_root)
                {
                    &allowed_import_roots
                } else {
                    std::slice::from_ref(&workspace_import_root)
                },
            };
            let contents = match expand_instruction_contents(
                &source.path,
                &source.kind,
                import_policy,
                import_max_depth,
                &mut seen_imports,
                0,
                &mut Vec::new(),
            )? {
                Some(contents) => contents,
                None => continue,
            };

            if contents.len() > remaining {
                truncated = true;
            }

            let slice_len = contents.len().min(remaining);
            let visible = String::from_utf8_lossy(&contents.as_bytes()[..slice_len]).to_string();
            if visible.trim().is_empty() {
                remaining = remaining.saturating_sub(slice_len);
                continue;
            }

            bytes_read += slice_len;
            remaining = remaining.saturating_sub(slice_len);
            segments.push(InstructionSegment { source, contents: visible });
        }

        Ok::<_, anyhow::Error>((segments, truncated, bytes_read))
    })
    .await
    .context("Instruction expansion task panicked")??;

    if segments.is_empty() {
        Ok(None)
    } else {
        Ok(Some(InstructionBundle { segments, truncated, bytes_read }))
    }
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

fn user_global_instruction_roots(home: &Path) -> Vec<PathBuf> {
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

#[derive(Clone, Copy)]
struct ImportPolicy<'a> {
    workspace_root: &'a PathBuf,
    allowed_roots: &'a [PathBuf],
}

fn expand_instruction_contents(
    path: &Path,
    kind: &InstructionSourceKind,
    import_policy: ImportPolicy<'_>,
    max_depth: usize,
    seen_imports: &mut HashSet<PathBuf>,
    depth: usize,
    stack: &mut Vec<PathBuf>,
) -> Result<Option<String>> {
    let canonical = canonicalize_with_context(path, "instruction source")?;
    if depth > 0 {
        if stack.contains(&canonical) {
            warn!("Skipping cyclic instruction import `{}` while expanding `{}`", canonical.display(), path.display());
            return Ok(None);
        }
        if depth > max_depth {
            warn!(
                "Skipping instruction import `{}` because it exceeds the max depth of {}",
                canonical.display(),
                max_depth
            );
            return Ok(None);
        }
        if !seen_imports.insert(canonical.clone()) {
            return Ok(None);
        }
    }

    stack.push(canonical.clone());

    let raw = match std::fs::read_to_string(&canonical) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            stack.pop();
            return Ok(None);
        }
        Err(err) => {
            stack.pop();
            return Err(err).with_context(|| format!("Failed to open instruction file {}", canonical.display()));
        }
    };
    let contents_without_frontmatter = match kind {
        InstructionSourceKind::Rule => strip_rule_frontmatter(&raw),
        InstructionSourceKind::Agents | InstructionSourceKind::Extra => raw,
    };
    let sanitized = strip_html_comments(&contents_without_frontmatter);
    let output = expand_inline_imports(&sanitized, &canonical, import_policy, max_depth, seen_imports, depth, stack)?;

    stack.pop();

    if output.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(output))
    }
}

fn expand_inline_imports(
    contents: &str,
    containing_file: &Path,
    import_policy: ImportPolicy<'_>,
    max_depth: usize,
    seen_imports: &mut HashSet<PathBuf>,
    depth: usize,
    stack: &mut Vec<PathBuf>,
) -> Result<String> {
    let mut output = String::new();
    let mut in_code_block = false;

    for line in contents.lines() {
        let trimmed = line.trim_start();
        if is_fence_line(trimmed) {
            in_code_block = !in_code_block;
            output.push_str(line);
            output.push('\n');
            continue;
        }

        output.push_str(line);
        output.push('\n');

        if in_code_block {
            continue;
        }

        let imports = collect_imports(line);
        for import in imports {
            let Some(import_path) = resolve_import_path(&import, containing_file, import_policy.allowed_roots)? else {
                continue;
            };

            let import_kind = infer_instruction_kind(&import_path);
            let child_policy = if import_path.starts_with(import_policy.workspace_root) {
                ImportPolicy {
                    allowed_roots: std::slice::from_ref(import_policy.workspace_root),
                    ..import_policy
                }
            } else {
                import_policy
            };
            let imported = expand_instruction_contents(
                &import_path,
                &import_kind,
                child_policy,
                max_depth,
                seen_imports,
                depth + 1,
                stack,
            )?;
            let Some(imported) = imported else {
                continue;
            };

            if imported.trim().is_empty() {
                continue;
            }

            let _ = writeln!(output, "[Imported from {}]", import_path.display());
            output.push_str(imported.trim());
            output.push_str("\n\n");
        }
    }

    Ok(output.trim().to_string())
}

fn infer_instruction_kind(path: &Path) -> InstructionSourceKind {
    if path.components().any(|component| component.as_os_str() == "rules") {
        InstructionSourceKind::Rule
    } else if path.file_name().and_then(|value| value.to_str()) == Some(AGENTS_FILENAME)
        || path.file_name().and_then(|value| value.to_str()) == Some(AGENTS_OVERRIDE_FILENAME)
    {
        InstructionSourceKind::Agents
    } else {
        InstructionSourceKind::Extra
    }
}

#[cfg(test)]
mod tests;
