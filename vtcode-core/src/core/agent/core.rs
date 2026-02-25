//! Core agent implementation and orchestration

use crate::config::core::PromptCachingConfig;
use crate::config::models::{ModelId, Provider};
use crate::config::types::*;
use crate::core::agent::bootstrap::{AgentComponentBuilder, AgentComponentSet};
use crate::core::memory_pool::global_pool;
use vtcode_config::OptimizationConfig;

use crate::core::agent::snapshots::{
    DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
};
use crate::core::decision_tracker::DecisionTracker;
use crate::core::error_recovery::{ErrorRecoveryManager, ErrorType};
use crate::llm::AnyClient;
use crate::tools::ToolRegistry;
use crate::utils::colors::style;
use anyhow::Result;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Main agent orchestrator
pub struct Agent {
    config: AgentConfig,
    client: AnyClient,
    tool_registry: Arc<ToolRegistry>,
    optimization_config: OptimizationConfig,
    decision_tracker: DecisionTracker,
    error_recovery: ErrorRecoveryManager,

    session_info: SessionInfo,
    start_time: std::time::Instant,
}

impl Agent {
    /// Create a new agent instance
    pub async fn new(config: AgentConfig) -> Result<Self> {
        let components = AgentComponentBuilder::new(&config).build().await?;
        Ok(Self::with_components(config, components))
    }

    /// Construct an agent from explicit components.
    ///
    /// This helper enables embedding scenarios where callers manage dependencies
    /// (for example in open-source integrations or when providing custom tool
    /// registries).
    pub fn with_components(config: AgentConfig, components: AgentComponentSet) -> Self {
        // Use default optimization config for now - will be enhanced to read from VTCodeConfig
        let optimization_config = OptimizationConfig::default();

        Self {
            config,
            client: components.client,
            tool_registry: components.tool_registry,
            optimization_config,
            decision_tracker: components.decision_tracker,
            error_recovery: components.error_recovery,
            session_info: components.session_info,
            start_time: std::time::Instant::now(),
        }
    }

    /// Construct an agent with optimization configuration from VTCodeConfig.
    ///
    /// This method provides full integration with the VT Code configuration system,
    /// enabling optimizations based on user settings in vtcode.toml.
    pub fn with_components_and_optimization(
        config: AgentConfig,
        components: AgentComponentSet,
        optimization_config: OptimizationConfig,
    ) -> Self {
        // Note: We can't modify the registry after it's in an Arc, so we'll need to
        // configure optimizations during registry creation in the future.
        // For now, we store the config and the registry will use default optimizations.

        Self {
            config,
            client: components.client,
            tool_registry: components.tool_registry,
            optimization_config,
            decision_tracker: components.decision_tracker,
            error_recovery: components.error_recovery,
            session_info: components.session_info,
            start_time: std::time::Instant::now(),
        }
    }

