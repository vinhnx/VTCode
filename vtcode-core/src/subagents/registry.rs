//! Subagent registry for managing specialized agents
//!
//! Loads subagent definitions from multiple sources with priority:
//! 1. Project-level: `.vtcode/agents/` (highest)
//! 2. CLI: `--agents` JSON flag
//! 3. User-level: `~/.vtcode/agents/`
//! 4. Built-in: shipped with binary (lowest)

use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use vtcode_config::subagent::{
    SubagentConfig, SubagentSource, SubagentsConfig,
    discover_subagents_in_dir,
};

/// Built-in subagent definitions
mod builtins {
    pub const EXPLORE_AGENT: &str = r#"---
name: explore
description: Fast, lightweight agent for searching and analyzing codebases. Use proactively for file discovery, code exploration, and understanding project structure. Operates in strict read-only mode.
tools: list_files, grep_file, read_file, run_pty_cmd
model: haiku
permissionMode: plan
---

You are a codebase exploration specialist optimized for speed and efficiency.

**Core Capabilities:**
- File pattern matching and discovery
- Content searching with regular expressions
- Reading and analyzing file contents
- Running read-only commands (ls, git status, git log, git diff, find, cat, head, tail)

**Execution Style:**
- Fast, minimal token output
- Focus on finding relevant information quickly
- Return absolute file paths for all discoveries
- Summarize findings concisely

**Constraints:**
- Strictly read-only - cannot create, modify, or delete files
- Cannot execute commands that modify state
- Focus on exploration, not modification

When invoked, immediately begin searching based on the query.
Return findings with file paths and relevant context.
"#;

    pub const PLAN_AGENT: &str = r#"---
name: plan
description: Research specialist for plan mode. Gathers context and analyzes codebase before presenting implementation plans. Use when Claude is in planning mode and needs to research the codebase.
tools: list_files, grep_file, read_file, run_pty_cmd
model: sonnet
permissionMode: plan
---

You are a research specialist for planning and analysis.

**Purpose:**
When the main agent is in plan mode, you research the codebase to gather
context needed for creating implementation plans.

**Process:**
1. Analyze the planning request
2. Search for relevant code, patterns, and dependencies
3. Identify affected files and components
4. Assess complexity and potential risks
5. Return structured findings for plan creation

**Output Format:**
Return findings organized by:
- Relevant files and their purposes
- Existing patterns to follow
- Dependencies and integration points
- Potential challenges or risks
- Recommended approach

Focus on gathering comprehensive context without making changes.
"#;

    pub const GENERAL_AGENT: &str = r#"---
name: general
description: Capable general-purpose agent for complex, multi-step tasks that require both exploration and action. Use for tasks that need reasoning, code modifications, and multiple strategies.
tools:
model: sonnet
---

You are a capable general-purpose agent for complex tasks.

**Capabilities:**
- Full read and write access to files
- Command execution and testing
- Multi-step reasoning and problem solving
- Code modifications and refactoring

**When to Use:**
- Complex research tasks requiring modifications
- Multi-step operations with dependencies
- Tasks where initial approaches may need adjustment
- Comprehensive code changes across multiple files

**Execution Style:**
- Thorough analysis before action
- Clear reasoning for decisions
- Verification of changes
- Detailed reporting of results

Approach tasks systematically, verify your work, and provide clear summaries.
"#;

    pub const CODE_REVIEWER_AGENT: &str = r#"---
name: code-reviewer
description: Expert code review specialist. Proactively reviews code for quality, security, and maintainability. Use immediately after writing or modifying code.
tools: read_file, grep_file, list_files, run_pty_cmd
model: inherit
permissionMode: plan
---

You are a senior code reviewer ensuring high standards of code quality and security.

**When invoked:**
1. Run git diff to see recent changes
2. Focus on modified files
3. Begin review immediately

**Review Checklist:**
- Code is clear and readable
- Functions and variables are well-named
- No duplicated code
- Proper error handling
- No exposed secrets or API keys
- Input validation implemented
- Good test coverage
- Performance considerations addressed

**Feedback Format:**
Organize by priority:
- **Critical** (must fix): Security issues, bugs, crashes
- **Warnings** (should fix): Code smells, maintainability
- **Suggestions** (consider): Style, optimization

Include specific examples of how to fix issues.
"#;

    pub const DEBUGGER_AGENT: &str = r#"---
name: debugger
description: Debugging specialist for errors, test failures, and unexpected behavior. Use proactively when encountering any issues.
tools: read_file, edit_file, run_pty_cmd, grep_file, list_files
model: inherit
---

You are an expert debugger specializing in root cause analysis.

**When invoked:**
1. Capture error message and stack trace
2. Identify reproduction steps
3. Isolate the failure location
4. Implement minimal fix
5. Verify solution works

**Debugging Process:**
- Analyze error messages and logs
- Check recent code changes
- Form and test hypotheses
- Add strategic debug logging
- Inspect variable states

**For Each Issue, Provide:**
- Root cause explanation
- Evidence supporting the diagnosis
- Specific code fix
- Testing approach
- Prevention recommendations

Focus on fixing the underlying issue, not the symptoms.
"#;
}

