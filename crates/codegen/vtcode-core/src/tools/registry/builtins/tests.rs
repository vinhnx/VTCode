use super::*;

#[test]
fn distributed_slice_contains_all_builtin_tools() {
    use crate::tools::registry::distributed::BUILTIN_TOOLS;
    // Consolidated action tools keep the factory count bounded.
    // This catches accidentally missing #[distributed_slice] annotations.
    assert!(
        BUILTIN_TOOLS.len() >= 24,
        "expected at least 24 distributed tool factories, found {}",
        BUILTIN_TOOLS.len()
    );
}

#[test]
fn tool_backed_builtins_register_native_cgp_factories() {
    let plan_state = PlanningWorkflowState::new(PathBuf::from("/workspace"));
    let registrations = builtin_tool_registrations(Some(&plan_state));

    for tool_name in [tools::REQUEST_USER_INPUT, tools::START_PLANNING, tools::TASK_TRACKER] {
        let registration = registrations
            .iter()
            .find(|registration| registration.name() == tool_name)
            .expect("builtin registration should exist");
        assert!(registration.native_cgp_factory().is_some(), "expected native CGP factory for {tool_name}");
    }

    assert!(
        registrations
            .iter()
            .all(|registration| registration.name() != tools::UNIFIED_SEARCH
                && registration.name() != tools::UNIFIED_FILE
                && registration.name() != tools::UNIFIED_EXEC),
        "removed unified tools must not have builtin registrations"
    );
}

#[test]
fn task_tracker_builtin_registration_matches_workflow_metadata() {
    let plan_state = PlanningWorkflowState::new(PathBuf::from("/workspace"));

    let standard = builtin_tool_registrations(Some(&plan_state))
        .into_iter()
        .find(|registration| registration.name() == tools::TASK_TRACKER)
        .expect("standard task_tracker registration should exist");
    assert_eq!(standard.metadata().description(), Some(task_tracker_description_for_workflow(false)));
    assert_eq!(
        standard.metadata().parameter_schema().expect("standard schema")["properties"]["index"]["minimum"],
        0
    );

    plan_state.enable();
    let planning = builtin_tool_registrations(Some(&plan_state))
        .into_iter()
        .find(|registration| registration.name() == tools::TASK_TRACKER)
        .expect("planning task_tracker registration should exist");
    assert_eq!(planning.metadata().description(), Some(task_tracker_description_for_workflow(true)));
    assert_eq!(
        planning.metadata().parameter_schema().expect("planning schema")["properties"]["index"]["minimum"],
        1
    );
}

#[test]
fn codex_baseline_builtins_are_canonical_public_tools() {
    let plan_state = PlanningWorkflowState::new(PathBuf::from("/workspace"));
    let registrations = builtin_tool_registrations(Some(&plan_state));

    for tool_name in [tools::EXEC_COMMAND, tools::WRITE_STDIN] {
        let registration = registrations
            .iter()
            .find(|registration| registration.name() == tool_name)
            .expect("canonical public registration should exist");
        assert!(registration.expose_in_llm(), "{tool_name} should be public");
        assert!(registration.metadata().aliases().is_empty(), "{tool_name} should not rely on aliases");
    }

    let code_search = registrations
        .iter()
        .find(|registration| registration.name() == tools::CODE_SEARCH)
        .expect("advanced public code_search registration should exist");
    assert!(code_search.expose_in_llm(), "code_search should be public");
    assert!(code_search.metadata().aliases().is_empty(), "code_search should not rely on aliases");
    let schema = code_search.metadata().parameter_schema().expect("code_search schema");
    assert_eq!(schema["required"], json!(["query"]));
    let mut property_names = schema["properties"]
        .as_object()
        .expect("properties")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    property_names.sort_unstable();
    assert_eq!(property_names, ["file_types", "max_results", "path", "query", "result_types"]);

    for tool_name in [tools::UNIFIED_SEARCH, tools::UNIFIED_EXEC, tools::UNIFIED_FILE] {
        assert!(
            registrations.iter().all(|registration| registration.name() != tool_name),
            "{tool_name} must not have a builtin registration"
        );
    }
}

#[test]
fn web_search_schema_requires_canonical_query() {
    let registrations = builtin_tool_registrations(None);
    let web_search = registrations
        .iter()
        .find(|registration| registration.name() == tools::WEB_SEARCH)
        .expect("web_search registration should exist");
    let schema = web_search.metadata().parameter_schema().expect("web_search schema");

    assert_eq!(schema["required"], json!(["query"]));
    assert_eq!(schema["additionalProperties"], json!(false));
    assert_eq!(schema["properties"]["query"]["minLength"], json!(1));
    assert_eq!(schema["properties"]["query"]["maxLength"], json!(500));
    let mut property_names = schema["properties"]
        .as_object()
        .expect("properties")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    property_names.sort_unstable();
    assert_eq!(property_names, ["max_results", "query"]);
}

