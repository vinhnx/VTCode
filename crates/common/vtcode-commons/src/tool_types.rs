#![expect(
    clippy::cast_possible_truncation,
    reason = "Quality scores are normalized to the documented 0..=100 display range before narrowing."
)]

//! Shared runtime types for the VT Code tool system.
//!
//! This module provides types shared between the LLM and tools subsystems,
//! breaking the circular dependency that would otherwise exist between them.
//!
//! # Overview
//!
//! The key types here are:
//! - [`CompactStr`] - stack-allocated string for short tool names
//! - [`EnhancedToolResult`] - tool result with quality metadata
//! - [`ResultMetadata`] - quality/confidence scoring for tool results
//! - [`tool_names`] - tool name constants used across subsystems

// ---------------------------------------------------------------------------
// CompactStr type alias
// ---------------------------------------------------------------------------

/// Compact inline string -- stack-allocated for strings up to 24 bytes.
/// Drop-in replacement for `String` with zero heap allocation for short strings.
pub type CompactStr = compact_str::CompactString;

// ---------------------------------------------------------------------------
// Tool name constants (canonical names used across subsystems)
// ---------------------------------------------------------------------------

/// Canonical tool name constants — the single source of truth for tool
/// identities across commons, config, core, and the CLI.
///
/// Names are grouped by purpose into submodules so policy/catalog layers can
/// consume a whole group as a slice; every name is also re-exported flat so
/// existing `tool_names::NAME` paths keep working. `vtcode-config` re-exports
/// this module as `constants::tools`. A raw `"read_file"` at a call site can
/// drift silently — reference the constant instead.
pub mod tool_names {
    /// Tools the model may call directly.
    pub mod model_facing {
        pub const EXEC_COMMAND: &str = "exec_command";
        pub const WRITE_STDIN: &str = "write_stdin";
        pub const APPLY_PATCH: &str = "apply_patch";
        pub const RECORD_DECISION: &str = "record_decision";
        pub const CODE_SEARCH: &str = "code_search";
        pub const COMPUTER_USE: &str = "computer_use";
    }

    /// Internal dispatchers retained behind the public tools.
    pub mod dispatchers {
        pub const UNIFIED_SEARCH: &str = "search_dispatch_internal";
        pub const UNIFIED_EXEC: &str = "command_session_internal";
        pub const UNIFIED_FILE: &str = "file_operation_internal";
    }

    /// Standalone tool ids: discovery, MCP, web, and legacy search helpers.
    pub mod ids {
        pub const THINK: &str = "think";
        pub const SEARCH_TOOLS: &str = "search_tools";
        pub const MCP: &str = "mcp";
        pub const MCP_SEARCH_TOOLS: &str = "mcp_search_tools";
        pub const MCP_GET_TOOL_DETAILS: &str = "mcp_get_tool_details";
        pub const MCP_LIST_SERVERS: &str = "mcp_list_servers";
        pub const MCP_CONNECT_SERVER: &str = "mcp_connect_server";
        pub const MCP_DISCONNECT_SERVER: &str = "mcp_disconnect_server";
        pub const WEB_SEARCH: &str = "web_search";
        pub const WEB_FETCH: &str = "web_fetch";
        pub const FETCH_URL: &str = "fetch_url";
        pub const LIST: &str = "list";
        pub const GREP: &str = "grep";
        pub const FETCH: &str = "fetch";
        pub const EXEC_PTY_CMD: &str = "exec_pty_cmd";
        pub const SHELL: &str = "shell";
        pub const GREP_FILE: &str = "grep_file";
        pub const LIST_FILES: &str = "list_files";
    }

    /// Internal execution helpers (PTY session management).
    pub mod execution {
        pub const RUN_PTY_CMD: &str = "run_pty_cmd";
        pub const CREATE_PTY_SESSION: &str = "create_pty_session";
        pub const LIST_PTY_SESSIONS: &str = "list_pty_sessions";
        pub const CLOSE_PTY_SESSION: &str = "close_pty_session";
        pub const SEND_PTY_INPUT: &str = "send_pty_input";
        pub const READ_PTY_SESSION: &str = "read_pty_session";
        pub const RESIZE_PTY_SESSION: &str = "resize_pty_session";
        pub const EXECUTE_CODE: &str = "execute_code";
    }

