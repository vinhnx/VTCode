use crate::config::constants::tools;
use crate::config::types::{CapabilityLevel, ResolvedShellPromptProfile};
use crate::core::agent::harness_kernel::SessionToolCatalogSnapshot;
use crate::llm::provider::ToolDefinition;
use crate::prompts::sections::{
    SectionBoundaryMode, append_prompt_block, remove_prompt_section, trim_trailing_newlines,
};
use crate::tools::registry::tool_groups;

const TOOL_EXEC_COMMAND: &str = tools::EXEC_COMMAND;
const TOOL_WRITE_STDIN: &str = tools::WRITE_STDIN;
const TOOL_CODE_SEARCH: &str = tools::CODE_SEARCH;
const TOOL_READ_FILE: &str = tools::READ_FILE;
const TOOL_LIST_FILES: &str = tools::LIST_FILES;
const TOOL_GREP_FILE: &str = tools::GREP_FILE;
const TOOL_APPLY_PATCH: &str = tools::APPLY_PATCH;
const TOOL_REQUEST_USER_INPUT: &str = tools::REQUEST_USER_INPUT;
const TOOL_ASK_SESSION: &str = tools::ASK_SESSION;
const TOOL_TASK_TRACKER: &str = tools::TASK_TRACKER;
const PUBLIC_DECISION_GUIDANCE: &str = "- Use optional `record_decision` for consequential choices, rejected approaches, or recovery changes. Give concise public rationale; ordinary reads and commands need no record.";
const MATRIX_GUIDANCE: &str = "- `matrix`: create persists explicit tasks, checks, resources, and timeouts; start freezes the spec. The coordinator delegates execution and verification to scheduler-owned workers; discovery is idle-only. Replay restores admission before discovery; resume reconciles owned cleanup. status is a projection; pause stops dispatch; user cancel is terminal. Internal failures require reconciliation, not cancellation. Workers retain effective permissions and sandbox gates. Read leases require read-only workers. Report identity is runtime-owned; final success needs durable declared checks for the current generation, never summaries or child completion. Source and declared input changes, including submodule state, invalidate checks. Automatic retry requires replay_safe, confirmed cleanup, and at most one interrupted/timeout retry; other failures need a coordinator decision.";
const SESSION_MESSAGING_GUIDANCE: &str = "- Peer sessions: `list_sessions` discovers live CLI sessions in this workspace; `ask_session` sends one a question and its `answer_session` reply arrives as a notice on a later turn. Delivery needs the peer running.";
const WEB_SEARCH_GUIDANCE: &str = "- web_search uses keyless DuckDuckGo; web_fetch needs separate approval. For `antiban_blocked` or `retryable: false`, do not retry; wait `retry_after_ms` for `cooldown`; retry `network_error`/`upstream_error` once. For GitHub queries prefer `gh search`/`gh api` via exec_command when available.";

const TOOL_START_PLANNING: &str = tools::START_PLANNING;

const OPTIONAL_MARKDOWN_VALIDATION_GUIDANCE: &str = "- `verify: [skip Markdown lint if unavailable]`: report skipped; review diff/links without installing tools. Lint errors remain failures.";

/// The single home of `start_planning` guidance, shared by the Minimal and
/// Default Active Tools. The tool itself asks the user before entering
/// planning (`require_confirmation`), except under full-auto or
/// skip-confirmations, so the model calls it rather than proposing it in prose.
const START_PLANNING_GUIDANCE_LINE: &str = "- For demanding, ambiguous, or multi-phase tasks, call `start_planning`; it asks the user before entering the read-only Planning workflow unless the session runs in full-auto or skips confirmations. Skip it for straightforward changes.";

/// Planning-workflow `task_tracker` rules that the tracker schema cannot
/// carry.
///
/// The call shapes (list/update/bulk, index vs. index_path vs. items), the
/// planning index rule, and "update as work progresses" all live in the tool
/// description and parameter schema, which reach the model with the tool
/// definitions. Only the tracker-to-verification relation is left, and it
/// applies equally at every density, so one constant serves both the Minimal
/// planning addendum and the Default planning Active Tools.
const PLANNING_TASK_TRACKER_LINE: &str = "- `task_tracker`: keep blockers and verification open. `scope_review` checks cannot satisfy functional `verify` checks.";