#[test]
fn multi_agent_builtins_expose_updated_descriptions() {
    let plan_state = PlanningWorkflowState::new(PathBuf::from("/workspace"));
    let registrations = builtin_tool_registrations(Some(&plan_state));

    let agent = registrations
        .iter()
        .find(|registration| registration.name() == tools::AGENT)
        .expect("agent registration should exist");
    assert!(
        agent
            .metadata()
            .description()
            .expect("agent description")
            .contains("delegated child agents")
    );
    for alias in [
        tools::SPAWN_AGENT,
        tools::SPAWN_BACKGROUND_SUBPROCESS,
        tools::SEND_INPUT,
        tools::RESUME_AGENT,
        tools::WAIT_AGENT,
        tools::CLOSE_AGENT,
    ] {
        assert!(agent.metadata().aliases().iter().any(|candidate| candidate == alias));
    }
}

#[test]
fn web_fetch_builtin_description_guides_agents_to_try_llms_txt_first() {
    let registrations = builtin_tool_registrations(None);
    let web_fetch = registrations
        .iter()
        .find(|registration| registration.name() == tools::WEB_FETCH)
        .expect("web_fetch registration should exist");
    let description = web_fetch.metadata().description().expect("web_fetch description");

    assert!(description.contains("/llms.txt"));
    assert!(description.contains("abc.com"));
    assert!(description.contains("https://abc.com/llms.txt"));
    assert!(description.contains("traverse"));
}

#[test]
fn web_fetch_builtin_description_matches_preview_and_temp_file_result() {
    let registrations = builtin_tool_registrations(None);
    let web_fetch = registrations
        .iter()
        .find(|registration| registration.name() == tools::WEB_FETCH)
        .expect("web_fetch registration should exist");
    let description = web_fetch.metadata().description().expect("web_fetch description");

    // The default mode returns a preview plus a temp_file path; it does
    // not produce an analyzed summary.
    assert!(description.contains("`preview`"));
    assert!(description.contains("`temp_file`"));
    assert!(description.contains("format=markdown"));
    assert!(!description.contains("analyzed summary"));
    assert!(!description.contains("Accepts:"));
    assert!(!description.contains("Do NOT"));
}

#[test]
fn web_fetch_schema_accepts_markdown_format() {
    let registrations = builtin_tool_registrations(None);
    let web_fetch = registrations
        .iter()
        .find(|registration| registration.name() == tools::WEB_FETCH)
        .expect("web_fetch registration should exist");
    let schema = web_fetch.metadata().parameter_schema().expect("web_fetch schema");

    assert_eq!(schema["properties"]["format"]["enum"], json!(["summary", "markdown"]));
    assert_eq!(schema["additionalProperties"], json!(false));
    let validator = jsonschema::validator_for(schema).expect("web_fetch schema should compile");
    assert!(validator.is_valid(&json!({"url": "https://example.com", "format": "markdown"})));
    assert!(validator.is_valid(&json!({"url": "https://example.com", "format": "summary"})));
    assert!(!validator.is_valid(&json!({"url": "https://example.com", "format": "html"})));
}

