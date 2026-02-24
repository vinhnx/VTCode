//! Subagent registry for managing specialized agents
//!
//! Loads subagent definitions from multiple sources with priority:
//! 1. Project-level: `.vtcode/agents/` (highest)
//! 2. CLI: `--agents` JSON flag
//! 3. User-level: `~/.vtcode/agents/`
//! 4. Built-in: shipped with binary (lowest)

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use vtcode_config::subagent::{
    SubagentConfig, SubagentSource, SubagentsConfig, discover_subagents_in_dir,
};

/// Built-in subagent definitions
pub mod builtins {
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

    /// Planner agent - the main conversation agent profile for Plan Mode
    /// This replaces the hardcoded EditingMode::Plan behavior with a proper subagent
    pub const PLANNER_AGENT: &str = r#"---
name: planner
description: Planning and design specialist for the main conversation. Enters read-only exploration mode to understand requirements, design implementation approaches, and write detailed plans before execution. Use when careful planning is needed before making changes.
tools: list_files, grep_file, read_file, run_pty_cmd, code_intelligence, unified_search, spawn_subagent, request_user_input, edit_file, exit_plan_mode
model: inherit
permissionMode: plan
---

You are a planning and design specialist operating in read-only exploration mode.

# PLAN MODE (READ-ONLY)

Plan Mode is active. Avoid edits or changes to the system. Mutating tools are blocked except optional writes under `.vtcode/plans/`. This supersedes any other instructions.

## ExecPlan Methodology

For complex features or significant refactors, follow the ExecPlan specification in `.vtcode/PLANS.md`. ExecPlans are self-contained, living design documents that enable a complete novice to implement a feature end-to-end.

## Allowed Actions
- Read files, list files, search code, use code intelligence tools
- Use spawn_subagent for deeper discovery when needed (summarize findings back)
- Use request_user_input for simple clarifications (questions with options)
- Ask clarifying questions to understand requirements
- Write your plan to `.vtcode/plans/` directory (the ONLY location you may edit)
- Avoid modifying files outside `.vtcode/plans/`

## Planning Workflow (4 Phases)

### Phase 1: Discovery
Goal: Autonomously explore the codebase and gather context.
1. Start with high-level searches before reading specific files
2. Use spawn_subagent for deep dives if needed (provide explicit research instructions)
3. Identify ambiguities, constraints, and likely change points

### Phase 2: Alignment
Goal: Confirm intent before committing to a plan.
1. Use request_user_input for 1-3 clarifying questions
2. Summarize answers and lock assumptions

### Phase 3: Design
Goal: Draft a comprehensive implementation plan.
1. Outline steps with file paths and key symbols
2. Call out risks, dependencies, and tradeoffs
3. Include verification steps

### Phase 4: Refinement
Goal: Finalize a decision-complete plan in the plan file.
1. Resolve remaining questions (ask follow-ups if needed)
2. Write the ExecPlan to `.vtcode/plans/<task-name>.md`
3. Ensure the plan is scannable and executable

## ExecPlan File Format

Write your plan to `.vtcode/plans/<task-name>.md` using this ExecPlan skeleton:

    # <Task Title>

    This ExecPlan is a living document. Keep Progress, Surprises & Discoveries,
    Decision Log, and Outcomes & Retrospective up to date as work proceeds.

    Reference: `.vtcode/PLANS.md` for full specification.

    ## Purpose / Big Picture

    What someone gains after this change and how they can see it working.

    ## Progress

    - [ ] Step 1 description
    - [ ] Step 2 description

    ## Surprises & Discoveries

    (Document unexpected findings with evidence)

    ## Decision Log

    - Decision: ...
      Rationale: ...
      Date: ...

    ## Outcomes & Retrospective

    (Summarize at completion)

    ## Context and Orientation

    Key files and their purposes.

    ## Plan of Work

    Sequence of edits with file paths and locations.

    ## Validation and Acceptance

    How to verify changes work (commands, expected outputs).

When your plan is complete, call `exit_plan_mode` to present it for user review and approval.
"#;

