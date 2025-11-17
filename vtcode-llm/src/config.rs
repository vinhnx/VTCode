//! Provider configuration traits decoupled from VTCode's dot-config storage.
//!
//! Consumers can implement [`ProviderConfig`] for their own types and use the
//! conversion helpers to build `vtcode_core` provider factories without
//! depending on VTCode's internal configuration structs.

use std::borrow::Cow;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Error;
use vtcode_commons::{ErrorFormatter, ErrorReporter, PathScope, TelemetrySink, WorkspacePaths};
use vtcode_core::config::core::PromptCachingConfig;

/// Trait describing the configuration required to instantiate an LLM provider.
///
/// The trait intentionally returns owned-friendly values so that consumers can
/// back the configuration with environment variables, secret managers, or
/// custom structs. The [`as_factory_config`] helper converts a trait object into
/// the concrete configuration type expected by `vtcode_core`'s provider
/// factory.
pub trait ProviderConfig {
    /// API key or bearer token used to authenticate with the provider.
    fn api_key(&self) -> Option<Cow<'_, str>>;

    /// Optional override for the provider's base URL.
    fn base_url(&self) -> Option<Cow<'_, str>> {
        None
    }

    /// Preferred model identifier for the provider.
    fn model(&self) -> Option<Cow<'_, str>> {
        None
    }

    /// Optional prompt cache configuration forwarded to providers that support
    /// caching.
    fn prompt_cache(&self) -> Option<Cow<'_, PromptCachingConfig>> {
        None
    }
}

/// Convert an implementor of [`ProviderConfig`] into the configuration used by
/// the `vtcode_core` provider factory.
pub fn as_factory_config(source: &dyn ProviderConfig) -> vtcode_core::llm::factory::ProviderConfig {
    vtcode_core::llm::factory::ProviderConfig {
        api_key: source.api_key().map(Cow::into_owned),
        base_url: source.base_url().map(Cow::into_owned),
        model: source.model().map(Cow::into_owned),
        prompt_cache: source.prompt_cache().map(|cfg| cfg.into_owned()),
        timeouts: None,
    }
}

/// Telemetry event emitted when adapter hooks adjust provider configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterEvent {
    /// Records the resolved prompt cache directory and its associated scope.
    PromptCacheResolved {
        scope: PathScope,
        cache_dir: PathBuf,
    },
    /// Emitted when the telemetry sink itself fails so callers can log a
    /// fallback message.
    TelemetryFailure { message: String },
    /// Raised when the adapter reports a validation or hook failure.
    AdapterError { message: String },
}

/// Shared adapter that enriches [`ProviderConfig`] conversions with
/// [`WorkspacePaths`], telemetry, and error-reporting hooks from
/// `vtcode-commons`.
pub struct AdapterHooks<'a, Paths, Telemetry, Reporter, Formatter>
where
    Paths: WorkspacePaths + ?Sized,
    Telemetry: TelemetrySink<AdapterEvent> + ?Sized,
    Reporter: ErrorReporter + ?Sized,
    Formatter: ErrorFormatter + ?Sized,
{
    workspace_paths: &'a Paths,
    telemetry: &'a Telemetry,
    error_reporter: &'a Reporter,
    error_formatter: &'a Formatter,
}

impl<'a, Paths, Telemetry, Reporter, Formatter>
    AdapterHooks<'a, Paths, Telemetry, Reporter, Formatter>