/// Running subagent instance
#[derive(Debug)]
pub struct RunningSubagent {
    /// Unique agent ID for this execution
    pub agent_id: String,
    /// Subagent configuration
    pub config: SubagentConfig,
    /// Transcript file path
    pub transcript_path: PathBuf,
    /// Start time
    pub started_at: std::time::Instant,
}

/// Registry for managing subagent configurations
pub struct SubagentRegistry {
    /// All loaded subagents by name
    agents: HashMap<String, SubagentConfig>,
    /// Priority order (project > cli > user > builtin)
    priority_order: Vec<String>,
    /// Configuration
    config: SubagentsConfig,
    /// Workspace root
    workspace_root: PathBuf,
    /// Active running subagents
    running: Arc<RwLock<HashMap<String, RunningSubagent>>>,
}

impl SubagentRegistry {
    /// Create a new registry and load all subagents
    pub async fn new(workspace_root: PathBuf, config: SubagentsConfig) -> Result<Self> {
        let mut registry = Self {
            agents: HashMap::new(),
            priority_order: Vec::new(),
            config,
            workspace_root: workspace_root.clone(),
            running: Arc::new(RwLock::new(HashMap::new())),
        };

        registry.load_all_agents().await?;
        Ok(registry)
    }

    /// Load subagents from all sources with proper priority
    async fn load_all_agents(&mut self) -> Result<()> {
        // 1. Load built-in agents (lowest priority)
        self.load_builtin_agents();

        // 2. Load user-level agents (~/.vtcode/agents/)
        if let Some(home) = dirs::home_dir() {
            let user_agents_dir = home.join(".vtcode").join("agents");
            self.load_agents_from_dir(&user_agents_dir, SubagentSource::User);
        }

        // 3. Load from additional configured directories
        for dir in &self.config.additional_agent_dirs.clone() {
            self.load_agents_from_dir(dir, SubagentSource::User);
        }

        // 4. Load project-level agents (highest priority)
        let project_agents_dir = self.workspace_root.join(".vtcode").join("agents");
        self.load_agents_from_dir(&project_agents_dir, SubagentSource::Project);

        info!(
            "Loaded {} subagents: {:?}",
            self.agents.len(),
            self.agents.keys().collect::<Vec<_>>()
        );

        Ok(())
    }

    /// Load built-in agent definitions
    fn load_builtin_agents(&mut self) {
        let builtins = [
            builtins::EXPLORE_AGENT,
            builtins::PLAN_AGENT,
            builtins::GENERAL_AGENT,
            builtins::CODE_REVIEWER_AGENT,
            builtins::DEBUGGER_AGENT,
        ];

        for content in builtins {
            match SubagentConfig::from_markdown(content, SubagentSource::Builtin, None) {
                Ok(config) => {
                    debug!("Loaded builtin agent: {}", config.name);
                    self.register_agent(config);
                }
                Err(e) => {
                    warn!("Failed to parse builtin agent: {}", e);
                }
            }
        }
    }

    /// Load agents from a directory
    fn load_agents_from_dir(&mut self, dir: &Path, source: SubagentSource) {
        if !dir.exists() {
            debug!("Subagent directory does not exist: {}", dir.display());
            return;
        }

        for result in discover_subagents_in_dir(dir, source) {
            match result {
                Ok(config) => {
                    debug!("Loaded agent from {}: {}", dir.display(), config.name);
                    self.register_agent(config);
                }
                Err(e) => {
                    warn!("Failed to load agent from {}: {}", dir.display(), e);
                }
            }
        }
    }

    /// Register a subagent (overwrites if same name with higher priority)
    fn register_agent(&mut self, config: SubagentConfig) {
        let name = config.name.clone();

        // Check if existing agent has higher priority
        if let Some(existing) = self.agents.get(&name) {
            let existing_priority = source_priority(&existing.source);
            let new_priority = source_priority(&config.source);

            if new_priority <= existing_priority {
                debug!(
                    "Skipping agent {} from {:?} (existing from {:?} has higher priority)",
                    name, config.source, existing.source
                );
                return;
            }
        }

        self.priority_order.retain(|n| n != &name);
        self.priority_order.push(name.clone());
        self.agents.insert(name, config);
    }

    /// Add agents from CLI --agents JSON flag
    pub fn add_cli_agents(&mut self, json: &Value) -> Result<()> {
        if let Some(obj) = json.as_object() {
            for (name, config_value) in obj {
                match SubagentConfig::from_json(name, config_value) {
                    Ok(config) => {
                        debug!("Loaded CLI agent: {}", config.name);
                        self.register_agent(config);
                    }
                    Err(e) => {
                        warn!("Failed to parse CLI agent {}: {}", name, e);
                    }
                }
            }
        }
        Ok(())
    }

    /// Get a subagent by name
    pub fn get(&self, name: &str) -> Option<&SubagentConfig> {
        self.agents.get(name)
    }

    /// Get all registered subagents
    pub fn all(&self) -> impl Iterator<Item = &SubagentConfig> {
        self.agents.values()
    }

    /// Get subagent names in priority order
    pub fn names(&self) -> &[String] {
        &self.priority_order
    }