/// Tool descriptions are part of the prompt and directly drive tool
/// selection accuracy (Section 18.3.4 of the agentic-AI guide). This test
/// enforces a structural contract so that regressions in description
/// quality are caught at `cargo test` time rather than via observed
/// agent misbehavior.
///
/// Every LLM-visible tool with a description must satisfy the rules
/// below. Rules 2 and 3 are checked against the description sent in the
/// default Progressive documentation mode, which must be the complete
/// source description:
/// 1. Length is between 40 and 1500 characters.
/// 2. Contains at least one verb cue ("Use", "Create", "List", "Fetch",
///    "Search", "Send", "Apply", "Read", "Edit", etc.) so the model can
///    recognize the action the tool performs.
/// 3. For tools that mutate state, network-call, schedule work, or
///    require confirmation, the description must contain a constraint cue
///    ("max ", "rate-limit", "session", "blocks", "timeout",
///    "requires approval", etc.) so the model knows the limits and side
///    effects. Prohibition phrasing ("Do NOT", "Avoid", "never") does not
///    satisfy this rule: models that follow descriptions literally
///    over-apply it, so descriptions state the concrete limit instead.
///
/// Tools exempted from rule 3 are simple read-only helpers where the
/// model can safely call them without explicit guard-rails.
#[test]
fn tool_descriptions_satisfy_documented_contract() {
    use crate::config::ToolDocumentationMode;
    use crate::tools::handlers::compact::compact_tool_description;
    use crate::tools::handlers::{SessionSurface, SessionToolCatalog, SessionToolsConfig, ToolModelCapabilities};

    let plan_state = PlanningWorkflowState::new(PathBuf::from("/workspace"));
    let registrations = builtin_tool_registrations(Some(&plan_state));

    let verb_cues = [
        "Use ",
        "Create ",
        "Control ",
        "List ",
        "Fetch ",
        "Search ",
        "Send ",
        "Apply ",
        "Read ",
        "Write ",
        "Edit ",
        "Patch ",
        "Delete ",
        "Move ",
        "Copy ",
        "Spawn ",
        "Launch ",
        "Close ",
        "Resume ",
        "Wait ",
        "Connect ",
        "Disconnect ",
        "Schedule ",
        "Inspect ",
        "Persist ",
        "Request ",
        "Open ",
        "Stop ",
        "Run ",
        "Track ",
        "Update ",
    ];
    let constraint_cues = [
        "max ",
        "rate-limit",
        "rate limit",
        "session",
        "blocks",
        "timeout",
        "cap ",
        "outlives",
        "inherits",
        "expires",
        "limited to",
        "max_bytes",
        "max_results",
        "max_lines",
        "max chars",
        "max size",
        "once per",
        "requires ",
        "requires approval",
        "permission",
        "approval",
        "exceeds",
        "scoped",
    ];
    // Read-only / single-action helpers where a constraint cue is not
    // strictly required. Entries are registration names; aliases such as
    // cron_list or mcp_search_tools never reach this check.
    let rule3_allowlist: &[&str] = &[
        tools::REQUEST_USER_INPUT,
        tools::SEARCH_TOOLS,
        tools::TASK_TRACKER,
        tools::START_PLANNING,
        tools::CODE_SEARCH,
    ];

    for allowed in rule3_allowlist {
        assert!(
            registrations.iter().any(|registration| registration.name() == *allowed),
            "rule3 allowlist entry {allowed} is not a builtin registration name"
        );
    }

    // Rules 2 and 3 are checked against the text the model receives in
    // the default Progressive documentation mode, not just the source
    // string, so a projection that drops later sentences cannot hide a
    // missing cue.
    let progressive_catalog =
        SessionToolCatalog::rebuild_from_registrations(builtin_tool_registrations(Some(&plan_state)));
    let progressive_entries = progressive_catalog.schema_entries(SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Progressive,
        ToolModelCapabilities::default(),
    ));
    assert!(!progressive_entries.is_empty(), "Progressive catalog must expose builtin tools");

    for registration in &registrations {
        if !registration.expose_in_llm() {
            continue;
        }
        let Some(source_description) = registration.metadata().description() else {
            continue;
        };
        let tool_name = registration.name();

        // Rule 1: length of the source description.
        let len = source_description.chars().count();
        assert!((40..=1500).contains(&len), "{tool_name}: description length {len} outside [40, 1500]");

        let projected = compact_tool_description(source_description, ToolDocumentationMode::Progressive, None);
        let sent = progressive_entries
            .iter()
            .find(|entry| entry.name == tool_name)
            .map_or(projected.as_str(), |entry| entry.description.as_str());
        assert_eq!(
            sent,
            compact_tool_description(source_description, ToolDocumentationMode::Full, None),
            "{tool_name}: Progressive mode must send the complete builtin description"
        );
        let description = sent;

        // Rule 2: verb cue (case-sensitive "Use " is the most common).
        let has_verb = verb_cues.iter().any(|cue| description.contains(cue));
        assert!(
            has_verb,
            "{tool_name}: description must contain a verb cue like 'Use ', 'Create ', 'Fetch ', etc.\nDescription: {description}"
        );

        // Rule 3: constraint cue for side-effect tools.
        if rule3_allowlist.contains(&tool_name) {
            continue;
        }
        let has_constraint = constraint_cues.iter().any(|cue| description.contains(cue));
        assert!(
            has_constraint,
            "{tool_name}: side-effect description must state a concrete constraint cue ('max ', 'rate-limit', \
             'session', 'timeout', 'requires approval', 'inherits', ...).\nDescription: {description}"
        );
    }
}
#[test]
fn default_config_exposed_tool_count_within_cap() {
    // Regression guard for the tool-consolidation work: the number of
    // LLM-exposed built-in tools must stay bounded so the model does not
    // waste attention choosing between near-duplicates. Any new
    // registration must either consolidate an existing tool, be deferred
    // behind the deferred-loading path, or deliberately raise this cap in
    // review.
    let registrations = builtin_tool_registrations(None);
    let exposed: usize = registrations.iter().filter(|registration| registration.expose_in_llm()).count();
    // The optional canonical public-decision surface is the fifteenth builtin.
    assert!(
        exposed <= 15,
        "exposed built-in tool count is {exposed}; expected <= 15. \
         Consolidate, defer, or raise the cap in review."
    );
}