    /// Internal file-operation helpers.
    pub mod files {
        pub const READ_FILE: &str = "read_file";
        pub const WRITE_FILE: &str = "write_file";
        pub const EDIT_FILE: &str = "edit_file";
        pub const DELETE_FILE: &str = "delete_file";
        pub const CREATE_FILE: &str = "create_file";
        pub const SEARCH_REPLACE: &str = "search_replace";
        pub const FILE_OP: &str = "file_op";
        pub const MOVE_FILE: &str = "move_file";
        pub const COPY_FILE: &str = "copy_file";
    }

    /// Skill progressive disclosure.
    pub mod skills {
        pub const LIST_SKILLS: &str = "list_skills";
        pub const LOAD_SKILL: &str = "load_skill";
        pub const LOAD_SKILL_RESOURCE: &str = "load_skill_resource";
    }

    /// Goal-loop control.
    pub mod goal {
        pub const GET_GOAL: &str = "get_goal";
        pub const UPDATE_GOAL: &str = "update_goal";
    }

    /// Diagnostics.
    pub mod diagnostics {
        pub const GET_ERRORS: &str = "get_errors";
    }

    /// Human-in-the-loop and durable scheduling control.
    pub mod control {
        pub const REQUEST_USER_INPUT: &str = "request_user_input";
        pub const MEMORY: &str = "memory";
        pub const ASK_QUESTIONS: &str = "ask_questions";
        pub const ASK_USER_QUESTION: &str = "ask_user_question";
        pub const MATRIX: &str = "matrix";
        pub const AGENT: &str = "agent";
        pub const CRON: &str = "cron";
        pub const CRON_CREATE: &str = "cron_create";
        pub const CRON_LIST: &str = "cron_list";
        pub const CRON_DELETE: &str = "cron_delete";
    }

    /// Planning workflow coordination.
    pub mod planning {
        pub const START_PLANNING: &str = "start_planning";
        pub const TASK_TRACKER: &str = "task_tracker";
    }

    /// Subagent collaboration and peer sessions.
    pub mod subagents {
        pub const SPAWN_AGENT: &str = "spawn_agent";
        pub const SPAWN_BACKGROUND_SUBPROCESS: &str = "spawn_background_subprocess";
        pub const SEND_INPUT: &str = "send_input";
        pub const WAIT_AGENT: &str = "wait_agent";
        pub const RESUME_AGENT: &str = "resume_agent";
        pub const CLOSE_AGENT: &str = "close_agent";
        pub const ASK_SESSION: &str = "ask_session";
        pub const ANSWER_SESSION: &str = "answer_session";
        pub const LIST_SESSIONS: &str = "list_sessions";
    }

    /// Wire aliases emitted by transcript layers, not canonical tool names.
    /// Dotted spellings are intentionally excluded from canonical-name shape
    /// validation, but producers still reference them by constant.
    pub mod aliases {
        pub const BASH: &str = "bash";
        pub const EXEC: &str = "exec";
        pub const CONTAINER_EXEC: &str = "container.exec";
        pub const REPO_BROWSER_LIST_FILES: &str = "repo_browser.list_files";
        pub const REPO_BROWSER_READ_FILE: &str = "repo_browser.read_file";
        pub const REPO_BROWSER_WRITE_FILE: &str = "repo_browser.write_file";
    }

    /// Wildcard (not a real tool name — exempt from the canonical-name shape).
    pub const WILDCARD_ALL: &str = "*";

