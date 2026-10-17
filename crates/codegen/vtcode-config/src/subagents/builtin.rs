//! Built-in primary and subagent specifications.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use vtcode_commons::reasoning::ReasoningEffortLevel;

use crate::constants::tools;
use crate::constants::ui;
use crate::core::permissions::{AgentPermissionsConfig, PermissionDefault};
use crate::core::tools::ToolPolicy;

use super::{AgentMode, SubagentSource, SubagentSpec};

const BUILTIN_DEFAULT_AGENT: &str = r#"You are the default VT Code execution subagent.

Work directly, keep context isolated from the parent session, and return concise summaries.
Match the repository's local patterns, verify changes, and avoid unrelated edits.
If a file is referenced, read it before answering; base claims about code on what you have read.
Only make changes that are directly requested or clearly necessary. Keep solutions simple and focused.
Do not add features, refactor code, or make improvements beyond what was asked.
Verify your work by running the smallest relevant check before reporting completion."#;

const BUILTIN_EXPLORER_AGENT: &str = r#"You are a fast read-only exploration subagent.

Search the codebase, inspect relevant files, and return concise findings with file references.
Do not modify files or take mutating actions.
Read files before making claims about their contents.
Use structural search and grep over shell exploration when possible.
Return findings with file paths and line numbers for easy navigation."#;

const BUILTIN_WORKER_AGENT: &str = r#"You are a write-capable worker subagent.

Handle bounded implementation work, verify results, and return a concise outcome summary with
any important risks or follow-up items.
Read files before editing them or describing what they contain.
Only make changes that are directly requested. Keep solutions simple and focused.
Do not add features, refactor surrounding code, or make improvements beyond the scope.
Verify your changes by running relevant tests or checks before reporting completion.
If calls repeat without progress, re-plan instead of retrying identically."#;

const BUILTIN_BUILD_PRIMARY_AGENT: &str = r#"You are the build agent.

Understand the user's request, then use available tools to inspect context and make changes.
Read files before editing. Search before guessing at code structure.
Edit, write, and run commands directly when the request is clear.
Keep changes focused on what was asked. Do not add unrelated features or refactors.
When planning is needed, state the plan briefly before implementation. When the user only wants
discussion or review, do not edit files.
Report changed files, validation, and remaining risks clearly."#;

const BUILTIN_AUTO_PRIMARY_AGENT: &str = r#"You are the auto agent.

Work autonomously within the active permission policy, taking direct action when the request is clear.
Inspect the relevant repository context before editing, keep changes focused, and verify with the
narrowest useful checks before reporting completion.
Pause for user input when the scope is ambiguous, risky, or outside the requested work."#;

const BUILTIN_COORDINATOR_PRIMARY_AGENT: &str = r#"You are the coordinator agent.

Define explicit matrix tasks, dependencies, resource capacities, timeouts, and verification commands.
Delegate all shell execution, file changes, and verification to scheduler-owned matrix workers.
Use matrix create to persist a specification, then start to freeze it and dispatch work.
Delegate discovery to a read-only explorer before making repository claims, while the matrix is idle.
During active execution, use matrix status and control actions; do not spawn independent workers.
Resolve failed checks, permission denials, exhausted budgets, and unsafe retries with the user.
Worker summaries are not verification evidence. Report success only after matrix final verification.
Pause stops dispatch; cancel is terminal. Preserve sandbox, approvals, and existing budgets."#;

const DISCUSSION_FIRST_GUIDANCE: &str = r#"Be discussion-first. Clarify scope, constraints, contradictions, and options before implementation.
Resolve ordinary ambiguity from repository evidence when possible; ask the user directly only when material ambiguity is critical.
Stop researching when existing evidence supports a decision."#;

const BUILTIN_PLAN_AGENT_ROLE: &str = r#"You are a read-only planning agent.