/// Context-token ceiling at or below which Active Tools documentation
/// resolves to the Minimal density.
const MINIMAL_CONTEXT_TOKEN_CEILING: usize = 32_000;

/// The single home of the parallel-call hint, shared by the Minimal and
/// Default Active Tools; emitted only when the provider keeps parallel tool
/// configuration enabled.
const PARALLEL_TOOLS_LINE: &str = "- Run independent tools in parallel when inputs do not depend on each other.";

/// The single home of the `request_user_input` threshold rule, shared by the
/// Minimal planning addendum and the planning Active Tools.
/// The `request_user_input` threshold for the planning workflow.
///
/// Planning mode already states this threshold in
/// `PLANNING_WORKFLOW_INTERVIEW_POLICY_LINE` ("only for a critical blocker
/// where proceeding could cause materially different, unsafe, or irreversible
/// work"), which the planning notice always renders. This line therefore
/// carries only the part the notice does not: that the blocker must survive
/// repository exploration.
const REQUEST_USER_INPUT_LINE: &str =
    "- Use `request_user_input` only for a blocker that repository exploration did not resolve.";

/// Read-only capability line shared by the capability-level and
/// tool-fallback arms of `capability_mode_line`.
const CAPABILITY_READ_ONLY_LINE: &str =
    "- Capabilities: read-only. Analyze and search, but do not modify files or run shell commands.";

/// Policy line shared by both shell profiles: the profile governs syntax
/// examples only, and VT Code never translates flags across shells.
const SHELL_PROFILE_POLICY_SUFFIX: &str = "- The shell profile controls prompt examples and expected command syntax only; command policy, sandboxing, and approvals remain separate runtime checks. VT Code does not translate GNU-to-BSD, BSD-to-GNU, Unix-to-PowerShell, or PowerShell-to-Unix command flags.";

/// Documentation density is independent of the tools a session may execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolGuidanceProfile {
    Minimal,
    Default,
}

impl ToolGuidanceProfile {
    #[must_use]
    pub(crate) fn resolve(
        context_tokens: usize,
        default_prompt_tokens: usize,
        max_prompt_tokens: usize,
        input_usd_per_token: Option<f64>,
        max_budget_usd: Option<f64>,
    ) -> Self {
        let exceeds_cost = input_usd_per_token.zip(max_budget_usd).is_some_and(|(price, budget)| {
            price.is_finite() && price >= 0.0 && budget.is_finite() && default_prompt_tokens as f64 * price > budget
        });
        if (context_tokens > 0 && context_tokens <= MINIMAL_CONTEXT_TOKEN_CEILING)
            || (max_prompt_tokens > 0 && default_prompt_tokens > max_prompt_tokens)
            || exceeds_cost
        {
            Self::Minimal
        } else {
            Self::Default
        }
    }
}

/// Which guidance-gating tools a session exposes, computed once per build so
/// every Active Tools variant branches on the same membership policy.
struct ToolPresence {
    exec: bool,
    write_stdin: bool,
    code_search: bool,
    web_search: bool,
    read_file: bool,
    grep_file: bool,
    list_files: bool,
    apply_patch: bool,
    request_user_input: bool,
    task_tracker: bool,
    start_planning: bool,
    matrix: bool,
    record_decision: bool,
    session_messaging: bool,
}

impl ToolPresence {
    fn of(available_tools: &[String]) -> Self {
        // Single pass over the tool list: the old form ran one linear
        // `any()` scan per flag (10 scans per call, up to 4 calls per
        // model-aware prompt build). Same membership policy, one walk.
        let mut presence = Self {
            exec: false,
            write_stdin: false,
            code_search: false,
            web_search: false,
            read_file: false,
            grep_file: false,
            list_files: false,
            apply_patch: false,
            request_user_input: false,
            task_tracker: false,
            start_planning: false,
            matrix: false,
            record_decision: false,
            session_messaging: false,
        };
        for tool in available_tools {
            match tool.as_str() {
                TOOL_EXEC_COMMAND => presence.exec = true,
                TOOL_WRITE_STDIN => presence.write_stdin = true,
                TOOL_CODE_SEARCH => presence.code_search = true,
                "web_search" => presence.web_search = true,
                TOOL_READ_FILE => presence.read_file = true,
                TOOL_GREP_FILE => presence.grep_file = true,
                TOOL_LIST_FILES => presence.list_files = true,
                TOOL_APPLY_PATCH => presence.apply_patch = true,
                TOOL_REQUEST_USER_INPUT => presence.request_user_input = true,
                TOOL_TASK_TRACKER => presence.task_tracker = true,
                TOOL_START_PLANNING => presence.start_planning = true,
                "matrix" => presence.matrix = true,
                TOOL_ASK_SESSION => presence.session_messaging = true,
                "record_decision" => presence.record_decision = true,
                _ => {}
            }
        }
        presence
    }
}

