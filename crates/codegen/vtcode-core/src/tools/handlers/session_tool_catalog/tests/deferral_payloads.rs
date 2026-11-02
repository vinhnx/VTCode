use super::*;

/// Phase 1.1 / 2.x regression: with several MCP servers attached, the
/// client-local deferred-loading path must keep the first-request wire
/// payload near the no-MCP baseline (because MCP schemas are omitted),
/// while eager exposure would balloon it. This proves the win the
/// `DIRECT_TOOL_EXPOSURE_THRESHOLD` / `has_mcp_tools` gating delivers.
#[test]
fn mcp_deferral_keeps_first_request_wire_payload_near_baseline() {
    let core_registrations = || {
        vec![
            registration(tools::EXEC_COMMAND)
                .with_description("Run a shell command in the workspace sandbox.")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::APPLY_PATCH)
                .with_llm_visibility(false)
                .with_description("Apply a structured patch to files.")
                .with_parameter_schema(apply_patch_parameters())
                .with_behavior(ToolBehavior::apply_patch(ToolMutationModel::Mutating, false, true)),
            registration(tools::MCP_SEARCH_TOOLS)
                .with_description("Search across deferred MCP tools by keyword.")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::CODE_SEARCH)
                .with_description("Search the codebase symbol index.")
                .with_parameter_schema(empty_object_schema()),
        ]
    };

    // Five simulated MCP servers, four tools each = 20 deferred MCP tools.
    let mut registrations = core_registrations();
    for server in ["context7", "filesystem", "github", "slack", "postgres"] {
        for index in 0..4 {
            let tool = format!("op_{index}");
            let description =
                format!("{server} operation {index}: query and mutate {server} resources with paging and filters.");
            registrations.push(mcp_server_tool_registration(server, &tool, &description));
        }
    }
    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);

    let make_config = |policy: DeferredToolPolicy| {
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(policy)
    };

    // Eager: no deferral, so all 20 MCP schemas travel on the first request.
    let eager_tokens = on_wire_schema_tokens(&catalog, make_config(DeferredToolPolicy::default()));
    // Client-local deferral: MCP tools are omitted from the wire payload.
    let deferred_tokens = on_wire_schema_tokens(&catalog, make_config(DeferredToolPolicy::client_local(Vec::new())));
    // Baseline: the same core tools, no MCP servers, under deferral.
    let baseline_catalog = SessionToolCatalog::rebuild_from_registrations(core_registrations());
    let baseline_tokens =
        on_wire_schema_tokens(&baseline_catalog, make_config(DeferredToolPolicy::client_local(Vec::new())));

    assert!(
        eager_tokens >= deferred_tokens * 3,
        "eager payload ({eager_tokens}) must be at least 3x the deferred payload \
         ({deferred_tokens}) -- otherwise the MCP schema tax deferral removes is not real"
    );
    assert!(
        deferred_tokens <= baseline_tokens * 5 / 4,
        "with 5 MCP servers under deferral, the first-request payload ({deferred_tokens}) \
         must stay within 25% of the no-MCP baseline ({baseline_tokens})"
    );
    assert!(
        deferred_tokens < eager_tokens,
        "deferral must shrink the first-request wire payload: \
         eager={eager_tokens} deferred={deferred_tokens}"
    );
}
/// Phase 7.2: the advisory warning condition for "deferred loading is
/// disabled but the catalog would benefit from it" is pure logic in
/// `catalog_would_benefit_from_deferral`. A disabled policy + a catalog
/// that would defer (MCP present, over the count threshold, or over the
/// schema-token budget) is the only non-noisy warning case -- the count/
/// budget thresholds *triggering* deferral when enabled is correct
/// behavior, not a warning condition.
#[test]
fn catalog_would_benefit_from_deferral_detects_each_trigger() {
    // Small builtin-only catalog: no benefit (deferral would not engage).
    assert!(
        !catalog_would_benefit_from_deferral(false, 3, 500),
        "a small builtin-only catalog does not benefit from deferral"
    );
    // Any MCP tool present -> benefit (MCP schemas are the dominant cost).
    assert!(catalog_would_benefit_from_deferral(true, 1, 100), "any MCP tool means deferral would engage");
    // At the count threshold -> benefit.
    assert!(
        catalog_would_benefit_from_deferral(false, DIRECT_TOOL_EXPOSURE_THRESHOLD, 500),
        "meeting the count threshold means deferral would engage"
    );
    // Just under the count threshold but over the token budget -> benefit
    // (the single-large-server backstop).
    assert!(
        catalog_would_benefit_from_deferral(
            false,
            DIRECT_TOOL_EXPOSURE_THRESHOLD - 1,
            DIRECT_TOOL_EXPOSURE_TOKEN_BUDGET + 1,
        ),
        "exceeding the schema-token budget means deferral would engage"
    );
    // Exactly at the token budget (<=, not >) and under the count threshold,
    // no MCP -> no benefit (boundary matches the `<=` in `model_tools`).
    assert!(
        !catalog_would_benefit_from_deferral(
            false,
            DIRECT_TOOL_EXPOSURE_THRESHOLD - 1,
            DIRECT_TOOL_EXPOSURE_TOKEN_BUDGET,
        ),
        "at exactly the token budget and below the count threshold, \
        deferral does not engage (boundary is <=)"
    );
}
#[test]
fn planning_read_tools_stay_core_with_mcp_deferral() {
    let registrations = [
        tools::EXEC_COMMAND,
        tools::READ_FILE,
        tools::LIST_FILES,
        tools::GREP_FILE,
        tools::CODE_SEARCH,
        tools::REQUEST_USER_INPUT,
    ]
    .into_iter()
    .map(|name| {
        registration(name)
            .with_description("planning read tool")
            .with_parameter_schema(empty_object_schema())
    })
    .chain(std::iter::once(
        registration("mcp::context7::search")
            .with_catalog_source(ToolCatalogSource::Mcp)
            .with_llm_visibility(false)
            .with_description("search docs")
            .with_parameter_schema(empty_object_schema())
            .with_aliases(["mcp__context7__search"]),
    ))
    .collect::<Vec<_>>();
    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let planning_config = SessionToolsConfig::full_public(
        SessionSurface::AgentRunner,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    )
    .with_planning_active(true)
    .with_deferred_tool_policy(DeferredToolPolicy::client_local(Vec::new()));
    let planning_definitions = catalog.model_tools(planning_config);
    for tool in [
        tools::READ_FILE,
        tools::LIST_FILES,
        tools::GREP_FILE,
        tools::CODE_SEARCH,
    ] {
        let definition = planning_definitions
            .iter()
            .find(|definition| definition.function_name() == tool)
            .unwrap_or_else(|| panic!("missing planning definition for {tool}"));
        assert_eq!(definition.defer_loading, None, "{tool} must stay on wire in planning even with MCP deferral");
    }

    let exec_config = SessionToolsConfig::full_public(
        SessionSurface::AgentRunner,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    )
    .with_deferred_tool_policy(DeferredToolPolicy::client_local(Vec::new()));
    let grep_entry = catalog
        .entries()
        .iter()
        .find(|entry| entry.public_name == tools::GREP_FILE)
        .expect("missing grep catalog entry");
    // Structured search is always-eager (session-efficiency 2026-09-28):
    // deferred search pushed models to shell out via exec_command.
    assert!(!should_defer_tool_loading(grep_entry, &exec_config), "grep_file stays eager outside planning");

    let interactive_planning_config = SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    )
    .with_planning_active(true)
    .with_deferred_tool_policy(DeferredToolPolicy::client_local(Vec::new()));
    let interactive_definitions = catalog.model_tools(interactive_planning_config);
    for tool in [tools::GREP_FILE, tools::CODE_SEARCH] {
        let definition = interactive_definitions
            .iter()
            .find(|definition| definition.function_name() == tool)
            .unwrap_or_else(|| panic!("missing interactive planning definition for {tool}"));
        assert_eq!(
            definition.defer_loading, None,
            "{tool} must stay on wire for Interactive planning with MCP deferral (session-20260923)"
        );
    }
}
#[test]
fn lean_defaults_defer_planner_and_skills_tools_outside_planning() {
    let always_eager = [
        tools::EXEC_COMMAND,
        tools::WRITE_STDIN,
        tools::SEARCH_TOOLS,
        tools::APPLY_PATCH,
        tools::CODE_SEARCH,
        tools::GREP_FILE,
        // Workflow-coordination control-plane tools stay directly callable in
        // every mode: full-auto references them in `allowed_tools` and OpenAI
        // rejects deferred references with 400 "not found in 'tools'".
        tools::TASK_TRACKER,
        tools::START_PLANNING,
    ];
    let deferrable = [
        tools::AGENT,
        tools::LIST_SKILLS,
        tools::LOAD_SKILL,
        tools::LOAD_SKILL_RESOURCE,
    ];
    let registrations = always_eager
        .into_iter()
        .chain(deferrable)
        .map(|name| {
            registration(name)
                .with_description("tool")
                .with_parameter_schema(empty_object_schema())
        })
        .collect::<Vec<_>>();
    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let policy = DeferredToolPolicy::client_local(Vec::new());
    let caps = ToolModelCapabilities { supports_apply_patch_tool: true };
    let idle_config = SessionToolsConfig::full_public(
        SessionSurface::AgentRunner,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        caps,
    )
    .with_deferred_tool_policy(policy.clone());

    for name in always_eager {
        let entry = catalog
            .entries()
            .iter()
            .find(|entry| entry.public_name == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        assert!(!should_defer_tool_loading(entry, &idle_config), "{name} must stay eager");
    }
    for name in deferrable {
        let entry = catalog
            .entries()
            .iter()
            .find(|entry| entry.public_name == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        assert!(should_defer_tool_loading(entry, &idle_config), "{name} must defer outside planning");
    }

    let planning_config = SessionToolsConfig::full_public(
        SessionSurface::AgentRunner,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        caps,
    )
    .with_planning_active(true)
    .with_deferred_tool_policy(policy);
    for name in [tools::TASK_TRACKER, tools::START_PLANNING] {
        let entry = catalog
            .entries()
            .iter()
            .find(|entry| entry.public_name == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        assert!(
            !should_defer_tool_loading(entry, &planning_config),
            "{name} must stay eager while planning is active"
        );
    }
}
#[test]
fn workflow_coordination_tools_stay_eager_under_hosted_deferral_outside_planning() {
    // Regression guard for full-auto turn 1 on OpenAI (gpt-6-luna,
    // gpt-5.6-sol): `task_tracker`/`start_planning` deferred outside planning
    // made the Responses `allowed_tools` reference unresolvable, and the
    // backend rejected the request with 400 "Tool choice 'task_tracker' not
    // found in 'tools' parameter". Workflow-coordination tools bypass the
    // full-auto allow-list, so they must stay directly callable in every
    // mode without a prior discovery round-trip.
    let registrations = [tools::TASK_TRACKER, tools::START_PLANNING, tools::AGENT]
        .into_iter()
        .map(|name| {
            registration(name)
                .with_description("tool")
                .with_parameter_schema(empty_object_schema())
        })
        .collect::<Vec<_>>();
    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let policy = DeferredToolPolicy::openai_hosted(Vec::<String>::new());
    let caps = ToolModelCapabilities { supports_apply_patch_tool: true };
    // Real full-auto runs under the AdvancedVtCode profile, where these tools
    // are visible: assert through `model_tools` (not just raw entries) so the
    // guard also fails if the tools ever drop out of the visible catalog.
    let execution_config = SessionToolsConfig::full_public(
        SessionSurface::AgentRunner,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        caps,
    )
    .with_tool_profile(ToolProfile::AdvancedVtCode)
    .with_deferred_tool_policy(policy);

    for name in [tools::TASK_TRACKER, tools::START_PLANNING] {
        let entry = catalog
            .entries()
            .iter()
            .find(|entry| entry.public_name == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        assert!(
            !should_defer_tool_loading(entry, &execution_config),
            "{name} must stay eager outside planning so allowed_tools references resolve"
        );
    }

    // Non-workflow tools keep the lean-defaults deferral outside planning.
    let entry = catalog
        .entries()
        .iter()
        .find(|entry| entry.public_name == tools::AGENT)
        .expect("missing agent");
    assert!(should_defer_tool_loading(entry, &execution_config), "agent must still defer outside planning");

    // End-to-end through the real pipeline: the workflow tools must reach the
    // wire catalog as eager definitions (no `defer_loading` marker), with the
    // hosted discovery tool present for the still-deferred `agent` tool.
    let wire = catalog.model_tools(execution_config);
    for name in [tools::TASK_TRACKER, tools::START_PLANNING] {
        let tool = wire
            .iter()
            .find(|tool| tool.function_name() == name)
            .unwrap_or_else(|| panic!("{name} must be visible in model_tools under AdvancedVtCode"));
        assert!(tool.defer_loading.is_none(), "{name} must be eager on the wire so allowed_tools references resolve");
    }
    assert!(
        wire.iter().any(|tool| tool.is_tool_search()),
        "hosted tool_search must be injected while deferrable tools remain"
    );
}