    // Flat re-exports: every existing `tool_names::NAME` path keeps working.
    pub use aliases::{
        BASH as BASH_ALIAS, CONTAINER_EXEC as CONTAINER_EXEC_ALIAS, EXEC as EXEC_ALIAS, REPO_BROWSER_LIST_FILES,
        REPO_BROWSER_READ_FILE, REPO_BROWSER_WRITE_FILE,
    };
    pub use control::{
        AGENT, ASK_QUESTIONS, ASK_USER_QUESTION, CRON, CRON_CREATE, CRON_DELETE, CRON_LIST, MATRIX, MEMORY,
        REQUEST_USER_INPUT,
    };
    pub use diagnostics::GET_ERRORS;
    pub use dispatchers::{UNIFIED_EXEC, UNIFIED_FILE, UNIFIED_SEARCH};
    pub use execution::{
        CLOSE_PTY_SESSION, CREATE_PTY_SESSION, EXECUTE_CODE, LIST_PTY_SESSIONS, READ_PTY_SESSION, RESIZE_PTY_SESSION,
        RUN_PTY_CMD, SEND_PTY_INPUT,
    };
    pub use files::{
        COPY_FILE, CREATE_FILE, DELETE_FILE, EDIT_FILE, FILE_OP, MOVE_FILE, READ_FILE, SEARCH_REPLACE, WRITE_FILE,
    };
    pub use goal::{GET_GOAL, UPDATE_GOAL};
    pub use ids::{
        EXEC_PTY_CMD, FETCH, FETCH_URL, GREP, GREP_FILE, LIST, LIST_FILES, MCP, MCP_CONNECT_SERVER,
        MCP_DISCONNECT_SERVER, MCP_GET_TOOL_DETAILS, MCP_LIST_SERVERS, MCP_SEARCH_TOOLS, SEARCH_TOOLS, SHELL, THINK,
        WEB_FETCH, WEB_SEARCH,
    };
    pub use model_facing::{APPLY_PATCH, CODE_SEARCH, COMPUTER_USE, EXEC_COMMAND, RECORD_DECISION, WRITE_STDIN};
    pub use planning::{START_PLANNING, TASK_TRACKER};
    pub use skills::{LIST_SKILLS, LOAD_SKILL, LOAD_SKILL_RESOURCE};
    pub use subagents::{
        ANSWER_SESSION, ASK_SESSION, CLOSE_AGENT, LIST_SESSIONS, RESUME_AGENT, SEND_INPUT, SPAWN_AGENT,
        SPAWN_BACKGROUND_SUBPROCESS, WAIT_AGENT,
    };

    /// Composable group slices for policy/catalog layers that operate on a
    /// whole category rather than one name at a time.
    pub mod groups {
        use super::{
            aliases, control, diagnostics, dispatchers, execution, files, goal, ids, model_facing, planning, skills,
            subagents,
        };
        pub const MODEL_FACING: &[&str] = &[
            model_facing::EXEC_COMMAND,
            model_facing::WRITE_STDIN,
            model_facing::APPLY_PATCH,
            model_facing::RECORD_DECISION,
            model_facing::CODE_SEARCH,
            model_facing::COMPUTER_USE,
        ];
        pub const DISPATCHERS: &[&str] = &[
            dispatchers::UNIFIED_SEARCH,
            dispatchers::UNIFIED_EXEC,
            dispatchers::UNIFIED_FILE,
        ];
        pub const EXECUTION: &[&str] = &[
            execution::RUN_PTY_CMD,
            execution::CREATE_PTY_SESSION,
            execution::LIST_PTY_SESSIONS,
            execution::CLOSE_PTY_SESSION,
            execution::SEND_PTY_INPUT,
            execution::READ_PTY_SESSION,
            execution::RESIZE_PTY_SESSION,
            execution::EXECUTE_CODE,
        ];
        pub const FILES: &[&str] = &[
            files::READ_FILE,
            files::WRITE_FILE,
            files::EDIT_FILE,
            files::DELETE_FILE,
            files::CREATE_FILE,
            files::SEARCH_REPLACE,
            files::FILE_OP,
            files::MOVE_FILE,
            files::COPY_FILE,
        ];
        pub const CONTROL: &[&str] = &[
            control::REQUEST_USER_INPUT,
            control::MEMORY,
            control::ASK_QUESTIONS,
            control::ASK_USER_QUESTION,
            control::MATRIX,
            control::AGENT,
            control::CRON,
            control::CRON_CREATE,
            control::CRON_LIST,
            control::CRON_DELETE,
        ];
        pub const PLANNING: &[&str] = &[planning::START_PLANNING, planning::TASK_TRACKER];
        pub const SUBAGENTS: &[&str] = &[
            subagents::SPAWN_AGENT,
            subagents::SPAWN_BACKGROUND_SUBPROCESS,
            subagents::SEND_INPUT,
            subagents::WAIT_AGENT,
            subagents::RESUME_AGENT,
            subagents::CLOSE_AGENT,
            subagents::ASK_SESSION,
            subagents::ANSWER_SESSION,
            subagents::LIST_SESSIONS,
        ];
        pub const SKILLS: &[&str] = &[skills::LIST_SKILLS, skills::LOAD_SKILL, skills::LOAD_SKILL_RESOURCE];
        pub const GOAL: &[&str] = &[goal::GET_GOAL, goal::UPDATE_GOAL];
        pub const DIAGNOSTICS: &[&str] = &[diagnostics::GET_ERRORS];
        pub const IDS: &[&str] = &[
            ids::THINK,
            ids::SEARCH_TOOLS,
            ids::MCP,
            ids::MCP_SEARCH_TOOLS,
            ids::MCP_GET_TOOL_DETAILS,
            ids::MCP_LIST_SERVERS,
            ids::MCP_CONNECT_SERVER,
            ids::MCP_DISCONNECT_SERVER,
            ids::WEB_SEARCH,
            ids::WEB_FETCH,
            ids::FETCH_URL,
            ids::LIST,
            ids::GREP,
            ids::FETCH,
            ids::EXEC_PTY_CMD,
            ids::SHELL,
            ids::GREP_FILE,
            ids::LIST_FILES,
        ];
        /// Wire aliases (dotted spellings included) — not canonical tool names.
        pub const ALIASES: &[&str] = &[
            aliases::BASH,
            aliases::EXEC,
            aliases::CONTAINER_EXEC,
            aliases::REPO_BROWSER_LIST_FILES,
            aliases::REPO_BROWSER_READ_FILE,
            aliases::REPO_BROWSER_WRITE_FILE,
        ];
    }
}

