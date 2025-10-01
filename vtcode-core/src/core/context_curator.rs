//! Context Curator - Dynamic per-turn context selection
//!
//! Implements the iterative curation principle from Anthropic's context engineering guide.
//! Each turn, we select the most relevant context from available information to pass to the model.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::decision_tracker::DecisionTracker;
use super::token_budget::TokenBudgetManager;

/// Conversation phase detection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversationPhase {
    /// Initial exploration - understanding the codebase
    Exploration,
    /// Implementation - making changes
    Implementation,
    /// Validation - testing and verifying
    Validation,
    /// Debugging - fixing errors
    Debugging,
    /// Unknown - default state
    Unknown,
}

impl Default for ConversationPhase {
    fn default() -> Self {
        Self::Unknown
    }
}

/// Error context for tracking and learning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorContext {
    pub error_message: String,
    pub tool_name: Option<String>,
    pub resolution: Option<String>,
    pub timestamp: std::time::SystemTime,
}

/// File summary for compact context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSummary {
    pub path: String,
    pub size_lines: usize,
    pub last_modified: Option<std::time::SystemTime>,
    pub summary: String,
}

/// Tool definition for context selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub estimated_tokens: usize,
}

/// Message for conversation history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    pub estimated_tokens: usize,
}

/// Curated context result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuratedContext {
    pub recent_messages: Vec<Message>,
    pub active_files: Vec<FileSummary>,
    pub ledger_summary: Option<String>,
    pub recent_errors: Vec<ErrorContext>,
    pub relevant_tools: Vec<ToolDefinition>,
    pub estimated_tokens: usize,
    pub phase: ConversationPhase,
}

impl CuratedContext {
    pub fn new() -> Self {
        Self {
            recent_messages: Vec::new(),
            active_files: Vec::new(),
            ledger_summary: None,
            recent_errors: Vec::new(),
            relevant_tools: Vec::new(),
            estimated_tokens: 0,
            phase: ConversationPhase::Unknown,
        }
    }

    pub fn add_recent_messages(&mut self, messages: &[Message], count: usize) {
        let start = messages.len().saturating_sub(count);
        self.recent_messages.extend_from_slice(&messages[start..]);
        self.estimated_tokens += self
            .recent_messages
            .iter()
            .map(|m| m.estimated_tokens)
            .sum::<usize>();
    }

    pub fn add_file_context(&mut self, summary: FileSummary) {
        self.estimated_tokens += summary.summary.len() / 4; // Rough estimate
        self.active_files.push(summary);
    }

    pub fn add_ledger_summary(&mut self, summary: String) {
        self.estimated_tokens += summary.len() / 4; // Rough estimate
        self.ledger_summary = Some(summary);
    }

    pub fn add_error_context(&mut self, error: ErrorContext) {
        self.estimated_tokens += error.error_message.len() / 4; // Rough estimate
        self.recent_errors.push(error);
    }

    pub fn add_tools(&mut self, tools: Vec<ToolDefinition>) {
        for tool in &tools {
            self.estimated_tokens += tool.estimated_tokens;
        }
        self.relevant_tools = tools;
    }
}

impl Default for CuratedContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for context curation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCurationConfig {
    /// Enable dynamic context curation
    pub enabled: bool,
    /// Maximum tokens per turn
    pub max_tokens_per_turn: usize,
    /// Number of recent messages to always include
    pub preserve_recent_messages: usize,
    /// Maximum tool descriptions to include
    pub max_tool_descriptions: usize,
    /// Include decision ledger summary
    pub include_ledger: bool,
    /// Maximum ledger entries
    pub ledger_max_entries: usize,
    /// Include recent errors
    pub include_recent_errors: bool,
    /// Maximum recent errors to include
    pub max_recent_errors: usize,
}

impl Default for ContextCurationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_tokens_per_turn: 100_000,
            preserve_recent_messages: 5,
            max_tool_descriptions: 10,
            include_ledger: true,
            ledger_max_entries: 12,
            include_recent_errors: true,
            max_recent_errors: 3,
        }
    }
}

/// Dynamic context curator
pub struct ContextCurator {
    config: ContextCurationConfig,
    token_budget: Arc<TokenBudgetManager>,
    decision_ledger: Arc<RwLock<DecisionTracker>>,
    active_files: HashSet<String>,
    recent_errors: VecDeque<ErrorContext>,
    file_summaries: HashMap<String, FileSummary>,
    current_phase: ConversationPhase,
}

impl ContextCurator {
    /// Create a new context curator
    pub fn new(
        config: ContextCurationConfig,
        token_budget: Arc<TokenBudgetManager>,
        decision_ledger: Arc<RwLock<DecisionTracker>>,
    ) -> Self {
        Self {
            config,
            token_budget,
            decision_ledger,
            active_files: HashSet::new(),
            recent_errors: VecDeque::new(),
            file_summaries: HashMap::new(),
            current_phase: ConversationPhase::Unknown,
        }
    }

    /// Mark a file as active in current context
    pub fn mark_file_active(&mut self, path: String) {
        self.active_files.insert(path);
    }