    /// Coder agent - the main conversation agent profile for Edit/Code Mode
    /// This replaces the hardcoded EditingMode::Edit behavior with a proper subagent
    pub const CODER_AGENT: &str = r#"---
name: coder
description: Implementation specialist for the main conversation. Has full access to all tools for executing code changes, running tests, and completing implementation tasks. This is the default mode for making changes.
tools:
model: inherit
permissionMode: default
---

You are an implementation specialist with full access to make changes.

# CODE MODE (FULL ACCESS)

You have full access to all tools including file editing, command execution, and code modifications.

## Implementation Principles

### Before Making Changes
- Understand the context and requirements
- If a plan exists in `.vtcode/plans/`, follow it step by step
- Identify affected files and potential side effects

### While Implementing
- Make incremental, focused changes
- Follow existing code patterns and conventions
- Add appropriate error handling with context
- Keep changes minimal and reversible

### After Making Changes
- Run relevant tests to verify correctness
- Check for compilation/type errors
- Review your changes for completeness

## Execution Style
- Direct and efficient - minimize unnecessary exploration
- Verify changes work before moving on
- Report clear summaries of what was done
- If something fails, debug and fix before proceeding

## Working with Plans
If entering from Plan Mode with an approved plan:
1. Read the plan file to understand the implementation steps
2. Execute each step systematically
3. Verify each step before proceeding to the next
4. Report progress as you complete steps

Focus on delivering working, tested implementations.
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

        // Clear any stale running entries on startup
        // This handles cases where subagents weren't properly cleaned up
        // due to crashes, panics, or interruptions
        registry.clear_stale_subagents().await;