    /// Find best matching subagent for a task description
    pub fn find_best_match(&self, description: &str) -> Option<&SubagentConfig> {
        let description_lower = description.to_lowercase();

        // Score each agent based on keyword matches in description
        let mut best_match: Option<(&SubagentConfig, usize)> = None;

        for agent in self.agents.values() {
            let agent_desc_lower = agent.description.to_lowercase();
            let mut score = 0;

            // Check for direct name mention
            if description_lower.contains(&agent.name) {
                score += 100;
            }

            // Check for keyword overlap
            for word in agent_desc_lower.split_whitespace() {
                if word.len() > 3 && description_lower.contains(word) {
                    score += 1;
                }
            }

            // Check for "proactively" or "use" hints in agent description
            if agent_desc_lower.contains("proactively")
                || agent_desc_lower.contains("use immediately")
            {
                score += 5;
            }

            if score > 0 {
                match &best_match {
                    Some((_, best_score)) if score > *best_score => {
                        best_match = Some((agent, score));
                    }
                    None => {
                        best_match = Some((agent, score));
                    }
                    _ => {}
                }
            }
        }

        best_match.map(|(agent, _)| agent)
    }

    /// Generate a unique agent ID for a new execution
    pub fn generate_agent_id(&self) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let random: u32 = rand::random::<u32>() % 10000;
        format!("{}-{}", timestamp, random)
    }

    /// Get transcript path for an agent execution
    pub fn transcript_path(&self, agent_id: &str) -> PathBuf {
        self.workspace_root
            .join(".vtcode")
            .join("transcripts")
            .join(format!("agent-{}.jsonl", agent_id))
    }

    /// Register a running subagent
    pub async fn register_running(&self, agent_id: String, config: SubagentConfig) {
        let transcript_path = self.transcript_path(&agent_id);
        let running = RunningSubagent {
            agent_id: agent_id.clone(),
            config,
            transcript_path,
            started_at: std::time::Instant::now(),
        };
        self.running.write().await.insert(agent_id, running);
    }

    /// Unregister a completed subagent
    pub async fn unregister_running(&self, agent_id: &str) -> Option<RunningSubagent> {
        self.running.write().await.remove(agent_id)
    }

    /// Get number of currently running subagents
    pub async fn running_count(&self) -> usize {
        self.running.read().await.len()
    }

    /// Check if we can spawn another subagent
    pub async fn can_spawn(&self) -> bool {
        self.running_count().await < self.config.max_concurrent
    }

    /// Get default timeout for subagent execution
    pub fn default_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.config.default_timeout_seconds)
    }

    /// Reload agents from disk
    pub async fn reload(&mut self) -> Result<()> {
        self.agents.clear();
        self.priority_order.clear();
        self.load_all_agents().await
    }
}

/// Get priority value for source (higher = takes precedence)
fn source_priority(source: &SubagentSource) -> u8 {
    match source {
        SubagentSource::Builtin => 0,
        SubagentSource::User => 1,
        SubagentSource::Plugin(_) => 2,
        SubagentSource::Cli => 3,
        SubagentSource::Project => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_registry_loads_builtins() {
        let registry =
            SubagentRegistry::new(PathBuf::from("/tmp/test"), SubagentsConfig::default())
                .await
                .unwrap();

        assert!(registry.get("explore").is_some());
        assert!(registry.get("plan").is_some());
        assert!(registry.get("general").is_some());
        assert!(registry.get("code-reviewer").is_some());
        assert!(registry.get("debugger").is_some());
    }

    #[tokio::test]
    async fn test_cli_agents_override_user() {
        let mut registry =
            SubagentRegistry::new(PathBuf::from("/tmp/test"), SubagentsConfig::default())
                .await
                .unwrap();

        let cli_json = serde_json::json!({
            "explore": {
                "description": "Custom explore agent",
                "prompt": "Custom prompt"
            }
        });

        registry.add_cli_agents(&cli_json).unwrap();

        let explore = registry.get("explore").unwrap();
        assert_eq!(explore.source, SubagentSource::Cli);
        assert_eq!(explore.description, "Custom explore agent");
    }

    #[tokio::test]
    async fn test_find_best_match() {
        let registry =
            SubagentRegistry::new(PathBuf::from("/tmp/test"), SubagentsConfig::default())
                .await
                .unwrap();

        let match1 = registry.find_best_match("use the code-reviewer to check my changes");
        assert!(match1.is_some());
        assert_eq!(match1.unwrap().name, "code-reviewer");

        let match2 = registry.find_best_match("search the codebase for authentication");
        assert!(match2.is_some());
        assert_eq!(match2.unwrap().name, "explore");
    }

    #[test]
    fn test_generate_agent_id() {
        let registry = SubagentRegistry {
            agents: HashMap::new(),
            priority_order: Vec::new(),
            config: SubagentsConfig::default(),
            workspace_root: PathBuf::from("/tmp"),
            running: Arc::new(RwLock::new(HashMap::new())),
        };

        let id1 = registry.generate_agent_id();
        let id2 = registry.generate_agent_id();
        assert_ne!(id1, id2);
    }
}