#[test]
fn public_decision_is_available_in_interactive_and_headless_planning_and_execution() {
    use crate::config::ToolDocumentationMode;
    use crate::tools::handlers::{SessionSurface, SessionToolCatalog, SessionToolsConfig, ToolModelCapabilities};
    let catalog = SessionToolCatalog::rebuild_from_registrations(builtin_tool_registrations(None));
    for surface in [SessionSurface::Interactive, SessionSurface::AgentRunner] {
        for planning in [false, true] {
            let config = SessionToolsConfig::full_public(
                surface,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_planning_active(planning);
            let names = catalog.public_tool_names(config);
            assert!(
                names.iter().any(|name| name == tools::RECORD_DECISION),
                "decision missing on {surface:?}, planning={planning}"
            );
        }
    }
}

/// End-to-end regression for the tool-consolidation work: builds the real
/// `SessionToolCatalog` from the actual builtin registrations (not a
/// synthetic subset) and asserts the number of tool definitions/function
/// declarations the model actually sees, for a default non-native-memory
/// config, stays within the post-fold cap. This complements
/// `default_config_exposed_tool_count_within_cap` (which only counts
/// `expose_in_llm()` registrations) by exercising the full catalog
/// pipeline, including dedup-by-public-name and deferred-loading
/// collapsing.
#[test]
fn emitted_model_tool_count_stays_within_cap_for_default_config() {
    use crate::config::ToolDocumentationMode;
    use crate::tools::handlers::{SessionSurface, SessionToolCatalog, SessionToolsConfig, ToolModelCapabilities};

    let registrations = builtin_tool_registrations(None);
    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let config = SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    );

    let model_tools = catalog.model_tools(config.clone());
    assert!(
        model_tools.len() <= 14,
        "emitted model_tools count is {}; expected <= 14. \
         Consolidate, defer, or raise the cap in review.",
        model_tools.len()
    );

    let function_declarations = catalog.function_declarations(config);
    assert!(
        function_declarations.len() <= 14,
        "emitted function_declarations count is {}; expected <= 14. \
         Consolidate, defer, or raise the cap in review.",
        function_declarations.len()
    );
}

/// End-to-end regression for the first-request token budget: the actual
/// builtin tool schemas sent in Progressive mode must fit in a small
/// token envelope, leaving room for the system prompt and conversation.
#[test]
fn emitted_model_tool_schema_fits_within_first_request_budget() {
    use crate::config::ToolDocumentationMode;
    use crate::tools::handlers::{SessionSurface, SessionToolCatalog, SessionToolsConfig, ToolModelCapabilities};
    use serde::Serialize;

    #[derive(Serialize)]
    struct ToolSchemaEstimate<'a> {
        name: &'a str,
        description: &'a str,
        parameters: &'a serde_json::Value,
    }

    let registrations = builtin_tool_registrations(None);
    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let config = SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Progressive,
        ToolModelCapabilities::default(),
    );

    let schema_entries = catalog.schema_entries(config);
    let total_tokens: usize = schema_entries
        .iter()
        .map(|entry| {
            let estimate = ToolSchemaEstimate {
                name: &entry.name,
                description: &entry.description,
                parameters: &entry.parameters,
            };
            serde_json::to_string(&estimate).map(|s| s.len() / 4).unwrap_or(0)
        })
        .sum();

    // Progressive mode sends complete builtin tool descriptions and keeps
    // parameter descriptions (trimming only long tails), because models
    // that follow tool definitions literally act on the whole text. The
    // cap is the harness-tax gate (HarnessTax): keep the emitted schema
    // envelope tight so first-call fixed overhead stays low. Measured
    // ~2,336 on the Codex-4 default profile with public decision recording
    // in the initial catalog and discovery surface. Retain a bounded
    // 2,400-token envelope.
    assert!(
        total_tokens <= 2_400,
        "emitted model tool schema tokens in Progressive mode is {total_tokens}; expected <= 2_400"
    );
}