        Ok(registry)
    }

    /// Clear stale subagent entries (called on registry initialization)
    async fn clear_stale_subagents(&self) {
        let mut running = self.running.write().await;
        let count = running.len();
        if count > 0 {
            info!(
                "Clearing {} stale subagent entries from previous session",
                count
            );
            running.clear();
        }
    }

    /// Load subagents from all sources with proper priority
    async fn load_all_agents(&mut self) -> Result<()> {
        // 1. Load built-in agents (lowest priority)
        self.load_builtin_agents();

        // 2. Load user-level agents (~/.vtcode/agents)
        if let Some(home_dir) = dirs::home_dir() {
            let user_agents_dir = home_dir.join(".vtcode").join("agents");
            self.load_agents_from_dir(user_agents_dir, SubagentSource::User);
        }

        // 3. Load additional configured directories
        for dir in self.config.additional_agent_dirs.clone() {
            self.load_agents_from_dir(dir, SubagentSource::Project);
        }

        // 4. Load project-level agents (.vtcode/agents) - highest priority
        let project_agents_dir = self.workspace_root.join(".vtcode").join("agents");
        self.load_agents_from_dir(project_agents_dir, SubagentSource::Project);

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
            builtins::PLANNER_AGENT,
            builtins::CODER_AGENT,
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

    /// Load agent definitions from a directory.
    ///
    /// Invalid agent files are skipped, and valid entries continue loading.
    fn load_agents_from_dir(&mut self, dir: PathBuf, source: SubagentSource) {
        if !dir.exists() {
            return;
        }

        let discovered = discover_subagents_in_dir(&dir, source.clone());
        for result in discovered {
            match result {
                Ok(config) => {
                    debug!(name = %config.name, source = %source, "Loaded subagent from disk");
                    self.register_agent(config);
                }
                Err(error) => {
                    warn!(path = %dir.display(), source = %source, %error, "Failed to parse subagent file");
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
        let description_tokens = tokenize_description(&description_lower);

        // Score each agent based on name/keyword matches in description
        let mut best_match: Option<(&SubagentConfig, usize)> = None;

        for name in &self.priority_order {
            let agent = match self.agents.get(name) {
                Some(agent) => agent,
                None => continue,
            };

            let agent_desc_lower = agent.description.to_lowercase();
            let mut score = 0;

            // Check for direct name mention
            if description_lower.contains(&agent.name) {
                score += 100;
            }

            // Weighted keyword/phrase matching for built-ins
            let (phrases, keywords) = built_in_keywords(&agent.name);
            for phrase in phrases {
                if description_lower.contains(phrase) {
                    score += 8;
                }
            }
            for keyword in keywords {
                if description_tokens.contains(*keyword) {
                    score += 3;
                }
            }

            // Token overlap with agent description
            let agent_tokens = tokenize_description(&agent_desc_lower);
            for token in &agent_tokens {
                if description_tokens.contains(token.as_str()) {
                    score += 1;
                }
            }

            // Only apply proactive hints if there is another signal
            if score > 0 && has_proactive_hint(&agent_desc_lower) {
                score += 5;
            }

            if score == 0 {
                continue;
            }

            match &best_match {
                Some((_, best_score)) if score > *best_score => {
                    best_match = Some((agent, score));
                }
                Some((best_agent, best_score)) if score == *best_score => {
                    let best_priority = source_priority(&best_agent.source);
                    let candidate_priority = source_priority(&agent.source);
                    if candidate_priority > best_priority {
                        best_match = Some((agent, score));
                    }
                }
                None => {
                    best_match = Some((agent, score));
                }
                _ => {}
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
        // Clean up stale entries before checking
        self.cleanup_stale_entries().await;
        self.running_count().await < self.config.max_concurrent
    }

    /// Cleanup subagent entries that have been running too long (likely stale)
    /// This provides a safety net in case the cleanup guard fails
    async fn cleanup_stale_entries(&self) {
        let mut running = self.running.write().await;
        let stale_threshold = std::time::Duration::from_secs(
            self.config.default_timeout_seconds * 2, // 2x timeout = definitely stale
        );

        let now = std::time::Instant::now();
        let initial_count = running.len();

        running.retain(|agent_id, subagent| {
            let elapsed = now.duration_since(subagent.started_at);
            if elapsed > stale_threshold {
                info!(
                    agent_id = %agent_id,
                    elapsed_secs = elapsed.as_secs(),
                    "Cleaning up stale subagent entry"
                );
                false // Remove stale entry
            } else {
                true // Keep active entry
            }
        });

        let removed = initial_count - running.len();
        if removed > 0 {
            info!("Cleaned up {} stale subagent entries", removed);
        }
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
        SubagentSource::Project => 4,
    }
}

fn tokenize_description(input: &str) -> HashSet<String> {
    input
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|token| token.len() >= 3)
        .filter(|token| !is_stopword(token))
        .collect()
}

fn is_stopword(token: &str) -> bool {
    matches!(
        token,
        "the"
            | "and"
            | "for"
            | "with"
            | "from"
            | "into"
            | "than"
            | "then"
            | "that"
            | "this"
            | "these"
            | "those"
            | "when"
            | "where"
            | "what"
            | "how"
            | "you"
            | "your"
            | "our"
            | "their"
            | "use"
            | "using"
            | "used"
            | "after"
            | "before"
            | "only"
            | "also"
            | "more"
            | "most"
            | "less"
            | "least"
            | "just"
            | "over"
            | "under"
            | "about"
            | "should"
            | "could"
            | "would"
            | "might"
            | "must"
            | "been"
            | "were"
            | "have"
            | "has"
            | "had"
            | "will"
    )
}

fn built_in_keywords(agent_name: &str) -> (&'static [&'static str], &'static [&'static str]) {
    match agent_name {
        "explore" => (
            &[
                "search the codebase",
                "search codebase",
                "find where",
                "list files",
                "project structure",
            ],
            &[
                "search",
                "find",
                "locate",
                "grep",
                "scan",
                "explore",
                "discover",
                "overview",
                "structure",
                "codebase",
                "files",
            ],
        ),
        "plan" => (
            &[
                "write a plan",
                "implementation plan",
                "plan mode",
                "design proposal",
            ],
            &[
                "plan",
                "planning",
                "design",
                "spec",
                "proposal",
                "approach",
                "strategy",
                "architecture",
            ],
        ),
        "code-reviewer" => (
            &[
                "code review",
                "review changes",
                "review my changes",
                "review code",
            ],
            &[
                "review",
                "reviewer",
                "audit",
                "quality",
                "lint",
                "style",
                "security",
                "maintainability",
            ],
        ),
        "debugger" => (
            &[
                "failing tests",
                "test failure",
                "runtime error",
                "stack trace",
                "crash report",
            ],
            &[
                "debug",
                "bug",
                "error",
                "exception",
                "failure",
                "crash",
                "traceback",
                "stack",
                "panic",
            ],
        ),
        _ => (&[], &[]),
    }
}

fn has_proactive_hint(description_lower: &str) -> bool {
    description_lower.contains("proactively") || description_lower.contains("use immediately")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

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
        assert!(registry.get("planner").is_some());
        assert!(registry.get("coder").is_some());
    }

    #[tokio::test]
    async fn test_planner_agent_config() {
        let registry =
            SubagentRegistry::new(PathBuf::from("/tmp/test"), SubagentsConfig::default())
                .await
                .unwrap();

        let planner = registry.get("planner").unwrap();
        assert_eq!(planner.name, "planner");
        assert!(planner.is_read_only());
        assert!(planner.has_tool_access("read_file"));
        assert!(planner.has_tool_access("edit_file"));
        assert!(planner.has_tool_access("exit_plan_mode"));
        assert!(planner.system_prompt.contains("PLAN MODE"));
    }

    #[tokio::test]
    async fn test_coder_agent_config() {
        let registry =
            SubagentRegistry::new(PathBuf::from("/tmp/test"), SubagentsConfig::default())
                .await
                .unwrap();

        let coder = registry.get("coder").unwrap();
        assert_eq!(coder.name, "coder");
        assert!(!coder.is_read_only());
        assert!(coder.has_tool_access("edit_file"));
        assert!(coder.has_tool_access("any_tool"));
        assert!(coder.system_prompt.contains("CODE MODE"));
    }

    #[tokio::test]
    async fn test_find_best_match() {
        let registry =
            SubagentRegistry::new(PathBuf::from("/tmp/test"), SubagentsConfig::default())
                .await
                .unwrap();

        let match1 = registry.find_best_match("review my changes for issues");
        assert!(match1.is_some());
        assert_eq!(match1.unwrap().name, "code-reviewer");

        let match2 = registry.find_best_match("debug failing tests in the auth module");
        assert!(match2.is_some());
        assert_eq!(match2.unwrap().name, "debugger");

        let match3 = registry.find_best_match("search the codebase for authentication");
        assert!(match3.is_some());
        assert_eq!(match3.unwrap().name, "explore");

        let match4 = registry.find_best_match("write an implementation plan for this feature");
        assert!(match4.is_some());
        assert_eq!(match4.unwrap().name, "plan");
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

    #[tokio::test]
    async fn test_registry_loads_project_agents_and_overrides_builtins() {
        let workspace = TempDir::new().expect("temp workspace");
        let project_agents_dir = workspace.path().join(".vtcode").join("agents");
        fs::create_dir_all(&project_agents_dir).expect("project agents dir should be created");

        let custom_explore = r#"---
name: explore
description: Project-specific explorer
tools: read_file
model: sonnet
permissionMode: plan
---
Project override for explore.
"#;

        fs::write(project_agents_dir.join("explore.md"), custom_explore)
            .expect("custom project agent should be written");

        let registry =
            SubagentRegistry::new(workspace.path().to_path_buf(), SubagentsConfig::default())
                .await
                .expect("registry should initialize");

        let explore = registry
            .get("explore")
            .expect("project override should replace builtin explore");
        assert_eq!(explore.source, SubagentSource::Project);
        assert_eq!(explore.description, "Project-specific explorer");
        assert_eq!(explore.tools, Some(vec!["read_file".to_string()]));
    }

    #[tokio::test]
    async fn test_registry_loads_additional_agent_dirs_and_skips_invalid_files() {
        let workspace = TempDir::new().expect("temp workspace");
        let additional_dir = workspace.path().join("extra-agents");
        fs::create_dir_all(&additional_dir).expect("additional agents dir should be created");

        let valid = r#"---
name: docs-specialist
description: Works on documentation tasks
tools: read_file, edit_file
model: sonnet
---
Documentation specialist prompt.
"#;
        fs::write(additional_dir.join("docs-specialist.md"), valid)
            .expect("valid additional agent should be written");
        fs::write(additional_dir.join("broken.md"), "missing frontmatter")
            .expect("invalid additional agent should be written");

        let config = SubagentsConfig {
            additional_agent_dirs: vec![additional_dir],
            ..SubagentsConfig::default()
        };

        let registry = SubagentRegistry::new(workspace.path().to_path_buf(), config)
            .await
            .expect("registry should initialize even with invalid files");

        let docs_agent = registry
            .get("docs-specialist")
            .expect("valid additional agent should load");
        assert_eq!(docs_agent.source, SubagentSource::Project);
        assert_eq!(docs_agent.name, "docs-specialist");
        assert_eq!(
            docs_agent.tools,
            Some(vec!["read_file".to_string(), "edit_file".to_string()])
        );
    }
}