/// Use direct tool name without alias resolution.
/// Alias resolution is now handled by the tool registry inventory
/// which maintains a mapping of aliases to canonical tool names.
pub const fn canonical_tool_name(name: &str) -> &str {
    name
}

// ---------------------------------------------------------------------------
// Operational constants shared across subsystems
// ---------------------------------------------------------------------------

/// Standard error patterns used for error detection across tools
pub const ERROR_DETECTION_PATTERNS: &[&str] = &[
    "error",
    "failed",
    "exception",
    "permission denied",
    "not found",
    "no such file",
    "cannot",
    "could not",
    "panic",
    "crash",
    "unhandled",
    "fatal",
    "timeout",
    "connection refused",
    "access denied",
    "stack trace",
    "traceback",
    "abort",
    "terminate",
];

/// Network-related error patterns for more specific error detection
pub const NETWORK_ERROR_PATTERNS: &[&str] = &["connection", "timeout", "network", "http", "ssl", "tls", "dns", "proxy"];

/// Default capacity hints for common collections
pub const DEFAULT_VEC_CAPACITY: usize = 32;
pub const DEFAULT_HASHMAP_CAPACITY: usize = 16;
pub const DEFAULT_STRING_CAPACITY: usize = 256;

/// Context optimization constants following AGENTS.md guidelines
pub const MAX_SEARCH_RESULTS: usize = 5;
pub const MAX_LIST_ITEMS_SUMMARY: usize = 5;
pub const OVERFLOW_INDICATOR_PREFIX: &str = "[+]";
pub const OVERFLOW_INDICATOR_SUFFIX: &str = "more items]";

/// Common tool operation limits
pub const MAX_FILE_SIZE_FOR_PROCESSING: usize = 100 * 1024 * 1024; // 100MB
pub const MAX_CONTEXT_LINES: usize = 20;
pub const MAX_OUTPUT_TOKENS: usize = 4000;

/// Reusable empty JSON object schema `{"type": "object"}` for tool parameter definitions.
/// Used by tools that accept no parameters or only optional parameters.
pub fn empty_object_schema() -> Value {
    serde_json::json!({"type": "object"})
}

// ---------------------------------------------------------------------------
// Tool result metadata types
// ---------------------------------------------------------------------------

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::time::SystemTime;

