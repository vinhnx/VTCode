//! Provider configuration traits decoupled from VT Code's dot-config storage.
//!
//! Consumers can implement [`ProviderConfig`] for their own types and use the
//! conversion helpers to build `vtcode_core` provider factories without
//! depending on VT Code's internal configuration structs.

use std::borrow::Cow;

use std::path::PathBuf;

use anyhow::Error;
use vtcode_commons::{ErrorFormatter, ErrorReporter, PathScope, TelemetrySink, WorkspacePaths};
use vtcode_core::components::HasComponent;
use vtcode_core::config::TimeoutsConfig;
use vtcode_core::config::core::{AnthropicConfig, OpenAIConfig};
use vtcode_core::config::core::{ModelConfig, PromptCachingConfig};

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
    fn project(ctx: &Ctx) -> vtcode_core::llm::factory::ProviderConfig;
}

trait CanProjectFactoryConfig {
    fn project_factory_config(&self) -> vtcode_core::llm::factory::ProviderConfig;
}

impl<Ctx> CanProjectFactoryConfig for Ctx
where
    Ctx: HasComponent<FactoryConfigProjectionComponent>,
    <Ctx as HasComponent<FactoryConfigProjectionComponent>>::Provider:
        FactoryConfigProjectionProvider<Ctx>,
{
    fn project_factory_config(&self) -> vtcode_core::llm::factory::ProviderConfig {
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
    fn project(ctx: &BorrowedConfigProjectionCtx<'_>) -> vtcode_core::llm::factory::ProviderConfig {
        project_provider_config(ctx.source)
    }
}

/// Convert an implementor of [`ProviderConfig`] into the configuration used by
/// the `vtcode_core` provider factory.
pub fn as_factory_config(source: &dyn ProviderConfig) -> vtcode_core::llm::factory::ProviderConfig {
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
        HookedConfigProjectionCtx {
            source,
            hooks: self,
        }
        .project_factory_config()
    }

    fn enrich_prompt_cache(&self, prompt_cache: &mut PromptCachingConfig) {
        let resolved = prompt_cache.resolve_cache_dir(Some(self.workspace_paths.workspace_root()));
        let scope = self.workspace_paths.scope_for_path(&resolved);
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
        if let Err(err) = self.telemetry.record(&event) {
            self.handle_error(err.context("failed to record vtcode-llm adapter telemetry event"));
        }
    }

    fn capture_error_message(&self, error: &Error) -> String {
        let message = self.error_formatter.format_error(error).into_owned();
        let _ = self.error_reporter.capture(error);
        message
    }

    fn report_error(&self, error: Error) {
        let message = self.capture_error_message(&error);
        // Best-effort recording of the formatted message; ignore additional
        // failures to avoid recursive error handling loops.
        let _ = self
            .telemetry
            .record(&AdapterEvent::AdapterError { message });
    }

    fn handle_error(&self, error: Error) {
        let message = self.capture_error_message(&error);
        let _ = self
            .telemetry
            .record(&AdapterEvent::TelemetryFailure { message });
    }
}

struct HookedConfigProjectionCtx<'source, 'hooks, Paths, Telemetry, Reporter, Formatter>
where
    Paths: WorkspacePaths + ?Sized,
    Telemetry: TelemetrySink<AdapterEvent> + ?Sized,
    Reporter: ErrorReporter + ?Sized,
    Formatter: ErrorFormatter + ?Sized,
{
    source: &'source dyn ProviderConfig,
    hooks: &'hooks AdapterHooks<'hooks, Paths, Telemetry, Reporter, Formatter>,
}

struct HookedConfigProjection;

impl<'source, 'hooks, Paths, Telemetry, Reporter, Formatter>
    HasComponent<FactoryConfigProjectionComponent>
    for HookedConfigProjectionCtx<'source, 'hooks, Paths, Telemetry, Reporter, Formatter>
where
    Paths: WorkspacePaths + ?Sized,
    Telemetry: TelemetrySink<AdapterEvent> + ?Sized,
    Reporter: ErrorReporter + ?Sized,
    Formatter: ErrorFormatter + ?Sized,
{
    type Provider = HookedConfigProjection;
}

impl<'source, 'hooks, Paths, Telemetry, Reporter, Formatter>
    FactoryConfigProjectionProvider<
        HookedConfigProjectionCtx<'source, 'hooks, Paths, Telemetry, Reporter, Formatter>,
    > for HookedConfigProjection
where
    Paths: WorkspacePaths + ?Sized,
    Telemetry: TelemetrySink<AdapterEvent> + ?Sized,
    Reporter: ErrorReporter + ?Sized,
    Formatter: ErrorFormatter + ?Sized,
{
    fn project(
        ctx: &HookedConfigProjectionCtx<'source, 'hooks, Paths, Telemetry, Reporter, Formatter>,
    ) -> vtcode_core::llm::factory::ProviderConfig {
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
    HookedConfigProjectionCtx { source, hooks }.project_factory_config()
}

fn project_provider_config(
    source: &dyn ProviderConfig,
) -> vtcode_core::llm::factory::ProviderConfig {
    vtcode_core::llm::factory::ProviderConfig {
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

        let prompt_cache = PromptCachingConfig {
            cache_dir: "relative/cache".to_string(),
            ..PromptCachingConfig::default()
        };
        let config = OwnedProviderConfig::new().with_prompt_cache(prompt_cache);
        let _ = as_factory_config_with_hooks(&config, &hooks);

        // Ensure the error reporter observed the telemetry failure and the
        // formatter was invoked to produce a fallback message.
        assert_eq!(reporter.errors.lock().unwrap().len(), 1);
        assert_eq!(formatter.messages.lock().unwrap().len(), 1);

        // The fallback telemetry event should succeed after the initial
        // failure flag is cleared.
        let events = telemetry.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], AdapterEvent::TelemetryFailure { .. }));
    }

    #[test]
    fn core_provider_config_exposes_borrowed_nested_values() {
        let config = vtcode_core::llm::factory::ProviderConfig {
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
            <vtcode_core::llm::factory::ProviderConfig as ProviderConfig>::prompt_cache(&config),
            Some(Cow::Borrowed(_))
        ));
        assert!(matches!(
            <vtcode_core::llm::factory::ProviderConfig as ProviderConfig>::timeouts(&config),
            Some(Cow::Borrowed(_))
        ));
        assert!(matches!(
            <vtcode_core::llm::factory::ProviderConfig as ProviderConfig>::openai(&config),
            Some(Cow::Borrowed(_))
        ));
        assert!(matches!(
            <vtcode_core::llm::factory::ProviderConfig as ProviderConfig>::anthropic(&config),
            Some(Cow::Borrowed(_))
        ));
        assert!(matches!(
            <vtcode_core::llm::factory::ProviderConfig as ProviderConfig>::model_behavior(&config),
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
        let source = vtcode_core::llm::factory::ProviderConfig {
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

/// [`ProviderConfig`] implementation for VT Code's dot-config provider entries.
impl ProviderConfig for vtcode_core::utils::dot_config::ProviderConfig {
    fn api_key(&self) -> Option<Cow<'_, str>> {
        borrowed_optional_str(&self.api_key)
    }

    fn base_url(&self) -> Option<Cow<'_, str>> {
        borrowed_optional_str(&self.base_url)
    }

    fn model(&self) -> Option<Cow<'_, str>> {
        borrowed_optional_str(&self.model)
    }
}

/// [`ProviderConfig`] implementation for the concrete factory configuration.
impl ProviderConfig for vtcode_core::llm::factory::ProviderConfig {
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
