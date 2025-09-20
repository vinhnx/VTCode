//! Common types and interfaces used throughout the application

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Configuration for the agent
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub model: String,
    pub api_key: String,
    pub provider: String,
    pub workspace: std::path::PathBuf,
    pub verbose: bool,
    pub theme: String,
}

/// Workshop agent capability levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CapabilityLevel {
    /// Basic chat only
    Basic,
    /// Can read files
    FileReading,
    /// Can read files and list directories
    FileListing,
    /// Can read files, list directories, and run bash commands
    Bash,
    /// Can read files, list directories, run bash commands, and edit files
    Editing,
    /// Full capabilities including code search
    CodeSearch,
}

/// Session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub start_time: u64,
    pub total_turns: usize,
    pub total_decisions: usize,
    pub error_count: usize,
}

/// Conversation turn information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub turn_number: usize,
    pub timestamp: u64,
    pub user_input: Option<String>,
    pub agent_response: Option<String>,
    pub tool_calls: Vec<ToolCallInfo>,
    pub decision: Option<DecisionInfo>,
}

/// Tool call information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub name: String,
    pub args: Value,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub execution_time_ms: Option<u64>,
}

/// Decision information for tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionInfo {
    pub turn_number: usize,
    pub action_type: String,
    pub description: String,
    pub reasoning: String,
    pub outcome: Option<String>,
    pub confidence_score: Option<f64>,
    pub timestamp: u64,
}

/// Error information for tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub error_type: String,
    pub message: String,
    pub turn_number: usize,
    pub recoverable: bool,
    pub timestamp: u64,
}

/// Task information for project workflows
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub task_type: String,
    pub description: String,
    pub completed: bool,
    pub success: bool,
    pub duration_seconds: Option<u64>,
    pub tools_used: Vec<String>,
    pub dependencies: Vec<String>,
}

/// Project creation specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSpec {
    pub name: String,
    pub features: Vec<String>,
    pub template: Option<String>,
    pub dependencies: HashMap<String, String>,
}

/// Workspace analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceAnalysis {
    pub root_path: String,
    pub project_type: Option<String>,
    pub languages: Vec<String>,
    pub frameworks: Vec<String>,
    pub config_files: Vec<String>,
    pub source_files: Vec<String>,
    pub test_files: Vec<String>,
    pub documentation_files: Vec<String>,
    pub total_files: usize,
    pub total_size_bytes: u64,
}

/// Command execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub command: String,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub execution_time_ms: u64,
}

/// File operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOperationResult {
    pub operation: String,
    pub path: String,
    pub success: bool,
    pub details: HashMap<String, Value>,
    pub error: Option<String>,
}

/// Performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub session_duration_seconds: u64,
    pub total_api_calls: usize,
    pub total_tokens_used: Option<usize>,
    pub average_response_time_ms: f64,
    pub tool_execution_count: usize,
    pub error_count: usize,
    pub recovery_success_rate: f64,
}

/// Quality metrics for agent actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityMetrics {
    pub decision_confidence_avg: f64,
    pub tool_success_rate: f64,
    pub error_recovery_rate: f64,
    pub context_preservation_rate: f64,
    pub user_satisfaction_score: Option<f64>,
}

/// Configuration for tool behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    pub enable_validation: bool,
    pub max_execution_time_seconds: u64,
    pub allow_file_creation: bool,
    pub allow_file_deletion: bool,
    pub working_directory: Option<String>,
}

/// Context management settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    pub max_context_length: usize,
    pub compression_threshold: usize,
    pub summarization_interval: usize,
    pub preservation_priority: Vec<String>,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub file_logging: bool,
    pub log_directory: Option<String>,
    pub max_log_files: usize,
    pub max_log_size_mb: usize,
}

/// Analysis depth for workspace analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnalysisDepth {
    Basic,
    Standard,
    Deep,
}

/// Output format for commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputFormat {
    Text,
    Json,
    Html,
}

/// Compression level for context compression
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompressionLevel {
    Light,
    Medium,
    Aggressive,
}
