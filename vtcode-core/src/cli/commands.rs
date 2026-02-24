//! Command definitions and interfaces

use crate::config::types::*;
use anyhow::{Context, Result};

/// Result of executing a command
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub success: bool,
    pub message: String,
    pub data: Option<serde_json::Value>,
    pub metrics: Option<PerformanceMetrics>,
}

/// Chat command configuration
pub struct ChatCommand {
    pub initial_prompt: Option<String>,
    pub verbose: bool,
    pub show_stats: bool,
}

impl Default for ChatCommand {
    fn default() -> Self {
        Self {
            initial_prompt: None,
            verbose: false,
            show_stats: true,
        }
    }
}

/// Analyze command configuration
pub struct AnalyzeCommand {
    pub depth: AnalysisDepth,
    pub format: OutputFormat,
    pub include_hidden: bool,
    pub max_depth: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum AnalysisDepth {
    Basic,
    Standard,
    Deep,
}

#[derive(Debug, Clone)]
pub enum OutputFormat {
    Text,
    Json,
    Html,
}

/// Project creation configuration
pub struct CreateProjectCommand {
    pub name: String,
    pub features: Vec<String>,
    pub template: Option<String>,
    pub initialize_git: bool,
    pub include_tests: bool,
    pub include_docs: bool,
}

/// Validation command configuration
pub struct ValidateCommand {
    pub check_api: bool,
    pub check_filesystem: bool,
    pub check_tools: bool,
    pub check_config: bool,
    pub verbose: bool,
}

/// Statistics command configuration
pub struct StatsCommand {
    pub detailed: bool,
    pub format: OutputFormat,
    pub include_history: bool,
    pub include_performance: bool,
}

/// Single prompt command configuration
pub struct AskCommand {
    pub prompt: String,
    pub show_reasoning: bool,
    pub include_metadata: bool,
}

/// Command factory for creating command configurations from CLI args
pub struct CommandFactory;

impl CommandFactory {
    pub fn create_chat_command(verbose: bool, prompt: Option<String>) -> ChatCommand {
        ChatCommand {
            initial_prompt: prompt,
            verbose,
            show_stats: true,
        }
    }

    pub fn create_analyze_command(depth: &str, format: &str) -> Result<AnalyzeCommand> {
        let depth = match depth.to_lowercase().as_str() {
            "basic" => AnalysisDepth::Basic,
            "standard" => AnalysisDepth::Standard,
            "deep" => AnalysisDepth::Deep,
            _ => return Err(anyhow::anyhow!("Invalid analysis depth: {}", depth)),
        };

        let format = match format.to_lowercase().as_str() {
            "text" => OutputFormat::Text,
            "json" => OutputFormat::Json,
            "html" => OutputFormat::Html,
            _ => return Err(anyhow::anyhow!("Invalid output format: {}", format)),
        };

        Ok(AnalyzeCommand {
            depth,
            format,
            include_hidden: false,
            max_depth: Some(10),
        })
    }

    pub fn create_project_command(
        name: String,
        features: String,
        template: Option<String>,
        git: bool,
    ) -> CreateProjectCommand {
        let features = if features.is_empty() {
            vec![]
        } else {
            features.split(',').map(|s| s.trim().to_string()).collect()
        };

        CreateProjectCommand {
            name,
            features,
            template,
            initialize_git: git,
            include_tests: true,
            include_docs: true,
        }
    }

    pub fn create_validate_command(api: bool, filesystem: bool, all: bool) -> ValidateCommand {
        if all {
            ValidateCommand {
                check_api: true,
                check_filesystem: true,
                check_tools: true,
                check_config: true,
                verbose: true,
            }
        } else {
            ValidateCommand {
                check_api: api,
                check_filesystem: filesystem,
                check_tools: false,
                check_config: false,
                verbose: false,
            }
        }
    }

    pub fn create_ask_command(prompt: Vec<String>) -> AskCommand {
        AskCommand {
            prompt: prompt.join(" "),
            show_reasoning: false,
            include_metadata: false,
        }
    }
}

/// Command execution context
pub struct CommandContext {
    pub agent_config: AgentConfig,
    pub session_info: SessionInfo,
    pub performance_metrics: PerformanceMetrics,
    pub start_time: std::time::Instant,
}

impl CommandContext {
    pub fn new(config: AgentConfig) -> Result<Self> {
        let start_time_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("System clock is before UNIX_EPOCH while initializing command context")?
            .as_secs();
        let session_id = format!("session_{start_time_secs}");

        Ok(Self {
            agent_config: config,
            session_info: SessionInfo {
                session_id,
                start_time: start_time_secs,
                total_turns: 0,
                total_decisions: 0,
                error_count: 0,
            },
            performance_metrics: PerformanceMetrics {
                session_duration_seconds: 0,
                total_api_calls: 0,
                total_tokens_used: None,
                average_response_time_ms: 0.0,
                tool_execution_count: 0,
                error_count: 0,
                recovery_success_rate: 0.0,
            },
            start_time: std::time::Instant::now(),
        })
    }

    pub fn update_metrics(&mut self) {
        self.performance_metrics.session_duration_seconds = self.start_time.elapsed().as_secs();
    }
}
