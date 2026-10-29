use super::*;

pub(super) type CachedManager = Arc<ConfigManager>;

#[derive(Debug)]
pub(super) struct RepositoryProviderSecurityViolation {
    pub(super) source: ConfigLayerSource,
    pub(super) message: String,
}

impl fmt::Display for RepositoryProviderSecurityViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for RepositoryProviderSecurityViolation {}

#[cfg(not(test))]
static WORKSPACE_CACHE: Mutex<Option<HashMap<PathBuf, CachedManager>>> = Mutex::new(None);

#[cfg(not(test))]
pub(super) fn with_cache_mut(f: impl FnOnce(&mut HashMap<PathBuf, CachedManager>)) {
    let mut guard = WORKSPACE_CACHE.lock().expect("config cache lock poisoned");
    guard.get_or_insert_with(HashMap::new);
    f(guard.as_mut().expect("cache initialized"))
}

#[cfg(not(test))]
pub(super) fn cache_get(workspace: &Path) -> Option<CachedManager> {
    WORKSPACE_CACHE
        .lock()
        .expect("config cache lock poisoned")
        .as_ref()
        .and_then(|map| map.get(workspace).cloned())
}

#[cfg(not(test))]
pub(super) fn cache_insert(workspace: PathBuf, manager: CachedManager) {
    with_cache_mut(move |map| {
        map.insert(workspace, manager);
    });
}

#[cfg(not(test))]
pub(super) fn cache_remove(workspace: &Path) {
    with_cache_mut(|map| {
        map.remove(workspace);
    });
}

pub(crate) fn canonicalize_workspace_root(path: &Path) -> PathBuf {
    canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(super) fn push_unique_layer_source(layer_sources: &mut Vec<ConfigLayerSource>, source: ConfigLayerSource) {
    let file = match &source {
        ConfigLayerSource::System { file }
        | ConfigLayerSource::User { file }
        | ConfigLayerSource::Project { file }
        | ConfigLayerSource::Workspace { file } => file,
        ConfigLayerSource::Runtime => {
            layer_sources.push(source);
            return;
        }
    };
    if !layer_sources.iter().any(|existing| {
        matches!(existing,
            ConfigLayerSource::System { file: existing_file }
            | ConfigLayerSource::User { file: existing_file }
            | ConfigLayerSource::Project { file: existing_file }
            | ConfigLayerSource::Workspace { file: existing_file }
            if existing_file == file)
    }) {
        layer_sources.push(source);
    }
}

fn is_optional_global_layer(source: &ConfigLayerSource) -> bool {
    matches!(source, ConfigLayerSource::System { .. } | ConfigLayerSource::User { .. })
}

pub(super) fn should_skip_optional_global_error(source: &ConfigLayerSource, error: &std::io::Error) -> bool {
    is_optional_global_layer(source) && error.kind() != std::io::ErrorKind::InvalidData
}
