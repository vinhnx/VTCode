//! System instructions and prompt management.
//!
//! Prompt variants share one canonical base contract plus thin mode deltas and
//! compact runtime addenda. Richer behavior comes from AGENTS.md, dynamic tool
//! guidance, skill metadata, and runtime notices.

use crate::config::constants::prompt_budget as prompt_budget_constants;
use crate::config::types::{ShellPromptProfile, SystemPromptMode};
use crate::llm::providers::gemini::wire::Content;
use crate::project_doc::read_project_doc;
use crate::prompts::context::PromptContext;
use crate::prompts::guidelines::{generate_tool_guidelines_for_profile, render_shell_profile_guidance};
use crate::prompts::output_styles::OutputStyleApplier;
use crate::prompts::render::render_environment_addenda;
use crate::prompts::resources::{apply_system_prompt_layers, resolve_system_prompt_layers};
pub use crate::prompts::static_prompts::{
    agent_identity_label, default_lightweight_prompt, default_system_prompt, lightweight_instruction_text,
    minimal_instruction_text, minimal_system_prompt, specialized_instruction_text, specialized_system_prompt,
    static_profile_prompt,
};
use crate::prompts::system_prompt_cache::PROMPT_CACHE;
use crate::skills::render::render_prompt_skills_section;
use std::env;
use std::path::Path;
use std::sync::OnceLock;
use tracing::warn;

/// Shared Planning workflow header used by both static and incremental prompt builders.
pub const PLANNING_WORKFLOW_READ_ONLY_HEADER: &str = "# PLANNING WORKFLOW (READ-ONLY)";
/// Shared Planning workflow notice line describing strict read-only enforcement.
pub const PLANNING_WORKFLOW_READ_ONLY_NOTICE_LINE: &str = "Mutating file edits are blocked, including `apply_patch`. Use `exec_command.cmd` only for read-only repository inspection with the active shell profile's syntax; keep `task_tracker` current. Plan artifacts under `.vtcode/plans/` are allowed.";
/// Shared Planning workflow instruction line for transitioning to implementation.
pub const PLANNING_WORKFLOW_EXIT_INSTRUCTION_LINE: &str =
    "Call `finish_planning` to present the plan. Mutating tools stay disabled until user approves.";
/// Compact, spec-like plan quality line. The previous wording ("summary,
/// steps, test cases, assumptions") let the model emit verbosely large plans
/// that blew the generation token budget and were cut off mid-`<proposed_plan>`
/// — which previously re-triggered the recovery loop forever. This mandates a
/// tight spec that fits a small token budget and prefers file:symbol
/// references over prose. It also forbids wrapping those references in
/// markdown link syntax or editor/IDE URI schemes (e.g. `vscode-file://`,
/// `file://`) — plans are read in terminals and other non-hyperlink
/// surfaces, and a bare `path/to/file.rs:42` reference is portable while a
/// broken pseudo-link pointing at the editor binary itself is not.
pub const PLANNING_WORKFLOW_PLAN_QUALITY_LINE: &str = "Keep plans compact and spec-like. Emit ONE `<proposed_plan>` that fits ~1500 tokens: a 1-3 line Summary; a tight numbered step list where each step is `Action -> files/symbols -> verify:`; one Validation line (build/lint + test commands); Assumptions as short bullets. Prefer file:symbol references over prose, written as plain text or inline code (e.g. `src/main.rs:42`) — never as markdown links or editor/IDE URIs (no `[label](url)`, no `vscode-file://`/`file://` schemes). Ask only material blocking questions; unresolved: `Next open decision: ...`.";
/// Scale research effort to the request instead of always exhaustively
/// enumerating the repository. Checkpoint turn_647 showed a "make a simple
/// plan to improve launch time" request burn 70+ tool calls across dozens of
/// files until the turn's tool wall-clock budget was exhausted with no plan
/// delivered — the model had no signal to stop researching and draft. This
/// line gives it a concrete budget to self-regulate against.
pub const PLANNING_WORKFLOW_RESEARCH_SCOPE_LINE: &str = "Scale research to the request: for a narrow or simple ask, ~5-10 targeted reads/searches is usually enough before drafting `<proposed_plan>` — do not exhaustively enumerate the whole repository. For a broad or ambiguous ask, research proportionally more, but stop and draft as soon as scope/decomposition/verification decisions are closed.";
/// Shared Planning workflow policy line requiring context-aware interview closure before final plans.
pub const PLANNING_WORKFLOW_INTERVIEW_POLICY_LINE: &str = "Use `request_user_input` for interview questions informed by repo context. Continue until scope/decomposition/verification decisions are closed before finalizing `<proposed_plan>`.";
/// Shared Planning workflow policy line for runtimes where `request_user_input` is unavailable.
pub const PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE: &str = "`request_user_input` unavailable here. Continue exploring read-only, finish unblocked planning, surface blockers in plain text.";
/// Shared Planning workflow guard line requiring explicit transition from planning to execution.
pub const PLANNING_WORKFLOW_NO_AUTO_EXIT_LINE: &str =
    "Do not auto-exit Planning workflow; wait for explicit implementation intent.";
/// Shared Planning workflow task-tracking line clarifying availability and aliasing.
/// Implementation prompt used when transitioning from planning to execution.
pub const PLANNING_WORKFLOW_IMPLEMENTATION_PROMPT: &str = "Implement the approved plan.";
/// Hint shown when planning workflow is active.
pub const PLANNING_WORKFLOW_HINT: &str =
    "Planning workflow is active. Type `implement` to start execution or continue refining the plan.";

pub const PLANNING_WORKFLOW_TASK_TRACKER_LINE: &str = "`task_tracker` remains available while planning.";
/// Shared reminder appended when presenting plans while still in Planning workflow.
pub const PLANNING_WORKFLOW_IMPLEMENT_REMINDER: &str = "• Planning workflow is active with read-only permissions. Say “implement” to present the plan for user approval, or “stay in planning workflow” to revise. Calling `finish_planning` only presents the plan; mutating tools stay disabled until the user approves the plan. If a write tool is unavailable because planning workflow is active, do not emit the full artifact content in the chat. Instead, summarize the blocker briefly and ask the user to save the content, or call `finish_planning` to present the plan for approval.";

pub const PROMPT_TITLE: &str = "# VT Code";
pub const PROMPT_INTRO: &str = "VT Code. Be concise and safe.";
pub const CONTRACT_HEADER: &str = "## Contract";

/// Contract rules shared across all prompt modes.
pub const SHARED_CONTRACT_LINES: &[&str] = &[
    "If context is missing, say so, do not guess, finish unblocked slices.",
    "Do not use emoji in responses.",
    "Use retrieved evidence when citation-sensitive.",
    "Preserve task goal, tracker state, touched files, verification status, and decisions across compaction.",
    "Keep outputs concise; keep agent loops simple and let the model choose the next useful step.",
    "`spool_path` holds full tool output. Inspect it once with a targeted shell command through `exec_command.cmd` instead of repeatedly dumping the whole file. Past-turn errors are already in history.",
];

/// Default/Lightweight/Specialized mode: expanded contract lines beyond shared rules.
pub const DEFAULT_SPECIFIC_LINES: &[&str] = &[
    "Start with existing `AGENTS.md` and `CLAUDE.md`; inspect code first and match local patterns.",
    "Take safe, reversible steps; recover from tool errors with corrected parameters, smaller scope, or one focused clarification.",
    "Ask only for material behavior, API, UX, or credential changes.",
    "Keep control on the main thread. Delegate bounded, independent work only.",
    "Verify changes yourself; never claim a check passed unless you ran it.",
    "Keep user updates brief and high-signal.",
    "Read files before answering. Never speculate about code you have not opened.",
    "Make only requested changes. When the active agent has tool access, use tools to implement directly; otherwise stay within the active agent mode.",
];

/// Minimal mode: additional contract lines beyond shared rules.
pub const MINIMAL_SPECIFIC_LINES: &[&str] = &[
    "Use existing `AGENTS.md` and `CLAUDE.md`; inspect code first.",
    "Take safe, reversible steps; verify changes yourself.",
    "Keep delegation and skills bounded, explicit, and narrow.",
];

pub const DEFAULT_OPERATING_PROFILE_DELTA: &str = r#"## Operating Profile

- Available tools in the default profile are `exec_command`, `write_stdin`, and `apply_patch`.
- Put normal shell commands in `exec_command.cmd`; they are not separate function tools. Follow the active shell profile's syntax.
- Treat completion language as a checkpoint, not proof; only stop when verification is resolved.
- When tools are available, read files and search the codebase before answering; use tools to implement directly rather than describing what should be done.
- Use Planning workflow for research/spec work; stay read-only until implementation intent is explicit."#;

pub const MINIMAL_OPERATING_PROFILE_DELTA: &str = r#"## Operating Profile

- Stay precise; use `task_tracker` once work stops being trivial.
- Treat completion language as a checkpoint.
- Use `AGENTS.md` and `CLAUDE.md` as the map; open repo docs only when structural rules matter."#;

pub const LIGHTWEIGHT_OPERATING_PROFILE_DELTA: &str = r#"## Operating Profile

- Act and verify in one thread.
- Completion language is a checkpoint.
- Use `task_tracker` for nontrivial work."#;

pub const SPECIALIZED_OPERATING_PROFILE_DELTA: &str = r#"## Operating Profile

- Explore, plan, then execute.
- Use `task_tracker` for multi-step work and Planning workflow when scope or verification is still open.
- Treat completion language as a checkpoint, not proof; only stop when tracker state, verification, and resumable state agree.
- End plan work with one `<proposed_plan>` block; if a path stalls, re-plan into smaller verified slices.
- Use `AGENTS.md`, `CLAUDE.md`, and `docs/harness/ARCHITECTURAL_INVARIANTS.md` when repo-wide invariants matter."#;

static DEFAULT_SYSTEM_PROMPT: OnceLock<String> = OnceLock::new();
static MINIMAL_SYSTEM_PROMPT: OnceLock<String> = OnceLock::new();
static DEFAULT_LIGHTWEIGHT_PROMPT: OnceLock<String> = OnceLock::new();
static DEFAULT_SPECIALIZED_PROMPT: OnceLock<String> = OnceLock::new();