/// Result completeness level
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResultCompleteness {
    /// Full result with no truncation
    Complete,
    /// Partial result (more data exists but not shown)
    Partial,
    /// Result truncated due to size limits
    Truncated,
    /// Empty result (no matches)
    Empty,
}

impl ResultCompleteness {
    /// Deprecated: prefer using the `Display` impl; `ToString` is derived from Display.
    fn to_static_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Truncated => "truncated",
            Self::Empty => "empty",
        }
    }
}

impl fmt::Display for ResultCompleteness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_static_str())
    }
}

/// Quality metadata for tool results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultMetadata {
    /// Confidence that result is correct (0.0-1.0)
    #[serde(default = "default_confidence")]
    pub confidence: f32,

    /// Relevance to current task (0.0-1.0)
    #[serde(default = "default_relevance")]
    pub relevance: f32,

    /// Result completeness level
    pub completeness: ResultCompleteness,

    /// Count of matches/results
    #[serde(default)]
    pub result_count: usize,

    /// Likelihood of false positives (0.0-1.0)
    #[serde(default)]
    pub false_positive_likelihood: f32,

    /// Detected content types (code, docs, config, binary, etc.)
    #[serde(default)]
    pub content_types: Vec<String>,

    /// Tool-specific metrics (lines matched, execution time, etc.)
    #[serde(default)]
    pub tool_metrics: HashMap<String, Value>,
}

fn default_confidence() -> f32 {
    0.5
}

fn default_relevance() -> f32 {
    0.5
}

impl Default for ResultMetadata {
    fn default() -> Self {
        Self {
            confidence: 0.5,
            relevance: 0.5,
            completeness: ResultCompleteness::Complete,
            result_count: 0,
            false_positive_likelihood: 0.1,
            content_types: vec![],
            tool_metrics: HashMap::new(),
        }
    }
}

impl ResultMetadata {
    /// Overall quality score (0.0-1.0)
    #[inline]
    pub fn quality_score(&self) -> f32 {
        let weighted = (self.confidence * 0.4) + (self.relevance * 0.4) + (self.false_positive_likelihood * -0.2);
        weighted.clamp(0.0, 1.0)
    }

    /// Create metadata for a successful tool execution
    #[inline]
    pub fn success(confidence: f32, relevance: f32) -> Self {
        Self {
            confidence: confidence.clamp(0.0, 1.0),
            relevance: relevance.clamp(0.0, 1.0),
            completeness: ResultCompleteness::Complete,
            result_count: 1,
            false_positive_likelihood: 0.05,
            ..Default::default()
        }
    }

    /// Create metadata for empty results
    #[inline]
    pub fn empty() -> Self {
        Self {
            completeness: ResultCompleteness::Empty,
            result_count: 0,
            confidence: 1.0, // High confidence in "no results"
            ..Default::default()
        }
    }

    /// Create metadata for error/inconclusive results
    pub fn error() -> Self {
        Self {
            confidence: 0.2,
            completeness: ResultCompleteness::Empty,
            ..Default::default()
        }
    }

    /// Merge with another metadata (for combining results)
    pub fn merge(&mut self, other: &ResultMetadata) {
        self.result_count += other.result_count;
        self.confidence = (self.confidence + other.confidence) / 2.0;
        self.relevance = (self.relevance + other.relevance) / 2.0;

        // Merge content types
        for ct in &other.content_types {
            if !self.content_types.contains(ct) {
                self.content_types.push(ct.clone());
            }
        }

        // Merge tool metrics - use extend to avoid double clone
        self.tool_metrics
            .extend(other.tool_metrics.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
}

/// Enhanced tool result with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedToolResult {
    /// The actual tool result
    value: Value,

    /// Quality metadata
    pub metadata: ResultMetadata,

    /// When result was produced
    timestamp: u64,

    /// Tool name that produced this
    tool_name: CompactStr,

    /// Whether this was from cache
    #[serde(default)]
    from_cache: bool,
}

impl EnhancedToolResult {
    pub fn new(value: Value, metadata: ResultMetadata, tool_name: impl Into<CompactStr>) -> Self {
        Self {
            value,
            metadata,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            tool_name: tool_name.into(),
            from_cache: false,
        }
    }

