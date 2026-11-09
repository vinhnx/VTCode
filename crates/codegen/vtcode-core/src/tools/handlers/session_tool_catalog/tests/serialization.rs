use super::*;

#[test]
fn model_tool_serialization_keeps_output_cap_separate_from_approval_policy() {
    let registration = registration(tools::EXEC_COMMAND)
        .with_description("Run the policy surface test")
        .with_parameter_schema(empty_object_schema())
        .with_permission(ToolPolicy::Allow);
    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
    let definitions = catalog.model_tools(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode),
    );
    let tool = definitions.first().expect("model tool definition");
    let serialized = serde_json::to_value(tool).expect("serialize model tool definition");

    assert_eq!(
        tool.function.as_ref().map(|function| function.description.as_str()),
        Some("Run the policy surface test")
    );
    assert_eq!(
        tool.function
            .as_ref()
            .map(|function| function.parameters["properties"]["max_output_tokens"]["default"].clone()),
        Some(json!(vtcode_utility_tool_specs::DEFAULT_MAX_OUTPUT_TOKENS))
    );
    for key in [
        "approval_policy",
        "default_permission",
        "permission",
        "tool_policy",
        "allow_patterns",
        "deny_patterns",
    ] {
        assert!(!contains_json_key(&serialized, key), "approval metadata leaked into model schema: {key}");
    }
}
#[test]
fn configured_spec_preserves_json_schema_field_names() {
    let registration = registration("schema_contract_tool")
        .with_description("schema contract")
        .with_parameter_schema(json!({
            "type": "object",
            "properties": {
                "input": {"type": "string"}
            },
            "additionalProperties": false,
            "anyOf": [
                {"required": ["input"]},
                {"required": ["patch"]}
            ]
        }));

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
    let entry = &catalog.entries()[0];
    let ToolSpec::Function(tool) = &entry.configured_spec.spec else {
        panic!("expected function tool spec");
    };

    let serialized = serde_json::to_value(&tool.parameters).expect("serialize parameters");
    assert_eq!(serialized["additionalProperties"], Value::Bool(false));
    assert!(serialized["anyOf"].is_array());
    assert!(serialized.get("additional_properties").is_none());
    assert!(serialized.get("any_of").is_none());
}
#[test]
fn compact_parameters_preserves_property_named_description() {
    let schema = RequestUserInputTool.parameter_schema().expect("request_user_input schema");

    let compacted = compact_parameters(schema, ToolDocumentationMode::Progressive);
    let description_property =
        &compacted["properties"]["questions"]["items"]["properties"]["options"]["items"]["properties"]["description"];

    assert!(description_property.is_object());
    assert_eq!(
        compacted["properties"]["questions"]["items"]["properties"]["options"]["items"]["required"],
        json!(["label", "description"])
    );
}
#[test]
fn cached_projections_preserve_schema_and_deferral_across_documentation_modes() {
    let exec_command = registration(tools::EXEC_COMMAND)
        .with_description("Run a command. Use the workspace shell safely.")
        .with_parameter_schema(json!({
            "type": "object",
            "properties": {
                "cmd": {"type": "string", "description": "Command to run."}
            },
            "required": ["cmd"]
        }));
    let mcp_tool = registration("mcp::context7::search")
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description(
            "Search the documentation server. This description is intentionally longer than minimal mode.",
        )
        .with_parameter_schema(json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Documentation query."}
            }
        }))
        .with_aliases(["mcp__context7__search"]);
    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![exec_command, mcp_tool]);

    for documentation_mode in [
        ToolDocumentationMode::Minimal,
        ToolDocumentationMode::Progressive,
        ToolDocumentationMode::Full,
    ] {
        let config = SessionToolsConfig::full_public(
            SessionSurface::AgentRunner,
            CapabilityLevel::CodeSearch,
            documentation_mode,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(DeferredToolPolicy::client_local(Vec::new()));

        let expected_schema = catalog
            .entries()
            .iter()
            .filter(|entry| entry.is_visible(&config))
            .map(|entry| ToolSchemaEntry {
                name: entry.public_name.clone(),
                description: compact_tool_description(
                    entry.description.as_str(),
                    documentation_mode,
                    entry.max_description_length,
                ),
                parameters: compact_parameters(entry.parameters.clone(), documentation_mode),
            })
            .collect::<Vec<_>>();
        assert_eq!(catalog.schema_entries(config.clone()), expected_schema);

        let visible_entries = catalog
            .entries()
            .iter()
            .filter(|entry| entry.is_visible(&config))
            .collect::<Vec<_>>();
        // Rounds up like the canonical byte-length heuristic (see
        // `vtcode_commons::tokens::heuristic_tokens_from_byte_len`) but keeps
        // the division literal so this oracle stays independent of it.
        let estimated_schema_tokens = expected_schema
            .iter()
            .map(|entry| {
                serde_json::to_string(entry)
                    .expect("serialize expected schema")
                    .len()
                    .div_ceil(4)
            })
            .sum();
        let deferable_tool_count = visible_entries
            .iter()
            .filter(|entry| should_defer_tool_loading(entry, &config))
            .count();
        let expose_tools_directly = config.deferred_tool_policy.is_client_local()
            && !catalog_would_benefit_from_deferral(
                visible_entries
                    .iter()
                    .any(|entry| matches!(entry.source, ToolCatalogSource::Mcp)),
                deferable_tool_count,
                estimated_schema_tokens,
            );
        let definitions = catalog.model_tools(config.clone());

        for entry in visible_entries {
            let expected_deferred = should_defer_tool_loading(entry, &config) && !expose_tools_directly;
            let definition = definitions
                .iter()
                .find(|tool| tool.function_name() == entry.public_name)
                .unwrap_or_else(|| panic!("missing model definition for {}", entry.public_name));
            let actual_deferred = definition.defer_loading == Some(true);
            assert_eq!(actual_deferred, expected_deferred, "deferral changed for {}", entry.public_name);
        }

        assert_eq!(catalog.model_tools(config.clone()), definitions);
        assert!(definitions.iter().any(|tool| tool.function_name() == tools::EXEC_COMMAND));
    }
}
#[test]
fn hosted_deferral_skips_schema_estimation_without_changing_tool_output() {
    let registrations = (0..(DIRECT_TOOL_EXPOSURE_THRESHOLD + 4))
        .map(|index| {
            ToolRegistration::new(format!("hosted_catalog_tool_{index}"), CapabilityLevel::CodeSearch, false, |_, _| {
                Box::pin(async { Ok(Value::Null) })
            })
            .with_description(format!("Search the hosted catalog for item {index}."))
            .with_parameter_schema(json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query." }
                },
                "required": ["query"]
            }))
        })
        .collect();
    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let config = SessionToolsConfig::full_public(
        SessionSurface::AgentRunner,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Progressive,
        ToolModelCapabilities::default(),
    )
    .with_tool_profile(ToolProfile::AdvancedVtCode)
    .with_deferred_tool_policy(DeferredToolPolicy::openai_hosted(Vec::new()));

    let expected_schema = catalog.schema_entries(config.clone());
    let definitions = catalog.model_tools(config.clone());

    assert_eq!(definitions.len(), expected_schema.len() + 1, "hosted search must be added to the catalog");
    assert!(definitions.iter().any(ToolDefinition::is_tool_search));
    for schema in expected_schema {
        let definition = definitions
            .iter()
            .find(|tool| tool.function_name() == schema.name)
            .unwrap_or_else(|| panic!("missing hosted definition for {}", schema.name));
        let function = definition.function.as_ref().expect("hosted catalog function definition");
        assert_eq!(function.name, schema.name);
        assert_eq!(function.description, schema.description);
        assert_eq!(function.parameters, schema.parameters);
        assert_eq!(definition.defer_loading, Some(true));
    }

    let estimates_initialized = catalog.visible_entry_indices(&config).into_iter().any(|index| {
        let entry = &catalog.entries[index];
        catalog
            .projection(index, entry, config.documentation_mode)
            .has_serialized_token_estimate()
    });
    assert!(!estimates_initialized, "hosted policies must not serialize schema-token estimates");
}
#[test]
fn eager_and_client_local_policies_still_estimate_schema_tokens_when_needed() {
    for deferred_tool_policy in [
        DeferredToolPolicy::default(),
        DeferredToolPolicy::client_local(Vec::new()),
    ] {
        let registrations = (0..(DIRECT_TOOL_EXPOSURE_THRESHOLD + 1))
            .map(|index| {
                ToolRegistration::new(
                    format!("estimated_catalog_tool_{index}"),
                    CapabilityLevel::CodeSearch,
                    false,
                    |_, _| Box::pin(async { Ok(Value::Null) }),
                )
                .with_description(format!("Estimate this catalog entry {index}."))
                .with_parameter_schema(json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } }
                }))
            })
            .collect();
        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let config = SessionToolsConfig::full_public(
            SessionSurface::AgentRunner,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Progressive,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(deferred_tool_policy);

        let _ = catalog.model_tools(config.clone());
        let estimates_initialized = catalog.visible_entry_indices(&config).into_iter().any(|index| {
            let entry = &catalog.entries[index];
            catalog
                .projection(index, entry, config.documentation_mode)
                .has_serialized_token_estimate()
        });
        assert!(estimates_initialized, "the active policy must retain its schema-token decision");
    }
}
#[test]
fn anthropic_policy_injects_tool_search_and_defers_non_core_tools() {
    let exec_command = registration(tools::EXEC_COMMAND)
        .with_description("Run command")
        .with_parameter_schema(empty_object_schema());
    let apply_patch = registration(tools::APPLY_PATCH)
        .with_llm_visibility(false)
        .with_description("Apply patch")
        .with_parameter_schema(apply_patch_parameters())
        .with_behavior(ToolBehavior::apply_patch(ToolMutationModel::Mutating, false, true));
    let mcp_tool = registration("mcp::context7::search")
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description("search docs")
        .with_parameter_schema(empty_object_schema())
        .with_aliases(["mcp__context7__search"]);

    let mut registrations = vec![exec_command, apply_patch, mcp_tool];
    for index in 0..DIRECT_TOOL_EXPOSURE_THRESHOLD {
        let name: &'static str = Box::leak(format!("mcp::context7::resolve_{index}").into_boxed_str());
        let alias = format!("mcp__context7__resolve_{index}");
        registrations.push(
            registration(name)
                .with_catalog_source(ToolCatalogSource::Mcp)
                .with_llm_visibility(false)
                .with_description(format!("resolve docs {index}"))
                .with_parameter_schema(empty_object_schema())
                .with_aliases([alias]),
        );
    }

    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let definitions = catalog.model_tools(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(DeferredToolPolicy::anthropic(ToolSearchAlgorithm::Regex, Vec::new())),
    );

    assert!(
        definitions
            .iter()
            .any(|tool| tool.tool_type == "tool_search_tool_regex_20251119"),
        "anthropic tool search should be injected when deferred tools exist"
    );
    let exec_tool = definitions
        .iter()
        .find(|tool| tool.function_name() == tools::EXEC_COMMAND)
        .expect("exec_command should be present");
    assert_eq!(exec_tool.defer_loading, None);

    let apply_patch = definitions
        .iter()
        .find(|tool| tool.function_name() == tools::APPLY_PATCH)
        .expect("apply_patch fallback should be present");
    assert_eq!(apply_patch.defer_loading, None);

    let mcp_tool = definitions
        .iter()
        .find(|tool| tool.function_name() == "mcp__context7__search")
        .expect("mcp tool should be present");
    assert_eq!(mcp_tool.defer_loading, Some(true));
}