const STRUCTURED_REASONING_INSTRUCTIONS: &str = r#"
## Structured Reasoning

Use tags when helpful: `<analysis>` facts/options, `<plan>` steps, `<uncertainty>` blockers, `<verification>` checks. When a decision must be consumed by code or tools, prefer JSON or function-call shaped output over prose.
"#;

/// System instruction configuration
#[derive(Debug, Clone, Default)]
pub struct SystemPromptConfig;

/// Generate system instruction
pub async fn generate_system_instruction(_config: &SystemPromptConfig) -> Content {
    let current_dir = env::current_dir();
    let instruction = if let Ok(project_root) = current_dir.as_deref() {
        compose_system_instruction_text(project_root, Some(&crate::config::VTCodeConfig::default()), None).await
    } else {
        let mut prompt = default_system_prompt().to_string();
        prompt.push_str("\n\n");
        prompt.push_str(&render_shell_profile_guidance(ShellPromptProfile::Auto.resolve_for_current_platform()));
        prompt
    };

    if let Ok(current_dir) = current_dir {
        let styled_instruction = apply_output_style(instruction, None, &current_dir).await;
        Content::system_text(styled_instruction)
    } else {
        Content::system_text(instruction)
    }
}

/// Read AGENTS.md file if present and extract agent guidelines
pub async fn read_agent_guidelines(project_root: &Path) -> Option<String> {
    let max_bytes = prompt_budget_constants::DEFAULT_MAX_BYTES;
    match read_project_doc(project_root, max_bytes).await {
        Ok(Some(bundle)) => Some(bundle.contents),
        Ok(None) => None,
        Err(err) => {
            warn!("failed to load project documentation: {err:#}");
            None
        }
    }
}

/// A named layer of the composed system prompt.
///
/// The token-budget trimmer (see [`SectionKind::trim_priority`]) drops whole
/// sections rather than truncating text mid-layer, so each section's text is
/// stored verbatim (including any leading/trailing whitespace baked into its
/// source constant) exactly as it would have been appended by the legacy
/// single-string builder.
struct PromptSection {
    kind: SectionKind,
    text: String,
}

/// Identifies which layer of the system prompt a `PromptSection` belongs to.
///
/// Variants mirror the layers `compose_system_instruction_text` actually
/// assembles today. Agent identity is not a separate variant: it is applied
/// as an in-place text substitution on the base contract (title/intro lines)
/// rather than an appended section, so it is folded into [`Self::BaseContract`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionKind {
    /// Canonical contract + operating profile (with any workspace prompt-layer
    /// override/append and agent-identity substitution already applied).
    /// Always present and never trimmed to satisfy the token budget.
    BaseContract,
    /// Optional `<analysis>/<plan>/<uncertainty>/<verification>` tagging
    /// guidance. Advisory; trimmed first when over budget.
    StructuredReasoning,
    /// Lean "## Skills" routing section rendered from available skill
    /// metadata. Advisory; trimmed alongside structured reasoning.
    Skills,
    /// "## Environment" addenda (languages, interaction mode, MCP sources,
    /// temporal context, working directory).
    EnvironmentAddenda,
    /// "## Active Tools" dynamic tool guidance derived from the active tool
    /// catalog.
    ToolGuidelines,
    /// "## Shell Profile" guidance for the current command environment.
    ShellProfile,
}

impl SectionKind {
    /// Static section name used in [`SystemPromptReport::trimmed_sections`].
    const fn name(self) -> &'static str {
        match self {
            Self::BaseContract => "base_contract",
            Self::StructuredReasoning => "structured_reasoning",
            Self::Skills => "skills",
            Self::EnvironmentAddenda => "environment_addenda",
            Self::ToolGuidelines => "tool_guidelines",
            Self::ShellProfile => "shell_profile",
        }
    }

    /// Trim order: lower values are dropped first. `None` means the section
    /// is never dropped to satisfy the token budget.
    const fn trim_priority(self) -> Option<u8> {
        match self {
            Self::StructuredReasoning => Some(0),
            Self::Skills => Some(1),
            Self::EnvironmentAddenda => Some(2),
            Self::ShellProfile => Some(3),
            Self::ToolGuidelines => Some(4),
            Self::BaseContract => None,
        }
    }
}

/// Result of measuring a composed system prompt against the configured token
/// budget (`agent.max_system_prompt_tokens`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SystemPromptReport {
    /// `estimate_token_count` of the final composed text (after trimming, if
    /// trimming occurred).
    pub token_estimate: u64,
    /// Whether `token_estimate` exceeds `agent.max_system_prompt_tokens`.
    pub over_budget: bool,
    /// Names of sections dropped to satisfy the budget, in drop order. Empty
    /// unless `agent.trim_system_prompt` is enabled and trimming occurred.
    pub trimmed_sections: Vec<&'static str>,
}

impl SystemPromptReport {
    /// Measure `text` against `max_tokens` with no trimming applied.
    ///
    /// Useful when a system prompt was assembled or overridden outside the
    /// normal section-based pipeline (e.g. downstream embedders calling
    /// `AgentRunner::set_system_prompt`, or appendix text appended after
    /// [`compose_system_instruction_with_report`] already measured the
    /// sectioned prompt).
    #[must_use]
    pub fn measure(text: &str, max_tokens: u64) -> Self {
        let token_estimate = estimate_token_count(text);
        Self {
            token_estimate,
            over_budget: token_estimate > max_tokens,
            trimmed_sections: Vec::new(),
        }
    }
}

/// Compose the base system instruction plus compact tool/skill/environment addenda.
pub async fn compose_system_instruction_text(
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    prompt_context: Option<&PromptContext>,
) -> String {
    compose_system_instruction_with_report(project_root, vtcode_config, prompt_context)
        .await
        .0
}

/// Compose the system instruction and return the token-budget report
/// alongside it. See [`SystemPromptReport`] and `SectionKind::trim_priority`
/// for the budget/trim behavior driven by `agent.max_system_prompt_tokens`,
/// `agent.system_prompt_budget_warning`, and `agent.trim_system_prompt`.
pub async fn compose_system_instruction_with_report(
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    prompt_context: Option<&PromptContext>,
) -> (String, SystemPromptReport) {
    let sections = build_prompt_sections(project_root, vtcode_config, prompt_context).await;
    let (max_tokens, warn_enabled, trim_enabled) = system_prompt_budget_settings(vtcode_config);
    apply_token_budget(sections, max_tokens, warn_enabled, trim_enabled)
}

/// Measure the system prompt size without applying budget trimming or warnings.
///
/// This is used at startup to warn about potential token budget overruns
/// before the first request is made. Unlike [`compose_system_instruction_with_report`],
/// this function does not apply `agent.trim_system_prompt` and does not emit
/// budget-exceeded warnings.
pub async fn measure_system_prompt_size(
    project_root: &Path,
    vtcode_config: &crate::config::VTCodeConfig,
) -> SystemPromptReport {
    let sections = build_prompt_sections(project_root, Some(vtcode_config), None).await;
    let text = join_prompt_sections(&sections);
    let token_estimate = estimate_token_count(&text);
    SystemPromptReport {
        token_estimate,
        over_budget: token_estimate > vtcode_config.agent.max_system_prompt_tokens,
        trimmed_sections: Vec::new(),
    }
}

/// Resolve the effective `(max_system_prompt_tokens, budget_warning_enabled,
/// trim_enabled)` settings, falling back to the `AgentConfig` defaults when
/// no config is available.
fn system_prompt_budget_settings(vtcode_config: Option<&crate::config::VTCodeConfig>) -> (u64, bool, bool) {
    vtcode_config.map_or((prompt_budget_constants::DEFAULT_MAX_SYSTEM_PROMPT_TOKENS, true, false), |cfg| {
        (
            cfg.agent.max_system_prompt_tokens,
            cfg.agent.system_prompt_budget_warning,
            cfg.agent.trim_system_prompt,
        )
    })
}

/// Build the ordered prompt sections. Each section's text is stored exactly
/// as the legacy single-string builder would have appended it, so
/// [`join_prompt_sections`] reproduces byte-identical output when nothing is
/// trimmed.
async fn build_prompt_sections(
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    prompt_context: Option<&PromptContext>,
) -> Vec<PromptSection> {
    let prompt_mode = vtcode_config
        .map(|c| c.agent.system_prompt_mode)
        .unwrap_or(SystemPromptMode::Default);
    let static_base_prompt = static_profile_prompt(prompt_mode);
    let resolved_layers = resolve_system_prompt_layers(project_root).await;
    let mut base_prompt = apply_system_prompt_layers(static_base_prompt, &resolved_layers);

    tracing::trace!(
        mode = ?prompt_mode,
        base_tokens_approx = base_prompt.len() / 4, // rough token estimate
        "Selected system prompt mode"
    );

    // Apply agent identity based on the default primary agent configuration.
    // This combines "VT Code" with the active agent mode so the LLM knows its role.
    if let Some(cfg) = vtcode_config {
        let agent_label = agent_identity_label(&cfg.default_primary_agent);
        base_prompt = apply_agent_identity(&base_prompt, &agent_label);
    }

    let mut sections = vec![PromptSection { kind: SectionKind::BaseContract, text: base_prompt }];

    if should_include_structured_reasoning(vtcode_config, prompt_mode) {
        sections.push(PromptSection {
            kind: SectionKind::StructuredReasoning,
            text: STRUCTURED_REASONING_INSTRUCTIONS.to_string(),
        });
    }

    let shell_profile = vtcode_config
        .map(|cfg| cfg.agent.shell_prompt_profile)
        .unwrap_or(ShellPromptProfile::Auto)
        .resolve_for_current_platform();
    sections.push(PromptSection {
        kind: SectionKind::ShellProfile,
        text: render_shell_profile_guidance(shell_profile),
    });

    if let Some(ctx) = prompt_context {
        let guidelines =
            generate_tool_guidelines_for_profile(&ctx.available_tools, ctx.capability_level, shell_profile);
        if !guidelines.is_empty() {
            sections.push(PromptSection {
                kind: SectionKind::ToolGuidelines,
                text: guidelines.trim_start_matches('\n').to_string(),
            });
        }
        if let Some(skills_section) = render_prompt_skills_section(&ctx.available_skill_metadata) {
            sections.push(PromptSection { kind: SectionKind::Skills, text: skills_section });
        }
    }

    if let Some(environment_section) = render_environment_addenda(vtcode_config, prompt_context) {
        sections.push(PromptSection {
            kind: SectionKind::EnvironmentAddenda,
            text: environment_section,
        });
    }

    sections
}