    pub fn from_cache(value: Value, metadata: ResultMetadata, tool_name: impl Into<CompactStr>) -> Self {
        Self {
            value,
            metadata,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            tool_name: tool_name.into(),
            from_cache: true,
        }
    }

    /// Whether this result is useful enough to include
    #[inline]
    pub fn is_useful(&self) -> bool {
        self.metadata.quality_score() > 0.3
    }

    /// Whether this result is high quality
    #[inline]
    pub fn is_high_quality(&self) -> bool {
        self.metadata.quality_score() > 0.7
    }

    /// Convert to a message-friendly format
    #[allow(
        clippy::cast_sign_loss,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )] // quality_score is always 0.0-1.0
    pub fn to_summary(&self) -> String {
        let quality = ((self.metadata.quality_score() * 100.0).round().max(0.0) as u32).min(100);
        match self.metadata.completeness {
            ResultCompleteness::Complete => {
                format!("{} found {} results (confidence: {}%)", self.tool_name, self.metadata.result_count, quality)
            }
            ResultCompleteness::Partial => {
                format!(
                    "{} found {} results (truncated, confidence: {}%)",
                    self.tool_name, self.metadata.result_count, quality
                )
            }
            ResultCompleteness::Empty => {
                format!("{} found no results", self.tool_name)
            }
            ResultCompleteness::Truncated => {
                format!("{} found results (truncated due to size, confidence: {}%)", self.tool_name, quality)
            }
        }
    }
}

/// Trait for scoring tool results
pub trait ResultScorer {
    /// Score a tool result and return metadata
    fn score(&self, result: &Value) -> ResultMetadata;

    /// Tool name this scorer handles
    fn tool_name(&self) -> &str;
}

#[cfg(test)]
mod tool_names_tests {
    use super::tool_names as names;

    #[test]
    fn every_tool_name_is_unique() {
        let all = [
            names::EXEC_COMMAND,
            names::WRITE_STDIN,
            names::APPLY_PATCH,
            names::RECORD_DECISION,
            names::CODE_SEARCH,
            names::COMPUTER_USE,
            names::UNIFIED_SEARCH,
            names::UNIFIED_EXEC,
            names::UNIFIED_FILE,
            names::THINK,
            names::SEARCH_TOOLS,
            names::MCP,
            names::MCP_SEARCH_TOOLS,
            names::MCP_GET_TOOL_DETAILS,
            names::MCP_LIST_SERVERS,
            names::MCP_CONNECT_SERVER,
            names::MCP_DISCONNECT_SERVER,
            names::WEB_SEARCH,
            names::WEB_FETCH,
            names::FETCH_URL,
            names::LIST,
            names::GREP,
            names::FETCH,
            names::EXEC_PTY_CMD,
            names::SHELL,
            names::GREP_FILE,
            names::LIST_FILES,
            names::LIST_SKILLS,
            names::LOAD_SKILL,
            names::LOAD_SKILL_RESOURCE,
            names::GET_GOAL,
            names::UPDATE_GOAL,
            names::RUN_PTY_CMD,
            names::CREATE_PTY_SESSION,
            names::LIST_PTY_SESSIONS,
            names::CLOSE_PTY_SESSION,
            names::SEND_PTY_INPUT,
            names::READ_PTY_SESSION,
            names::RESIZE_PTY_SESSION,
            names::EXECUTE_CODE,
            names::READ_FILE,
            names::WRITE_FILE,
            names::EDIT_FILE,
            names::DELETE_FILE,
            names::CREATE_FILE,
            names::SEARCH_REPLACE,
            names::FILE_OP,
            names::MOVE_FILE,
            names::COPY_FILE,
            names::GET_ERRORS,
            names::REQUEST_USER_INPUT,
            names::MEMORY,
            names::ASK_QUESTIONS,
            names::ASK_USER_QUESTION,
            names::MATRIX,
            names::AGENT,
            names::CRON,
            names::CRON_CREATE,
            names::CRON_LIST,
            names::CRON_DELETE,
            names::START_PLANNING,
            names::TASK_TRACKER,
            names::SPAWN_AGENT,
            names::SPAWN_BACKGROUND_SUBPROCESS,
            names::SEND_INPUT,
            names::WAIT_AGENT,
            names::RESUME_AGENT,
            names::CLOSE_AGENT,
            names::ASK_SESSION,
            names::ANSWER_SESSION,
            names::LIST_SESSIONS,
        ];
        let mut seen = std::collections::HashSet::new();
        for name in all {
            assert!(!name.is_empty());
            assert!(seen.insert(name), "duplicate tool name: {name}");
        }
    }