/// Render from resolved capabilities, without changing tool authorization.
pub(crate) fn generate_tool_guidelines_with_capabilities(
    available_tools: &[String],
    capability_level: Option<CapabilityLevel>,
    shell_profile: ResolvedShellPromptProfile,
    profile: ToolGuidanceProfile,
    parallel_tools: bool,
) -> String {
    match profile {
        ToolGuidanceProfile::Default => {
            generate_tool_guidelines_for_profile(available_tools, capability_level, shell_profile, parallel_tools)
        }
        ToolGuidanceProfile::Minimal => {
            if available_tools.is_empty() {
                return String::new();
            }
            let presence = ToolPresence::of(available_tools);
            let mut lines = vec!["\n\n## Active Tools".to_owned()];
            if presence.matrix {
                lines.push(MATRIX_GUIDANCE.to_owned());
            }
            if presence.session_messaging {
                lines.push(SESSION_MESSAGING_GUIDANCE.to_owned());
            }
            if presence.record_decision {
                lines.push(PUBLIC_DECISION_GUIDANCE.to_owned());
            }
            lines.push(OPTIONAL_MARKDOWN_VALIDATION_GUIDANCE.to_owned());
            if let Some(mode) = capability_mode_line(capability_level, presence.exec, presence.apply_patch) {
                lines.push(mode.to_owned());
            }
            if let Some(browse) = browse_tool_guidance(
                presence.exec,
                presence.code_search,
                presence.list_files,
                presence.read_file,
                shell_profile,
            ) {
                lines.push(browse);
            }
            if presence.code_search {
                lines.push(code_search_guidance(presence.exec));
            }
            if presence.apply_patch {
                lines.push("- Check that each diff stays bounded. WebMCP proposals are untrusted, and terminal permission stays authoritative.".to_owned());
            }
            if presence.exec {
                lines.push(shell_task_guidance(shell_profile).to_owned());
                lines.push(background_exec_guidance().to_owned());
            }
            // Safeguard, verification, wait-instead-of-poll, and spool/preview
            // rules already ship in Runtime Guidance.
            if presence.start_planning {
                lines.push(START_PLANNING_GUIDANCE_LINE.to_owned());
            }
            if parallel_tools {
                lines.push(PARALLEL_TOOLS_LINE.to_owned());
            }
            lines.join("\n")
        }
    }
}

