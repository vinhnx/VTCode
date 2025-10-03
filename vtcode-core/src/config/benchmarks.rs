use crate::config::constants::api_keys;
use crate::config::constants::benchmarks::env;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn default_enabled() -> bool {
    false
}

fn default_command_env_var() -> String {
    env::TBENCH_CLI.to_string()
}

fn default_attach_workspace_env() -> bool {
    true
}

/// Top-level benchmark configuration wrapper
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BenchmarkConfig {
    /// Terminal benchmark (TBench) execution settings
    #[serde(default)]
    pub tbench: TBenchConfig,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            tbench: TBenchConfig::default(),
        }
    }
}

/// Supported providers whose credentials can be bridged to TBench automatically
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub enum BenchmarkProvider {
    #[serde(rename = "gemini")]
    Gemini,
    #[serde(rename = "openai")]
    OpenAI,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "deepseek")]
    DeepSeek,
    #[serde(rename = "openrouter")]
    OpenRouter,
    #[serde(rename = "xai")]
    XAI,
}

impl BenchmarkProvider {
    fn api_key_env(&self) -> &'static str {
        match self {
            Self::Gemini => api_keys::GEMINI,
            Self::OpenAI => api_keys::OPENAI,
            Self::Anthropic => api_keys::ANTHROPIC,
            Self::DeepSeek => api_keys::DEEPSEEK,
            Self::OpenRouter => api_keys::OPENROUTER,
            Self::XAI => api_keys::XAI,
        }
    }
}

/// Configuration for launching Terminal Benchmark (TBench) runs
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TBenchConfig {
    /// Toggle to enable the benchmark command
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Explicit command to execute (takes precedence over environment variable)
    #[serde(default)]
    pub command: Option<String>,

    /// Environment variable that resolves the benchmark CLI path when `command` is unset
    #[serde(default = "default_command_env_var")]
    pub command_env: String,

    /// Arguments passed to the benchmark CLI
    #[serde(default)]
    pub args: Vec<String>,

    /// Optional path to a TBench scenario/config file
    #[serde(default)]
    pub config_path: Option<PathBuf>,

    /// Directory to execute the benchmark command from (defaults to workspace)
    #[serde(default)]
    pub working_directory: Option<PathBuf>,

    /// Directory where benchmark artifacts/logs should be written (created automatically)
    #[serde(default)]
    pub results_dir: Option<PathBuf>,

    /// Optional path to capture combined stdout/stderr from the benchmark runner
    #[serde(default)]
    pub run_log: Option<PathBuf>,

    /// Additional environment variables injected into the benchmark process
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Environment variable keys to pass through from the current process if present
    #[serde(default)]
    pub passthrough_env: Vec<String>,

    /// Providers whose API key environment variables should be bridged automatically
    #[serde(default)]
    pub providers: Vec<BenchmarkProvider>,

    /// Whether to inject VTCode workspace metadata environment variables
    #[serde(default = "default_attach_workspace_env")]
    pub attach_workspace_env: bool,
}

impl Default for TBenchConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            command: None,
            command_env: default_command_env_var(),
            args: Vec::new(),
            config_path: None,
            working_directory: None,
            results_dir: None,
            run_log: None,
            env: HashMap::new(),
            passthrough_env: Vec::new(),
            providers: Vec::new(),
            attach_workspace_env: default_attach_workspace_env(),
        }
    }
}

impl TBenchConfig {
    /// Determine the command used to launch TBench, preferring explicit configuration
    pub fn resolved_command(&self) -> Option<String> {
        if let Some(command) = &self.command {
            if !command.trim().is_empty() {
                return Some(command.clone());
            }
        }

        let env_value = std::env::var(&self.command_env).ok();
        env_value.filter(|value| !value.trim().is_empty())
    }

    /// Resolve passthrough environment variables including provider shortcuts
    pub fn resolved_passthrough_env(&self) -> Vec<String> {
        let mut combined: Vec<String> = self.passthrough_env.clone();
        let mut seen: HashSet<String> = combined.iter().cloned().collect();

        for provider in &self.providers {
            let key = provider.api_key_env();
            if seen.insert(key.to_string()) {
                combined.push(key.to_string());
            }
        }

        combined
    }
}