Use repository-grounded, read-only discovery to gather the minimum context needed to support a plan or design decision.
Return findings, risks, and constraints clearly, with specific code references and file paths.
Read relevant files before making claims about the codebase.
Use structural search to find patterns across the repository.
When ready, emit exactly one final <proposed_plan> block for review.
Never write the plan file with shell or file-editing tools; the runtime persists the plan and tracker artifacts.
When the user asks for implementation, present the plan and wait for explicit user approval before implementation instead of suggesting an immediate edit."#;

const BUILTIN_DUCK_PRIMARY_AGENT_ROLE: &str = r#"You are the duck agent.

Do not edit files; you are for rubber-ducking only.
If the user asks for edits, suggest pressing Tab to switch to the Build agent for implementation."#;
/// Cached built-in subagent specifications.
///
/// Built once on first access and reused for every subsequent discovery call.
/// The constructors below (`builtin_primary_build_agent`, etc.) each allocate
/// a `SubagentSpec` with a `BTreeMap` of policy overrides and a cloned prompt
/// string; reconstructing all of them on every `discover_subagents` call is the
/// expensive part, so the result is memoized here. Callers still receive an
/// owned `Vec` (cloned from this cache) so they can mutate freely without
/// touching the shared canonical specs.
static BUILTIN_SUBAGENTS: LazyLock<Vec<SubagentSpec>> = LazyLock::new(builtin_subagents_inner);

pub fn builtin_subagents() -> Vec<SubagentSpec> {
    BUILTIN_SUBAGENTS.clone()
}

fn builtin_subagents_inner() -> Vec<SubagentSpec> {
    vec![
        builtin_primary_build_agent(),
        builtin_primary_auto_agent(),
        builtin_primary_coordinator_agent(),
        builtin_primary_duck_agent(),
        builtin_plan_agent(),
        SubagentSpec {
            name: "default".to_string(),
            description: "Default inheriting subagent for general delegated work.".to_string(),
            prompt: BUILTIN_DEFAULT_AGENT.to_string(),
            tools: None,
            disallowed_tools: Vec::new(),
            model: Some("inherit".to_string()),
            color: Some("blue".to_string()),
            reasoning_effort: None,
            permissions: mutating_agent_permissions(),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            mode: AgentMode::Subagent,
            max_turns: None,
            nickname_candidates: vec!["default".to_string()],
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
            source: SubagentSource::Builtin,
            file_path: None,
            warnings: Vec::new(),
            tool_policy_overrides: BTreeMap::new(),
        },
        SubagentSpec {
            name: "explorer".to_string(),
            description: "Read-only exploration specialist. Use proactively for code search, file discovery, and repository understanding.".to_string(),
            prompt: BUILTIN_EXPLORER_AGENT.to_string(),
            tools: Some(builtin_readonly_tool_ids()),
            disallowed_tools: builtin_readonly_disallowed_tool_ids(),
            model: Some("small".to_string()),
            color: Some("cyan".to_string()),
            reasoning_effort: Some(ReasoningEffortLevel::Low),
            permissions: readonly_agent_permissions(),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            mode: AgentMode::Subagent,
            max_turns: None,
            nickname_candidates: vec!["explore".to_string(), "search".to_string()],
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: vec!["explore".to_string()],
            source: SubagentSource::Builtin,
            file_path: None,
            warnings: Vec::new(),
            tool_policy_overrides: BTreeMap::new(),
        },
        SubagentSpec {
            name: "worker".to_string(),
            description: "Write-capable execution subagent for bounded implementation or multi-step action.".to_string(),
            prompt: BUILTIN_WORKER_AGENT.to_string(),
            tools: None,
            disallowed_tools: Vec::new(),
            model: Some("inherit".to_string()),
            color: Some("magenta".to_string()),
            reasoning_effort: None,
            permissions: mutating_agent_permissions(),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            mode: AgentMode::Subagent,
            max_turns: None,
            nickname_candidates: vec!["general".to_string(), "worker".to_string()],
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: vec!["general".to_string(), "general-purpose".to_string()],
            source: SubagentSource::Builtin,
            file_path: None,
            warnings: Vec::new(),
            tool_policy_overrides: BTreeMap::new(),
        },
    ]
}