/// Join ordered prompt sections exactly as the legacy single-string builder
/// did: the first section verbatim, then each subsequent section separated
/// by a blank line.
fn join_prompt_sections(sections: &[PromptSection]) -> String {
    let capacity = sections.iter().map(|section| section.text.len() + 2).sum();
    let mut joined = String::with_capacity(capacity);
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            joined.push_str("\n\n");
        }
        joined.push_str(&section.text);
    }
    joined
}

/// Enforce the configured system-prompt token budget against the composed
/// sections.
///
/// When under budget, sections are joined and returned unchanged. When over
/// budget and `trim_enabled` is false, the full untrimmed text is still used
/// but a warning is logged (gated on `warn_enabled`). When over budget and
/// `trim_enabled` is true, whole sections are dropped in
/// [`SectionKind::trim_priority`] order (lowest first), re-measuring after
/// each drop, until the prompt fits or only untrimmable sections remain.
fn apply_token_budget(
    mut sections: Vec<PromptSection>,
    max_tokens: u64,
    warn_enabled: bool,
    trim_enabled: bool,
) -> (String, SystemPromptReport) {
    let mut text = join_prompt_sections(&sections);
    let mut token_estimate = estimate_token_count(&text);
    let mut trimmed_sections: Vec<&'static str> = Vec::new();

    if token_estimate > max_tokens {
        if trim_enabled {
            while token_estimate > max_tokens {
                let drop_index = sections
                    .iter()
                    .enumerate()
                    .filter_map(|(index, section)| section.kind.trim_priority().map(|priority| (priority, index)))
                    .min_by_key(|(priority, _)| *priority)
                    .map(|(_, index)| index);
                let Some(drop_index) = drop_index else {
                    break;
                };
                let dropped = sections.remove(drop_index);
                trimmed_sections.push(dropped.kind.name());
                text = join_prompt_sections(&sections);
                token_estimate = estimate_token_count(&text);
            }

            if !trimmed_sections.is_empty() {
                tracing::warn!(
                    token_estimate,
                    max_system_prompt_tokens = max_tokens,
                    dropped_sections = ?trimmed_sections,
                    "Trimmed system prompt sections to satisfy token budget"
                );
            }
        } else if warn_enabled {
            tracing::warn!(
                token_estimate,
                max_system_prompt_tokens = max_tokens,
                "System prompt exceeds configured token budget"
            );
        }
    }

    let report = SystemPromptReport {
        token_estimate,
        over_budget: token_estimate > max_tokens,
        trimmed_sections,
    };
    (text, report)
}

/// Apply agent identity to the system prompt by replacing the title and intro lines.
/// This combines the "VT Code" identity with the active agent mode so the LLM
/// knows its role (e.g., "VT Code (Build mode)" or "VT Code (Auto mode)").
fn apply_agent_identity(prompt: &str, agent_label: &str) -> String {
    let mut result = prompt.to_string();

    // Replace the title line: "# VT Code" -> "# {agent_label}"
    let old_title = PROMPT_TITLE;
    if let Some(pos) = result.find(old_title) {
        result.replace_range(pos..pos + old_title.len(), &format!("# {agent_label}"));
    }

    // Replace the intro line: "VT Code. Be concise and safe." -> "{agent_label}. Be concise and safe."
    let old_intro = PROMPT_INTRO;
    if let Some(pos) = result.find(old_intro) {
        result.replace_range(pos..pos + old_intro.len(), &format!("{agent_label}. Be concise and safe."));
    }

    result
}

fn should_include_structured_reasoning(
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    mode: SystemPromptMode,
) -> bool {
    if let Some(cfg) = vtcode_config {
        return cfg.agent.should_include_structured_reasoning_tags();
    }

    // Backward-compatible fallback when no config is available.
    matches!(mode, SystemPromptMode::Specialized)
}

/// Generate the stable base system instruction with configuration-aware sections.
///
/// Note: This function maintains backward compatibility by not accepting prompt_context.
/// For enhanced prompts with dynamic guidelines, call `compose_system_instruction_text` directly.
pub async fn generate_system_instruction_with_config(
    config: &SystemPromptConfig,
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
) -> Content {
    let (content, _report) =
        generate_system_instruction_with_config_and_report(config, project_root, vtcode_config).await;
    content
}

/// Same as [`generate_system_instruction_with_config`] but also returns the
/// [`SystemPromptReport`] for the composed prompt, whether served from cache
/// or freshly built.
pub async fn generate_system_instruction_with_config_and_report(
    _config: &SystemPromptConfig,
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
) -> (Content, SystemPromptReport) {
    let cache_key = cache_key(project_root, vtcode_config, None);
    let (instruction, report) = match PROMPT_CACHE.get(&cache_key) {
        Some(cached) => cached,
        None => {
            let built = compose_system_instruction_with_report(project_root, vtcode_config, None).await;
            PROMPT_CACHE.insert(cache_key, built.clone());
            built
        }
    };

    // Apply output style if configured
    let styled_instruction = apply_output_style(instruction, vtcode_config, project_root).await;
    (Content::system_text(styled_instruction), report)
}

/// Generate the stable base system instruction without workspace configuration.
pub async fn generate_system_instruction_with_guidelines(config: &SystemPromptConfig, project_root: &Path) -> Content {
    let (content, _report) = generate_system_instruction_with_guidelines_and_report(config, project_root).await;
    content
}

/// Same as [`generate_system_instruction_with_guidelines`] but also returns
/// the [`SystemPromptReport`] for the composed prompt.
pub async fn generate_system_instruction_with_guidelines_and_report(
    _config: &SystemPromptConfig,
    project_root: &Path,
) -> (Content, SystemPromptReport) {
    let cache_key = cache_key(project_root, None, None);
    let (instruction, report) = match PROMPT_CACHE.get(&cache_key) {
        Some(cached) => cached,
        None => {
            let built = compose_system_instruction_with_report(project_root, None, None).await;
            PROMPT_CACHE.insert(cache_key, built.clone());
            built
        }
    };
    // Apply output style if configured
    let styled_instruction = apply_output_style(instruction, None, project_root).await;
    (Content::system_text(styled_instruction), report)
}

/// Apply output style to a generated system instruction
pub async fn apply_output_style(
    instruction: String,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    project_root: &Path,
) -> String {
    if let Some(config) = vtcode_config {
        let output_style_applier = OutputStyleApplier::new();
        if let Err(e) = output_style_applier.load_styles_from_config(config, project_root).await {
            tracing::warn!("Failed to load output styles: {}", e);
            instruction // Return original if loading fails
        } else {
            output_style_applier
                .apply_style(&config.output_style.active_style, &instruction, config)
                .await
        }
    } else {
        instruction // Return original if no config
    }
}

/// Build a cache key for the system prompt.
///
/// `catalog_epoch` is the tool-catalog version at the time of the request. When
/// the tool set changes (e.g. planning workflow is toggled, MCP tools are refreshed), the
/// epoch advances and the old cached prompt is superseded rather than served stale.
/// Pass `None` to get the same behaviour as before epoch tracking was introduced.
fn cache_key(
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    catalog_epoch: Option<u64>,
) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut hasher = DefaultHasher::new();

    project_root.hash(&mut hasher);

    if let Some(cfg) = vtcode_config {
        cfg.agent.include_working_directory.hash(&mut hasher);
        cfg.agent.include_temporal_context.hash(&mut hasher);
        cfg.prompt_cache.cache_friendly_prompt_shaping.hash(&mut hasher);
        cfg.agent.include_structured_reasoning_tags.hash(&mut hasher);
        std::mem::discriminant(&cfg.agent.system_prompt_mode).hash(&mut hasher);
        std::mem::discriminant(&cfg.agent.tool_documentation_mode).hash(&mut hasher);
        cfg.agent.max_system_prompt_tokens.hash(&mut hasher);
        cfg.agent.system_prompt_budget_warning.hash(&mut hasher);
        cfg.agent.trim_system_prompt.hash(&mut hasher);
        cfg.default_primary_agent.hash(&mut hasher);

        if cfg.agent.include_temporal_context && !cfg.prompt_cache.cache_friendly_prompt_shaping {
            let epoch_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() / 60)
                .unwrap_or(0);
            epoch_secs.hash(&mut hasher);
        }
    } else {
        "default".hash(&mut hasher);
    }

    catalog_epoch.unwrap_or(0).hash(&mut hasher);

    format!("sys_prompt:{:016x}", hasher.finish())
}

/// Generate a minimal system instruction (pi-inspired, <1K tokens)
pub fn generate_minimal_instruction() -> Content {
    Content::system_text(minimal_instruction_text())
}

/// Generate a lightweight system instruction for simple operations
pub fn generate_lightweight_instruction() -> Content {
    Content::system_text(lightweight_instruction_text())
}

/// Generate a specialized system instruction for advanced operations
pub fn generate_specialized_instruction() -> Content {
    Content::system_text(specialized_instruction_text())
}

// ─── Token Estimation ────────────────────────────────────────────────────────

