use hashbrown::HashSet;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use tokio::io;

use crate::utils::file_utils::canonicalize_with_context;
use anyhow::{Context, Result, anyhow};
use glob::{Pattern, glob};
use tracing::warn;
use walkdir::WalkDir;

const AGENTS_FILENAME: &str = "AGENTS.md";
const AGENTS_OVERRIDE_FILENAME: &str = "AGENTS.override.md";
const GLOBAL_CONFIG_DIRECTORY: &str = ".config/vtcode";
const RULES_DIRECTORY: &str = ".vtcode/rules";
const IMPORT_PROBE_NAME: &str = "__vtcode_instruction_probe__";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum InstructionScope {
    User,
    Workspace,
    Custom,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstructionSourceKind {
    Agents,
    Rule,
    Extra,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstructionSource {
    pub path: PathBuf,
    pub scope: InstructionScope,
    pub kind: InstructionSourceKind,
    pub matched: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstructionSegment {
    pub source: InstructionSource,
    pub contents: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstructionBundle {
    pub segments: Vec<InstructionSegment>,
    pub truncated: bool,
    pub bytes_read: usize,
}

impl InstructionBundle {
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

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
struct RuleFrontmatter {
    #[serde(default)]
    paths: Vec<String>,
}

#[derive(Debug, Clone)]
struct RuleDescriptor {
    patterns: Vec<String>,
    specificity: usize,
}

#[derive(Debug, Clone)]
struct MatchCandidate {
    relative_path: String,
    is_dir: bool,
}

#[derive(Debug, Clone)]
struct MatchContext {
    candidates: Vec<MatchCandidate>,
}

impl MatchContext {
    fn new(project_root: &Path, match_paths: &[PathBuf]) -> Self {
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        let canonical_root =
            canonicalize_with_context(project_root, "instruction match project root").ok();

        for raw_path in match_paths {
            let candidate = if raw_path.is_absolute() {
                raw_path.to_path_buf()
            } else {
                project_root.join(raw_path)
            };

            let normalized =
                std::fs::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone());
            let relative = normalized
                .strip_prefix(project_root)
                .ok()
                .or_else(|| {
                    canonical_root
                        .as_ref()
                        .and_then(|root| normalized.strip_prefix(root).ok())
                })
                .or_else(|| candidate.strip_prefix(project_root).ok())
                .or_else(|| {
                    canonical_root
                        .as_ref()
                        .and_then(|root| candidate.strip_prefix(root).ok())
                });
            let Some(relative) = relative else {
                continue;
            };

            let relative = relative.display().to_string();
            if relative.is_empty() {
                continue;
            }

            let is_dir = normalized.is_dir();
            let key = format!("{relative}:{is_dir}");
            if seen.insert(key) {
                candidates.push(MatchCandidate {
                    relative_path: relative,
                    is_dir,
                });
            }
        }

        candidates.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Self { candidates }
    }

    fn matches_any(&self, patterns: &[String]) -> bool {
        if patterns.is_empty() {
            return false;
        }

        patterns.iter().any(|raw_pattern| {
            let trimmed = raw_pattern.trim();
            if trimmed.is_empty() {
                return false;
            }

            let Ok(pattern) = Pattern::new(trimmed) else {
                warn!("Ignoring invalid instruction rule path pattern `{trimmed}`");
                return false;
            };

            self.candidates
                .iter()
                .any(|candidate| pattern_matches_candidate(&pattern, candidate))
        })
    }
}

#[derive(Debug, Clone)]
struct ExclusionMatcher {
    patterns: Vec<Pattern>,
}

impl ExclusionMatcher {
    fn compile(
        project_root: &Path,
        home_dir: Option<&Path>,
        raw_patterns: &[String],
    ) -> Result<Self> {
        let mut patterns = Vec::new();
        for raw in raw_patterns {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let resolved = resolve_pattern(trimmed, project_root, home_dir)?;
            let pattern = Pattern::new(&resolved).with_context(|| {
                format!("Failed to compile instruction exclude pattern `{trimmed}`")
            })?;
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

pub fn extract_instruction_highlights(
    segments: &[InstructionSegment],
    limit: usize,
) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }

    let mut highlights = Vec::with_capacity(limit);
    for segment in segments {
        for line in segment.contents.lines() {
            if highlights.len() >= limit {
                break;
            }

            let trimmed = line.trim();
            if trimmed.starts_with('-') {
                let highlight = trimmed.trim_start_matches('-').trim();
                if !highlight.is_empty() {
                    highlights.push(highlight.to_string());
                }
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
    highlight_limit: usize,
    truncation_note: &str,
) -> String {
    let combined_len = segments
        .iter()
        .map(|segment| segment.contents.len())
        .sum::<usize>();
    let mut section = String::with_capacity(combined_len.saturating_add(512));
    let _ = writeln!(section, "## {title}\n");
    section.push_str(
        "Instructions are listed from lowest to highest precedence. When conflicts exist, defer to the later entries.\n\n",
    );

    if !segments.is_empty() {
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

        let highlights = extract_instruction_highlights(segments, highlight_limit);
        if !highlights.is_empty() {
            section.push_str("\n### Key points\n");
            for highlight in highlights {
                let _ = writeln!(section, "- {highlight}");
            }
        }

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

pub fn render_instruction_summary_markdown(
    title: &str,
    segments: &[InstructionSegment],
    truncated: bool,
    project_root: &Path,
    home_dir: Option<&Path>,
    highlight_limit: usize,
    truncation_note: &str,
) -> String {
    let mut section = String::with_capacity(1024);
    let _ = writeln!(section, "## {title}\n");
    section.push_str(
        "Instructions are listed from lowest to highest precedence. When conflicts exist, defer to the later entries.\n\n",
    );

    if !segments.is_empty() {
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

        let highlights = extract_instruction_highlights(segments, highlight_limit);
        if !highlights.is_empty() {
            section.push_str("\n### Key points\n");
            for highlight in highlights {
                let _ = writeln!(section, "- {highlight}");
            }
        }

        section.push_str(
            "\n### On-demand loading\n- This prompt only indexes instruction files.\n- Full instruction files stay on disk and are not inlined here.\n- Use the available file-read tools to open a listed file when exact wording or deeper details matter.\n",
        );
    }

    if truncated && !truncation_note.is_empty() {
        let _ = writeln!(section, "\n_{truncation_note}_");
    }

    section.push('\n');
    section
}

pub fn instruction_scope_label(scope: &InstructionScope) -> &'static str {
    match scope {
        InstructionScope::User => "user",
        InstructionScope::Workspace => "workspace",
        InstructionScope::Custom => "custom",
    }
}

pub fn instruction_source_label(source: &InstructionSource) -> String {
    match source.kind {
        InstructionSourceKind::Agents => {
            format!("{} AGENTS", instruction_scope_label(&source.scope))
        }
        InstructionSourceKind::Extra => {
            format!(
                "{} extra instructions",
                instruction_scope_label(&source.scope)
            )
        }
        InstructionSourceKind::Rule if source.matched => {
            format!("{} matched rule", instruction_scope_label(&source.scope))
        }
        InstructionSourceKind::Rule => {
            format!("{} rule", instruction_scope_label(&source.scope))
        }
    }
}

pub fn format_instruction_path(
    path: &Path,
    project_root: &Path,
    home_dir: Option<&Path>,
) -> String {
    if let Ok(relative) = path.strip_prefix(project_root) {
        let display = relative.display().to_string();
        if !display.is_empty() {
            return display;
        }

        if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
            return name.to_string();
        }
    }

    if let Some(home) = home_dir
        && let Ok(relative) = path.strip_prefix(home)
    {
        let display = relative.display().to_string();
        if display.is_empty() {
            return "~".to_string();
        }

        return format!("~/{display}");
    }

    path.display().to_string()
}

pub async fn discover_instruction_sources(
    options: &InstructionDiscoveryOptions<'_>,
) -> Result<Vec<InstructionSource>> {
    let mut sources = Vec::with_capacity(16);
    let mut seen_paths = HashSet::new();
    let excludes = ExclusionMatcher::compile(
        options.project_root,
        options.home_dir,
        options.exclude_patterns,
    )?;
    let match_context = MatchContext::new(options.project_root, options.match_paths);

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

        let (user_unconditional_rules, user_matched_rules) = discover_rule_sources(
            user_rules_roots(home),
            InstructionScope::User,
            &match_context,
            &excludes,
        )
        .await?;
        for source in user_unconditional_rules
            .into_iter()
            .chain(user_matched_rules.into_iter())
        {
            if seen_paths.insert(source.path.clone()) {
                sources.push(source);
            }
        }
    }

    let extra_paths = expand_instruction_patterns(
        options.project_root,
        options.home_dir,
        options.extra_patterns,
        &excludes,
    )
    .await?;
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

    let root = canonicalize_with_context(options.project_root, "project root")?;
    let mut cursor = canonicalize_with_context(options.current_dir, "working directory")?;
    if !cursor.starts_with(&root) {
        cursor = root.clone();
    }

    let mut workspace_paths = Vec::with_capacity(4);
    loop {
        let chosen =
            select_workspace_instruction_candidate(&cursor, options.fallback_filenames, &excludes)
                .await?;

        if let Some(path) = chosen
            && seen_paths.insert(path.clone())
        {
            workspace_paths.push(InstructionSource {
                path,
                scope: InstructionScope::Workspace,
                kind: InstructionSourceKind::Agents,
                matched: false,
            });
        }

        if cursor == root {
            break;
        }

        cursor = cursor
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("Reached filesystem root before encountering project root"))?;
    }

    workspace_paths.reverse();
    sources.extend(workspace_paths);

    let (workspace_unconditional_rules, workspace_matched_rules) = discover_rule_sources(
        vec![root.join(RULES_DIRECTORY)],
        InstructionScope::Workspace,
        &match_context,
        &excludes,
    )
    .await?;
    for source in workspace_unconditional_rules
        .into_iter()
        .chain(workspace_matched_rules.into_iter())
    {
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

    let allowed_import_roots = allowed_import_roots(options.project_root, options.home_dir)?;
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

        let contents = match expand_instruction_contents(
            &source.path,
            &source.kind,
            &allowed_import_roots,
            options.import_max_depth.max(1),
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
        segments.push(InstructionSegment {
            source,
            contents: visible,
        });
    }

    if segments.is_empty() {
        Ok(None)
    } else {
        Ok(Some(InstructionBundle {
            segments,
            truncated,
            bytes_read,
        }))
    }
}

fn user_instruction_candidates(home: &Path, fallback_filenames: &[String]) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let roots = [
        home.to_path_buf(),
        home.join(".vtcode"),
        home.join(GLOBAL_CONFIG_DIRECTORY),
    ];
    for root in roots {
        candidates.extend(instruction_candidates_for_dir(&root, fallback_filenames));
    }
    candidates
}

fn user_rules_roots(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(RULES_DIRECTORY),
        home.join(GLOBAL_CONFIG_DIRECTORY).join("rules"),
    ]
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
    fallback_filenames: &[String],
    excludes: &ExclusionMatcher,
) -> Result<Option<PathBuf>> {
    for candidate in instruction_candidates_for_dir(dir, fallback_filenames) {
        if let Some(path) = normalize_instruction_candidate(&candidate, excludes).await? {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

async fn normalize_instruction_candidate(
    candidate: &Path,
    excludes: &ExclusionMatcher,
) -> Result<Option<PathBuf>> {
    if !instruction_exists(candidate).await? {
        return Ok(None);
    }

    let canonical = canonicalize_with_context(candidate, "instruction candidate")?;
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
) -> Result<(Vec<InstructionSource>, Vec<InstructionSource>)> {
    let mut unconditional = Vec::new();
    let mut matched = Vec::new();
    let mut seen = HashSet::new();

    for root in rule_roots {
        if !root.exists() {
            continue;
        }

        for entry in WalkDir::new(&root)
            .follow_links(true)
            .sort_by_file_name()
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }

            if entry.path().extension().and_then(|value| value.to_str()) != Some("md") {
                continue;
            }

            let path = canonicalize_with_context(entry.path(), "instruction rule")?;
            if excludes.matches(&path) || !seen.insert(path.clone()) {
                continue;
            }

            let descriptor = read_rule_descriptor(&path).await?;
            let is_matched = if descriptor.patterns.is_empty() {
                false
            } else {
                match_context.matches_any(&descriptor.patterns)
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
    }

    unconditional.sort_by(|left, right| left.path.cmp(&right.path));
    matched.sort_by(|(left_specificity, left), (right_specificity, right)| {
        left_specificity
            .cmp(right_specificity)
            .then(left.path.cmp(&right.path))
    });

    Ok((
        unconditional,
        matched.into_iter().map(|(_, source)| source).collect(),
    ))
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

    Ok(RuleDescriptor {
        patterns: frontmatter.paths,
        specificity,
    })
}

async fn expand_instruction_patterns(
    project_root: &Path,
    home_dir: Option<&Path>,
    patterns: &[String],
    excludes: &ExclusionMatcher,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    for pattern in patterns {
        let resolved = resolve_pattern(pattern, project_root, home_dir)?;
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

        let mut matches = Vec::new();
        for path in glob_matches {
            match normalize_instruction_candidate(&path, excludes).await {
                Ok(Some(canonical)) if seen.insert(canonical.clone()) => matches.push(canonical),
                Ok(Some(_)) | Ok(None) => {}
                Err(err) => {
                    warn!(
                        "Failed to inspect potential instruction `{}`: {err:#}",
                        path.display()
                    );
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

fn resolve_pattern(pattern: &str, project_root: &Path, home_dir: Option<&Path>) -> Result<String> {
    if let Some(stripped) = pattern.strip_prefix("~/") {
        let home = home_dir.ok_or_else(|| {
            anyhow!("Cannot expand `~` in instruction pattern `{pattern}` without a home directory")
        })?;
        let resolved = home.join(stripped);
        if !contains_glob_meta(stripped) && resolved.exists() {
            return Ok(canonicalize_with_context(&resolved, "instruction pattern")?
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

    if !contains_glob_meta(pattern) && full_path.exists() {
        return Ok(
            canonicalize_with_context(&full_path, "instruction pattern")?
                .to_string_lossy()
                .into_owned(),
        );
    }

    Ok(full_path.to_string_lossy().into_owned())
}

async fn instruction_exists(path: &Path) -> Result<bool> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => Ok(metadata.file_type().is_file() || metadata.file_type().is_symlink()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err)
            .with_context(|| format!("Failed to inspect instruction candidate {}", path.display())),
    }
}

fn expand_instruction_contents(
    path: &Path,
    kind: &InstructionSourceKind,
    allowed_roots: &[PathBuf],
    max_depth: usize,
    seen_imports: &mut HashSet<PathBuf>,
    depth: usize,
    stack: &mut Vec<PathBuf>,
) -> Result<Option<String>> {
    let canonical = canonicalize_with_context(path, "instruction source")?;
    if depth > 0 {
        if stack.contains(&canonical) {
            warn!(
                "Skipping cyclic instruction import `{}` while expanding `{}`",
                canonical.display(),
                path.display()
            );
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
            return Err(err).with_context(|| {
                format!("Failed to open instruction file {}", canonical.display())
            });
        }
    };
    let contents_without_frontmatter = match kind {
        InstructionSourceKind::Rule => strip_rule_frontmatter(&raw),
        InstructionSourceKind::Agents | InstructionSourceKind::Extra => raw,
    };
    let sanitized = strip_html_comments(&contents_without_frontmatter);
    let output = expand_inline_imports(
        &sanitized,
        &canonical,
        allowed_roots,
        max_depth,
        seen_imports,
        depth,
        stack,
    )?;

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
    allowed_roots: &[PathBuf],
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
            let Some(import_path) = resolve_import_path(&import, containing_file, allowed_roots)?
            else {
                continue;
            };

            let import_kind = infer_instruction_kind(&import_path);
            let imported = expand_instruction_contents(
                &import_path,
                &import_kind,
                allowed_roots,
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
    if path
        .components()
        .any(|component| component.as_os_str() == "rules")
    {
        InstructionSourceKind::Rule
    } else if path.file_name().and_then(|value| value.to_str()) == Some(AGENTS_FILENAME)
        || path.file_name().and_then(|value| value.to_str()) == Some(AGENTS_OVERRIDE_FILENAME)
    {
        InstructionSourceKind::Agents
    } else {
        InstructionSourceKind::Extra
    }
}

fn parse_rule_frontmatter(contents: &str, path: &Path) -> Result<RuleFrontmatter> {
    let Some((frontmatter, _body)) = split_frontmatter(contents) else {
        return Ok(RuleFrontmatter::default());
    };

    serde_yaml::from_str(frontmatter).with_context(|| {
        format!(
            "Failed to parse YAML frontmatter for instruction rule {}",
            path.display()
        )
    })
}

fn strip_rule_frontmatter(contents: &str) -> String {
    split_frontmatter(contents)
        .map(|(_, body)| body.to_string())
        .unwrap_or_else(|| contents.to_string())
}

fn split_frontmatter(contents: &str) -> Option<(&str, &str)> {
    let mut lines = contents.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }

    let mut offset = first.len();
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed == "---" || trimmed == "..." {
            let body_start = offset + line.len();
            let frontmatter = &contents[first.len()..offset];
            let body = contents.get(body_start..).unwrap_or_default();
            return Some((frontmatter, body));
        }
        offset += line.len();
    }

    None
}

fn pattern_matches_candidate(pattern: &Pattern, candidate: &MatchCandidate) -> bool {
    if pattern_matches_path(pattern, candidate) {
        return true;
    }

    zero_directory_pattern_variants(pattern.as_str())
        .into_iter()
        .any(|variant| {
            Pattern::new(&variant)
                .ok()
                .is_some_and(|variant_pattern| pattern_matches_path(&variant_pattern, candidate))
        })
}

fn pattern_matches_path(pattern: &Pattern, candidate: &MatchCandidate) -> bool {
    let path = Path::new(candidate.relative_path.as_str());
    if pattern.matches_path(path) || pattern.matches(candidate.relative_path.as_str()) {
        return true;
    }

    if candidate.is_dir {
        let probe_path = path.join(IMPORT_PROBE_NAME);
        return pattern.matches_path(&probe_path);
    }

    false
}

fn zero_directory_pattern_variants(pattern: &str) -> Vec<String> {
    let mut variants = Vec::new();
    let mut seen = HashSet::new();
    collect_zero_directory_variants(pattern, &mut seen, &mut variants);
    variants
}

fn collect_zero_directory_variants(
    pattern: &str,
    seen: &mut HashSet<String>,
    variants: &mut Vec<String>,
) {
    let mut search_start = 0usize;
    while let Some(relative_index) = pattern[search_start..].find("**/") {
        let index = search_start + relative_index;
        let variant = format!("{}{}", &pattern[..index], &pattern[index + 3..]);
        if seen.insert(variant.clone()) {
            variants.push(variant.clone());
            collect_zero_directory_variants(&variant, seen, variants);
        }
        search_start = index + 3;
    }
}

fn rule_specificity(pattern: &str) -> usize {
    pattern
        .chars()
        .filter(|ch| !matches!(ch, '*' | '?' | '[' | ']' | '{' | '}' | ','))
        .count()
}

fn contains_glob_meta(pattern: &str) -> bool {
    pattern
        .chars()
        .any(|ch| matches!(ch, '*' | '?' | '[' | ']' | '{' | '}'))
}

fn strip_html_comments(contents: &str) -> String {
    let mut output = String::with_capacity(contents.len());
    let mut in_code_block = false;
    let mut in_comment = false;

    for line in contents.lines() {
        let trimmed = line.trim_start();
        if !in_comment && is_fence_line(trimmed) {
            in_code_block = !in_code_block;
            output.push_str(line);
            output.push('\n');
            continue;
        }

        if in_code_block {
            output.push_str(line);
            output.push('\n');
            continue;
        }

        let mut cursor = line;
        let mut rendered = String::new();
        loop {
            if in_comment {
                if let Some(end) = cursor.find("-->") {
                    cursor = &cursor[end + 3..];
                    in_comment = false;
                    continue;
                }
                cursor = "";
                break;
            }

            let Some(start) = cursor.find("<!--") else {
                rendered.push_str(cursor);
                cursor = "";
                break;
            };

            rendered.push_str(&cursor[..start]);
            cursor = &cursor[start + 4..];
            if let Some(end) = cursor.find("-->") {
                cursor = &cursor[end + 3..];
                continue;
            }

            in_comment = true;
            cursor = "";
            break;
        }

        if !rendered.is_empty() || !cursor.is_empty() {
            output.push_str(rendered.trim_end_matches('\r'));
        }
        output.push('\n');
    }

    output
}

fn is_fence_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

fn collect_imports(contents: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut in_code_block = false;

    for line in contents.lines() {
        let trimmed = line.trim_start();
        if is_fence_line(trimmed) {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        for token in line.split_whitespace() {
            let Some(candidate) = token.strip_prefix('@') else {
                continue;
            };
            let trimmed = candidate.trim_matches(|ch: char| {
                matches!(ch, ')' | '(' | '[' | ']' | '{' | '}' | ',' | ';' | ':')
            });
            let trimmed = trimmed.trim_end_matches('.');
            if trimmed.is_empty() {
                continue;
            }
            imports.push(trimmed.to_string());
        }
    }

    imports
}

fn resolve_import_path(
    import: &str,
    containing_file: &Path,
    allowed_roots: &[PathBuf],
) -> Result<Option<PathBuf>> {
    let parent = containing_file.parent().ok_or_else(|| {
        anyhow!(
            "Instruction file {} has no parent",
            containing_file.display()
        )
    })?;
    let home_dir = dirs::home_dir();

    let candidate = if let Some(stripped) = import.strip_prefix("~/") {
        let Some(home_dir) = home_dir else {
            warn!(
                "Skipping instruction import `@{import}` because the home directory is unavailable"
            );
            return Ok(None);
        };
        home_dir.join(stripped)
    } else {
        let import_path = Path::new(import);
        if import_path.is_absolute() {
            import_path.to_path_buf()
        } else {
            parent.join(import_path)
        }
    };

    let canonical = match std::fs::canonicalize(&candidate) {
        Ok(path) => path,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            warn!(
                "Skipping missing instruction import `@{}` referenced from {}",
                import,
                containing_file.display()
            );
            return Ok(None);
        }
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "Failed to resolve instruction import `@{}` from {}",
                    import,
                    containing_file.display()
                )
            });
        }
    };

    let allowed = allowed_roots.iter().any(|root| canonical.starts_with(root));
    if !allowed {
        warn!(
            "Skipping instruction import `{}` because it is outside the allowed roots",
            canonical.display()
        );
        return Ok(None);
    }

    Ok(Some(canonical))
}

fn allowed_import_roots(project_root: &Path, home_dir: Option<&Path>) -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    roots.push(canonicalize_with_context(
        project_root,
        "project root import root",
    )?);

    if let Some(home) = home_dir {
        roots.push(canonicalize_with_context(home, "home import root")?);

        let vtcode_dir = home.join(".vtcode");
        if vtcode_dir.exists() {
            roots.push(canonicalize_with_context(
                &vtcode_dir,
                "vtcode user import root",
            )?);
        }

        let legacy_dir = home.join(GLOBAL_CONFIG_DIRECTORY);
        if legacy_dir.exists() {
            roots.push(canonicalize_with_context(
                &legacy_dir,
                "legacy vtcode user import root",
            )?);
        }
    }

    roots.sort();
    roots.dedup();
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_doc(dir: &Path, content: &str) -> Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("AGENTS.md");
        std::fs::write(&path, content)?;
        Ok(path)
    }

    fn write_rule(dir: &Path, name: &str, content: &str) -> Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(name);
        std::fs::write(&path, content)?;
        Ok(path)
    }

    fn default_options<'a>(
        current_dir: &'a Path,
        project_root: &'a Path,
        home_dir: Option<&'a Path>,
        match_paths: &'a [PathBuf],
    ) -> InstructionDiscoveryOptions<'a> {
        InstructionDiscoveryOptions {
            current_dir,
            project_root,
            home_dir,
            extra_patterns: &[],
            fallback_filenames: &[],
            exclude_patterns: &[],
            match_paths,
            import_max_depth: 5,
        }
    }

    #[tokio::test]
    async fn discovers_user_workspace_and_rule_sources_in_priority_order() -> Result<()> {
        let workspace = tempdir()?;
        let project_root = workspace.path();
        let nested = project_root.join("src/app");
        std::fs::create_dir_all(&nested)?;

        let home = tempdir()?;
        write_doc(&home.path().join(".vtcode"), "user agents")?;
        write_rule(
            &home.path().join(".vtcode/rules"),
            "shared.md",
            "# Shared\n- user shared rule\n",
        )?;
        write_rule(
            &home.path().join(".vtcode/rules"),
            "matched.md",
            "---\npaths:\n  - \"src/**/*.rs\"\n---\n# Matched\n- user matched rule\n",
        )?;

        write_doc(project_root, "root agents")?;
        write_doc(&project_root.join("src"), "nested agents")?;
        write_rule(
            &project_root.join(".vtcode/rules"),
            "workspace.md",
            "# Workspace\n- workspace rule\n",
        )?;
        write_rule(
            &project_root.join(".vtcode/rules"),
            "workspace-matched.md",
            "---\npaths:\n  - \"src/**/*.rs\"\n---\n# Workspace Matched\n- workspace matched rule\n",
        )?;

        let match_paths = vec![project_root.join("src/main.rs")];
        let sources = discover_instruction_sources(&default_options(
            &nested,
            project_root,
            Some(home.path()),
            &match_paths,
        ))
        .await?;

        let labels = sources
            .iter()
            .map(instruction_source_label)
            .collect::<Vec<_>>();

        assert_eq!(
            labels,
            vec![
                "user AGENTS",
                "user rule",
                "user matched rule",
                "workspace AGENTS",
                "workspace AGENTS",
                "workspace rule",
                "workspace matched rule",
            ]
        );

        Ok(())
    }

    #[tokio::test]
    async fn expands_imports_strips_comments_and_matches_rule_paths() -> Result<()> {
        let workspace = tempdir()?;
        let project_root = workspace.path();
        let docs_dir = project_root.join("docs");
        std::fs::create_dir_all(&docs_dir)?;
        std::fs::write(docs_dir.join("shared.md"), "# Shared\n- imported detail\n")?;

        write_doc(
            project_root,
            "# Root\n<!-- hidden -->\n- visible\n\nSee @docs/shared.md\n- trailing detail\n",
        )?;
        write_rule(
            &project_root.join(".vtcode/rules"),
            "rust.md",
            "---\npaths:\n  - \"src/**/*.rs\"\n---\n# Rust\n- rust rule\n",
        )?;

        let match_paths = vec![project_root.join("src/lib.rs")];
        let bundle = read_instruction_bundle(
            &default_options(project_root, project_root, None, &match_paths),
            16 * 1024,
        )
        .await?
        .expect("instruction bundle");

        assert_eq!(bundle.segments.len(), 2);
        let combined = bundle.combined_text();
        assert!(combined.contains("- visible"));
        assert!(combined.contains("imported detail"));
        assert!(combined.contains("- rust rule"));
        assert!(!combined.contains("hidden"));
        assert!(combined.find("See @docs/shared.md") < combined.find("imported detail"));
        assert!(combined.find("imported detail") < combined.find("- trailing detail"));

        Ok(())
    }

    #[tokio::test]
    async fn excludes_instruction_paths_with_globs() -> Result<()> {
        let workspace = tempdir()?;
        let project_root = workspace.path();
        let nested = project_root.join("src");
        std::fs::create_dir_all(&nested)?;
        write_doc(project_root, "root agents")?;
        write_doc(&nested, "nested agents")?;

        let exclude = vec![project_root.join("src/AGENTS.md").display().to_string()];
        let options = InstructionDiscoveryOptions {
            exclude_patterns: &exclude,
            ..default_options(&nested, project_root, None, &[])
        };
        let sources = discover_instruction_sources(&options).await?;

        assert_eq!(sources.len(), 1);
        assert_eq!(
            sources[0]
                .path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default(),
            "AGENTS.md"
        );

        Ok(())
    }

    #[tokio::test]
    async fn render_summary_uses_source_labels() -> Result<()> {
        let segments = vec![InstructionSegment {
            source: InstructionSource {
                path: PathBuf::from("AGENTS.md"),
                scope: InstructionScope::Workspace,
                kind: InstructionSourceKind::Agents,
                matched: false,
            },
            contents: "- first\n".to_string(),
        }];

        let rendered = render_instruction_summary_markdown(
            "PROJECT DOCUMENTATION",
            &segments,
            false,
            Path::new("/workspace"),
            None,
            4,
            "",
        );

        assert!(rendered.contains("workspace AGENTS"));
        assert!(rendered.contains("first"));
        Ok(())
    }
}