    /// Add error context
    pub fn add_error(&mut self, error: ErrorContext) {
        self.recent_errors.push_back(error);
        if self.recent_errors.len() > self.config.max_recent_errors {
            self.recent_errors.pop_front();
        }
        // Errors trigger debugging phase
        self.current_phase = ConversationPhase::Debugging;
    }

    /// Add file summary
    pub fn add_file_summary(&mut self, summary: FileSummary) {
        self.file_summaries.insert(summary.path.clone(), summary);
    }

    /// Detect conversation phase from recent messages
    fn detect_phase(&mut self, messages: &[Message]) -> ConversationPhase {
        let mut detected_phase = ConversationPhase::Unknown;

        if let Some(recent) = messages.last() {
            let content_lower = recent.content.to_lowercase();

            // Simple heuristic-based phase detection
            if content_lower.contains("search")
                || content_lower.contains("find")
                || content_lower.contains("list")
            {
                detected_phase = ConversationPhase::Exploration;
            } else if content_lower.contains("edit")
                || content_lower.contains("write")
                || content_lower.contains("create")
                || content_lower.contains("modify")
            {
                detected_phase = ConversationPhase::Implementation;
            } else if content_lower.contains("test")
                || content_lower.contains("run")
                || content_lower.contains("check")
                || content_lower.contains("verify")
            {
                detected_phase = ConversationPhase::Validation;
            } else if content_lower.contains("error")
                || content_lower.contains("fix")
                || content_lower.contains("debug")
            {
                detected_phase = ConversationPhase::Debugging;
            }
        }

        if detected_phase == ConversationPhase::Unknown && !self.recent_errors.is_empty() {
            detected_phase = ConversationPhase::Debugging;
        }

        if detected_phase == ConversationPhase::Unknown {
            detected_phase = self.current_phase;
        }

        self.current_phase = detected_phase;
        detected_phase
    }

    /// Select relevant tools based on phase
    fn select_relevant_tools(
        &self,
        available_tools: &[ToolDefinition],
        phase: ConversationPhase,
    ) -> Vec<ToolDefinition> {
        let mut selected = Vec::new();
        let max_tools = self.config.max_tool_descriptions;

        match phase {
            ConversationPhase::Exploration => {
                // Prioritize search and exploration tools
                for tool in available_tools {
                    if tool.name.contains("grep")
                        || tool.name.contains("list")
                        || tool.name.contains("search")
                        || tool.name.contains("ast_grep")
                    {
                        selected.push(tool.clone());
                        if selected.len() >= max_tools {
                            break;
                        }
                    }
                }
            }
            ConversationPhase::Implementation => {
                // Prioritize file operation tools
                for tool in available_tools {
                    if tool.name.contains("edit")
                        || tool.name.contains("write")
                        || tool.name.contains("read")
                    {
                        selected.push(tool.clone());
                        if selected.len() >= max_tools {
                            break;
                        }
                    }
                }
            }
            ConversationPhase::Validation => {
                // Prioritize execution tools
                for tool in available_tools {
                    if tool.name.contains("run") || tool.name.contains("terminal") {
                        selected.push(tool.clone());
                        if selected.len() >= max_tools {
                            break;
                        }
                    }
                }
            }
            ConversationPhase::Debugging => {
                // Include diverse tools for debugging
                selected
                    .extend_from_slice(&available_tools[..max_tools.min(available_tools.len())]);
            }
            ConversationPhase::Unknown => {
                // Include most commonly used tools
                selected
                    .extend_from_slice(&available_tools[..max_tools.min(available_tools.len())]);
            }
        }

        // If we haven't filled our quota, add more tools
        if selected.len() < max_tools {
            for tool in available_tools {
                if !selected.iter().any(|t| t.name == tool.name) {
                    selected.push(tool.clone());
                    if selected.len() >= max_tools {
                        break;
                    }
                }
            }
        }

        selected
    }

    /// Compress context if needed
    fn compress_context(&self, mut context: CuratedContext, budget: usize) -> CuratedContext {
        if context.estimated_tokens <= budget {
            return context;
        }

        info!(
            "Context compression needed: {} tokens > {} budget",
            context.estimated_tokens, budget
        );

        // Reduce tools first
        while context.estimated_tokens > budget && context.relevant_tools.len() > 5 {
            if let Some(tool) = context.relevant_tools.pop() {
                context.estimated_tokens = context
                    .estimated_tokens
                    .saturating_sub(tool.estimated_tokens);
            }
        }

        // Reduce file contexts
        while context.estimated_tokens > budget && !context.active_files.is_empty() {
            context.active_files.pop();
            context.estimated_tokens = context.estimated_tokens.saturating_sub(100); // Rough estimate
        }

        // Reduce errors
        while context.estimated_tokens > budget && !context.recent_errors.is_empty() {
            if let Some(error) = context.recent_errors.pop() {
                context.estimated_tokens = context
                    .estimated_tokens
                    .saturating_sub(error.error_message.len() / 4);
            }
        }

        // Reduce messages (keep at least 3)
        while context.estimated_tokens > budget && context.recent_messages.len() > 3 {
            if let Some(msg) = context.recent_messages.first() {
                context.estimated_tokens = context
                    .estimated_tokens
                    .saturating_sub(msg.estimated_tokens);
                context.recent_messages.remove(0);
            }
        }

        warn!(
            "Context compressed to {} tokens (target: {})",
            context.estimated_tokens, budget
        );

        context
    }