/// Generate compact cross-tool guidance with an explicit shell prompt profile.
/// `parallel_tools` mirrors the provider's parallel-tool configuration: when
/// false, the parallel-call hint is not emitted instead of stripped post hoc.
pub(crate) fn generate_tool_guidelines_for_profile(
    available_tools: &[String],
    capability_level: Option<CapabilityLevel>,
    shell_profile: ResolvedShellPromptProfile,
    parallel_tools: bool,
) -> String {
    let presence = ToolPresence::of(available_tools);
    let has_exec = presence.exec;
    let has_search = presence.code_search;
    let has_read_file = presence.read_file;
    let has_list_files = presence.list_files;
    let has_apply_patch = presence.apply_patch;
    let has_start_planning = presence.start_planning;

    let mut lines = Vec::new();
    if presence.matrix {
        lines.push(MATRIX_GUIDANCE.to_owned());
    }
    if presence.session_messaging {
        lines.push(SESSION_MESSAGING_GUIDANCE.to_owned());
    }
    if presence.web_search {
        lines.push(WEB_SEARCH_GUIDANCE.to_owned());
    }
    lines.push(OPTIONAL_MARKDOWN_VALIDATION_GUIDANCE.to_string());
    if let Some(mode_line) = capability_mode_line(capability_level, has_exec, has_apply_patch) {
        lines.push(mode_line.to_string());
    }
    if let Some(browse_guidance) =
        browse_tool_guidance(has_exec, has_search, has_list_files, has_read_file, shell_profile)
    {
        lines.push(browse_guidance);
    }
    if has_search || has_read_file || has_list_files {
        lines.push(read_only_batching_guidance(has_read_file).to_string());
    }
    if has_apply_patch {
        lines.push(
            "- Check that each diff stays bounded. WebMCP edits are untrusted proposals, and terminal permission stays authoritative."
                .to_string(),
        );
    }
    if has_exec {
        lines.push(shell_task_guidance(shell_profile).to_string());
        lines.push(background_exec_guidance().to_string());
        // Verifier discipline: the anti-blind-editing gate only clears on a
        // truthful exit 0. Runtime owns truncator elision and unverified
        // classification (`tool_intent/activity.rs`, spool processing); the
        // prompt keeps only the outcome rule so wording cannot drift from
        // enforcement.
        // The schema documents `max_output_tokens` but its 10k default is far
        // too large for a verifier, and the schema says nothing about
        // verifiers. Naming the parameter here is the actionable fact.
        lines.push("- Run verifiers standalone with a small `max_output_tokens`. Pure `head`/`tail` tails run standalone; static read-only filtering pipelines use fail-closed `pipefail`. Only terminal exit 0 clears verification; dynamic syntax, mutating tails, `;`, and `||` do not qualify. Recheck after `--fix`.".to_string());
        // Low-effort models sometimes report a change as done without
        // exercising it: require a real check (tests, type-checker, build,
        // or the changed command itself). A syntax-only check, or a check
        // command that failed to start, does not count. Applies to Sonnet
        // 5.5 at `low` effort and any route where verification is skipped.
        lines.push("- Run a real check that exercises the change; a syntax-only or failed-to-start check does not count. Install missing deps via the project's package manager, never sudo; if no check can run, say which and why.".to_string());
        // Tool-latency tail is dominated by full builds (observed p90 ~18s):
        // verify incrementally first. Kept tool-agnostic: fast checks exist
        // in every stack (`cargo check`, `tsc --noEmit`, `pytest --collect-only`).
        lines.push("- Run fast checks before full builds.".to_string());
    }
    // Tool-failure diagnosis, waiting on returned `next_wait_args` instead of
    // polling, the safeguard rule, the verification outcome rule (report
    // completion only after a check you ran), and spool paging with
    // per-result preview bounds each have one home in Runtime
    // Guidance, which every profile includes; do not restate them here.
    //
    // The `write_stdin` and `code_search` contracts (session-id handling and
    // filter shape) live in those tools' own descriptions, which the model
    // receives with the tool definitions; restating them here only
    // duplicates the schema.
    if has_search {
        lines.push(code_search_guidance(has_exec));
    }
    if has_apply_patch || has_exec {
        lines.push(
            "- Build and Auto share tools and safety gates; Auto changes confirmation behavior only after explicit approval or full-auto policy."
                .to_string(),
        );
    }
    if (has_search || has_exec) && parallel_tools {
        lines.push(PARALLEL_TOOLS_LINE.to_string());
    }
    if has_start_planning {
        lines.push(START_PLANNING_GUIDANCE_LINE.to_string());
    }

    if lines.is_empty() {
        return String::new();
    }

    if presence.record_decision {
        lines.push(PUBLIC_DECISION_GUIDANCE.to_owned());
    }
    format!("\n\n## Active Tools\n{}", lines.join("\n"))
}

pub fn append_runtime_tool_prompt_sections_for_profile(
    prompt: &mut String,
    tool_snapshot: &SessionToolCatalogSnapshot,
    include_catalog_metadata: bool,
    shell_profile: ResolvedShellPromptProfile,
) {
    let names = snapshot_tool_names(tool_snapshot);
    append_runtime_tool_sections_with_names(prompt, tool_snapshot, names, include_catalog_metadata, shell_profile);
}