    /// Convenience constructor for customizing agent components via the builder
    /// pattern without manually importing the bootstrap module.
    pub fn component_builder(config: &AgentConfig) -> AgentComponentBuilder<'_> {
        AgentComponentBuilder::new(config)
    }

    /// Initialize the agent with system setup
    pub async fn initialize(&mut self) -> Result<()> {
        // Initialize memory pool if enabled
        if self.optimization_config.memory_pool.enabled {
            let pool = global_pool();
            let _test_string = pool.get_string();
            pool.return_string(_test_string);
        }

        // Initialize available tools in decision tracker
        let tool_names = self.tool_registry.available_tools().await;
        let tool_count = tool_names.len();
        self.decision_tracker.update_available_tools(tool_names);

        // Update session info
        self.session_info.start_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_else(|_| std::time::Duration::from_secs(0))
            .as_secs();

        if self.config.verbose {
            println!("{} Agent initialized", style("[INIT]").cyan().bold());
            println!("  {} Model: {}", style("").dim(), self.config.model);
            println!(
                "  {} Workspace: {}",
                style("").dim(),
                self.config.workspace.display()
            );
            println!("  {} Tools loaded: {}", style("").dim(), tool_count);

            // Show REAL optimization status
            if self.optimization_config.memory_pool.enabled {
                println!("  {} Memory pool: enabled", style("").dim());
            }
            if self.tool_registry.has_optimizations_enabled() {
                let (cache_size, cache_cap) = self.tool_registry.hot_cache_stats();
                println!(
                    "  {} Tool registry optimizations: enabled (cache: {}/{})",
                    style("").dim(),
                    cache_size,
                    cache_cap
                );
            }
            println!();
        }

        Ok(())
    }

    /// Get the agent's current configuration
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// Get session information
    pub fn session_info(&self) -> &SessionInfo {
        &self.session_info
    }

    /// Get the optimization configuration
    pub fn optimization_config(&self) -> &OptimizationConfig {
        &self.optimization_config
    }

    /// Get the tool registry (now with integrated optimizations)
    pub fn tool_registry(&self) -> &Arc<ToolRegistry> {
        &self.tool_registry
    }

    /// Check if optimizations are enabled in the tool registry
    pub fn has_optimizations_enabled(&self) -> bool {
        self.optimization_config.memory_pool.enabled
            || self.tool_registry.has_optimizations_enabled()
    }

    /// Get performance metrics
    pub fn performance_metrics(&self) -> PerformanceMetrics {
        let duration = self.start_time.elapsed();

        PerformanceMetrics {
            session_duration_seconds: duration.as_secs(),
            total_api_calls: self.session_info.total_turns,
            total_tokens_used: None, // Would need to track from API responses
            average_response_time_ms: if self.session_info.total_turns > 0 {
                duration.as_millis() as f64 / self.session_info.total_turns as f64
            } else {
                0.0
            },
            tool_execution_count: self.session_info.total_decisions,
            error_count: self.session_info.error_count,
            recovery_success_rate: self.calculate_recovery_rate(),
        }
    }

    /// Get decision tracker reference
    pub fn decision_tracker(&self) -> &DecisionTracker {
        &self.decision_tracker
    }

    /// Get mutable decision tracker reference
    pub fn decision_tracker_mut(&mut self) -> &mut DecisionTracker {
        &mut self.decision_tracker
    }

    /// Get error recovery manager reference
    pub fn error_recovery(&self) -> &ErrorRecoveryManager {
        &self.error_recovery
    }

    /// Get mutable error recovery manager reference
    pub fn error_recovery_mut(&mut self) -> &mut ErrorRecoveryManager {
        &mut self.error_recovery
    }

    /// Get tool registry reference
    pub fn tool_registry_clone(&self) -> Arc<ToolRegistry> {
        Arc::clone(&self.tool_registry)
    }

    /// Get mutable tool registry reference
    ///
    /// # Errors
    /// Returns an error if the Arc has outstanding references (another clone exists).
    pub fn tool_registry_mut(&mut self) -> anyhow::Result<&mut ToolRegistry> {
        Arc::get_mut(&mut self.tool_registry).ok_or_else(|| {
            anyhow::anyhow!("ToolRegistry has outstanding references; cannot get mutable access")
        })
    }

    /// Get model-agnostic client reference
    pub fn llm(&self) -> &AnyClient {
        &self.client
    }

    /// Update session statistics
    pub fn update_session_stats(&mut self, turns: usize, decisions: usize, errors: usize) {
        self.session_info.total_turns = turns;
        self.session_info.total_decisions = decisions;
        self.session_info.error_count = errors;
    }

    // Removed: Context compression check has been removed as part of complete context optimization feature removal

    /// Generate context preservation plan
    pub fn generate_context_plan(
        &self,
        context_size: usize,
    ) -> crate::core::error_recovery::ContextPreservationPlan {
        self.error_recovery
            .generate_context_preservation_plan(context_size, self.session_info.error_count)
    }

    /// Check for error patterns
    pub fn detect_error_pattern(&self, error_type: &ErrorType, time_window_seconds: u64) -> bool {
        self.error_recovery
            .detect_error_pattern(error_type, time_window_seconds)
    }

    /// Calculate recovery success rate
    fn calculate_recovery_rate(&self) -> f64 {
        let stats = self.error_recovery.get_error_statistics();
        if stats.total_errors > 0 {
            stats.resolved_errors as f64 / stats.total_errors as f64
        } else {
            1.0 // Perfect rate if no errors
        }
    }

    /// Show transparency report
    pub fn show_transparency_report(&self, detailed: bool) {
        let report = self.decision_tracker.generate_transparency_report();
        let error_stats = self.error_recovery.get_error_statistics();

        if detailed && self.config.verbose {
            println!(
                "{} Session Transparency Summary:",
                style("[TRANSPARENCY]").magenta().bold()
            );
            println!(
                "  {} total decisions made",
                style(report.total_decisions).cyan()
            );
            println!(
                "  {} successful ({}% success rate)",
                style(report.successful_decisions).green(),
                if report.total_decisions > 0 {
                    (report.successful_decisions * 100) / report.total_decisions
                } else {
                    0
                }
            );
            println!(
                "  {} failed decisions",
                style(report.failed_decisions).red()
            );
            println!("  {} tool calls executed", style(report.tool_calls).cyan());
            println!(
                "  Session duration: {} seconds",
                style(report.session_duration).cyan()
            );
            if let Some(avg_confidence) = report.avg_confidence {
                println!(
                    "  {:.1}% average decision confidence",
                    avg_confidence * 100.0
                );
            }

            // Error recovery statistics
            println!(
                "\n{} Error Statistics:",
                style("[ERROR RECOVERY]").red().bold()
            );
            println!(
                "  {} total errors occurred",
                style(error_stats.total_errors).red()
            );
            println!(
                "  {} errors resolved ({}% recovery rate)",
                style(error_stats.resolved_errors).green(),
                if error_stats.total_errors > 0 {
                    (error_stats.resolved_errors * 100) / error_stats.total_errors
                } else {
                    0
                }
            );
            println!(
                "  {:.1} average recovery attempts per error",
                style(error_stats.avg_recovery_attempts).cyan()
            );
        } else {
            // Brief summary for non-verbose mode
            println!("{}", style(format!("  ↳ Session complete: {} decisions, {} successful ({}% success rate), {} errors",
                         report.total_decisions, report.successful_decisions,
                         if report.total_decisions > 0 { (report.successful_decisions * 100) / report.total_decisions } else { 0 },
                         error_stats.total_errors)).dim());
        }
    }

    /// Shutdown the agent and cleanup resources
    pub async fn shutdown(&mut self) -> Result<()> {
        // Show final transparency report
        self.show_transparency_report(true);

        if self.config.verbose {
            println!(
                "{} Agent shutdown complete",
                style("[SHUTDOWN]").cyan().bold()
            );
        }

        Ok(())
    }
}