    /// Curate context for the next model call
    pub async fn curate_context(
        &mut self,
        conversation: &[Message],
        available_tools: &[ToolDefinition],
    ) -> Result<CuratedContext> {
        if !self.config.enabled {
            debug!("Context curation disabled, returning default context");
            let mut context = CuratedContext::new();
            context.add_recent_messages(conversation, conversation.len());
            context.add_tools(available_tools.to_vec());
            return Ok(context);
        }

        let remaining = self.token_budget.remaining_tokens().await;
        let budget = remaining.min(self.config.max_tokens_per_turn);

        debug!("Curating context with budget: {} tokens", budget);

        let mut context = CuratedContext::new();

        // Detect phase
        let phase = self.detect_phase(conversation);
        context.phase = phase;
        debug!("Detected conversation phase: {:?}", phase);

        // Priority 1: Recent messages (always include)
        let message_count = self.config.preserve_recent_messages.min(conversation.len());
        context.add_recent_messages(conversation, message_count);
        debug!("Added {} recent messages", message_count);

        // Priority 2: Active work context (files being modified)
        for file_path in &self.active_files {
            if let Some(summary) = self.file_summaries.get(file_path) {
                context.add_file_context(summary.clone());
            }
        }
        debug!("Added {} active files", context.active_files.len());

        // Priority 3: Decision ledger (compact)
        if self.config.include_ledger {
            let ledger = self.decision_ledger.read().await;
            let summary = ledger.render_ledger_brief(self.config.ledger_max_entries);
            if !summary.is_empty() {
                context.add_ledger_summary(summary);
                debug!("Added decision ledger summary");
            }
        }

        // Priority 4: Recent errors and resolutions
        if self.config.include_recent_errors {
            let error_count = self.config.max_recent_errors.min(self.recent_errors.len());
            for error in self.recent_errors.iter().rev().take(error_count) {
                context.add_error_context(error.clone());
            }
            debug!("Added {} recent errors", error_count);
        }

        // Priority 5: Relevant tools (phase-aware selection)
        let relevant_tools = self.select_relevant_tools(available_tools, phase);
        context.add_tools(relevant_tools.clone());
        debug!("Added {} relevant tools", relevant_tools.len());

        // Check budget and compress if needed
        if context.estimated_tokens > budget {
            context = self.compress_context(context, budget);
        }

        info!(
            "Curated context: {} tokens (budget: {}), phase: {:?}",
            context.estimated_tokens, budget, phase
        );

        Ok(context)
    }

    /// Get current conversation phase
    pub fn current_phase(&self) -> ConversationPhase {
        self.current_phase
    }

    /// Clear active files (after task completion)
    pub fn clear_active_files(&mut self) {
        self.active_files.clear();
    }

    /// Clear recent errors (after resolution)
    pub fn clear_errors(&mut self) {
        self.recent_errors.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::token_budget::TokenBudgetConfig as CoreTokenBudgetConfig;

    #[tokio::test]
    async fn test_context_curation_basic() {
        let token_budget_config = CoreTokenBudgetConfig::for_model("gpt-4o-mini", 128_000);
        let token_budget = Arc::new(TokenBudgetManager::new(token_budget_config));
        let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
        let curation_config = ContextCurationConfig::default();

        let mut curator = ContextCurator::new(curation_config, token_budget, decision_ledger);

        let messages = vec![Message {
            role: "user".to_string(),
            content: "Search for the main function".to_string(),
            estimated_tokens: 10,
        }];

        let tools = vec![
            ToolDefinition {
                name: "grep_search".to_string(),
                description: "Search for patterns".to_string(),
                estimated_tokens: 50,
            },
            ToolDefinition {
                name: "edit_file".to_string(),
                description: "Edit a file".to_string(),
                estimated_tokens: 50,
            },
        ];

        let context = curator.curate_context(&messages, &tools).await.unwrap();

        assert_eq!(context.phase, ConversationPhase::Exploration);
        assert_eq!(context.recent_messages.len(), 1);
        assert!(!context.relevant_tools.is_empty());
    }

    #[test]
    fn test_phase_detection() {
        let token_budget_config = CoreTokenBudgetConfig::for_model("gpt-4o-mini", 128_000);
        let token_budget = Arc::new(TokenBudgetManager::new(token_budget_config));
        let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
        let curation_config = ContextCurationConfig::default();

        let mut curator = ContextCurator::new(curation_config, token_budget, decision_ledger);

        let messages = vec![Message {
            role: "user".to_string(),
            content: "Edit the config file".to_string(),
            estimated_tokens: 10,
        }];

        let phase = curator.detect_phase(&messages);
        assert_eq!(phase, ConversationPhase::Implementation);
    }
}