/// Select documentation density using the active route and session budgets.
pub fn append_runtime_tool_prompt_sections_for_model(
    prompt: &mut String,
    tool_snapshot: &SessionToolCatalogSnapshot,
    include_catalog_metadata: bool,
    shell_profile: ResolvedShellPromptProfile,
    provider: &dyn crate::llm::provider::LLMProvider,
    model: &str,
    config: Option<&crate::config::VTCodeConfig>,
) {
    let names = snapshot_tool_names(tool_snapshot);
    let default_guidance =
        append_runtime_tool_sections_with_names(prompt, tool_snapshot, names, include_catalog_metadata, shell_profile);
    let pricing = crate::config::models::model_catalog_entry(provider.name(), model).map(|entry| entry.pricing);
    let profile = ToolGuidanceProfile::resolve(
        crate::compaction::effective_context_budget(config, provider, model),
        vtcode_commons::estimate_tokens(prompt),
        config.map_or(0, |cfg| cfg.agent.max_system_prompt_tokens as usize),
        pricing.and_then(|price| price.input),
        config.and_then(|cfg| cfg.agent.harness.max_budget_usd),
    );
    let parallel_tools = provider.supports_parallel_tool_config(model);
    // The planning mode section owns the output contract at every density.
    // Default planning tool guidance contains no parallel-call hint.
    if tool_snapshot.planning_active && profile == ToolGuidanceProfile::Default {
        return;
    }
    remove_prompt_section(prompt, "## Active Tools", SectionBoundaryMode::BracketOrMarkdown);
    let presence = ToolPresence::of(names);
    let capability_level = Some(infer_capability_level_from_presence(&presence));
    let mut guidance = match profile {
        ToolGuidanceProfile::Default => {
            let mut guidance = default_guidance;
            if let (Some(original), Some(effective)) = (
                capability_mode_line(None, presence.exec, presence.apply_patch),
                capability_mode_line(capability_level, presence.exec, presence.apply_patch),
            ) && original != effective
                && let Some(start) = guidance.find(original)
            {
                guidance.replace_range(start..start + original.len(), effective);
            }
            if !parallel_tools && let Some(start) = guidance.find(PARALLEL_TOOLS_LINE) {
                guidance.replace_range(start.saturating_sub(1)..start + PARALLEL_TOOLS_LINE.len(), "");
            }
            guidance
        }
        ToolGuidanceProfile::Minimal => {
            generate_tool_guidelines_with_capabilities(names, capability_level, shell_profile, profile, parallel_tools)
        }
    };
    if tool_snapshot.planning_active {
        append_minimal_planning_addendum(&mut guidance, &presence);
    }
    append_prompt_block(prompt, guidance.trim_start_matches('\n'));
}

/// Replace the runtime tool sections with guidance built from `names`, plus
/// optional catalog metadata. Return the Default guidance so model-aware
/// callers can reuse it after measuring the assembled prompt.
fn append_runtime_tool_sections_with_names(
    prompt: &mut String,
    tool_snapshot: &SessionToolCatalogSnapshot,
    names: &[String],
    include_catalog_metadata: bool,
    shell_profile: ResolvedShellPromptProfile,
) -> String {
    remove_prompt_section(prompt, "## Active Tools", SectionBoundaryMode::BracketOrMarkdown);
    remove_prompt_section(prompt, "[Runtime Tool Catalog]", SectionBoundaryMode::BracketOrMarkdown);
    trim_trailing_newlines(prompt);

    let guidelines = generate_runtime_tool_guidelines_for_profile(names, tool_snapshot.planning_active, shell_profile);
    if !guidelines.is_empty() {
        append_prompt_block(prompt, guidelines.trim_start_matches('\n'));
    }

    if include_catalog_metadata && tool_snapshot.snapshot.is_some() {
        let active_tools = if tool_snapshot.active_tool_names.is_empty() {
            "none".to_string()
        } else {
            tool_snapshot.active_tool_names.join(", ")
        };
        // One payload line: this block is machine diagnostics for the
        // prompt/catalog alignment guard, not model guidance, so it must not
        // spend prompt lines. The guard parses `key: value` pairs separated
        // by `;`, so the format stays machine-readable.
        let catalog_metadata = format!(
            "[Runtime Tool Catalog]\n- version: {}; epoch: {}; catalog_tools: {}; available_tools: {}; currently_available_tools: {}; request_user_input_enabled: {}",
            tool_snapshot.version,
            tool_snapshot.epoch,
            tool_snapshot.catalog_tools(),
            tool_snapshot.available_tools(),
            active_tools,
            tool_snapshot.request_user_input_enabled,
        );
        append_prompt_block(prompt, &catalog_metadata);
    }
    guidelines
}

