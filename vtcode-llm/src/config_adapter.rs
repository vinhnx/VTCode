//! Provider configuration traits decoupled from VT Code's dot-config storage.
//!
//! Consumers can implement [`ProviderConfig`] for their own types and use the
//! conversion helpers to build `vtcode_core` provider factories without
//! depending on VT Code's internal configuration structs.

use std::borrow::Cow;

use std::path::PathBuf;

use anyhow::Error;
use vtcode_commons::cgp::HasComponent;
use vtcode_commons::{ErrorFormatter, ErrorReporter, PathScope, TelemetrySink, WorkspacePaths};
use vtcode_config::TimeoutsConfig;
use vtcode_config::core::{AnthropicConfig, OpenAIConfig};
use vtcode_config::core::{ModelConfig, PromptCachingConfig};

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

    /// Optional request timeout configuration forwarded to the provider factory.
    fn timeouts(&self) -> Option<Cow<'_, TimeoutsConfig>> {
        None
    }

    /// Optional OpenAI-specific configuration forwarded to the provider factory.
    fn openai(&self) -> Option<Cow<'_, OpenAIConfig>> {
        None
    }

    /// Optional Anthropic-specific configuration forwarded to the provider factory.
    fn anthropic(&self) -> Option<Cow<'_, AnthropicConfig>> {
        None
    }

    /// Optional model behavior configuration (loop detection, capability overrides).
    fn model_behavior(&self) -> Option<Cow<'_, ModelConfig>> {
        None
    }
}

/// Marker component for projecting a borrowed provider config into the owned
/// config bag consumed by `vtcode-core`.
pub enum FactoryConfigProjectionComponent {}

trait FactoryConfigProjectionProvider<Ctx> {
    fn project(ctx: &Ctx) -> crate::factory_types::ProviderConfig;
}

trait CanProjectFactoryConfig {
    fn project_factory_config(&self) -> crate::factory_types::ProviderConfig;
}

impl<Ctx> CanProjectFactoryConfig for Ctx
where
    Ctx: HasComponent<FactoryConfigProjectionComponent>,
    <Ctx as HasComponent<FactoryConfigProjectionComponent>>::Provider:
        FactoryConfigProjectionProvider<Ctx>,
{
    fn project_factory_config(&self) -> crate::factory_types::ProviderConfig {
        <<Ctx as HasComponent<FactoryConfigProjectionComponent>>::Provider as FactoryConfigProjectionProvider<Ctx>>::project(self)
    }
}

struct BorrowedConfigProjectionCtx<'a> {
    source: &'a dyn ProviderConfig,
}

struct BorrowedConfigProjection;

impl HasComponent<FactoryConfigProjectionComponent> for BorrowedConfigProjectionCtx<'_> {
    type Provider = BorrowedConfigProjection;
}

impl FactoryConfigProjectionProvider<BorrowedConfigProjectionCtx<'_>> for BorrowedConfigProjection {
    fn project(ctx: &BorrowedConfigProjectionCtx<'_>) -> crate::factory_types::ProviderConfig {
        project_provider_config(ctx.source)
    }
}

/// Convert an implementor of [`ProviderConfig`] into the configuration used by
/// the `vtcode_core` provider factory.
pub fn as_factory_config(source: &dyn ProviderConfig) -> crate::factory_types::ProviderConfig {
    BorrowedConfigProjectionCtx { source }.project_factory_config()
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

/// Trait that bundles the adapter hook dependencies into a single interface.
/// This replaces the previous four-parameter generic bound
/// (`Paths`, `Telemetry`, `Reporter`, `Formatter`) with a single trait,
/// following the **Config Trait pattern** from the Rust Patterns guide
/// (Ch 3 — Config Trait Pattern).
///
/// Associated types keep the trait dyn-incompatible (which is intentional —
/// these are compile-time wiring, not runtime polymorphism).
pub trait AdapterHooksProvider: Send + Sync {
    type Paths: WorkspacePaths + ?Sized;
    type Telemetry: TelemetrySink<AdapterEvent> + ?Sized;
    type Reporter: ErrorReporter + ?Sized;
    type Formatter: ErrorFormatter + ?Sized;

    fn workspace_paths(&self) -> &Self::Paths;
    fn telemetry(&self) -> &Self::Telemetry;
    fn error_reporter(&self) -> &Self::Reporter;
    fn error_formatter(&self) -> &Self::Formatter;
}

/// Shared adapter that enriches [`ProviderConfig`] conversions with
/// [`WorkspacePaths`], telemetry, and error-reporting hooks from
/// `vtcode-commons`.
///
/// Uses the **Config Trait pattern** — one generic parameter (`Hooks`) instead
/// of the previous four (`Paths`, `Telemetry`, `Reporter`, `Formatter`).
pub struct AdapterHooks<'a, Hooks: AdapterHooksProvider> {
    hooks: &'a Hooks,
}