/// End-to-end regression for the combined first-request token budget:
/// the effective-default system prompt (Minimal) plus Progressive tool
/// schemas, instruction appendix, and welcome addendum must stay under
/// 6k tokens with no MCP, and under 8k tokens with 5 simulated MCP servers.
#[test]
fn first_request_total_token_budget_within_limit() {
    use crate::config::ToolDocumentationMode;
    use crate::config::types::SystemPromptMode;
    use crate::prompts::system::static_profile_prompt;
    use crate::tools::handlers::{SessionSurface, SessionToolCatalog, SessionToolsConfig, ToolModelCapabilities};
    use serde::Serialize;

    #[derive(Serialize)]
    struct ToolSchemaEstimate<'a> {
        name: &'a str,
        description: &'a str,
        parameters: &'a serde_json::Value,
    }

    let registrations = builtin_tool_registrations(None);
    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let config = SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Progressive,
        ToolModelCapabilities::default(),
    );

    let schema_entries = catalog.schema_entries(config);
    let tool_schema_tokens: usize = schema_entries
        .iter()
        .map(|entry| {
            let estimate = ToolSchemaEstimate {
                name: &entry.name,
                description: &entry.description,
                parameters: &entry.parameters,
            };
            serde_json::to_string(&estimate).map(|s| s.len() / 4).unwrap_or(0)
        })
        .sum();

    // Measure the configured default profile (Minimal after lean-defaults).
    let system_prompt = static_profile_prompt(SystemPromptMode::default());
    let system_prompt_tokens = system_prompt.len() / 4;

    let instruction_appendix_tokens = 250;
    let welcome_addendum_tokens = 200;

    let total_no_mcp =
        system_prompt_tokens + tool_schema_tokens + instruction_appendix_tokens + welcome_addendum_tokens;

    assert!(
        total_no_mcp <= 6_000,
        "first-request token budget exceeded: {total_no_mcp} tokens (system={system_prompt_tokens}, tools={tool_schema_tokens}, instructions={instruction_appendix_tokens}, addendum={welcome_addendum_tokens}); expected <= 6_000"
    );

    let mcp_tool_count = 5;
    let estimated_mcp_schema_tokens = mcp_tool_count * 300;
    let total_with_mcp = total_no_mcp + estimated_mcp_schema_tokens;

    assert!(
        total_with_mcp <= 8_000,
        "first-request token budget with MCP exceeded: {total_with_mcp} tokens; expected <= 8_000"
    );

    // Explicit Default-mode profile must also stay inside the no-MCP
    // ceiling when selected (S2-prompt).
    let default_prompt = static_profile_prompt(SystemPromptMode::Default);
    let total_default_mode =
        default_prompt.len() / 4 + tool_schema_tokens + instruction_appendix_tokens + welcome_addendum_tokens;
    assert!(
        total_default_mode <= 6_000,
        "Default-mode first-request budget exceeded: {total_default_mode} tokens; expected <= 6_000"
    );
}

/// Cache-stability guard: building the same system prompt + Progressive
/// tool schema twice must produce byte-identical output. Any non-determinism
/// (random ordering, uninitialized memory, race) invalidates provider
/// prompt caches and re-pays full input cost on every turn.
#[test]
fn system_prompt_and_tools_prefix_is_byte_stable_across_rebuilds() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    use crate::config::ToolDocumentationMode;
    use crate::prompts::system::default_system_prompt;
    use crate::tools::handlers::{SessionSurface, SessionToolCatalog, SessionToolsConfig, ToolModelCapabilities};
    use serde::Serialize;

    #[derive(Serialize)]
    struct ToolSchemaEstimate<'a> {
        name: &'a str,
        description: &'a str,
        parameters: &'a serde_json::Value,
    }

    fn hash_system_prompt_and_tools() -> u64 {
        let mut hasher = DefaultHasher::new();

        let system_prompt = default_system_prompt();
        system_prompt.hash(&mut hasher);

        let registrations = builtin_tool_registrations(None);
        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let config = SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Progressive,
            ToolModelCapabilities::default(),
        );

        let schema_entries = catalog.schema_entries(config);
        for entry in &schema_entries {
            let estimate = ToolSchemaEstimate {
                name: &entry.name,
                description: &entry.description,
                parameters: &entry.parameters,
            };
            if let Ok(json) = serde_json::to_string(&estimate) {
                json.hash(&mut hasher);
            }
        }

        hasher.finish()
    }

    let hash1 = hash_system_prompt_and_tools();
    let hash2 = hash_system_prompt_and_tools();

    assert_eq!(hash1, hash2, "system prompt + tool schema prefix is not byte-stable across rebuilds");
}
