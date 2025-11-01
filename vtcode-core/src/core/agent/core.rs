//! Core agent implementation and orchestration

use crate::config::core::PromptCachingConfig;
use crate::config::models::{ModelId, Provider};
use crate::config::types::*;
use crate::core::agent::bootstrap::{AgentComponentBuilder, AgentComponentSet};
use crate::core::agent::compaction::CompactionEngine;
use crate::core::agent::snapshots::{
    DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
};
use crate::core::conversation_summarizer::ConversationSummarizer;
use crate::core::decision_tracker::DecisionTracker;
use crate::core::error_recovery::{ErrorRecoveryManager, ErrorType};
use crate::llm::AnyClient;
use crate::tools::ToolRegistry;
use crate::tools::tree_sitter::{CodeAnalysis, TreeSitterAnalyzer};
use crate::utils::colors::style;
use anyhow::{Result, anyhow};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Main agent orchestrator
pub struct Agent {
    config: AgentConfig,
    client: AnyClient,
    tool_registry: Arc<ToolRegistry>,
    decision_tracker: DecisionTracker,
    error_recovery: ErrorRecoveryManager,
    summarizer: ConversationSummarizer,
    tree_sitter_analyzer: TreeSitterAnalyzer,
    compaction_engine: Arc<CompactionEngine>,
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
        Self {
            config,
            client: components.client,
            tool_registry: components.tool_registry,
            decision_tracker: components.decision_tracker,
            error_recovery: components.error_recovery,
            summarizer: components.summarizer,
            tree_sitter_analyzer: components.tree_sitter_analyzer,
            compaction_engine: components.compaction_engine,
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
        // Initialize available tools in decision tracker
        let tool_names = self.tool_registry.available_tools().await;
        let tool_count = tool_names.len();
        self.decision_tracker.update_available_tools(tool_names);

        // Update session info
        self.session_info.start_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if self.config.verbose {
            println!("{} {}", style("[INIT]").cyan().bold(), "Agent initialized");
            println!("  {} Model: {}", style("").dim(), self.config.model);
            println!(
                "  {} Workspace: {}",
                style("").dim(),
                self.config.workspace.display()
            );
            println!("  {} Tools loaded: {}", style("").dim(), tool_count);
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

    /// Get conversation summarizer reference
    pub fn summarizer(&self) -> &ConversationSummarizer {
        &self.summarizer
    }

    /// Get tool registry reference
    pub fn tool_registry(&self) -> Arc<ToolRegistry> {
        Arc::clone(&self.tool_registry)
    }

    /// Get mutable tool registry reference
    pub fn tool_registry_mut(&mut self) -> &mut ToolRegistry {
        Arc::get_mut(&mut self.tool_registry)
            .expect("ToolRegistry should not have other references")
    }

    /// Get model-agnostic client reference
    pub fn llm(&self) -> &AnyClient {
        &self.client
    }

    /// Get tree-sitter analyzer reference
    pub fn tree_sitter_analyzer(&self) -> &TreeSitterAnalyzer {
        &self.tree_sitter_analyzer
    }

    /// Get mutable tree-sitter analyzer reference
    pub fn tree_sitter_analyzer_mut(&mut self) -> &mut TreeSitterAnalyzer {
        &mut self.tree_sitter_analyzer
    }

    /// Get compaction engine reference
    pub fn compaction_engine(&self) -> Arc<CompactionEngine> {
        Arc::clone(&self.compaction_engine)
    }

    /// Make intelligent compaction decision using context analysis
    pub async fn make_intelligent_compaction_decision(
        &self,
    ) -> Result<crate::core::agent::intelligence::CompactionDecision> {
        let stats = self.compaction_engine.get_statistics().await?;
        let should_compact = self.compaction_engine.should_compact().await?;
        let strategy = if should_compact {
            crate::core::agent::intelligence::CompactionStrategy::Aggressive
        } else {
            crate::core::agent::intelligence::CompactionStrategy::Conservative
        };
        let reasoning = if should_compact {
            format!("{} messages exceed thresholds", stats.total_messages)
        } else {
            "within configured thresholds".to_string()
        };

        Ok(crate::core::agent::intelligence::CompactionDecision {
            should_compact,
            strategy,
            reasoning,
            estimated_benefit: stats.total_memory_usage,
        })
    }

    /// Check if compaction is needed
    pub async fn should_compact(&self) -> Result<bool> {
        self.compaction_engine.should_compact().await
    }

    /// Perform intelligent message compaction
    pub async fn compact_messages(&self) -> Result<crate::core::agent::types::CompactionResult> {
        self.compaction_engine
            .compact_messages_intelligently()
            .await
    }

    /// Perform context compaction
    pub async fn compact_context(
        &self,
        context_key: &str,
        context_data: &mut std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<crate::core::agent::types::CompactionResult> {
        self.compaction_engine
            .compact_context(context_key, context_data)
            .await
    }

    /// Get compaction statistics
    pub async fn get_compaction_stats(
        &self,
    ) -> Result<crate::core::agent::types::CompactionStatistics> {
        self.compaction_engine.get_statistics().await
    }

    /// Analyze a file using tree-sitter
    pub fn analyze_file_with_tree_sitter(
        &mut self,
        file_path: &std::path::Path,
        source_code: &str,
    ) -> Result<CodeAnalysis> {
        // Detect language from file extension
        let language = self
            .tree_sitter_analyzer
            .detect_language_from_path(file_path)
            .map_err(|e| {
                anyhow!(
                    "Failed to detect language for {}: {}",
                    file_path.display(),
                    e
                )
            })?;

        // Parse the file
        let syntax_tree = self
            .tree_sitter_analyzer
            .parse(source_code, language.clone())?;

        // Extract symbols
        let symbols = self
            .tree_sitter_analyzer
            .extract_symbols(&syntax_tree, source_code, language.clone())
            .unwrap_or_default();

        // Extract dependencies
        let dependencies = self
            .tree_sitter_analyzer
            .extract_dependencies(&syntax_tree, language.clone())
            .unwrap_or_default();

        // Calculate metrics
        let metrics = self
            .tree_sitter_analyzer
            .calculate_metrics(&syntax_tree, source_code)
            .unwrap_or_default();

        Ok(CodeAnalysis {
            file_path: file_path.to_string_lossy().to_string(),
            language,
            symbols,
            dependencies,
            metrics,
            issues: Vec::new(),
            complexity: crate::tools::tree_sitter::analysis::ComplexityMetrics::default(),
            structure: crate::tools::tree_sitter::analysis::CodeStructure::default(),
        })
    }

    /// Update session statistics
    pub fn update_session_stats(&mut self, turns: usize, decisions: usize, errors: usize) {
        self.session_info.total_turns = turns;
        self.session_info.total_decisions = decisions;
        self.session_info.error_count = errors;
    }

    /// Check if context compression is needed
    pub fn should_compress_context(&self, context_size: usize) -> bool {
        self.error_recovery.should_compress_context(context_size)
    }

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
                "{} {}",
                style("[TRANSPARENCY]").magenta().bold(),
                "Session Transparency Summary:"
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
            println!("  {} tool calls executed", style(report.tool_calls).blue());
            println!(
                "  Session duration: {} seconds",
                style(report.session_duration).yellow()
            );
            if let Some(avg_confidence) = report.avg_confidence {
                println!(
                    "  {:.1}% average decision confidence",
                    avg_confidence * 100.0
                );
            }

            // Error recovery statistics
            println!(
                "\n{} {}",
                style("[ERROR RECOVERY]").red().bold(),
                "Error Statistics:"
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
                style(error_stats.avg_recovery_attempts).yellow()
            );

            // Conversation summarization statistics
            let summaries = self.summarizer.get_summaries();
            if !summaries.is_empty() {
                println!(
                    "\n{} {}",
                    style("[CONVERSATION SUMMARY]").green().bold(),
                    "Statistics:"
                );
                println!("  {} summaries generated", style(summaries.len()).cyan());
                if let Some(latest) = self.summarizer.get_latest_summary() {
                    println!(
                        "  {} Latest summary: {} turns, {:.1}% compression",
                        style("(SUMMARY)").dim(),
                        latest.total_turns,
                        latest.compression_ratio * 100.0
                    );
                }
            }
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
                "{} {}",
                style("[SHUTDOWN]").cyan().bold(),
                "Agent shutdown complete"
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
                model: ModelId::default().as_str().to_string(),
                api_key: String::new(),
                provider: Provider::Gemini.to_string(),
                api_key_env: Provider::Gemini.default_api_key_env().to_string(),
                workspace: std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from(".")),
                verbose: false,
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