impl<'a, Hooks: AdapterHooksProvider> AdapterHooks<'a, Hooks> {
    /// Create a new adapter that enriches provider configuration conversions.
    pub fn new(hooks: &'a Hooks) -> Self {
        Self { hooks }
    }

    /// Convert a [`ProviderConfig`] into the factory configuration while
    /// applying workspace-aware prompt cache resolution and telemetry hooks.
    pub fn apply_to(&self, source: &dyn ProviderConfig) -> crate::factory_types::ProviderConfig {
        HookedConfigProjectionCtx {
            source,
            hooks: self,
        }
        .project_factory_config()
    }

    fn enrich_prompt_cache(&self, prompt_cache: &mut PromptCachingConfig) {
        let resolved =
            prompt_cache.resolve_cache_dir(Some(self.hooks.workspace_paths().workspace_root()));
        let scope = self.hooks.workspace_paths().scope_for_path(&resolved);
        // Check absoluteness before moving `resolved` so we can report a meaningful error.
        let is_abs = resolved.is_absolute();
        let resolved_display = resolved.display().to_string();

        // Set the prompt cache dir (borrows from `resolved`) then move `resolved` into the event
        prompt_cache.cache_dir = resolved.to_string_lossy().into_owned();
        self.record_event(AdapterEvent::PromptCacheResolved {
            scope,
            cache_dir: resolved,
        });

        if !is_abs {
            let error = Error::msg(format!(
                "Prompt cache directory `{}` could not be resolved to an absolute path",
                resolved_display
            ));
            self.report_error(error);
        }
    }

    fn record_event(&self, event: AdapterEvent) {
        if let Err(err) = self.hooks.telemetry().record(&event) {
            self.handle_error(err.context("failed to record LLM adapter telemetry event"));
        }
    }

    fn capture_error_message(&self, error: &Error) -> String {
        let message = self
            .hooks
            .error_formatter()
            .format_error(error)
            .into_owned();
        let _ = self.hooks.error_reporter().capture(error);
        message
    }

    fn report_error(&self, error: Error) {
        let message = self.capture_error_message(&error);
        // Best-effort recording of the formatted message; ignore additional
        // failures to avoid recursive error handling loops.
        let _ = self
            .hooks
            .telemetry()
            .record(&AdapterEvent::AdapterError { message });
    }

    fn handle_error(&self, error: Error) {
        let message = self.capture_error_message(&error);
        let _ = self
            .hooks
            .telemetry()
            .record(&AdapterEvent::TelemetryFailure { message });
    }
}

struct HookedConfigProjectionCtx<'source, 'hooks, Hooks: AdapterHooksProvider> {
    source: &'source dyn ProviderConfig,
    hooks: &'hooks AdapterHooks<'hooks, Hooks>,
}

struct HookedConfigProjection;

impl<'source, 'hooks, Hooks: AdapterHooksProvider> HasComponent<FactoryConfigProjectionComponent>
    for HookedConfigProjectionCtx<'source, 'hooks, Hooks>
{
    type Provider = HookedConfigProjection;
}

impl<'source, 'hooks, Hooks: AdapterHooksProvider>
    FactoryConfigProjectionProvider<HookedConfigProjectionCtx<'source, 'hooks, Hooks>>
    for HookedConfigProjection
{
    fn project(
        ctx: &HookedConfigProjectionCtx<'source, 'hooks, Hooks>,
    ) -> crate::factory_types::ProviderConfig {
        let mut config =
            BorrowedConfigProjectionCtx { source: ctx.source }.project_factory_config();
        if let Some(prompt_cache) = config.prompt_cache.as_mut() {
            ctx.hooks.enrich_prompt_cache(prompt_cache);
        }
        config
    }
}

/// Convert a [`ProviderConfig`] into the factory configuration using the
/// supplied adapter hooks for workspace, telemetry, and error integration.
pub fn as_factory_config_with_hooks<'a, Hooks: AdapterHooksProvider>(
    source: &dyn ProviderConfig,
    hooks: &AdapterHooks<'a, Hooks>,
) -> crate::factory_types::ProviderConfig {
    HookedConfigProjectionCtx { source, hooks }.project_factory_config()
}