/// Compact tool reminders; the planning mode section retains the canonical
/// research, step-quality, output, and persistence contract at every density.
fn append_minimal_planning_addendum(guidance: &mut String, presence: &ToolPresence) {
    guidance.push_str("\n- Planning is read-only.");
    let mut read_tools = Vec::with_capacity(4);
    if presence.read_file {
        read_tools.push(TOOL_READ_FILE);
    }
    if presence.grep_file {
        read_tools.push(TOOL_GREP_FILE);
    }
    if presence.code_search {
        read_tools.push(TOOL_CODE_SEARCH);
    }
    if presence.list_files {
        read_tools.push(TOOL_LIST_FILES);
    }
    guidance.push_str("\n- Keep inspections small: ");
    if !read_tools.is_empty() {
        guidance.push_str(&format!("prefer `{}` over `exec_command` shell reads; ", read_tools.join("`/`")));
    }
    guidance.push_str("keep `max_output_tokens` small, avoid batching multiple large inspections in parallel; start git history with `git log --oneline` before targeted `git show --stat`.");
    if presence.task_tracker {
        guidance.push('\n');
        guidance.push_str(PLANNING_TASK_TRACKER_LINE);
    }
    if presence.request_user_input {
        guidance.push('\n');
        guidance.push_str(REQUEST_USER_INPUT_LINE);
    }
}

/// Append a compact summary of tools omitted from a client-local wire payload.
///
/// The listing is bounded like the `## Skills` routing section: at most
/// `DEFERRED_TOOLS_MAX_GROUPS` groups are named and the rest collapse into one
/// overflow line, so MCP-heavy sessions cannot bloat every request. Group
/// descriptions are server-provided and unbounded, so each line is truncated
/// to `DEFERRED_TOOLS_MAX_DESC_CHARS` characters.
pub fn append_deferred_tools_prompt_section(prompt: &mut String, tools: &[ToolDefinition]) {
    remove_prompt_section(prompt, "[Deferred Tools]", SectionBoundaryMode::BracketOrMarkdown);

    let mut groups: Vec<_> = tool_groups(tools)
        .into_iter()
        .filter(|group| group.deferred_count > 0)
        .collect();
    let overflow = groups.len().saturating_sub(DEFERRED_TOOLS_MAX_GROUPS);
    groups.truncate(DEFERRED_TOOLS_MAX_GROUPS);

    let mut lines: Vec<String> = groups
        .into_iter()
        .map(|group| {
            let description = truncate_deferred_description(group.description.as_deref().unwrap_or_default());
            format!("- {} ({} tools): {}", group.name, group.deferred_count, description)
        })
        .collect();
    if overflow > 0 {
        lines.push(format!("(+{overflow} more deferred groups available — use `search_tools` to find them)"));
    }

    let unnamespaced_deferred = tools
        .iter()
        .filter(|tool| tool.namespace.is_none() && tool.defer_loading == Some(true))
        .count();
    if unnamespaced_deferred > 0 {
        lines.push(format!("- {unnamespaced_deferred} additional deferred tools"));
    }

    if lines.is_empty() {
        return;
    }

    let section = format!(
        "[Deferred Tools]\n{}\nUse `search_tools` to find a deferred capability. Selected definitions become available in the next request segment.",
        lines.join("\n")
    );
    append_prompt_block(prompt, &section);
}

/// Maximum deferred-tool groups named in the `[Deferred Tools]` prompt
/// section before the rest collapse into one overflow line.
const DEFERRED_TOOLS_MAX_GROUPS: usize = 8;
/// Maximum characters of a group description in the section. Mirrors the
/// `## Skills` line truncation: server-provided text must not bloat the prompt.
const DEFERRED_TOOLS_MAX_DESC_CHARS: usize = 120;

fn truncate_deferred_description(description: &str) -> String {
    // Single pass: visit at most `MAX + 1` chars. The old form ran
    // `chars().count()` over the whole string, then `chars().take()` again.
    let mut chars = description.chars();
    let truncated: String = chars.by_ref().take(DEFERRED_TOOLS_MAX_DESC_CHARS).collect();
    if chars.next().is_none() {
        return description.to_string();
    }
    format!("{}...", truncated.trim_end())
}