pub fn builtin_primary_build_agent() -> SubagentSpec {
    SubagentSpec {
        name: "build".to_string(),
        description: "Built-in implementation agent for the main session.".to_string(),
        prompt: BUILTIN_BUILD_PRIMARY_AGENT.to_string(),
        tools: None,
        disallowed_tools: Vec::new(),
        model: Some("inherit".to_string()),
        color: Some(ui::AGENT_COLOR_BUILD.to_string()),
        reasoning_effort: None,
        permissions: mutating_agent_permissions(),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: AgentMode::Primary,
        max_turns: None,
        nickname_candidates: vec!["build".to_string(), "builder".to_string()],
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: vec!["builder".to_string()],
        source: SubagentSource::Builtin,
        file_path: None,
        warnings: Vec::new(),
        tool_policy_overrides: {
            let mut m = BTreeMap::new();
            m.insert("exec_command".to_string(), ToolPolicy::Allow);
            m
        },
    }
}

pub fn builtin_primary_auto_agent() -> SubagentSpec {
    SubagentSpec {
        name: "auto".to_string(),
        description: "Built-in autonomous implementation agent for the main session.".to_string(),
        prompt: BUILTIN_AUTO_PRIMARY_AGENT.to_string(),
        tools: None,
        disallowed_tools: Vec::new(),
        model: Some("inherit".to_string()),
        color: Some(ui::AGENT_COLOR_AUTO.to_string()),
        reasoning_effort: None,
        permissions: auto_agent_permissions(),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: AgentMode::Primary,
        max_turns: None,
        nickname_candidates: vec!["auto".to_string()],
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: vec!["autonomous".to_string()],
        source: SubagentSource::Builtin,
        file_path: None,
        warnings: Vec::new(),
        tool_policy_overrides: {
            let mut m = BTreeMap::new();
            m.insert("exec_command".to_string(), ToolPolicy::Allow);
            m
        },
    }
}