fn project_provider_config(source: &dyn ProviderConfig) -> crate::factory_types::ProviderConfig {
    crate::factory_types::ProviderConfig {
        api_key: source.api_key().map(Cow::into_owned),
        openai_chatgpt_auth: None,
        copilot_auth: None,
        base_url: source.base_url().map(Cow::into_owned),
        model: source.model().map(Cow::into_owned),
        prompt_cache: source.prompt_cache().map(Cow::into_owned),
        timeouts: source.timeouts().map(Cow::into_owned),
        openai: source.openai().map(Cow::into_owned),
        anthropic: source.anthropic().map(Cow::into_owned),
        model_behavior: source.model_behavior().map(Cow::into_owned),
        workspace_root: None,
    }
}

fn borrowed_optional_str(value: &Option<String>) -> Option<Cow<'_, str>> {
    value.as_deref().map(Cow::Borrowed)
}

fn borrowed_optional<T: Clone>(value: &Option<T>) -> Option<Cow<'_, T>> {
    value.as_ref().map(Cow::Borrowed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Error, Result, anyhow};
    use assert_fs::TempDir;
    use std::borrow::Cow;
    use std::path::Path;
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

    /// Test harness that bundles all adapter hook dependencies into a single
    /// struct, demonstrating the Config Trait pattern.
    struct TestHooks {
        paths: TestPaths,
        telemetry: RecordingTelemetry,
        reporter: RecordingReporter,
        formatter: RecordingFormatter,
    }

    impl AdapterHooksProvider for TestHooks {
        type Paths = TestPaths;
        type Telemetry = RecordingTelemetry;
        type Reporter = RecordingReporter;
        type Formatter = RecordingFormatter;

        fn workspace_paths(&self) -> &TestPaths {
            &self.paths
        }
        fn telemetry(&self) -> &RecordingTelemetry {
            &self.telemetry
        }
        fn error_reporter(&self) -> &RecordingReporter {
            &self.reporter
        }
        fn error_formatter(&self) -> &RecordingFormatter {
            &self.formatter
        }
    }

    /// Test harness that uses a failing telemetry sink.
    struct FailingHooks {
        paths: TestPaths,
        telemetry: FailingTelemetry,
        reporter: RecordingReporter,
        formatter: RecordingFormatter,
    }

    impl AdapterHooksProvider for FailingHooks {
        type Paths = TestPaths;
        type Telemetry = FailingTelemetry;
        type Reporter = RecordingReporter;
        type Formatter = RecordingFormatter;

        fn workspace_paths(&self) -> &TestPaths {
            &self.paths
        }
        fn telemetry(&self) -> &FailingTelemetry {
            &self.telemetry
        }
        fn error_reporter(&self) -> &RecordingReporter {
            &self.reporter
        }
        fn error_formatter(&self) -> &RecordingFormatter {
            &self.formatter
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

        let hooks = TestHooks {
            paths: TestPaths {
                root,
                config,
                cache,
            },
            telemetry: RecordingTelemetry::default(),
            reporter: RecordingReporter::default(),
            formatter: RecordingFormatter::default(),
        };
        let adapter = AdapterHooks::new(&hooks);

        let prompt_cache = PromptCachingConfig {
            cache_dir: "relative/cache".to_string(),
            ..PromptCachingConfig::default()
        };

        let config = OwnedProviderConfig::new().with_prompt_cache(prompt_cache);
        let adapted = as_factory_config_with_hooks(&config, &adapter);

        let prompt_cache = adapted.prompt_cache.expect("prompt cache present");
        assert!(prompt_cache.cache_dir.ends_with("relative/cache"));

        let events = hooks.telemetry.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            AdapterEvent::PromptCacheResolved { scope, cache_dir } => {
                assert_eq!(*scope, PathScope::Workspace);
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

        let hooks = FailingHooks {
            paths: TestPaths {
                root,
                config,
                cache,
            },
            telemetry: {
                let t = FailingTelemetry::default();
                *t.fail_next.lock().unwrap() = true;
                t
            },
            reporter: RecordingReporter::default(),
            formatter: RecordingFormatter::default(),
        };
        let adapter = AdapterHooks::new(&hooks);

        let prompt_cache = PromptCachingConfig {
            cache_dir: "relative/cache".to_string(),
            ..PromptCachingConfig::default()
        };
        let config = OwnedProviderConfig::new().with_prompt_cache(prompt_cache);
        let _ = as_factory_config_with_hooks(&config, &adapter);

        // Ensure the error reporter observed the telemetry failure and the
        // formatter was invoked to produce a fallback message.
        assert_eq!(hooks.reporter.errors.lock().unwrap().len(), 1);
        assert_eq!(hooks.formatter.messages.lock().unwrap().len(), 1);

        // The fallback telemetry event should succeed after the initial
        // failure flag is cleared.
        let events = hooks.telemetry.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], AdapterEvent::TelemetryFailure { .. }));
    }

    #[test]
    fn core_provider_config_exposes_borrowed_nested_values() {
        let config = crate::factory_types::ProviderConfig {
            api_key: None,
            openai_chatgpt_auth: None,
            copilot_auth: None,
            base_url: None,
            model: None,
            prompt_cache: Some(PromptCachingConfig::default()),
            timeouts: Some(TimeoutsConfig::default()),
            openai: Some(OpenAIConfig::default()),
            anthropic: Some(AnthropicConfig::default()),
            model_behavior: Some(ModelConfig::default()),
            workspace_root: None,
        };

        assert!(matches!(
            <crate::factory_types::ProviderConfig as ProviderConfig>::prompt_cache(&config),
            Some(Cow::Borrowed(_))
        ));
        assert!(matches!(
            <crate::factory_types::ProviderConfig as ProviderConfig>::timeouts(&config),
            Some(Cow::Borrowed(_))
        ));
        assert!(matches!(
            <crate::factory_types::ProviderConfig as ProviderConfig>::openai(&config),
            Some(Cow::Borrowed(_))
        ));
        assert!(matches!(
            <crate::factory_types::ProviderConfig as ProviderConfig>::anthropic(&config),
            Some(Cow::Borrowed(_))
        ));
        assert!(matches!(
            <crate::factory_types::ProviderConfig as ProviderConfig>::model_behavior(&config),
            Some(Cow::Borrowed(_))
        ));
    }

    #[test]
    fn owned_provider_config_exposes_borrowed_nested_values() {
        let config = OwnedProviderConfig::new()
            .with_prompt_cache(PromptCachingConfig::default())
            .with_timeouts(TimeoutsConfig::default())
            .with_openai(OpenAIConfig::default())
            .with_anthropic(AnthropicConfig::default())
            .with_model_behavior(ModelConfig::default());

        assert!(matches!(
            <OwnedProviderConfig as ProviderConfig>::prompt_cache(&config),
            Some(Cow::Borrowed(_))
        ));
        assert!(matches!(
            <OwnedProviderConfig as ProviderConfig>::timeouts(&config),
            Some(Cow::Borrowed(_))
        ));
        assert!(matches!(
            <OwnedProviderConfig as ProviderConfig>::openai(&config),
            Some(Cow::Borrowed(_))
        ));
        assert!(matches!(
            <OwnedProviderConfig as ProviderConfig>::anthropic(&config),
            Some(Cow::Borrowed(_))
        ));
        assert!(matches!(
            <OwnedProviderConfig as ProviderConfig>::model_behavior(&config),
            Some(Cow::Borrowed(_))
        ));
    }

    #[test]
    fn preserves_provider_specific_fields_from_core_config() {
        let source = crate::factory_types::ProviderConfig {
            api_key: Some("secret".to_string()),
            openai_chatgpt_auth: None,
            copilot_auth: None,
            base_url: Some("https://api.example.com".to_string()),
            model: Some("gpt-5".to_string()),
            prompt_cache: Some(PromptCachingConfig::default()),
            timeouts: Some(TimeoutsConfig::default()),
            openai: Some(OpenAIConfig {
                websocket_mode: true,
                ..OpenAIConfig::default()
            }),
            anthropic: Some(AnthropicConfig {
                count_tokens_enabled: true,
                ..AnthropicConfig::default()
            }),
            model_behavior: Some(ModelConfig::default()),
            workspace_root: None,
        };

        let adapted = as_factory_config(&source);

        assert_eq!(adapted.api_key, source.api_key);
        assert_eq!(adapted.base_url, source.base_url);
        assert_eq!(adapted.model, source.model);
        assert!(adapted.prompt_cache.is_some());
        assert_eq!(adapted.timeouts.as_ref().unwrap().pty_ceiling_seconds, 300);
        assert!(adapted.openai.as_ref().unwrap().websocket_mode);
        assert!(adapted.anthropic.as_ref().unwrap().count_tokens_enabled);
        assert!(adapted.model_behavior.is_some());
    }

    #[test]
    fn owned_provider_config_keeps_provider_specific_fields() {
        let config = OwnedProviderConfig::new()
            .with_timeouts(TimeoutsConfig::default())
            .with_openai(OpenAIConfig {
                websocket_mode: true,
                ..OpenAIConfig::default()
            })
            .with_anthropic(AnthropicConfig {
                count_tokens_enabled: true,
                ..AnthropicConfig::default()
            });

        let adapted = as_factory_config(&config);

        assert_eq!(
            adapted.timeouts.as_ref().unwrap().streaming_ceiling_seconds,
            600
        );
        assert!(adapted.openai.as_ref().unwrap().websocket_mode);
        assert!(adapted.anthropic.as_ref().unwrap().count_tokens_enabled);
    }
}