fn generate_runtime_tool_guidelines_for_profile(
    available_tools: &[String],
    planning_active: bool,
    shell_profile: ResolvedShellPromptProfile,
) -> String {
    if !planning_active {
        // Measurement parity: this Default-density build feeds the prompt size
        // that `ToolGuidanceProfile::resolve` reads, so it keeps shipping the
        // parallel-call hint unconditionally like the static composition path.
        return generate_tool_guidelines_for_profile(available_tools, None, shell_profile, true);
    }

    let presence = ToolPresence::of(available_tools);
    let has_exec = presence.exec;
    let has_search = presence.code_search;
    let has_read_file = presence.read_file;
    let has_list_files = presence.list_files;
    let has_request_user_input = presence.request_user_input;
    let has_task_tracker = presence.task_tracker;

    let mut lines = vec!["- Planning workflow active: stay within the read-safe tool list.".to_string()];
    if presence.web_search {
        lines.push(WEB_SEARCH_GUIDANCE.to_owned());
    }
    lines.push(OPTIONAL_MARKDOWN_VALIDATION_GUIDANCE.to_string());
    if let Some(browse_guidance) =
        browse_tool_guidance(has_exec, has_search, has_list_files, has_read_file, shell_profile)
    {
        lines.push(browse_guidance);
    }
    if has_exec {
        lines.push("- In Planning workflow, use `exec_command` only for read-only verification.".to_string());
    }
    if has_search {
        lines.push(code_search_guidance(has_exec));
    }
    if has_task_tracker {
        // The tracker call shapes (list/update/bulk, index vs. index_path vs.
        // items, and "omit unused fields") are documented by the tool
        // description, which reaches the model with the tool definitions.
        // This line keeps only the planning-only facts the schema does not
        // carry.
        lines.push(PLANNING_TASK_TRACKER_LINE.to_string());
    }
    if has_request_user_input {
        lines.push(REQUEST_USER_INPUT_LINE.to_string());
    }
    if has_search || has_exec {
        lines.push("- If calls repeat without progress, tighten the plan instead of retrying identically.".to_string());
    }
    if presence.record_decision {
        lines.push(PUBLIC_DECISION_GUIDANCE.to_owned());
    }

    format!("\n\n## Active Tools\n{}", lines.join("\n"))
}

fn snapshot_tool_names(tool_snapshot: &SessionToolCatalogSnapshot) -> &[String] {
    &tool_snapshot.active_tool_names
}

fn browse_tool_guidance(
    has_exec: bool,
    has_search: bool,
    has_list_files: bool,
    has_read_file: bool,
    shell_profile: ResolvedShellPromptProfile,
) -> Option<String> {
    if has_exec {
        return Some(shell_browse_guidance(shell_profile, has_search));
    }

    if !(has_search || has_list_files || has_read_file) {
        return None;
    }

    Some("- Use available read-only repository tools for browsing; do not modify files.".to_string())
}

pub(crate) fn render_shell_profile_guidance(shell_profile: ResolvedShellPromptProfile) -> String {
    let profile_lines = match shell_profile {
        ResolvedShellPromptProfile::UnixLike => {
            "- Active shell profile: `unix_like`.\n- On macOS, write BSD-compatible flags for BSD tools. VT Code does not rewrite GNU flags for macOS BSD tools.\n- Commands run under the user's login shell, which may be zsh: avoid bash-only syntax such as `${PIPESTATUS[...]}` (it expands empty outside bash); rely on the harness-reported exit status instead of capturing codes through pipes."
        }
        ResolvedShellPromptProfile::PowerShell => {
            "- Active shell profile: `powershell`.\n- On native Windows, use WSL when you need Unix-like workflows or Unix command examples."
        }
    };
    format!("## Shell Profile\n{profile_lines}\n{SHELL_PROFILE_POLICY_SUFFIX}")
}

