//! Shared prompt resource discovery for system prompt layers and prompt templates.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, SystemTime};

use serde::Deserialize;
use tokio::fs;
use tracing::warn;
use vtcode_commons::VtCodePaths;

const PROMPTS_DIR: &str = ".vtcode/prompts";
const TEMPLATES_DIR: &str = "templates";
const SYSTEM_PROMPT_FILENAME: &str = "system.md";
const APPEND_SYSTEM_PROMPT_FILENAME: &str = "append-system.md";
const PROMPT_RESOURCE_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const PROMPT_RESOURCE_CACHE_MAX_ENTRIES: usize = 32;

static SYSTEM_PROMPT_LAYERS_CACHE: OnceLock<RwLock<HashMap<PromptResourceCacheKey, CachedSystemPromptLayers>>> =
    OnceLock::new();
static PROMPT_TEMPLATES_CACHE: OnceLock<RwLock<HashMap<PromptResourceCacheKey, CachedPromptTemplates>>> =
    OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTemplate {
    pub name: String,
    pub description: String,
    pub body: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemPromptLayers {
    pub override_body: Option<String>,
    pub append_bodies: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum PromptResourceScope {
    User,
    Workspace,
}

#[derive(Debug, Clone)]
struct PromptResourceOptions<'a> {
    workspace_root: &'a Path,
    home_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PromptTemplateFrontmatter {
    description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PromptResourceCacheKey {
    workspace_root: PathBuf,
    home_dir: Option<PathBuf>,
}

impl PromptResourceCacheKey {
    fn new(options: &PromptResourceOptions<'_>) -> Self {
        Self {
            workspace_root: normalize_cache_path(options.workspace_root),
            home_dir: options.home_dir.as_deref().map(normalize_cache_path),
        }
    }
}

#[derive(Clone)]
struct CachedPromptTemplates {
    templates: Vec<PromptTemplate>,
    timestamp: SystemTime,
    source_mtime: SystemTime,
    source_paths: (PathBuf, Option<PathBuf>),
}

impl CachedPromptTemplates {
    fn is_expired(&self) -> bool {
        self.timestamp.elapsed().unwrap_or(PROMPT_RESOURCE_CACHE_TTL) > PROMPT_RESOURCE_CACHE_TTL
    }

    fn is_source_stale(&self) -> bool {
        let mut check: Vec<&Path> = vec![self.source_paths.0.as_path()];
        if let Some(ref h) = self.source_paths.1 {
            check.push(h.as_path());
        }
        newest_mtime(check) > self.source_mtime
    }
}

#[derive(Clone)]
struct CachedSystemPromptLayers {
    layers: SystemPromptLayers,
    timestamp: SystemTime,
    source_mtime: SystemTime,
}

impl CachedSystemPromptLayers {
    fn is_expired(&self) -> bool {
        self.timestamp.elapsed().unwrap_or(PROMPT_RESOURCE_CACHE_TTL) > PROMPT_RESOURCE_CACHE_TTL
    }

    fn is_source_stale(&self, check_paths: &[&Path]) -> bool {
        newest_mtime(check_paths.iter().copied()) > self.source_mtime
    }
}

fn normalize_cache_path(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn user_prompts_dir(home: &Path) -> PathBuf {
    if dirs::home_dir().as_deref() == Some(home)
        && let Ok(paths) = VtCodePaths::resolve()
        && let Ok(prompt_dir) = paths.config_path("prompts")
    {
        return prompt_dir;
    }
    home.join(PROMPTS_DIR)
}

/// Newest mtime across `paths`; missing or unreadable paths are ignored
/// (they contribute `UNIX_EPOCH`, never the maximum).
fn newest_mtime<'a>(paths: impl IntoIterator<Item = &'a Path>) -> SystemTime {
    let mut newest = SystemTime::UNIX_EPOCH;
    for path in paths {
        if let Ok(metadata) = std::fs::metadata(path) {
            if let Ok(mtime) = metadata.modified() {
                newest = newest.max(mtime);
            }
        }
    }
    newest
}

fn system_prompt_layers_cache() -> &'static RwLock<HashMap<PromptResourceCacheKey, CachedSystemPromptLayers>> {
    SYSTEM_PROMPT_LAYERS_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn prompt_templates_cache() -> &'static RwLock<HashMap<PromptResourceCacheKey, CachedPromptTemplates>> {
    PROMPT_TEMPLATES_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Entry contract for the bounded prompt-resource caches: expiry drives
/// reads, and expiry plus insertion age drive eviction.
trait ExpirableCacheEntry {
    fn is_expired(&self) -> bool;
    fn timestamp(&self) -> SystemTime;
}

impl ExpirableCacheEntry for CachedSystemPromptLayers {
    fn is_expired(&self) -> bool {
        self.is_expired()
    }

    fn timestamp(&self) -> SystemTime {
        self.timestamp
    }
}

impl ExpirableCacheEntry for CachedPromptTemplates {
    fn is_expired(&self) -> bool {
        self.is_expired()
    }

    fn timestamp(&self) -> SystemTime {
        self.timestamp
    }
}

/// Read-through guard shared by both caches: expired entries miss.
fn get_cached<K, V>(cache: &RwLock<HashMap<K, V>>, key: &K, what: &str) -> Option<V>
where
    K: Eq + std::hash::Hash,
    V: ExpirableCacheEntry + Clone,
{
    match cache.read() {
        Ok(cache) => cache.get(key).filter(|cached| !cached.is_expired()).cloned(),
        Err(_) => {
            warn!("{what} cache lock poisoned while reading cache");
            None
        }
    }
}

/// Make room for `key` when the cache is full: purge expired entries first,
/// then evict the oldest. No-op when under budget or when `key` is present.
fn evict_if_full<K, V>(cache: &mut HashMap<K, V>, key: &K)
where
    K: Eq + std::hash::Hash + Clone,
    V: ExpirableCacheEntry,
{
    if cache.len() < PROMPT_RESOURCE_CACHE_MAX_ENTRIES || cache.contains_key(key) {
        return;
    }
    let expired: Vec<K> = cache
        .iter()
        .filter(|(_, value)| value.is_expired())
        .map(|(cache_key, _)| cache_key.clone())
        .collect();
    for cache_key in expired {
        cache.remove(&cache_key);
    }

    if cache.len() >= PROMPT_RESOURCE_CACHE_MAX_ENTRIES {
        let oldest_key = cache
            .iter()
            .min_by_key(|(_, value)| value.timestamp())
            .map(|(cache_key, _)| cache_key.clone());
        if let Some(oldest_key) = oldest_key {
            cache.remove(&oldest_key);
        }
    }
}

fn get_cached_system_prompt_layers(key: &PromptResourceCacheKey) -> Option<CachedSystemPromptLayers> {
    get_cached(system_prompt_layers_cache(), key, "system prompt layers")
}

fn cache_system_prompt_layers(key: PromptResourceCacheKey, layers: &SystemPromptLayers) {
    match system_prompt_layers_cache().write() {
        Ok(mut cache) => {
            evict_if_full(&mut cache, &key);

            let mut source_paths = vec![
                key.workspace_root.join(PROMPTS_DIR).join(SYSTEM_PROMPT_FILENAME),
                key.workspace_root.join(PROMPTS_DIR).join(APPEND_SYSTEM_PROMPT_FILENAME),
            ];
            if let Some(ref home) = key.home_dir {
                let prompt_dir = user_prompts_dir(home);
                source_paths.push(prompt_dir.join(SYSTEM_PROMPT_FILENAME));
                source_paths.push(prompt_dir.join(APPEND_SYSTEM_PROMPT_FILENAME));
            }
            let source_mtime = newest_mtime(source_paths.iter().map(PathBuf::as_path));

            cache.insert(
                key,
                CachedSystemPromptLayers {
                    layers: layers.clone(),
                    timestamp: SystemTime::now(),
                    source_mtime,
                },
            );
        }
        Err(_) => warn!("system prompt layers cache lock poisoned while writing cache"),
    }
}

fn get_cached_prompt_templates(key: &PromptResourceCacheKey) -> Option<CachedPromptTemplates> {
    get_cached(prompt_templates_cache(), key, "prompt templates")
}

fn cache_prompt_templates(key: PromptResourceCacheKey, templates: &[PromptTemplate]) {
    match prompt_templates_cache().write() {
        Ok(mut cache) => {
            evict_if_full(&mut cache, &key);

            let source_paths = (
                key.workspace_root.join(PROMPTS_DIR).join(TEMPLATES_DIR),
                key.home_dir.as_ref().map(|h| user_prompts_dir(h).join(TEMPLATES_DIR)),
            );
            let mut mtime_paths = vec![source_paths.0.clone()];
            if let Some(ref p) = source_paths.1 {
                mtime_paths.push(p.clone());
            }
            let source_mtime = newest_mtime(mtime_paths.iter().map(PathBuf::as_path));

            cache.insert(
                key,
                CachedPromptTemplates {
                    templates: templates.to_vec(),
                    timestamp: SystemTime::now(),
                    source_mtime,
                    source_paths,
                },
            );
        }
        Err(_) => warn!("prompt templates cache lock poisoned while writing cache"),
    }
}

#[cfg(test)]
fn clear_prompt_resource_caches() {
    if let Ok(mut cache) = system_prompt_layers_cache().write() {
        cache.clear();
    }
    if let Ok(mut cache) = prompt_templates_cache().write() {
        cache.clear();
    }
}

pub async fn resolve_system_prompt_layers(workspace_root: &Path) -> SystemPromptLayers {
    resolve_system_prompt_layers_with_options(PromptResourceOptions::new(workspace_root)).await
}

pub async fn discover_prompt_templates(workspace_root: &Path) -> Vec<PromptTemplate> {
    discover_prompt_templates_with_options(PromptResourceOptions::new(workspace_root)).await
}

pub async fn find_prompt_template(workspace_root: &Path, name: &str) -> Option<PromptTemplate> {
    let normalized = name.trim();
    if normalized.is_empty() {
        return None;
    }

    find_prompt_template_with_options(PromptResourceOptions::new(workspace_root), normalized).await
}

impl SystemPromptLayers {
    /// Override body that reaches the final prompt: trimmed and non-empty.
    fn effective_override_body(&self) -> Option<&str> {
        self.override_body.as_deref().map(str::trim).filter(|body| !body.is_empty())
    }

    /// Append bodies that reach the final prompt: trimmed, non-empty, in order.
    fn effective_append_bodies(&self) -> impl Iterator<Item = &str> {
        self.append_bodies
            .iter()
            .map(|body| body.trim())
            .filter(|body| !body.is_empty())
    }

    /// Exact layer text spliced into the composed prompt: the effective
    /// override (if any) plus every effective append body, joined the way
    /// [`apply_system_prompt_layers`] splices them. Empty when no layer file
    /// contributes. Token accounting measures this string so the figure tracks
    /// what the model sees, modulo separators and later identity substitutions.
    pub fn applied_text(&self) -> String {
        let mut text = String::new();
        let bodies = self.effective_override_body().into_iter().chain(self.effective_append_bodies());
        for body in bodies {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(body);
        }
        text
    }
}

pub fn apply_system_prompt_layers(base_prompt: &str, layers: &SystemPromptLayers) -> String {
    let mut prompt = String::new();

    if let Some(override_body) = layers.effective_override_body() {
        prompt.push_str(override_body);
    } else {
        prompt.push_str(base_prompt);
    }

    for append_body in layers.effective_append_bodies() {
        if !prompt.is_empty() {
            prompt.push_str("\n\n");
        }
        prompt.push_str(append_body);
    }

    prompt
}

pub fn expand_prompt_template(body: &str, args: &[String]) -> String {
    let joined_args = args.join(" ");
    let mut expanded = String::with_capacity(body.len() + joined_args.len());
    let chars: Vec<char> = body.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        if chars[index] != '$' {
            expanded.push(chars[index]);
            index += 1;
            continue;
        }

        if index + 1 >= chars.len() {
            expanded.push('$');
            index += 1;
            continue;
        }

        match chars[index + 1] {
            '@' => {
                expanded.push_str(&joined_args);
                index += 2;
            }
            'A' => {
                const ARGUMENTS_TOKEN: &str = "ARGUMENTS";
                let remaining: String = chars[index + 1..].iter().collect();
                if remaining.starts_with(ARGUMENTS_TOKEN) {
                    expanded.push_str(&joined_args);
                    index += ARGUMENTS_TOKEN.chars().count() + 1;
                } else {
                    expanded.push('$');
                    index += 1;
                }
            }
            digit if digit.is_ascii_digit() => {
                let mut cursor = index + 1;
                while cursor < chars.len() && chars[cursor].is_ascii_digit() {
                    cursor += 1;
                }
                let ordinal: String = chars[index + 1..cursor].iter().collect();
                let replacement = ordinal
                    .parse::<usize>()
                    .ok()
                    .and_then(|value| value.checked_sub(1))
                    .and_then(|position| args.get(position))
                    .map(String::as_str)
                    .unwrap_or("");
                expanded.push_str(replacement);
                index = cursor;
            }
            _ => {
                expanded.push('$');
                index += 1;
            }
        }
    }

    expanded
}

impl<'a> PromptResourceOptions<'a> {
    fn new(workspace_root: &'a Path) -> Self {
        #[cfg(test)]
        let home_dir = None;

        #[cfg(not(test))]
        let home_dir = dirs::home_dir();

        Self { workspace_root, home_dir }
    }
}

async fn resolve_system_prompt_layers_with_options(options: PromptResourceOptions<'_>) -> SystemPromptLayers {
    let cache_key = PromptResourceCacheKey::new(&options);
    if let Some(cached) = get_cached_system_prompt_layers(&cache_key) {
        let check_paths = [
            options.workspace_root.join(PROMPTS_DIR).join(SYSTEM_PROMPT_FILENAME),
            options
                .home_dir
                .as_ref()
                .map(|h| user_prompts_dir(h).join(SYSTEM_PROMPT_FILENAME))
                .unwrap_or_default(),
            options.workspace_root.join(PROMPTS_DIR).join(APPEND_SYSTEM_PROMPT_FILENAME),
            options
                .home_dir
                .as_ref()
                .map(|h| user_prompts_dir(h).join(APPEND_SYSTEM_PROMPT_FILENAME))
                .unwrap_or_default(),
        ];
        let check_refs: Vec<_> = check_paths.iter().map(|p| p.as_path()).collect();
        if !cached.is_source_stale(&check_refs) {
            return cached.layers;
        }
    }

    let layers = resolve_system_prompt_layers_uncached(&options).await;
    cache_system_prompt_layers(cache_key, &layers);
    layers
}

async fn resolve_system_prompt_layers_uncached(options: &PromptResourceOptions<'_>) -> SystemPromptLayers {
    let mut layers = SystemPromptLayers::default();

    let user_system_path = options
        .home_dir
        .as_ref()
        .map(|home| user_prompts_dir(home).join(SYSTEM_PROMPT_FILENAME));
    let workspace_system_path = options.workspace_root.join(PROMPTS_DIR).join(SYSTEM_PROMPT_FILENAME);

    if let Some(path) = user_system_path.as_ref() {
        layers.override_body = read_optional_markdown(path).await;
    }

    if let Some(workspace_override) = read_optional_markdown(&workspace_system_path).await {
        layers.override_body = Some(workspace_override);
    }

    if let Some(path) = options
        .home_dir
        .as_ref()
        .map(|home| user_prompts_dir(home).join(APPEND_SYSTEM_PROMPT_FILENAME))
        && let Some(contents) = read_optional_markdown(&path).await
    {
        layers.append_bodies.push(contents);
    }

    let workspace_append = options.workspace_root.join(PROMPTS_DIR).join(APPEND_SYSTEM_PROMPT_FILENAME);
    if let Some(contents) = read_optional_markdown(&workspace_append).await {
        layers.append_bodies.push(contents);
    }

    layers
}

async fn discover_prompt_templates_with_options(options: PromptResourceOptions<'_>) -> Vec<PromptTemplate> {
    let cache_key = PromptResourceCacheKey::new(&options);
    if let Some(cached) = get_cached_prompt_templates(&cache_key) {
        if !cached.is_source_stale() {
            return cached.templates;
        }
    }

    let templates = discover_prompt_templates_uncached(&options).await;
    cache_prompt_templates(cache_key, &templates);
    templates
}

async fn discover_prompt_templates_uncached(options: &PromptResourceOptions<'_>) -> Vec<PromptTemplate> {
    let mut discovered = BTreeMap::new();

    if let Some(home) = options.home_dir.as_deref() {
        let user_templates = user_prompts_dir(home).join(TEMPLATES_DIR);
        merge_prompt_templates(&mut discovered, &user_templates, PromptResourceScope::User).await;
    }

    let workspace_templates = options.workspace_root.join(PROMPTS_DIR).join(TEMPLATES_DIR);
    merge_prompt_templates(&mut discovered, &workspace_templates, PromptResourceScope::Workspace).await;

    discovered.into_values().collect()
}

async fn find_prompt_template_with_options(options: PromptResourceOptions<'_>, name: &str) -> Option<PromptTemplate> {
    if !is_safe_template_name(name) {
        return None;
    }

    discover_prompt_templates_with_options(options)
        .await
        .into_iter()
        .find(|template| template.name == name)
}

async fn merge_prompt_templates(
    discovered: &mut BTreeMap<String, PromptTemplate>,
    directory: &Path,
    scope: PromptResourceScope,
) {
    let Ok(mut entries) = fs::read_dir(directory).await else {
        return;
    };

    let mut markdown_files = Vec::new();
    loop {
        match entries.next_entry().await {
            Ok(Some(entry)) => {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
                    markdown_files.push(path);
                }
            }
            Ok(None) => break,
            Err(err) => {
                warn!("failed to read prompt templates directory {}: {}", directory.display(), err);
                break;
            }
        }
    }

    markdown_files.sort();

    for path in markdown_files {
        let Some(name) = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::trim)
            .filter(|stem| !stem.is_empty())
            .map(str::to_string)
        else {
            continue;
        };

        match load_prompt_template(&path, name.clone()).await {
            Some(template) => {
                if matches!(scope, PromptResourceScope::Workspace) {
                    discovered.insert(name, template);
                } else {
                    discovered.entry(name).or_insert(template);
                }
            }
            None => continue,
        }
    }
}