/// Fast character-based token count estimation.
///
/// Uses the heuristic `tokens ~= chars / 4` which is accurate within ~20%
/// for English text with code. This is intentionally approximate — the goal
/// is monitoring and budget enforcement, not precise accounting.
#[must_use]
pub fn estimate_token_count(text: &str) -> u64 {
    // Round up to avoid underestimation
    text.len().div_ceil(4) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::VTCodeConfig;
    use crate::config::constants::tools;
    use crate::config::types::ResolvedShellPromptProfile;
    use std::path::PathBuf;

    const REMOVED_MODEL_FACING_TOOL_NAMES: &[&str] = &[
        "command_session",
        "file_operation",
        "search_dispatch",
        "list_files",
        "read_file",
        "write_file",
        "edit_file",
        "grep_file",
    ];

    fn assert_no_removed_model_facing_tool_names(prompt: &str) {
        for tool_name in REMOVED_MODEL_FACING_TOOL_NAMES {
            assert!(!prompt.contains(tool_name), "prompt should not mention removed tool name {tool_name}");
        }
    }

    #[tokio::test]
    async fn test_minimal_mode_selection() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Minimal;
        // Disable enhancements for base prompt size testing
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        // Minimal prompt should remain compact and deterministic without AGENTS.md injection
        assert!(result.len() < 2800, "Minimal mode should produce <2.8K chars (was {} chars)", result.len());
        assert!(result.contains("VT Code") || result.contains("VT Code"), "Should contain VT Code identifier");
    }

    #[tokio::test]
    async fn test_default_prompt_selection() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Default;
        // Disable enhancements for base prompt size testing
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(result.len() <= 2800, "Default mode should stay sparse (<=2.8K chars, was {} chars)", result.len());
        assert!(result.contains("`exec_command`, `write_stdin`, and `apply_patch`"));
        assert!(result.contains("## Shell Profile"));
        assert!(!result.contains("task_tracker"));
        assert!(!result.contains("@file"));
        assert!(result.contains("Planning workflow"));
    }

    #[tokio::test]
    async fn test_lightweight_mode_selection() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Lightweight;
        // Disable enhancements for base prompt size testing
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(result.len() > 100, "Lightweight should be >100 chars");
        assert!(result.len() < 2200, "Lightweight should be compact (<2.2K chars, was {} chars)", result.len());
        assert!(result.contains("task_tracker"));
        assert!(!result.contains("@file"));
        assert!(result.contains("Act and verify in one thread"));
    }

    #[tokio::test]
    async fn test_lightweight_mode_skips_structured_reasoning_by_default() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Lightweight;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;
        config.agent.include_structured_reasoning_tags = None;

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(
            !result.contains("## Structured Reasoning"),
            "Lightweight mode should omit structured reasoning by default"
        );
    }

    #[tokio::test]
    async fn test_lightweight_mode_allows_explicit_structured_reasoning() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Lightweight;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;
        config.agent.include_structured_reasoning_tags = Some(true);

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(
            result.contains("## Structured Reasoning"),
            "Lightweight mode should include structured reasoning when explicitly enabled"
        );
    }

    #[tokio::test]
    async fn test_default_prompt_omits_structured_reasoning_by_default() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Default;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;
        config.agent.include_structured_reasoning_tags = None;

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(
            !result.contains("## Structured Reasoning"),
            "Default mode should omit structured reasoning by default"
        );
    }

    #[tokio::test]
    async fn test_specialized_mode_selection() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Specialized;
        // Disable enhancements for base prompt size testing
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(result.len() <= 2900, "Specialized should stay sparse (<=2.9K chars, was {} chars)", result.len());
        assert!(result.contains("task_tracker"));
        assert!(result.contains("<proposed_plan>"));
        assert!(result.contains("ARCHITECTURAL_INVARIANTS"));
    }

    #[test]
    fn test_prompt_mode_enum_parsing() {
        assert_eq!(SystemPromptMode::parse("minimal"), Some(SystemPromptMode::Minimal));
        assert_eq!(SystemPromptMode::parse("LIGHTWEIGHT"), Some(SystemPromptMode::Lightweight));
        assert_eq!(SystemPromptMode::parse("Default"), Some(SystemPromptMode::Default));
        assert_eq!(SystemPromptMode::parse("specialized"), Some(SystemPromptMode::Specialized));
        assert_eq!(SystemPromptMode::parse("invalid"), None);
    }

    /// Regression guard: `PLANNING_WORKFLOW_PLAN_QUALITY_LINE` must keep
    /// instructing the model to write file:symbol references as plain text
    /// / inline code, not as markdown links or editor/IDE URI schemes (a
    /// model was observed emitting `vscode-file://` pseudo-links pointing at
    /// the editor binary instead of the referenced repo file).
    #[test]
    fn plan_quality_line_forbids_markdown_link_file_references() {
        let line = PLANNING_WORKFLOW_PLAN_QUALITY_LINE;
        assert!(line.contains("never as markdown links or editor/IDE URIs"));
        assert!(line.contains("vscode-file://"));
        assert!(line.contains("plain text or inline code"));
    }

    #[test]
    fn test_minimal_prompt_token_count() {
        // Rough estimate: 1 token ≈ 4 characters
        let approx_tokens = minimal_system_prompt().len() / 4;
        assert!(approx_tokens < 300, "Minimal prompt should stay compact, got ~{approx_tokens}");
    }

    #[test]
    fn test_default_prompt_token_count() {
        let approx_tokens = default_system_prompt().len() / 4;
        assert!(approx_tokens < 550, "Default prompt should stay compact, got ~{approx_tokens}");
    }

    #[tokio::test]
    async fn test_default_live_prompt_budget_with_instruction_summary() {
        use crate::project_doc::build_instruction_appendix_with_context;

        let workspace = tempfile::TempDir::new().expect("workspace");
        std::fs::write(workspace.path().join(".git"), "gitdir: /tmp/git").expect("git marker");
        std::fs::write(
            workspace.path().join("AGENTS.md"),
            "- run ./scripts/check.sh\n- avoid adding to vtcode-core\n- use Conventional Commits\n- start with docs/ARCHITECTURE.md\n",
        )
        .expect("write agents");
        std::fs::create_dir_all(workspace.path().join(".vtcode/rules")).expect("rules dir");
        std::fs::write(
            workspace.path().join(".vtcode/rules/rust.md"),
            "---\npaths:\n  - \"**/*.rs\"\n---\n# Rust\n- keep changes surgical\n",
        )
        .expect("write rust rule");

        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        let base = compose_system_instruction_text(workspace.path(), Some(&config), None).await;
        let appendix = build_instruction_appendix_with_context(
            &config.agent,
            workspace.path(),
            &[workspace.path().join("src/lib.rs")],
        )
        .await
        .expect("instruction appendix");
        let prompt = format!("{base}\n\n# INSTRUCTIONS\n{appendix}");
        let approx_tokens = prompt.len() / 4;

        assert!(prompt.contains("### Instruction map"));
        assert!(prompt.contains("### On-demand loading"));
        assert!(approx_tokens <= 1250, "got ~{approx_tokens} tokens");
    }

    #[tokio::test]
    async fn test_generated_prompts_do_not_use_deprecated_update_plan() {
        let project_root = PathBuf::from(".");

        for (mode_name, mode) in [
            ("default", SystemPromptMode::Default),
            ("minimal", SystemPromptMode::Minimal),
            ("specialized", SystemPromptMode::Specialized),
        ] {
            let mut config = VTCodeConfig::default();
            config.agent.system_prompt_mode = mode;
            config.agent.include_temporal_context = false;
            config.agent.include_working_directory = false;
            config.agent.instruction_max_bytes = 0;

            let result = compose_system_instruction_text(&project_root, Some(&config), None).await;

            assert!(!result.contains("update_plan"), "{mode_name} prompt should not reference deprecated update_plan");
        }
    }

    #[tokio::test]
    async fn test_default_prompt_omits_non_baseline_tools() {
        let project_root = PathBuf::from(".");
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Default;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;

        let result = compose_system_instruction_text(&project_root, Some(&config), None).await;

        assert!(result.contains("`exec_command`, `write_stdin`, and `apply_patch`"));
        assert!(result.contains("exec_command.cmd"));
        assert!(result.contains("## Shell Profile"));
        assert!(!result.contains("task_tracker"));
        assert!(!result.contains("list_files"));
        assert!(!result.contains("read_file"));
    }

    #[tokio::test]
    async fn test_default_and_specialized_prompts_drop_rigid_summary_template() {
        let project_root = PathBuf::from(".");

        for (mode_name, mode) in [
            ("default", SystemPromptMode::Default),
            ("specialized", SystemPromptMode::Specialized),
        ] {
            let mut config = VTCodeConfig::default();
            config.agent.system_prompt_mode = mode;
            config.agent.include_temporal_context = false;
            config.agent.include_working_directory = false;
            config.agent.instruction_max_bytes = 0;

            let result = compose_system_instruction_text(&project_root, Some(&config), None).await;

            assert!(!result.contains("References\n"), "{mode_name} prompt should not force a References section");
            assert!(!result.contains("Next action"), "{mode_name} prompt should not force a Next action section");
            assert!(
                !result.contains("Scope checkpoint"),
                "{mode_name} prompt should not require the old plan blueprint bullets"
            );
        }
    }

    #[tokio::test]
    async fn test_generated_prompts_keep_sparse_execution_contract() {
        let project_root = PathBuf::from(".");

        for (mode_name, mode) in [
            ("default", SystemPromptMode::Default),
            ("minimal", SystemPromptMode::Minimal),
            ("lightweight", SystemPromptMode::Lightweight),
            ("specialized", SystemPromptMode::Specialized),
        ] {
            let mut config = VTCodeConfig::default();
            config.agent.system_prompt_mode = mode;
            config.agent.include_temporal_context = false;
            config.agent.include_working_directory = false;
            config.agent.instruction_max_bytes = 0;

            let result = compose_system_instruction_text(&project_root, Some(&config), None).await;
            let normalized = result.to_ascii_lowercase();

            assert!(
                normalized.contains("compact") || normalized.contains("concise"),
                "{mode_name} prompt should keep output guidance compact"
            );
            assert!(
                normalized.contains("low-risk") || normalized.contains("reversible"),
                "{mode_name} prompt should include follow-through guidance"
            );
            assert!(
                normalized.contains("verify") || normalized.contains("validation"),
                "{mode_name} prompt should include verification guidance"
            );
            assert!(normalized.contains("do not guess"), "{mode_name} prompt should gate missing context");
            assert!(
                normalized.contains("unblocked portion")
                    || normalized.contains("unblocked slices")
                    || normalized.contains("answerable without a missing detail"),
                "{mode_name} prompt should require partial progress before clarification"
            );
            assert!(
                normalized.contains("retrieved sources") || normalized.contains("retrieved evidence"),
                "{mode_name} prompt should include grounding/citation guidance"
            );
            assert!(!result.contains('ƒ'), "{mode_name} prompt should not contain stray prompt characters");
        }
    }

    #[test]
    fn test_prompt_text_avoids_hardcoded_loop_thresholds() {
        let specialized_prompt = specialized_instruction_text();
        assert!(!default_system_prompt().contains("stuck twice"));
        assert!(!minimal_system_prompt().contains("stuck twice"));
        assert!(!specialized_prompt.contains("stuck twice"));
        assert!(!specialized_prompt.contains("10+ calls without progress"));
        assert!(!specialized_prompt.contains("Same tool+params twice"));
    }

    #[test]
    fn test_harness_awareness_in_prompts() {
        assert!(default_system_prompt().contains("AGENTS.md"), "Default prompt should reference AGENTS.md as map");
        assert!(
            specialized_instruction_text().contains("ARCHITECTURAL_INVARIANTS"),
            "Specialized prompt should reference architectural invariants"
        );
        assert!(minimal_system_prompt().contains("AGENTS.md"), "Minimal prompt should still reference AGENTS.md");
    }

    #[test]
    fn test_prompts_reject_guessing_when_context_is_missing() {
        assert!(default_system_prompt().contains("do not guess"), "Default prompt should reject guessing");
        assert!(specialized_instruction_text().contains("do not guess"), "Specialized prompt should reject guessing");
        assert!(minimal_system_prompt().contains("do not guess"), "Minimal prompt should still reject guessing");
    }

    #[test]
    fn test_prompts_include_compaction_preservation_contract() {
        assert!(
            default_system_prompt().contains("touched files"),
            "Default prompt should preserve touched files across compaction"
        );
        assert!(
            default_system_prompt().contains("decisions across compaction"),
            "Default prompt should preserve decision rationale across compaction"
        );
        assert!(
            default_system_prompt().contains("tracker state"),
            "Default prompt should preserve tracker state across compaction"
        );
        assert!(
            default_system_prompt().contains("verification status"),
            "Default prompt should preserve verification status across compaction"
        );
        assert!(
            minimal_system_prompt().contains("touched files"),
            "Minimal prompt should preserve touched files across compaction"
        );
    }

    #[test]
    fn test_default_prompt_stays_lean_but_complete() {
        let prompt = default_system_prompt();

        assert!(prompt.contains("## Contract"), "Default prompt should include the lean contract section");
        assert!(prompt.contains("Keep outputs concise"), "Default prompt should clamp output shape");
        assert!(
            prompt.contains("Verify changes yourself"),
            "Default prompt should require verification before finalizing"
        );
        assert!(
            prompt.contains("Keep user updates brief and high-signal"),
            "Default prompt should constrain progress updates"
        );
    }

    #[test]
    fn test_default_prompt_omits_removed_model_facing_tool_names() {
        let prompt = default_system_prompt();

        assert_no_removed_model_facing_tool_names(prompt);
        assert!(prompt.contains("exec_command"), "Default prompt should keep baseline shell guidance");
    }

    #[tokio::test]
    async fn test_composed_default_prompt_omits_removed_model_facing_tool_names() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Default;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;

        let prompt = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert_no_removed_model_facing_tool_names(&prompt);
        assert!(prompt.contains("exec_command"), "Composed default prompt should keep baseline shell guidance");
        assert!(prompt.contains("## Shell Profile"));
        assert!(prompt.contains("controls prompt examples and expected command syntax only"));
    }

    #[tokio::test]
    async fn test_composed_prompts_render_explicit_shell_profiles() {
        let project_root = PathBuf::from(".");
        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;

        config.agent.shell_prompt_profile = ShellPromptProfile::UnixLike;
        let unix_prompt = compose_system_instruction_text(&project_root, Some(&config), None).await;
        assert!(unix_prompt.contains("Active shell profile: `unix_like`"));
        assert!(unix_prompt.contains("does not rewrite GNU flags for macOS BSD tools"));
        assert!(unix_prompt.contains("does not translate GNU-to-BSD"));

        config.agent.shell_prompt_profile = ShellPromptProfile::PowerShell;
        let powershell_prompt = compose_system_instruction_text(&project_root, Some(&config), None).await;
        assert!(powershell_prompt.contains("Active shell profile: `powershell`"));
        assert!(powershell_prompt.contains("`Get-ChildItem`"));
        assert!(powershell_prompt.contains("use WSL"));
        assert!(powershell_prompt.contains("Unix-to-PowerShell"));
        assert!(!powershell_prompt.contains("`ls`, `rg`, `find`, `cat`, `sed`, and `awk`"));
    }

    #[test]
    fn test_planning_notice_omits_removed_model_facing_tool_names() {
        assert_no_removed_model_facing_tool_names(PLANNING_WORKFLOW_READ_ONLY_NOTICE_LINE);
        assert!(PLANNING_WORKFLOW_READ_ONLY_NOTICE_LINE.contains("exec_command"));
        assert!(PLANNING_WORKFLOW_READ_ONLY_NOTICE_LINE.contains("apply_patch"));
    }

    #[test]
    fn test_all_prompt_modes_treat_completion_as_checkpoint_not_proof() {
        for (mode_name, prompt) in [
            ("default", default_system_prompt()),
            ("minimal", minimal_system_prompt()),
            ("lightweight", default_lightweight_prompt()),
            ("specialized", specialized_instruction_text().as_str()),
        ] {
            assert!(
                prompt.contains("completion language as a checkpoint")
                    || prompt.contains("Verify changes yourself")
                    || prompt.contains("verification"),
                "{mode_name} prompt should include verification guidance"
            );
        }
    }

    #[test]
    fn test_prompts_encode_explicit_delegation_contract() {
        let prompt = default_system_prompt();

        assert!(
            prompt.contains("Keep control on the main thread"),
            "Default prompt should keep control on the main thread"
        );
        assert!(
            prompt.contains("Delegate bounded, independent work"),
            "Default prompt should restrict delegation to bounded independent work"
        );
        assert!(
            minimal_system_prompt().contains("Keep delegation and skills bounded, explicit, and narrow"),
            "Minimal prompt should preserve the delegation contract"
        );
    }

    #[test]
    fn test_default_prompt_includes_grounding_and_action_bias() {
        let prompt = default_system_prompt();
        assert!(
            prompt.contains("Never speculate about code you have not opened"),
            "Default prompt should include grounding guidance"
        );
        assert!(
            prompt.contains("Make only requested changes"),
            "Default prompt should include anti-overengineering guidance"
        );
        assert!(
            prompt.contains("use tools to implement directly"),
            "Default prompt should include action bias for tool-using agents"
        );
    }

    #[test]
    fn test_default_prompt_omits_accuracy_addendum() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let config = VTCodeConfig::default();
        let prompt = runtime.block_on(compose_system_instruction_text(&PathBuf::from("."), Some(&config), None));

        assert!(
            !prompt.contains("## Accuracy Optimization"),
            "Runtime prompt should omit the accuracy optimization section"
        );
        assert!(prompt.contains("do not guess"), "Prompt should still preserve the uncertainty guardrail");
    }

    #[tokio::test]
    async fn test_generated_prompts_keep_operating_profiles_bounded() {
        let project_root = PathBuf::from(".");

        for (mode_name, mode) in [
            ("default", SystemPromptMode::Default),
            ("minimal", SystemPromptMode::Minimal),
            ("lightweight", SystemPromptMode::Lightweight),
            ("specialized", SystemPromptMode::Specialized),
        ] {
            let mut config = VTCodeConfig::default();
            config.agent.system_prompt_mode = mode;
            config.agent.include_temporal_context = false;
            config.agent.include_working_directory = false;
            config.agent.instruction_max_bytes = 0;

            let result = compose_system_instruction_text(&project_root, Some(&config), None).await;

            assert!(result.contains("## Contract"), "{mode_name} prompt should reuse the canonical base prompt");
            assert!(
                result.matches("## Operating Profile").count() == 1,
                "{mode_name} prompt should add only one operating profile"
            );
        }
    }

    #[test]
    fn test_search_guidance_prefers_structural_and_rg() {
        let guidelines = generate_tool_guidelines_for_profile(
            &[tools::EXEC_COMMAND.to_string()],
            None,
            ResolvedShellPromptProfile::UnixLike,
        );
        assert!(
            guidelines.contains("`exec_command.cmd` with `ls`, `rg`"),
            "Tool guidance should browse through shell commands"
        );
        assert!(guidelines.contains("git diff -- <path>"), "Tool guidance should keep diff guidance explicit");
    }

    // ENHANCEMENT TESTS

    #[tokio::test]
    async fn test_dynamic_guidelines_read_only() {
        use crate::config::types::CapabilityLevel;

        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Default;

        let ctx = PromptContext {
            capability_level: Some(CapabilityLevel::FileReading),
            ..PromptContext::default()
        };

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        assert!(
            result.contains("Capabilities: read-only"),
            "Should detect read-only capabilities when no edit/write/exec tools available"
        );
        assert!(result.contains("do not modify files"), "Should explain read-only constraints");
    }

    #[tokio::test]
    async fn test_dynamic_guidelines_tool_preferences() {
        let config = VTCodeConfig::default();

        let mut ctx = PromptContext::default();
        ctx.add_tool(tools::EXEC_COMMAND.to_string());
        ctx.add_tool(tools::WRITE_STDIN.to_string());
        ctx.add_tool(tools::APPLY_PATCH.to_string());

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        assert!(
            result.contains("exec_command") && result.contains("apply_patch"),
            "Should suggest baseline shell and patch tools"
        );
        assert_no_removed_model_facing_tool_names(&result);
    }

    #[tokio::test]
    async fn test_live_prompt_renders_workspace_language_hints() {
        let workspace = tempfile::TempDir::new().expect("workspace tempdir");
        std::fs::create_dir_all(workspace.path().join("src")).expect("create src");
        std::fs::create_dir_all(workspace.path().join("web")).expect("create web");
        std::fs::write(workspace.path().join("src/lib.rs"), "fn alpha() {}\n").expect("write rust");
        std::fs::write(workspace.path().join("web/app.ts"), "const app = 1;\n").expect("write ts");

        let config = VTCodeConfig::default();
        let ctx = PromptContext::from_workspace_tools(workspace.path(), [tools::EXEC_COMMAND]);
        let result = compose_system_instruction_text(workspace.path(), Some(&config), Some(&ctx)).await;

        assert!(result.contains("## Environment"));
        assert!(result.contains("Rust, TypeScript"));
        assert!(result.contains("structural-search `lang`"));
    }

    #[tokio::test]
    async fn test_live_prompt_omits_workspace_language_hints_without_languages() {
        let workspace = tempfile::TempDir::new().expect("workspace tempdir");
        let config = VTCodeConfig::default();
        let ctx = PromptContext::from_workspace_tools(workspace.path(), [tools::EXEC_COMMAND]);
        let result = compose_system_instruction_text(workspace.path(), Some(&config), Some(&ctx)).await;

        assert!(!result.contains("Languages:"));
    }

    #[tokio::test]
    async fn test_live_prompt_omits_project_docs_and_user_instructions_from_base_prompt() {
        let workspace = tempfile::TempDir::new().expect("workspace tempdir");
        std::fs::write(workspace.path().join("AGENTS.md"), "- Root summary\n\nFollow the root guidance.\n")
            .expect("write agents");

        let mut config = VTCodeConfig::default();
        config.agent.user_instructions = Some("keep responses terse".to_string());
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 4096;

        let result = compose_system_instruction_text(workspace.path(), Some(&config), None).await;

        assert!(!result.contains("## AGENTS.MD INSTRUCTION HIERARCHY"));
        assert!(!result.contains("### Instruction map"));
        assert!(!result.contains("### Key points"));
        assert!(!result.contains("keep responses terse"));
        assert!(!result.contains("Root summary"));
        assert!(!result.contains("Follow the root guidance."));
    }

    #[tokio::test]
    async fn test_workspace_prompt_resources_override_base_and_keep_dynamic_sections() {
        use crate::skills::model::{SkillMetadata, SkillScope};

        let workspace = tempfile::TempDir::new().expect("workspace tempdir");
        let prompts_dir = workspace.path().join(".vtcode/prompts");
        std::fs::create_dir_all(&prompts_dir).expect("create prompts dir");
        std::fs::write(prompts_dir.join("system.md"), "# Workspace system base").expect("system");
        std::fs::write(prompts_dir.join("append-system.md"), "Workspace prompt appendix").expect("append");

        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = true;

        let mut ctx = PromptContext::default();
        ctx.add_tool(tools::EXEC_COMMAND.to_string());
        ctx.add_skill_metadata(SkillMetadata {
            name: "skill-creator".to_string(),
            description: "Create skills".to_string(),
            short_description: None,
            path: PathBuf::from("/tmp/skill-creator/SKILL.md"),
            scope: SkillScope::System,
            manifest: None,
        });
        ctx.set_current_directory(workspace.path().to_path_buf());

        let result = compose_system_instruction_text(workspace.path(), Some(&config), Some(&ctx)).await;

        assert!(result.starts_with("# Workspace system base"));
        assert!(result.contains("Workspace prompt appendix"));
        assert!(result.contains("## Active Tools"));
        assert!(result.contains("## Skills"));
        assert!(result.contains("## Environment"));

        let appendix_pos = result.find("Workspace prompt appendix").expect("append text");
        let tools_pos = result.find("## Active Tools").expect("tools section");
        let skills_pos = result.find("## Skills").expect("skills section");
        let env_pos = result.find("## Environment").expect("environment section");

        assert!(appendix_pos < tools_pos);
        assert!(tools_pos < skills_pos);
        assert!(skills_pos < env_pos);
    }

    #[tokio::test]
    async fn test_temporal_context_inclusion() {
        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = true;
        config.prompt_cache.cache_friendly_prompt_shaping = false;
        config.agent.temporal_context_use_utc = false; // Local time

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(result.contains("Time:"), "Should include temporal context when enabled");
        let env_pos = result.find("## Environment");
        let temporal_pos = result.find("Time:");
        if let (Some(t), Some(e)) = (temporal_pos, env_pos) {
            assert!(t > e, "Temporal context should appear inside the environment section");
        }
    }

    #[tokio::test]
    async fn test_temporal_context_utc_format() {
        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = true;
        config.prompt_cache.cache_friendly_prompt_shaping = false;
        config.agent.temporal_context_use_utc = true; // UTC format

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(result.contains("UTC"), "Should indicate UTC when temporal_context_use_utc is true");
        assert!(result.contains("T") && result.contains("Z"), "Should use RFC3339 format for UTC (contains T and Z)");
    }

    #[tokio::test]
    async fn test_temporal_context_disabled() {
        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = false;

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(!result.contains("Time:"), "Should not include temporal context when disabled");
    }

    #[tokio::test]
    async fn test_cache_friendly_temporal_context_stays_out_of_base_prompt() {
        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = true;
        config.prompt_cache.cache_friendly_prompt_shaping = true;

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(
            !result.contains("Time:"),
            "Stable system prompt should omit temporal context when cache-friendly shaping is enabled"
        );
    }

    #[tokio::test]
    async fn test_configuration_awareness_stays_behavior_focused() {
        let mut config = VTCodeConfig::default();
        config.security.human_in_the_loop = true;
        config.chat.ask_questions.enabled = false;
        config.mcp.enabled = true;
        config.ide_context.enabled = true;
        config.ide_context.inject_into_prompt = true;

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(result.contains("## Environment"));
        assert!(result.contains("Interaction: approval may gate sensitive actions"));
        assert!(result.contains("request_user_input"));
        assert!(result.contains("Sources: prefer MCP"));
        assert!(!result.contains("PTY functionality"));
        assert!(!result.contains("Loop guards"));
        assert!(!result.contains(".vtcode/context/tool_outputs/"));
        assert!(!result.contains("IDE context:"));
    }

    #[tokio::test]
    async fn test_configuration_awareness_mentions_reduced_approval_when_disabled() {
        let mut config = VTCodeConfig::default();
        config.security.human_in_the_loop = false;

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(result.contains("Interaction: approval reduced by config"));
    }

    #[tokio::test]
    async fn test_default_environment_omits_default_interaction_guidance() {
        let config = VTCodeConfig::default();

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(!result.contains("Interaction:"), "Default-on interaction guidance should stay out of the prompt");
    }

    #[tokio::test]
    async fn test_working_directory_inclusion() {
        let mut config = VTCodeConfig::default();
        config.agent.include_working_directory = true;

        let mut ctx = PromptContext::default();
        ctx.set_current_directory(PathBuf::from("/tmp/test"));

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        assert!(result.contains("Working directory"), "Should include working directory label");
        assert!(result.contains("/tmp/test"), "Should show actual directory path");
        let wd_pos = result.find("Working directory");
        let env_pos = result.find("## Environment");
        if let (Some(w), Some(e)) = (wd_pos, env_pos) {
            assert!(w > e, "Working directory should appear inside the environment section");
        }
    }

    #[tokio::test]
    async fn test_working_directory_disabled() {
        let mut config = VTCodeConfig::default();
        config.agent.include_working_directory = false;

        let mut ctx = PromptContext::default();
        ctx.set_current_directory(PathBuf::from("/tmp/test"));

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        assert!(!result.contains("Working directory"), "Should not include working directory when disabled");
    }

    #[tokio::test]
    async fn test_backward_compatibility() {
        let config = VTCodeConfig::default();

        // Old signature: no prompt context
        let result = compose_system_instruction_text(
            &PathBuf::from("."),
            Some(&config),
            None, // No context - backward compatible
        )
        .await;

        // Should still work without new features
        assert!(result.len() > 600, "Should generate substantial prompt");
        assert!(result.contains("VT Code"), "Should contain base prompt content");
        // Should not have dynamic guidelines without context
        assert!(!result.contains("## Active Tools"), "Should not have tool guidelines without prompt context");
    }

    #[tokio::test]
    async fn test_all_enhancements_combined() {
        use crate::skills::model::{SkillMetadata, SkillScope};

        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = true;
        config.agent.include_working_directory = true;
        config.prompt_cache.cache_friendly_prompt_shaping = false;

        let mut ctx = PromptContext::default();
        ctx.add_tool(tools::APPLY_PATCH.to_string());
        ctx.add_tool(tools::EXEC_COMMAND.to_string());
        ctx.infer_capability_level();
        ctx.set_current_directory(PathBuf::from("/workspace"));
        ctx.add_skill_metadata(SkillMetadata {
            name: "rust-skills".to_string(),
            description: "Rust coding guidance".to_string(),
            short_description: None,
            path: PathBuf::from("/tmp/rust-skills/SKILL.md"),
            scope: SkillScope::System,
            manifest: None,
        });

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        // Verify all enhancements present
        assert!(result.contains("## Active Tools"), "Should have dynamic guidelines");
        assert!(result.contains("## Skills"), "Should have lean skills routing");
        assert!(result.contains("## Environment"), "Should have environment addenda");
        assert!(result.contains("Time:"), "Should have temporal context");
        assert!(result.contains("Working directory"), "Should have working directory");
        assert!(result.contains("/workspace"), "Should show workspace path");

        // Verify specific guideline for this tool set
        assert!(result.contains("after inspection"), "Should have read-before-edit guideline");
        assert_no_removed_model_facing_tool_names(&result);
    }

    #[tokio::test]
    async fn test_prompt_layers_render_in_stable_order() {
        use crate::skills::model::{SkillMetadata, SkillScope};

        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = true;
        config.agent.include_working_directory = true;

        let mut ctx = PromptContext::default();
        ctx.add_tool(tools::EXEC_COMMAND.to_string());
        ctx.add_tool(tools::APPLY_PATCH.to_string());
        ctx.add_skill_metadata(SkillMetadata {
            name: "skill-creator".to_string(),
            description: "Create skills".to_string(),
            short_description: None,
            path: PathBuf::from("/tmp/skill-creator/SKILL.md"),
            scope: SkillScope::System,
            manifest: None,
        });
        ctx.add_language("Rust".to_string());
        ctx.set_current_directory(PathBuf::from("/workspace"));

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        let mode_pos = result.find("## Operating Profile").expect("operating profile section");
        let tools_pos = result.find("## Active Tools").expect("tools section");
        let skills_pos = result.find("## Skills").expect("skills section");
        let env_pos = result.find("## Environment").expect("environment section");

        assert!(mode_pos < tools_pos, "operating profile should precede tools");
        assert!(tools_pos < skills_pos, "tools should precede skills");
        assert!(skills_pos < env_pos, "skills should precede environment");
    }

    #[tokio::test]
    async fn test_skills_section_stays_lean_and_routing_focused() {
        use crate::skills::model::SkillScope;
        use crate::skills::types::SkillManifest;

        let config = VTCodeConfig::default();
        let mut ctx = PromptContext::default();
        ctx.available_skill_metadata.push(crate::skills::model::SkillMetadata {
            name: "skill-creator".to_string(),
            description: "Create or update skills".to_string(),
            short_description: None,
            path: PathBuf::from("/tmp/skill-creator/SKILL.md"),
            scope: SkillScope::System,
            manifest: Some(
                SkillManifest {
                    when_to_use: Some("Use when creating or updating a skill.".to_string()),
                    when_not_to_use: Some("Avoid for unrelated implementation work.".to_string()),
                    ..SkillManifest::default()
                }
                .into(),
            ),
        });

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        assert!(result.contains("## Skills"));
        assert!(result.contains("skill-creator: Create or update skills"));
        assert!(result.contains("Use a skill only when the user names it"));
        assert!(!result.contains("Discovery: Available skills are listed"));
        assert!(!result.contains("/tmp/skill-creator/SKILL.md"));
        assert!(!result.contains("use: Use when creating or updating a skill."));
        assert!(!result.contains("avoid: Avoid for unrelated implementation work."));
    }

    #[test]
    fn test_static_prompts_have_no_placeholders() {
        let _minimal = generate_minimal_instruction();
        let _lightweight = generate_lightweight_instruction();
        let _specialized = generate_specialized_instruction();

        let minimal_text = minimal_instruction_text();
        let lightweight_text = lightweight_instruction_text();
        let specialized_text = specialized_instruction_text();

        assert!(!minimal_text.contains("__UNIFIED_TOOL_GUIDANCE__"), "Minimal prompt has uninterpolated placeholder");
        assert!(
            !lightweight_text.contains("__UNIFIED_TOOL_GUIDANCE__"),
            "Lightweight prompt has uninterpolated placeholder"
        );
        assert!(
            !specialized_text.contains("__UNIFIED_TOOL_GUIDANCE__"),
            "Specialized prompt has uninterpolated placeholder"
        );
        assert!(
            !default_system_prompt().contains("__UNIFIED_TOOL_GUIDANCE__"),
            "Default prompt has uninterpolated placeholder"
        );
    }

    #[test]
    fn test_agent_identity_labels() {
        // Test known agent names
        assert_eq!(agent_identity_label("build"), "VT Code (Build mode)");
        assert_eq!(agent_identity_label("auto"), "VT Code (Auto mode)");
        assert_eq!(agent_identity_label("duck"), "VT Code (Duck mode)");
        assert_eq!(agent_identity_label("plan"), "VT Code (Plan mode)");
        assert_eq!(agent_identity_label("explorer"), "VT Code (Explorer mode)");
        assert_eq!(agent_identity_label("worker"), "VT Code (Worker mode)");

        // Test unknown agent names
        assert_eq!(agent_identity_label("unknown"), "VT Code (unknown)");
        assert_eq!(agent_identity_label("custom"), "VT Code (custom)");
    }

    #[test]
    fn test_apply_agent_identity() {
        let prompt = "# VT Code\n\nVT Code. Be concise and safe.\n\n## Contract\n- Rule 1";
        let result = apply_agent_identity(prompt, "VT Code (Build mode)");
        assert_eq!(
            result,
            "# VT Code (Build mode)\n\nVT Code (Build mode). Be concise and safe.\n\n## Contract\n- Rule 1"
        );
    }

    #[tokio::test]
    async fn test_system_prompt_includes_agent_identity() {
        let mut config = VTCodeConfig {
            default_primary_agent: "build".to_string(),
            ..Default::default()
        };
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(result.starts_with("# VT Code (Build mode)"), "Should start with agent identity: {}", &result[..50]);
        assert!(
            result.contains("VT Code (Build mode). Be concise and safe."),
            "Should include agent identity in intro"
        );
    }

    #[tokio::test]
    async fn test_system_prompt_auto_agent_identity() {
        let mut config = VTCodeConfig {
            default_primary_agent: "auto".to_string(),
            ..Default::default()
        };
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(result.starts_with("# VT Code (Auto mode)"), "Should start with auto agent identity");
    }

    #[tokio::test]
    async fn test_system_prompt_duck_agent_identity() {
        let mut config = VTCodeConfig {
            default_primary_agent: "duck".to_string(),
            ..Default::default()
        };
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;

        let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(result.starts_with("# VT Code (Duck mode)"), "Should start with duck agent identity");
    }

    #[test]
    fn test_estimate_token_count() {
        assert_eq!(estimate_token_count(""), 0);
        assert_eq!(estimate_token_count("hello"), 2); // 5 chars / 4 = 1.25 -> ceil = 2
        assert_eq!(estimate_token_count("1234"), 1); // 4 chars / 4 = 1
        assert_eq!(estimate_token_count("12345"), 2); // 5 chars / 4 = 1.25 -> ceil = 2

        // Realistic prompt size check — these are estimates, not exact token counts
        let minimal_tokens = estimate_token_count(minimal_system_prompt());
        let default_tokens = estimate_token_count(default_system_prompt());
        assert!(minimal_tokens < 400, "Minimal prompt tokens: {minimal_tokens}");
        assert!(default_tokens < 600, "Default prompt tokens: {default_tokens}");
    }

    #[tokio::test]
    async fn test_golden_under_budget_output_is_byte_identical() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;

        let result = compose_system_instruction_text(workspace.path(), Some(&config), None).await;

        let expected = r#"# VT Code (Build mode)