/// [`ProviderConfig`] implementation for the concrete factory configuration.
impl ProviderConfig for crate::factory_types::ProviderConfig {
    fn api_key(&self) -> Option<Cow<'_, str>> {
        borrowed_optional_str(&self.api_key)
    }

    fn base_url(&self) -> Option<Cow<'_, str>> {
        borrowed_optional_str(&self.base_url)
    }

    fn model(&self) -> Option<Cow<'_, str>> {
        borrowed_optional_str(&self.model)
    }

    fn prompt_cache(&self) -> Option<Cow<'_, PromptCachingConfig>> {
        borrowed_optional(&self.prompt_cache)
    }

    fn timeouts(&self) -> Option<Cow<'_, TimeoutsConfig>> {
        borrowed_optional(&self.timeouts)
    }

    fn openai(&self) -> Option<Cow<'_, OpenAIConfig>> {
        borrowed_optional(&self.openai)
    }

    fn anthropic(&self) -> Option<Cow<'_, AnthropicConfig>> {
        borrowed_optional(&self.anthropic)
    }

    fn model_behavior(&self) -> Option<Cow<'_, ModelConfig>> {
        borrowed_optional(&self.model_behavior)
    }
}

/// Simple builder-friendly provider configuration backed by owned values.
#[derive(Clone, Debug, Default)]
#[must_use = "builders do nothing unless consumed"]
pub struct OwnedProviderConfig {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    prompt_cache: Option<PromptCachingConfig>,
    timeouts: Option<TimeoutsConfig>,
    openai: Option<OpenAIConfig>,
    anthropic: Option<AnthropicConfig>,
    model_behavior: Option<ModelConfig>,
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