where
    Paths: WorkspacePaths + ?Sized,
    Telemetry: TelemetrySink<AdapterEvent> + ?Sized,
    Reporter: ErrorReporter + ?Sized,
    Formatter: ErrorFormatter + ?Sized,
{
    /// Create a new adapter that enriches provider configuration conversions.
    pub fn new(
        workspace_paths: &'a Paths,
        telemetry: &'a Telemetry,
        error_reporter: &'a Reporter,
        error_formatter: &'a Formatter,
    ) -> Self {
        Self {
            workspace_paths,
            telemetry,
            error_reporter,
            error_formatter,
        }
    }

    /// Convert a [`ProviderConfig`] into the factory configuration while
    /// applying workspace-aware prompt cache resolution and telemetry hooks.
    pub fn apply_to(
        &self,
        source: &dyn ProviderConfig,
    ) -> vtcode_core::llm::factory::ProviderConfig {
        let mut config = as_factory_config(source);
        if let Some(prompt_cache) = config.prompt_cache.as_mut() {
            self.enrich_prompt_cache(prompt_cache);
        }
        config
    }

    fn enrich_prompt_cache(&self, prompt_cache: &mut PromptCachingConfig) {
        let resolved = prompt_cache.resolve_cache_dir(Some(self.workspace_paths.workspace_root()));
        let scope = self.scope_for_path(&resolved);
        self.record_event(AdapterEvent::PromptCacheResolved {
            scope,
            cache_dir: resolved.clone(),
        });

        if !resolved.is_absolute() {
            let error = Error::msg(format!(
                "Prompt cache directory `{}` could not be resolved to an absolute path",
                resolved.display()
            ));
            self.report_error(error);
        }

        prompt_cache.cache_dir = resolved.to_string_lossy().into_owned();
    }

    fn scope_for_path(&self, path: &Path) -> PathScope {
        if path.starts_with(self.workspace_paths.workspace_root()) {
            return PathScope::Workspace;
        }

        let config_dir = self.workspace_paths.config_dir();
        if path.starts_with(&config_dir) {
            return PathScope::Config;
        }

        if let Some(cache_dir) = self.workspace_paths.cache_dir() {
            if path.starts_with(&cache_dir) {
                return PathScope::Cache;
            }
        }

        if let Some(telemetry_dir) = self.workspace_paths.telemetry_dir() {
            if path.starts_with(&telemetry_dir) {
                return PathScope::Telemetry;
            }
        }

        PathScope::Cache
    }

    fn record_event(&self, event: AdapterEvent) {
        if let Err(err) = self.telemetry.record(&event) {
            self.handle_error(err.context("failed to record vtcode-llm adapter telemetry event"));
        }
    }

    fn report_error(&self, error: Error) {
        let message = self.error_formatter.format_error(&error).into_owned();
        let _ = self.error_reporter.capture(&error);
        // Best-effort recording of the formatted message; ignore additional
        // failures to avoid recursive error handling loops.
        let _ = self
            .telemetry
            .record(&AdapterEvent::AdapterError { message });
    }

    fn handle_error(&self, error: Error) {
        let message = self.error_formatter.format_error(&error).into_owned();
        let _ = self.error_reporter.capture(&error);
        let _ = self
            .telemetry
            .record(&AdapterEvent::TelemetryFailure { message });
    }
}

/// Convert a [`ProviderConfig`] into the factory configuration using the
/// supplied adapter hooks for workspace, telemetry, and error integration.
pub fn as_factory_config_with_hooks<'a, Paths, Telemetry, Reporter, Formatter>(
    source: &dyn ProviderConfig,
    hooks: &AdapterHooks<'a, Paths, Telemetry, Reporter, Formatter>,
) -> vtcode_core::llm::factory::ProviderConfig
where
    Paths: WorkspacePaths + ?Sized,
    Telemetry: TelemetrySink<AdapterEvent> + ?Sized,
    Reporter: ErrorReporter + ?Sized,
    Formatter: ErrorFormatter + ?Sized,
{
    hooks.apply_to(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Error, Result, anyhow};
    use assert_fs::TempDir;
    use std::borrow::Cow;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct TestPaths {
        root: PathBuf,
        config: PathBuf,
        cache: PathBuf,
    }

    impl WorkspacePaths for TestPaths {
        fn workspace_root(&self) -> &Path {
            &self.root
        }

        fn config_dir(&self) -> PathBuf {
            self.config.clone()
        }

        fn cache_dir(&self) -> Option<PathBuf> {
            Some(self.cache.clone())
        }
    }

    #[derive(Default)]
    struct RecordingTelemetry {
        events: Arc<Mutex<Vec<AdapterEvent>>>,
    }

    impl TelemetrySink<AdapterEvent> for RecordingTelemetry {
        fn record(&self, event: &AdapterEvent) -> Result<()> {
            self.events.lock().unwrap().push(event.clone());
            Ok(())
        }
    }

    #[derive(Default)]
    struct FailingTelemetry {
        events: Arc<Mutex<Vec<AdapterEvent>>>,
        fail_next: Arc<Mutex<bool>>,
    }

    impl TelemetrySink<AdapterEvent> for FailingTelemetry {
        fn record(&self, event: &AdapterEvent) -> Result<()> {
            let mut fail = self.fail_next.lock().unwrap();
            if std::mem::take(&mut *fail) {
                Err(anyhow!("telemetry unavailable"))
            } else {
                self.events.lock().unwrap().push(event.clone());
                Ok(())
            }
        }
    }

    #[derive(Default)]
    struct RecordingReporter {
        errors: Arc<Mutex<Vec<String>>>,
    }

    impl ErrorReporter for RecordingReporter {
        fn capture(&self, error: &Error) -> Result<()> {
            self.errors.lock().unwrap().push(error.to_string());
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingFormatter {
        messages: Arc<Mutex<Vec<String>>>,
    }

    impl ErrorFormatter for RecordingFormatter {
        fn format_error(&self, error: &Error) -> Cow<'_, str> {
            let message = error.to_string();
            self.messages.lock().unwrap().push(message.clone());
            Cow::Owned(message)
        }
    }

    #[test]
    fn applies_workspace_paths_to_prompt_cache() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("workspace");
        let config = root.join("config");
        let cache = root.join("cache");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&cache).unwrap();

        let paths = TestPaths {
            root,
            config,
            cache,
        };
        let telemetry = RecordingTelemetry::default();
        let reporter = RecordingReporter::default();
        let formatter = RecordingFormatter::default();
        let hooks = AdapterHooks::new(&paths, &telemetry, &reporter, &formatter);

        let prompt_cache = PromptCachingConfig {
            cache_dir: "relative/cache".to_string(),
            ..PromptCachingConfig::default()
        };

        let config = OwnedProviderConfig::new().with_prompt_cache(prompt_cache);
        let adapted = as_factory_config_with_hooks(&config, &hooks);

        let prompt_cache = adapted.prompt_cache.expect("prompt cache present");
        assert!(prompt_cache.cache_dir.ends_with("relative/cache"));

        let events = telemetry.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            AdapterEvent::PromptCacheResolved { scope, cache_dir } => {
                assert_eq!(*scope, PathScope::Cache);
                assert!(cache_dir.ends_with("relative/cache"));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn reports_errors_when_telemetry_fails() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("workspace");
        let config = root.join("config");
        let cache = root.join("cache");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&cache).unwrap();

        let paths = TestPaths {
            root,
            config,
            cache,
        };
        let telemetry = FailingTelemetry::default();
        *telemetry.fail_next.lock().unwrap() = true;
        let reporter = RecordingReporter::default();
        let formatter = RecordingFormatter::default();
        let hooks = AdapterHooks::new(&paths, &telemetry, &reporter, &formatter);

        let config = OwnedProviderConfig::new();
        let _ = as_factory_config_with_hooks(&config, &hooks);

        // Ensure the error reporter observed the telemetry failure and the
        // formatter was invoked to produce a fallback message.
        assert_eq!(reporter.errors.lock().unwrap().len(), 1);
        assert_eq!(formatter.messages.lock().unwrap().len(), 1);

        // The fallback telemetry event should succeed after the initial
        // failure flag is cleared.
        let events = telemetry.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        matches!(events[0], AdapterEvent::TelemetryFailure { .. });
    }
}