VT Code (Build mode). Be concise and safe.

## Contract

- If context is missing, say so, do not guess, finish unblocked slices.
- Do not use emoji in responses.
- Use retrieved evidence when citation-sensitive.
- Preserve task goal, tracker state, touched files, verification status, and decisions across compaction.
- Keep outputs concise; keep agent loops simple and let the model choose the next useful step.
- `spool_path` holds full tool output. Inspect it once with a targeted shell command through `exec_command.cmd` instead of repeatedly dumping the whole file. Past-turn errors are already in history.
- Start with existing `AGENTS.md` and `CLAUDE.md`; inspect code first and match local patterns.
- Take safe, reversible steps; recover from tool errors with corrected parameters, smaller scope, or one focused clarification.
- Ask only for material behavior, API, UX, or credential changes.
- Keep control on the main thread. Delegate bounded, independent work only.
- Verify changes yourself; never claim a check passed unless you ran it.
- Keep user updates brief and high-signal.
- Read files before answering. Never speculate about code you have not opened.
- Make only requested changes. When the active agent has tool access, use tools to implement directly; otherwise stay within the active agent mode.

## Operating Profile

- Available tools in the default profile are `exec_command`, `write_stdin`, and `apply_patch`.
- Put normal shell commands in `exec_command.cmd`; they are not separate function tools. Follow the active shell profile's syntax.
- Treat completion language as a checkpoint, not proof; only stop when verification is resolved.
- When tools are available, read files and search the codebase before answering; use tools to implement directly rather than describing what should be done.
- Use Planning workflow for research/spec work; stay read-only until implementation intent is explicit.

