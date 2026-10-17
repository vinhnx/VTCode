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

pub fn apply_system_prompt_layers(base_prompt: &str, layers: &SystemPromptLayers) -> String {
    let mut prompt = String::new();

    if let Some(override_body) = layers.override_body.as_deref().map(str::trim)
        && !override_body.is_empty()
    {
        prompt.push_str(override_body);
    } else {
        prompt.push_str(base_prompt);
    }

    for append_body in &layers.append_bodies {
        let trimmed = append_body.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !prompt.is_empty() {
            prompt.push_str("\n\n");
        }
        prompt.push_str(trimmed);
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
mod tests {
    use super::*;
    use serial_test::serial;

    async fn discover_with_roots(workspace: &Path, home: Option<&Path>) -> Vec<PromptTemplate> {
        discover_prompt_templates_with_options(PromptResourceOptions {
            workspace_root: workspace,
            home_dir: home.map(Path::to_path_buf),
        })
        .await
    }

    async fn layers_with_roots(workspace: &Path, home: Option<&Path>) -> SystemPromptLayers {
        resolve_system_prompt_layers_with_options(PromptResourceOptions {
            workspace_root: workspace,
            home_dir: home.map(Path::to_path_buf),
        })
        .await
    }

    async fn find_with_roots(workspace: &Path, home: Option<&Path>, name: &str) -> Option<PromptTemplate> {
        find_prompt_template_with_options(
            PromptResourceOptions {
                workspace_root: workspace,
                home_dir: home.map(Path::to_path_buf),
            },
            name,
        )
        .await
    }

    #[tokio::test]
    #[serial]
    async fn system_layers_reuse_process_wide_cache_until_cleared() {
        clear_prompt_resource_caches();

        let workspace = tempfile::TempDir::new().expect("workspace");
        let home = tempfile::TempDir::new().expect("home");
        let workspace_prompts = workspace.path().join(PROMPTS_DIR);
        std::fs::create_dir_all(&workspace_prompts).expect("workspace prompts");
        std::fs::write(workspace_prompts.join(SYSTEM_PROMPT_FILENAME), "workspace system override")
            .expect("write workspace system");

        let first = layers_with_roots(workspace.path(), Some(home.path())).await;
        assert_eq!(first.override_body.as_deref(), Some("workspace system override"));

        std::fs::remove_file(workspace_prompts.join(SYSTEM_PROMPT_FILENAME)).expect("remove workspace system");

        let second = layers_with_roots(workspace.path(), Some(home.path())).await;
        assert_eq!(second.override_body.as_deref(), Some("workspace system override"));

        clear_prompt_resource_caches();

        let third = layers_with_roots(workspace.path(), Some(home.path())).await;
        assert_eq!(third.override_body, None);
    }

    #[tokio::test]
    #[serial]
    async fn prompt_template_discovery_reuses_process_wide_cache_until_cleared() {
        clear_prompt_resource_caches();

        let workspace = tempfile::TempDir::new().expect("workspace");
        let home = tempfile::TempDir::new().expect("home");
        let workspace_templates = workspace.path().join(PROMPTS_DIR).join(TEMPLATES_DIR);
        std::fs::create_dir_all(&workspace_templates).expect("workspace templates");
        std::fs::write(workspace_templates.join("cache-test.md"), "# Cache test\n\nBody")
            .expect("write workspace template");

        let first = discover_with_roots(workspace.path(), Some(home.path())).await;
        assert!(first.iter().any(|template| template.name == "cache-test"));

        std::fs::remove_file(workspace_templates.join("cache-test.md")).expect("remove workspace template");

        let second = discover_with_roots(workspace.path(), Some(home.path())).await;
        assert!(!second.iter().any(|template| template.name == "cache-test"));
        assert!(
            find_with_roots(workspace.path(), Some(home.path()), "cache-test")
                .await
                .is_none()
        );

        clear_prompt_resource_caches();

        let third = discover_with_roots(workspace.path(), Some(home.path())).await;
        assert!(!third.iter().any(|template| template.name == "cache-test"));
        assert!(
            find_with_roots(workspace.path(), Some(home.path()), "cache-test")
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn system_layers_prefer_workspace_override_and_append_user_then_workspace() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let home = tempfile::TempDir::new().expect("home");

        let user_prompts = home.path().join(PROMPTS_DIR);
        let workspace_prompts = workspace.path().join(PROMPTS_DIR);
        std::fs::create_dir_all(&user_prompts).expect("user prompts");
        std::fs::create_dir_all(&workspace_prompts).expect("workspace prompts");

        std::fs::write(user_prompts.join(SYSTEM_PROMPT_FILENAME), "user system override").expect("write user system");
        std::fs::write(workspace_prompts.join(SYSTEM_PROMPT_FILENAME), "workspace system override")
            .expect("write workspace system");
        std::fs::write(user_prompts.join(APPEND_SYSTEM_PROMPT_FILENAME), "user append").expect("write user append");
        std::fs::write(workspace_prompts.join(APPEND_SYSTEM_PROMPT_FILENAME), "workspace append")
            .expect("write workspace append");

        let layers = layers_with_roots(workspace.path(), Some(home.path())).await;
        assert_eq!(layers.override_body.as_deref(), Some("workspace system override"));
        assert_eq!(layers.append_bodies, vec!["user append".to_string(), "workspace append".to_string()]);

        let composed = apply_system_prompt_layers("fallback base", &layers);
        assert_eq!(composed, "workspace system override\n\nuser append\n\nworkspace append");
    }

    #[tokio::test]
    async fn template_discovery_prefers_workspace_and_derives_descriptions() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let home = tempfile::TempDir::new().expect("home");
        let user_templates = home.path().join(PROMPTS_DIR).join(TEMPLATES_DIR);
        let workspace_templates = workspace.path().join(PROMPTS_DIR).join(TEMPLATES_DIR);
        std::fs::create_dir_all(&user_templates).expect("user templates");
        std::fs::create_dir_all(&workspace_templates).expect("workspace templates");

        std::fs::write(user_templates.join("review.md"), "---\ndescription: User review template\n---\nReview $1")
            .expect("user review");
        std::fs::write(workspace_templates.join("review.md"), "# Workspace review\n\nReview workspace $1")
            .expect("workspace review");
        std::fs::write(workspace_templates.join("audit.md"), "First non-empty line becomes description.\n\nAudit $@")
            .expect("workspace audit");

        let templates = discover_with_roots(workspace.path(), Some(home.path())).await;
        assert_eq!(templates.len(), 2);
        assert_eq!(templates[0].name, "audit");
        assert_eq!(templates[0].description, "First non-empty line becomes description.");
        assert_eq!(templates[1].name, "review");
        assert_eq!(templates[1].description, "Workspace review");
        assert_eq!(templates[1].body, "# Workspace review\n\nReview workspace $1");
    }

    #[tokio::test]
    async fn direct_template_lookup_uses_workspace_precedence() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let home = tempfile::TempDir::new().expect("home");
        let user_templates = home.path().join(PROMPTS_DIR).join(TEMPLATES_DIR);
        let workspace_templates = workspace.path().join(PROMPTS_DIR).join(TEMPLATES_DIR);
        std::fs::create_dir_all(&user_templates).expect("user templates");
        std::fs::create_dir_all(&workspace_templates).expect("workspace templates");

        std::fs::write(user_templates.join("review.md"), "User review body").expect("user template");
        std::fs::write(workspace_templates.join("review.md"), "Workspace review body").expect("workspace template");

        let template = find_with_roots(workspace.path(), Some(home.path()), "review")
            .await
            .expect("template");
        assert_eq!(template.body, "Workspace review body");
    }

    #[tokio::test]
    async fn direct_template_lookup_rejects_unsafe_names() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let home = tempfile::TempDir::new().expect("home");

        let template = find_with_roots(workspace.path(), Some(home.path()), "../escape").await;
        assert!(template.is_none());
    }

    #[test]
    fn template_expansion_supports_positional_and_all_arguments() {
        let expanded = expand_prompt_template(
            "Review $1 against $2.\nArgs: $@\nAgain: $ARGUMENTS\nMissing: '$3'",
            &["src/lib.rs".to_string(), "main".to_string()],
        );

        assert_eq!(
            expanded,
            "Review src/lib.rs against main.\nArgs: src/lib.rs main\nAgain: src/lib.rs main\nMissing: ''"
        );
    }

    struct TestEntry {
        timestamp: SystemTime,
        expired: bool,
    }

    impl ExpirableCacheEntry for TestEntry {
        fn is_expired(&self) -> bool {
            self.expired
        }

        fn timestamp(&self) -> SystemTime {
            self.timestamp
        }
    }

    fn full_live_cache(now: SystemTime) -> HashMap<u32, TestEntry> {
        (0..PROMPT_RESOURCE_CACHE_MAX_ENTRIES as u32)
            .map(|i| {
                (
                    i,
                    TestEntry {
                        // Key 0 is newest, the last key is oldest.
                        timestamp: now - Duration::from_secs((i + 1) as u64 * 60),
                        expired: false,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn evict_if_full_removes_oldest_live_entry_at_budget() {
        let now = SystemTime::now();
        let mut cache = full_live_cache(now);
        assert_eq!(cache.len(), PROMPT_RESOURCE_CACHE_MAX_ENTRIES);

        evict_if_full(&mut cache, &u32::MAX);
        assert_eq!(cache.len(), PROMPT_RESOURCE_CACHE_MAX_ENTRIES - 1);
        // Oldest (last key) evicted, newest (key 0) retained.
        assert!(!cache.contains_key(&(PROMPT_RESOURCE_CACHE_MAX_ENTRIES as u32 - 1)));
        assert!(cache.contains_key(&0));
    }

    #[test]
    fn evict_if_full_prefers_expired_over_oldest_live_and_keeps_present_keys() {
        let now = SystemTime::now();
        let mut cache = full_live_cache(now);
        // Expire the newest entry: it must go before the oldest live one.
        cache.insert(0, TestEntry { timestamp: now, expired: true });

        evict_if_full(&mut cache, &u32::MAX);
        assert!(!cache.contains_key(&0));
        assert!(cache.contains_key(&(PROMPT_RESOURCE_CACHE_MAX_ENTRIES as u32 - 1)));

        // A present key at full budget means in-place update: no eviction.
        let mut full = full_live_cache(now);
        evict_if_full(&mut full, &0);
        assert_eq!(full.len(), PROMPT_RESOURCE_CACHE_MAX_ENTRIES);

        // Under budget: no-op for a new key.
        full.remove(&0);
        evict_if_full(&mut full, &u32::MAX);
        assert_eq!(full.len(), PROMPT_RESOURCE_CACHE_MAX_ENTRIES - 1);
        assert!(full.contains_key(&1));
    }

    #[test]
    fn newest_mtime_is_order_independent_and_ignores_missing_paths() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let older = dir.path().join("older.md");
        let newer = dir.path().join("newer.md");
        std::fs::write(&older, "old").expect("write older");
        std::fs::write(&newer, "new").expect("write newer");
        let base = SystemTime::now();
        std::fs::File::options()
            .write(true)
            .open(&older)
            .expect("open older")
            .set_modified(base - Duration::from_secs(100))
            .expect("set older mtime");
        std::fs::File::options()
            .write(true)
            .open(&newer)
            .expect("open newer")
            .set_modified(base)
            .expect("set newer mtime");
        let missing = dir.path().join("missing.md");

        // Newer file wins regardless of argument order.
        let forward = newest_mtime([older.as_path(), newer.as_path()]);
        let reversed = newest_mtime([newer.as_path(), older.as_path()]);
        assert_eq!(forward, reversed);
        assert!(forward > base - Duration::from_secs(100));

        // Missing paths never contribute: same result with or without them,
        // and a lone missing path degrades to UNIX_EPOCH.
        assert_eq!(newest_mtime([missing.as_path(), older.as_path(), newer.as_path()]), forward);
        assert_eq!(newest_mtime([missing.as_path()]), SystemTime::UNIX_EPOCH);
        assert_eq!(newest_mtime(Vec::<&Path>::new()), SystemTime::UNIX_EPOCH);
    }
}