/// [`ProviderConfig`] implementation for VTCode's dot-config provider entries.
impl ProviderConfig for vtcode_core::utils::dot_config::ProviderConfig {
    fn api_key(&self) -> Option<Cow<'_, str>> {
        self.api_key.as_deref().map(Cow::Borrowed)
    }

    fn base_url(&self) -> Option<Cow<'_, str>> {
        self.base_url.as_deref().map(Cow::Borrowed)
    }

    fn model(&self) -> Option<Cow<'_, str>> {
        self.model.as_deref().map(Cow::Borrowed)
    }
}

/// [`ProviderConfig`] implementation for the concrete factory configuration.
impl ProviderConfig for vtcode_core::llm::factory::ProviderConfig {
    fn api_key(&self) -> Option<Cow<'_, str>> {
        self.api_key.as_deref().map(Cow::Borrowed)
    }

    fn base_url(&self) -> Option<Cow<'_, str>> {
        self.base_url.as_deref().map(Cow::Borrowed)
    }

    fn model(&self) -> Option<Cow<'_, str>> {
        self.model.as_deref().map(Cow::Borrowed)
    }

    fn prompt_cache(&self) -> Option<Cow<'_, PromptCachingConfig>> {
        self.prompt_cache
            .as_ref()
            .map(|cfg| Cow::Owned(cfg.clone()))
    }
}

/// Simple builder-friendly provider configuration backed by owned values.
#[derive(Clone, Debug, Default)]
pub struct OwnedProviderConfig {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    prompt_cache: Option<PromptCachingConfig>,
}

impl OwnedProviderConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_api_key(mut self, value: impl Into<String>) -> Self {
        self.api_key = Some(value.into());
        self
    }

    pub fn with_base_url(mut self, value: impl Into<String>) -> Self {
        self.base_url = Some(value.into());
        self
    }

    pub fn with_model(mut self, value: impl Into<String>) -> Self {
        self.model = Some(value.into());
        self
    }

    pub fn with_prompt_cache(mut self, value: PromptCachingConfig) -> Self {
        self.prompt_cache = Some(value);
        self
    }
}

impl ProviderConfig for OwnedProviderConfig {
    fn api_key(&self) -> Option<Cow<'_, str>> {
        self.api_key.as_deref().map(Cow::Borrowed)
    }

    fn base_url(&self) -> Option<Cow<'_, str>> {
        self.base_url.as_deref().map(Cow::Borrowed)
    }

    fn model(&self) -> Option<Cow<'_, str>> {
        self.model.as_deref().map(Cow::Borrowed)
    }

    fn prompt_cache(&self) -> Option<Cow<'_, PromptCachingConfig>> {
        self.prompt_cache
            .as_ref()
            .map(|cfg| Cow::Owned(cfg.clone()))
    }
}