## Shell Profile
- Active shell profile: `unix_like`. Use Unix-like command syntax in `exec_command.cmd`, for example `ls`, `rg`, `find`, `cat`, `sed`, and `awk`.
- On macOS, write BSD-compatible flags for BSD tools. VT Code does not rewrite GNU flags for macOS BSD tools.
- The shell profile controls prompt examples and expected command syntax only; command policy, sandboxing, and approvals remain separate runtime checks.
- VT Code does not translate GNU-to-BSD, BSD-to-GNU, Unix-to-PowerShell, or PowerShell-to-Unix command flags."#;
        assert_eq!(result, expected, "single-section base-contract output must stay byte-identical");
    }

    #[tokio::test]
    async fn test_golden_multi_section_output_is_byte_identical() {
        use crate::skills::model::{SkillMetadata, SkillScope};

        let workspace = tempfile::TempDir::new().expect("workspace");
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Lightweight;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = true;
        config.agent.instruction_max_bytes = 0;
        config.agent.include_structured_reasoning_tags = Some(true);

        let mut ctx = PromptContext::default();
        ctx.add_tool(tools::CODE_SEARCH.to_string());
        ctx.add_tool(tools::EXEC_COMMAND.to_string());
        ctx.add_skill_metadata(SkillMetadata {
            name: "skill-creator".to_string(),
            description: "Create skills".to_string(),
            short_description: None,
            path: PathBuf::from("/tmp/skill-creator/SKILL.md"),
            scope: SkillScope::System,
            manifest: None,
        });
        ctx.set_current_directory(PathBuf::from("/workspace"));

        let result = compose_system_instruction_text(workspace.path(), Some(&config), Some(&ctx)).await;

        let expected = r#"# VT Code (Build mode)

VT Code (Build mode). Be concise and safe.

## Contract

- If context is missing, say so, do not guess, finish unblocked slices.
- Do not use emoji in responses.
- Use retrieved evidence when citation-sensitive.
- Preserve task goal, tracker state, touched files, verification status, and decisions across compaction.
- Keep outputs concise; keep agent loops simple and let the model choose the next useful step.
- `spool_path` holds full tool output. Inspect it once with a targeted shell command through `exec_command.cmd` instead of repeatedly dumping the whole file. Past-turn errors are already in history.
- Start with existing `AGENTS.md` and `CLAUDE.md`; inspect code first and match local patterns.
- Take safe, reversible steps; recover from tool errors with corrected parameters, smaller scope, or one focused clarification.
- Ask only for material behavior, API, UX, or credential changes.
- Keep control on the main thread. Delegate bounded, independent work only.
- Verify changes yourself; never claim a check passed unless you ran it.
- Keep user updates brief and high-signal.
- Read files before answering. Never speculate about code you have not opened.
- Make only requested changes. When the active agent has tool access, use tools to implement directly; otherwise stay within the active agent mode.

## Operating Profile

- Act and verify in one thread.
- Completion language is a checkpoint.
- Use `task_tracker` for nontrivial work.


## Structured Reasoning

Use tags when helpful: `<analysis>` facts/options, `<plan>` steps, `<uncertainty>` blockers, `<verification>` checks. When a decision must be consumed by code or tools, prefer JSON or function-call shaped output over prose.


## Shell Profile
- Active shell profile: `unix_like`. Use Unix-like command syntax in `exec_command.cmd`, for example `ls`, `rg`, `find`, `cat`, `sed`, and `awk`.
- On macOS, write BSD-compatible flags for BSD tools. VT Code does not rewrite GNU flags for macOS BSD tools.
- The shell profile controls prompt examples and expected command syntax only; command policy, sandboxing, and approvals remain separate runtime checks.
- VT Code does not translate GNU-to-BSD, BSD-to-GNU, Unix-to-PowerShell, or PowerShell-to-Unix command flags.

## Active Tools
- Use `exec_command.cmd` with `ls`, `rg`, `find`, `cat`, `sed`, and `awk` for repository browsing.
- Use `exec_command.cmd` for build tools, test tools, `git diff -- <path>`, and shell-only tasks.
- Completion is a checkpoint: keep verification resolved.
- Advanced `code_search` takes `query` plus optional `path`, `file_types`, `result_types`, and `max_results`; results are recognised definitions, exact syntactic usages that are not resolved references, literal text, and matching paths. Queries use literal smart-case. If results are truncated, narrow a filter in another call. Use `exec_command` or a specialised skill for arbitrary syntax-pattern work.
- If calls repeat, re-plan instead of retrying.
- Run independent tools in parallel when their inputs do not depend on each other.

## Skills
Use a skill only when the user names it or the task clearly matches. Load details on demand.
- skill-creator: Create skills

## Environment
- Working directory: /workspace"#;
        assert_eq!(result, expected, "multi-section joined output must stay byte-identical");
    }

    #[tokio::test]
    async fn test_over_budget_without_trim_keeps_full_text_and_reports_over_budget() {
        use crate::skills::model::{SkillMetadata, SkillScope};

        let workspace = tempfile::TempDir::new().expect("workspace");
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Lightweight;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = true;
        config.agent.instruction_max_bytes = 0;
        config.agent.include_structured_reasoning_tags = Some(true);
        config.agent.max_system_prompt_tokens = 1;
        config.agent.trim_system_prompt = false;
        config.agent.system_prompt_budget_warning = true;

        let mut ctx = PromptContext::default();
        ctx.add_tool(tools::CODE_SEARCH.to_string());
        ctx.add_tool(tools::EXEC_COMMAND.to_string());
        ctx.add_skill_metadata(SkillMetadata {
            name: "skill-creator".to_string(),
            description: "Create skills".to_string(),
            short_description: None,
            path: PathBuf::from("/tmp/skill-creator/SKILL.md"),
            scope: SkillScope::System,
            manifest: None,
        });
        ctx.set_current_directory(PathBuf::from("/workspace"));

        let sections = build_prompt_sections(workspace.path(), Some(&config), Some(&ctx)).await;
        let full_text = join_prompt_sections(&sections);
        let full_tokens = estimate_token_count(&full_text);
        assert!(full_tokens > config.agent.max_system_prompt_tokens, "test setup must exceed the configured budget");

        let (text, report) = compose_system_instruction_with_report(workspace.path(), Some(&config), Some(&ctx)).await;

        assert_eq!(text, full_text, "trim disabled: full untrimmed text must still be used");
        assert!(report.over_budget, "token estimate exceeds configured budget");
        assert_eq!(report.token_estimate, full_tokens);
        assert!(report.trimmed_sections.is_empty(), "no sections should be dropped when trimming is disabled");
    }

    #[tokio::test]
    async fn test_over_budget_with_trim_drops_sections_in_priority_order() {
        use crate::skills::model::{SkillMetadata, SkillScope};

        let workspace = tempfile::TempDir::new().expect("workspace");
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Lightweight;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = true;
        config.agent.instruction_max_bytes = 0;
        config.agent.include_structured_reasoning_tags = Some(true);
        config.agent.trim_system_prompt = true;
        config.agent.system_prompt_budget_warning = true;

        let mut ctx = PromptContext::default();
        ctx.add_tool(tools::CODE_SEARCH.to_string());
        ctx.add_tool(tools::EXEC_COMMAND.to_string());
        ctx.add_skill_metadata(SkillMetadata {
            name: "skill-creator".to_string(),
            description: "Create skills".to_string(),
            short_description: None,
            path: PathBuf::from("/tmp/skill-creator/SKILL.md"),
            scope: SkillScope::System,
            manifest: None,
        });
        ctx.set_current_directory(PathBuf::from("/workspace"));

        let sections = build_prompt_sections(workspace.path(), Some(&config), Some(&ctx)).await;
        // Budget set to exactly the base-contract-only token count so every
        // droppable (trim_priority = Some(_)) section must be dropped, while
        // the untrimmable base contract always survives.
        let base_only_tokens = sections
            .iter()
            .find(|section| section.kind == SectionKind::BaseContract)
            .map(|section| estimate_token_count(&section.text))
            .expect("base contract section is always present");
        config.agent.max_system_prompt_tokens = base_only_tokens;

        let (text, report) = compose_system_instruction_with_report(workspace.path(), Some(&config), Some(&ctx)).await;

        assert_eq!(
            report.trimmed_sections,
            vec![
                "structured_reasoning",
                "skills",
                "environment_addenda",
                "shell_profile",
                "tool_guidelines",
            ],
            "sections must drop in lowest-trim-priority-first order"
        );
        assert!(text.contains("## Contract"), "base contract must never be dropped");
        assert!(!text.contains("## Structured Reasoning"));
        assert!(!text.contains("## Skills"));
        assert!(!text.contains("## Environment"));
        assert!(!text.contains("## Active Tools"));
        assert!(!report.over_budget, "text should fit budget once every droppable section is gone");
    }

    #[test]
    fn test_cache_key_changes_with_budget_settings() {
        let project_root = PathBuf::from("/workspace");
        let base_config = VTCodeConfig::default();
        let base_key = cache_key(&project_root, Some(&base_config), None);

        let mut max_tokens_changed = VTCodeConfig::default();
        max_tokens_changed.agent.max_system_prompt_tokens += 1;
        assert_ne!(
            base_key,
            cache_key(&project_root, Some(&max_tokens_changed), None),
            "cache key must change when max_system_prompt_tokens changes"
        );

        let mut warning_changed = VTCodeConfig::default();
        warning_changed.agent.system_prompt_budget_warning = !warning_changed.agent.system_prompt_budget_warning;
        assert_ne!(
            base_key,
            cache_key(&project_root, Some(&warning_changed), None),
            "cache key must change when system_prompt_budget_warning changes"
        );

        let mut trim_changed = VTCodeConfig::default();
        trim_changed.agent.trim_system_prompt = !trim_changed.agent.trim_system_prompt;
        assert_ne!(
            base_key,
            cache_key(&project_root, Some(&trim_changed), None),
            "cache key must change when trim_system_prompt changes"
        );
    }

    #[test]
    fn test_cache_key_changes_with_default_primary_agent() {
        let project_root = PathBuf::from("/workspace");
        let base_config = VTCodeConfig {
            default_primary_agent: "build".to_string(),
            ..Default::default()
        };
        let base_key = cache_key(&project_root, Some(&base_config), None);

        let auto_config = VTCodeConfig {
            default_primary_agent: "auto".to_string(),
            ..Default::default()
        };
        assert_ne!(
            base_key,
            cache_key(&project_root, Some(&auto_config), None),
            "cache key must change when default_primary_agent changes, since \
             agent_identity_label rewrites the composed prompt"
        );
    }

    #[tokio::test]
    async fn measure_system_prompt_size_returns_non_empty_report_for_empty_workspace() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let config = VTCodeConfig::default();
        let report = measure_system_prompt_size(temp.path(), &config).await;
        assert!(
            report.token_estimate > 0,
            "default system prompt should be non-empty, got {} tokens",
            report.token_estimate
        );
        assert!(
            !report.over_budget,
            "default config should be within default budget, got {} tokens",
            report.token_estimate
        );
        assert!(report.trimmed_sections.is_empty());
    }

    #[tokio::test]
    async fn measure_system_prompt_size_flags_over_budget() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let mut config = VTCodeConfig::default();
        config.agent.max_system_prompt_tokens = 1;
        let report = measure_system_prompt_size(temp.path(), &config).await;
        assert!(report.over_budget, "tiny budget should flag as over budget");
    }

    #[tokio::test]
    async fn measure_system_prompt_size_respects_max_budget_setting() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let mut config = VTCodeConfig::default();
        config.agent.max_system_prompt_tokens = 8_000;
        let report = measure_system_prompt_size(temp.path(), &config).await;
        // Default base prompt is well under 8k tokens for an empty workspace.
        assert!(!report.over_budget, "default prompt should fit within 8k tokens, got {}", report.token_estimate);
    }
}