    #[test]
    fn composable_groups_cover_the_flat_namespace() {
        use super::tool_names::groups as g;
        let groups: [&[&str]; 12] = [
            g::MODEL_FACING,
            g::DISPATCHERS,
            g::EXECUTION,
            g::FILES,
            g::CONTROL,
            g::PLANNING,
            g::SUBAGENTS,
            g::SKILLS,
            g::GOAL,
            g::DIAGNOSTICS,
            g::IDS,
            g::ALIASES,
        ];
        let flat = [
            names::EXEC_COMMAND,
            names::WRITE_STDIN,
            names::APPLY_PATCH,
            names::RECORD_DECISION,
            names::CODE_SEARCH,
            names::COMPUTER_USE,
            names::UNIFIED_SEARCH,
            names::UNIFIED_EXEC,
            names::UNIFIED_FILE,
            names::GET_ERRORS,
            names::REQUEST_USER_INPUT,
            names::MEMORY,
            names::ASK_QUESTIONS,
            names::ASK_USER_QUESTION,
            names::MATRIX,
            names::AGENT,
            names::CRON,
            names::CRON_CREATE,
            names::CRON_LIST,
            names::CRON_DELETE,
            names::START_PLANNING,
            names::TASK_TRACKER,
            names::SPAWN_AGENT,
            names::SPAWN_BACKGROUND_SUBPROCESS,
            names::SEND_INPUT,
            names::WAIT_AGENT,
            names::RESUME_AGENT,
            names::CLOSE_AGENT,
            names::ASK_SESSION,
            names::ANSWER_SESSION,
            names::LIST_SESSIONS,
            names::LIST_SKILLS,
            names::LOAD_SKILL,
            names::LOAD_SKILL_RESOURCE,
            names::GET_GOAL,
            names::UPDATE_GOAL,
            names::RUN_PTY_CMD,
            names::CREATE_PTY_SESSION,
            names::LIST_PTY_SESSIONS,
            names::CLOSE_PTY_SESSION,
            names::SEND_PTY_INPUT,
            names::READ_PTY_SESSION,
            names::RESIZE_PTY_SESSION,
            names::EXECUTE_CODE,
            names::READ_FILE,
            names::WRITE_FILE,
            names::EDIT_FILE,
            names::DELETE_FILE,
            names::CREATE_FILE,
            names::SEARCH_REPLACE,
            names::FILE_OP,
            names::MOVE_FILE,
            names::COPY_FILE,
            names::THINK,
            names::SEARCH_TOOLS,
            names::MCP,
            names::MCP_SEARCH_TOOLS,
            names::MCP_GET_TOOL_DETAILS,
            names::MCP_LIST_SERVERS,
            names::MCP_CONNECT_SERVER,
            names::MCP_DISCONNECT_SERVER,
            names::WEB_SEARCH,
            names::WEB_FETCH,
            names::FETCH_URL,
            names::LIST,
            names::GREP,
            names::FETCH,
            names::EXEC_PTY_CMD,
            names::SHELL,
            names::GREP_FILE,
            names::LIST_FILES,
            names::BASH_ALIAS,
            names::EXEC_ALIAS,
            names::CONTAINER_EXEC_ALIAS,
            names::REPO_BROWSER_LIST_FILES,
            names::REPO_BROWSER_READ_FILE,
            names::REPO_BROWSER_WRITE_FILE,
        ];
        let mut covered = std::collections::HashSet::new();
        for group in groups {
            assert!(!group.is_empty());
            for name in group {
                assert!(flat.contains(name), "group member not in flat namespace: {name}");
                assert!(covered.insert(*name), "tool name in two groups: {name}");
            }
        }
        for name in flat {
            assert!(covered.contains(name), "flat tool name missing from every group: {name}");
        }
    }
}