/// Opt-in orchestration role; worker permissions remain inherited from the session.
pub fn builtin_primary_coordinator_agent() -> SubagentSpec {
    let mut spec = builtin_primary_build_agent();
    spec.name = "coordinator".to_string();
    spec.description = "Deterministic local matrix orchestration for the main session.".to_string();
    spec.prompt = BUILTIN_COORDINATOR_PRIMARY_AGENT.to_string();
    spec.tools = Some(
        [
            "matrix",
            tools::REQUEST_USER_INPUT,
            "agent",
            tools::RECORD_DECISION,
            tools::TASK_TRACKER,
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
    );
    spec.nickname_candidates = vec!["coordinator".to_string()];
    spec.aliases.clear();
    spec.tool_policy_overrides.clear();
    spec
}

pub fn builtin_plan_agent() -> SubagentSpec {
    SubagentSpec {
        name: "plan".to_string(),
        description: "Built-in read-only planning agent definition.".to_string(),
        prompt: format!("{DISCUSSION_FIRST_GUIDANCE}\n\n{BUILTIN_PLAN_AGENT_ROLE}"),
        tools: Some(builtin_primary_readonly_tool_ids()),
        disallowed_tools: builtin_readonly_disallowed_tool_ids(),
        model: Some("inherit".to_string()),
        color: Some(ui::AGENT_COLOR_PLAN.to_string()),
        reasoning_effort: None,
        permissions: plan_agent_permissions(),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: AgentMode::Primary,
        max_turns: None,
        nickname_candidates: vec!["plan".to_string(), "planner".to_string()],
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: vec!["planner".to_string()],
        source: SubagentSource::Builtin,
        file_path: None,
        warnings: Vec::new(),
        tool_policy_overrides: BTreeMap::new(),
    }
}

pub fn builtin_primary_duck_agent() -> SubagentSpec {
    SubagentSpec {
        name: "duck".to_string(),
        description: "Built-in discussion-first agent for the main session.".to_string(),
        prompt: format!("{DISCUSSION_FIRST_GUIDANCE}\n\n{BUILTIN_DUCK_PRIMARY_AGENT_ROLE}"),
        tools: Some(builtin_primary_readonly_tool_ids()),
        disallowed_tools: builtin_readonly_disallowed_tool_ids(),
        model: Some("inherit".to_string()),
        color: Some(ui::AGENT_COLOR_DUCK.to_string()),
        reasoning_effort: None,
        permissions: readonly_interview_agent_permissions(),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: AgentMode::Primary,
        max_turns: None,
        nickname_candidates: vec!["duck".to_string()],
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: Vec::new(),
        source: SubagentSource::Builtin,
        file_path: None,
        warnings: Vec::new(),
        tool_policy_overrides: BTreeMap::new(),
    }
}

fn builtin_readonly_tool_ids() -> Vec<String> {
    // The direct read tools must be listed explicitly: read-only agents deny
    // `bash`, so the wire shaper hides `exec_command` for them — without
    // grep_file/read_file/list_files their effective catalog collapses to
    // bare `code_search` (the turn_912/913 planning failure shape). The
    // planning profile and Interactive surface intersect this list further
    // (e.g. read_file/list_files stay hidden on Interactive).
    vec![
        tools::CODE_SEARCH.to_string(),
        tools::EXEC_COMMAND.to_string(),
        tools::GREP_FILE.to_string(),
        tools::READ_FILE.to_string(),
        tools::LIST_FILES.to_string(),
    ]
}

/// Readonly tools for primary agents that interact with the user directly.
/// Extends the base readonly set with `request_user_input` so plan/duck can
/// ask clarifying questions.
fn builtin_primary_readonly_tool_ids() -> Vec<String> {
    let mut ids = builtin_readonly_tool_ids();
    ids.push(tools::REQUEST_USER_INPUT.to_string());
    ids.push(tools::RECORD_DECISION.to_string());
    ids
}

fn builtin_readonly_disallowed_tool_ids() -> Vec<String> {
    Vec::new()
}

pub(super) fn mutating_agent_permissions() -> AgentPermissionsConfig {
    AgentPermissionsConfig::new(PermissionDefault::Ask)
}

fn auto_agent_permissions() -> AgentPermissionsConfig {
    AgentPermissionsConfig::new(PermissionDefault::Auto)
}

pub(super) fn readonly_agent_permissions() -> AgentPermissionsConfig {
    let mut permissions = AgentPermissionsConfig::new(PermissionDefault::Deny);
    permissions.allow = vec!["read".to_string()];
    permissions
}

/// Read-only agents that talk to the user (duck, plan) also list
/// `request_user_input` and `record_decision` in their tool set. Allow both
/// explicitly: the default-deny fallback otherwise rejects their semantic
/// kind "other" and hides them from the wire catalog.
pub(super) fn readonly_interview_agent_permissions() -> AgentPermissionsConfig {
    let mut permissions = readonly_agent_permissions();
    permissions.allow.push("request_user_input".to_string());
    permissions.allow.push(tools::RECORD_DECISION.to_string());
    permissions
}

/// Plan-agent permissions extend the read-only interview set with `bash` so
/// `exec_command` stays on the wire and read-only shell inspection (`rg`,
/// `cat`, `wc`, ...) works while planning. This stays read-only in practice:
/// selecting the plan agent always activates the planning workflow, whose
/// dispatch gate (`ToolRegistry::is_planning_active_allowed` +
/// `assess_plan_mode_read_only_bash_command`) hard-blocks every mutating
/// command before execution — the bash permission only admits the tool, the
/// planning gate keeps it read-only. Explorer/duck stay bash-denied via
/// [`readonly_agent_permissions`]/[`readonly_interview_agent_permissions`].
fn plan_agent_permissions() -> AgentPermissionsConfig {
    let mut permissions = readonly_interview_agent_permissions();
    permissions.allow.push("bash".to_string());
    permissions
}