async fn load_prompt_template(path: &Path, name: String) -> Option<PromptTemplate> {
    let raw = read_optional_markdown(path).await?;
    let normalized = normalize_newlines(&raw);
    let (frontmatter, body) = parse_frontmatter(&normalized);
    let description = frontmatter
        .description
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| derive_template_description(&body, &name));

    Some(PromptTemplate {
        name,
        description,
        body: body.trim().to_string(),
        path: path.to_path_buf(),
    })
}

async fn read_optional_markdown(path: &Path) -> Option<String> {
    match fs::read_to_string(path).await {
        Ok(contents) => Some(contents),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            warn!("failed to read prompt resource {}: {}", path.display(), err);
            None
        }
    }
}

fn parse_frontmatter(content: &str) -> (PromptTemplateFrontmatter, String) {
    if !content.starts_with("---\n") {
        return (PromptTemplateFrontmatter::default(), content.to_string());
    }

    let Some(frontmatter_end) = content[4..].find("\n---\n").map(|idx| idx + 4) else {
        return (PromptTemplateFrontmatter::default(), content.to_string());
    };

    let yaml = &content[4..frontmatter_end];
    let body_start = frontmatter_end + 5;
    let body = if body_start < content.len() {
        content[body_start..].to_string()
    } else {
        String::new()
    };

    let metadata = match serde_saphyr::from_str::<PromptTemplateFrontmatter>(yaml.trim()) {
        Ok(value) => value,
        Err(err) => {
            warn!("failed to parse prompt template frontmatter: {}", err);
            PromptTemplateFrontmatter::default()
        }
    };

    (metadata, body)
}

fn derive_template_description(body: &str, name: &str) -> String {
    for line in body.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if let Some(heading) = line.strip_prefix('#') {
            let trimmed = heading.trim_start_matches('#').trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        return line.to_string();
    }

    format!("Prompt template `{name}`")
}

fn normalize_newlines(content: &str) -> String {
    content.replace("\r\n", "\n")
}

fn is_safe_template_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && !name.contains("..")
}

#[cfg(test)]
mod tests;