fn shell_browse_guidance(shell_profile: ResolvedShellPromptProfile, has_search: bool) -> String {
    // Sessions show `exec_command` + `rg` crowding out `code_search`
    // (observed 654 exec vs 15 code_search in one run): `rg` scans text
    // while `code_search` resolves definitions and exact usages, so route
    // code search to the dedicated tool whenever it is available.
    const SEARCH_PREFERENCE_UNIX: &str = " Prefer `code_search` over `rg`/`grep` for code.";
    const SEARCH_PREFERENCE_POWERSHELL: &str = " Prefer `code_search` over `Select-String` for code.";
    match shell_profile {
        ResolvedShellPromptProfile::UnixLike => {
            let mut line = String::from("- Browse the repository with `exec_command.cmd`.");
            if has_search {
                line.push_str(SEARCH_PREFERENCE_UNIX);
            }
            line
        }
        ResolvedShellPromptProfile::PowerShell => {
            let mut line = "- Browse the repository with `exec_command.cmd`.".to_string();
            if has_search {
                line.push_str(SEARCH_PREFERENCE_POWERSHELL);
            }
            line
        }
    }
}

fn shell_task_guidance(shell_profile: ResolvedShellPromptProfile) -> &'static str {
    match shell_profile {
        ResolvedShellPromptProfile::UnixLike => {
            "- In one-shot `exec_command` calls, do not use `!!`, `!$`, `!ssh`, or `fc`; write full command arguments explicitly from conversation or tool results. Interactive shells: review-safe history expansion (Bash `histverify`, zsh `HIST_VERIFY`)."
        }
        ResolvedShellPromptProfile::PowerShell => {
            "- Use native PowerShell syntax in `exec_command.cmd` for shell-only tasks."
        }
    }
}

fn background_exec_guidance() -> &'static str {
    // `background`, the three-process cap, `session_id`, `monitor_filter`, and
    // `monitor_digest` are all documented by the `exec_command` schema, which
    // reaches the model with the tool definitions, and the runtime rejects an
    // over-limit background start with an actionable error. The only fact the
    // schema cannot state is *when* to reach for it.
    "- For long-lived commands, start them in the background rather than blocking a turn."
}

fn read_only_batching_guidance(has_read_file: bool) -> &'static str {
    if has_read_file {
        "- Batch independent read-only calls; use bounded `read_file` ranges, order dependencies, serialize mutations; narrow the range on `line_truncated`; prefer wider windows and reuse evidence over overlapping slices."
    } else {
        "- Batch independent read-only calls; order dependent reads, and serialize mutations."
    }
}

fn code_search_guidance(has_exec: bool) -> String {
    // The schema owns the query contract ("Literal code or path query",
    // smart-case) and the filter names. What it cannot state is that `|`
    // separates alternatives, that a truncated or unavailable result needs a
    // different query rather than a retry, and that the runtime coerces a
    // JSON-encoded array or integer. The runtime also emits per-call hints
    // when it truncates or drops a result category, so this line carries the
    // standing rule rather than repeating them.
    const BASE: &str = "- `code_search` queries are literal and smart-case; `|` separates alternatives. On a truncated or unavailable result, change the query instead of retrying it. Do not JSON-encode arrays or integers as strings.";
    if has_exec {
        format!("{BASE} Use `exec_command` or a skill for syntax patterns.")
    } else {
        BASE.to_string()
    }
}

fn capability_mode_line(
    capability_level: Option<CapabilityLevel>,
    has_exec: bool,
    has_file: bool,
) -> Option<&'static str> {
    match capability_level {
        Some(CapabilityLevel::Basic) => {
            Some("- Capabilities: limited. Ask the user to enable more capabilities if file work is required.")
        }
        Some(CapabilityLevel::FileReading | CapabilityLevel::FileListing) => Some(CAPABILITY_READ_ONLY_LINE),
        _ if !has_exec && !has_file => Some(CAPABILITY_READ_ONLY_LINE),
        _ => None,
    }
}

/// Infer capability level from available tools.
pub(crate) fn infer_capability_level(available_tools: &[String]) -> CapabilityLevel {
    infer_capability_level_from_presence(&ToolPresence::of(available_tools))
}

fn infer_capability_level_from_presence(presence: &ToolPresence) -> CapabilityLevel {
    if presence.code_search {
        CapabilityLevel::CodeSearch
    } else if presence.apply_patch {
        CapabilityLevel::Editing
    } else if presence.exec {
        CapabilityLevel::Bash
    } else if presence.list_files {
        CapabilityLevel::FileListing
    } else if presence.read_file {
        CapabilityLevel::FileReading
    } else {
        CapabilityLevel::Basic
    }
}

#[cfg(test)]
mod tests;