    pub fn with_timeouts(mut self, value: TimeoutsConfig) -> Self {
        self.timeouts = Some(value);
        self
    }

    pub fn with_openai(mut self, value: OpenAIConfig) -> Self {
        self.openai = Some(value);
        self
    }

    pub fn with_anthropic(mut self, value: AnthropicConfig) -> Self {
        self.anthropic = Some(value);
        self
    }

    pub fn with_model_behavior(mut self, value: ModelConfig) -> Self {
        self.model_behavior = Some(value);
        self
    }
}

impl ProviderConfig for OwnedProviderConfig {
    fn api_key(&self) -> Option<Cow<'_, str>> {
        borrowed_optional_str(&self.api_key)
    }

    fn base_url(&self) -> Option<Cow<'_, str>> {
        borrowed_optional_str(&self.base_url)
    }

    fn model(&self) -> Option<Cow<'_, str>> {
        borrowed_optional_str(&self.model)
    }

    fn prompt_cache(&self) -> Option<Cow<'_, PromptCachingConfig>> {
        borrowed_optional(&self.prompt_cache)
    }

    fn timeouts(&self) -> Option<Cow<'_, TimeoutsConfig>> {
        borrowed_optional(&self.timeouts)
    }

    fn openai(&self) -> Option<Cow<'_, OpenAIConfig>> {
        borrowed_optional(&self.openai)
    }

    fn anthropic(&self) -> Option<Cow<'_, AnthropicConfig>> {
        borrowed_optional(&self.anthropic)
    }

    fn model_behavior(&self) -> Option<Cow<'_, ModelConfig>> {
        borrowed_optional(&self.model_behavior)
    }
}