/// Builder pattern for creating agents with custom configuration
pub struct AgentBuilder {
    config: AgentConfig,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            config: AgentConfig {
                model: ModelId::default().to_string(),
                api_key: String::new(),
                provider: Provider::Gemini.to_string(),
                api_key_env: Provider::Gemini.default_api_key_env().to_string(),
                workspace: std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from(".")),
                verbose: false,
                quiet: false,
                theme: crate::config::constants::defaults::DEFAULT_THEME.to_string(),
                reasoning_effort: ReasoningEffortLevel::default(),
                ui_surface: UiSurfacePreference::default(),
                prompt_cache: PromptCachingConfig::default(),
                model_source: ModelSelectionSource::WorkspaceConfig,
                custom_api_keys: BTreeMap::new(),
                checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
                checkpointing_storage_dir: None,
                checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
                checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
                max_conversation_turns:
                    crate::config::constants::defaults::DEFAULT_MAX_CONVERSATION_TURNS,
                model_behavior: None,
            },
        }
    }

    pub fn with_provider<S: Into<String>>(mut self, provider: S) -> Self {
        self.config.provider = provider.into();
        self
    }

    pub fn with_model<S: Into<String>>(mut self, model: S) -> Self {
        self.config.model = model.into();
        self.config.model_source = ModelSelectionSource::CliOverride;
        self
    }

    pub fn with_api_key<S: Into<String>>(mut self, api_key: S) -> Self {
        self.config.api_key = api_key.into();
        self
    }

    pub fn with_workspace<P: Into<std::path::PathBuf>>(mut self, workspace: P) -> Self {
        self.config.workspace = workspace.into();
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.config.verbose = verbose;
        self
    }

    pub async fn build(self) -> Result<Agent> {
        Agent::new(self.config).await
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
