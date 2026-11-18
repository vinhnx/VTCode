use super::*;
use crate::config::constants::models;
use vtcode_config::core::permissions::PermissionDefault;
use vtcode_config::{AgentMode, IsolationMode, McpProviderConfig, SubagentMcpServer, SubagentSource, SubagentSpec};

fn test_subagent_spec() -> SubagentSpec {
    SubagentSpec {
        name: "test-agent".to_string(),
        description: "Test agent".to_string(),
        prompt: "Do the thing".to_string(),
        tools: None,
        disallowed_tools: Vec::new(),
        model: None,
        color: None,
        reasoning_effort: None,
        permissions: AgentPermissionsConfig::new(PermissionDefault::Ask),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: AgentMode::default(),
        max_turns: None,
        nickname_candidates: Vec::new(),
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

#[test]
fn child_config_uses_lightweight_default_profile() {
    let mut parent = VTCodeConfig::default();
    parent.agent.system_prompt_mode = SystemPromptMode::Specialized;
    parent.agent.tool_documentation_mode = ToolDocumentationMode::Full;
    parent.mcp.providers.push(McpProviderConfig::default());

    let spec = test_subagent_spec();
    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);

    assert_eq!(
        child.agent.system_prompt_mode,
        SystemPromptMode::Minimal,
        "subagent should default to Minimal system prompt mode"
    );
    assert_eq!(
        child.agent.tool_documentation_mode,
        ToolDocumentationMode::Minimal,
        "subagent should default to Minimal tool documentation mode"
    );
    assert!(
        child.mcp.providers.is_empty(),
        "subagent should not inherit parent MCP providers unless explicitly requested"
    );
}

#[test]
fn child_config_attaches_explicit_mcp_servers() {
    let mut parent = VTCodeConfig::default();
    parent.mcp.providers.push(McpProviderConfig::default());

    let mut spec = test_subagent_spec();
    parent.mcp.providers[0].name = "context7".to_string();
    spec.mcp_servers = vec![SubagentMcpServer::Named("context7".to_string())];

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);

    assert_eq!(child.mcp.providers.len(), 1);
    assert_eq!(child.mcp.providers[0].name, "context7");
}

/// Guard-rail test for the extracted MCP-resolution helper: it must
/// isolatedly drop every parent provider unless the spec names it.
#[test]
fn resolve_child_mcp_providers_drops_unnamed_parent_servers() {
    let mut parent = VTCodeConfig::default();
    parent.mcp.providers.push(McpProviderConfig::default());

    let spec = test_subagent_spec();
    let runtime = ResolvedAgentRuntimeView::from_spec(&spec);

    let providers = resolve_child_mcp_providers(&parent, &runtime);
    assert!(providers.is_empty(), "parent MCP providers must not leak into the child unless explicitly named");
}

#[test]
fn resolve_child_mcp_providers_keeps_named_parent_server() {
    let mut parent = VTCodeConfig::default();
    parent.mcp.providers.push(McpProviderConfig::default());
    parent.mcp.providers[0].name = "context7".to_string();

    let mut spec = test_subagent_spec();
    spec.mcp_servers = vec![SubagentMcpServer::Named("context7".to_string())];
    let runtime = ResolvedAgentRuntimeView::from_spec(&spec);

    let providers = resolve_child_mcp_providers(&parent, &runtime);
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].name, "context7");
}

#[test]
fn child_mcp_inline_definition_replaces_named_parent_and_skips_malformed_entries() {
    let mut parent = VTCodeConfig::default();
    parent.mcp.providers.push(
        serde_json::from_value(serde_json::json!({
            "name": "context7", "command": "parent-mcp", "args": ["parent"]
        }))
        .expect("parent provider"),
    );
    let mut spec = test_subagent_spec();
    spec.mcp_servers = vec![
        SubagentMcpServer::Named("context7".to_string()),
        SubagentMcpServer::Inline(BTreeMap::from([
            ("context7".to_string(), serde_json::json!({"type": "stdio", "command": "child-mcp"})),
            ("invalid".to_string(), serde_json::json!({"command": "bad", "args": 42})),
            ("nonobject".to_string(), serde_json::json!("bad")),
        ])),
    ];
    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);
    let providers = serde_json::to_value(&child.mcp.providers).expect("serialize providers");
    assert_eq!(providers.as_array().expect("providers array").len(), 1);
    assert_eq!(providers[0]["name"], "context7");
    assert_eq!(providers[0]["command"], "child-mcp");
    assert_eq!(providers[0]["args"], serde_json::json!([]));
    let original = serde_json::to_value(&parent.mcp.providers).expect("serialize parent");
    assert_eq!(original[0]["command"], "parent-mcp");
    assert_eq!(original[0]["args"], serde_json::json!(["parent"]));
}

/// Phase 4.3 guard-rail: a default subagent's composed system prompt must
/// bootstrap materially cheaper than its parent's. The lightweight profile
/// (`apply_subagent_lightweight_profile`) forces `Minimal` system-prompt and
/// tool-documentation modes; this measures the *actual* composed prompt
/// token estimate (the dominant bootstrap cost) rather than just the config
/// shape, which `child_config_uses_lightweight_default_profile` already
/// covers. Cache prefix-stable conditions (no temporal context, no project
/// docs) are used so the ratio reflects only the mode-driven difference.
#[tokio::test]
async fn default_subagent_bootstrap_tokens_are_materially_below_parent() {
    use crate::config::constants::tools;
    use crate::prompts::context::PromptContext;
    use crate::prompts::system::compose_system_instruction_with_report;
    use crate::skills::model::{SkillMetadata, SkillScope};
    use std::path::PathBuf;

    let workspace = tempfile::TempDir::new().expect("workspace");

    let mut parent = VTCodeConfig::default();
    parent.agent.system_prompt_mode = SystemPromptMode::Default;
    parent.agent.tool_documentation_mode = ToolDocumentationMode::Progressive;
    parent.agent.include_temporal_context = false;
    parent.agent.include_working_directory = true;
    parent.agent.instruction_max_bytes = 0;

    // A non-trivial tool/skill surface so Progressive tool guidelines and
    // the base contract are exercised on both sides.
    let mut ctx = PromptContext::default();
    ctx.add_tool(tools::CODE_SEARCH.to_string());
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
    ctx.set_current_directory(PathBuf::from("/workspace"));

    let (_parent_text, parent_report) =
        compose_system_instruction_with_report(workspace.path(), Some(&parent), Some(&ctx)).await;

    let spec = test_subagent_spec();
    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);

    // The lightweight profile is the contract under test.
    assert_eq!(child.agent.system_prompt_mode, SystemPromptMode::Minimal);
    assert_eq!(child.agent.tool_documentation_mode, ToolDocumentationMode::Minimal,);
    // The deterministic env settings must be inherited so the ratio
    // reflects only the mode-driven difference, not environmental noise.
    assert!(!child.agent.include_temporal_context);
    assert_eq!(child.agent.instruction_max_bytes, 0);

    let (_child_text, child_report) =
        compose_system_instruction_with_report(workspace.path(), Some(&child), Some(&ctx)).await;

    let parent_tokens = parent_report.token_estimate;
    let child_tokens = child_report.token_estimate;
    assert!(parent_tokens > 0, "parent prompt must be non-empty for a meaningful ratio");
    // The composed system-prompt reduction from the lightweight profile is
    // bounded by the shared base-contract content. Universal runtime
    // guidance is deliberately shared by every profile, so the dominant
    // subagent bootstrap win remains the tool-schema/MCP drop, guarded by
    // `mcp_deferral_keeps_first_request_wire_payload_near_baseline` and
    // `child_config_uses_lightweight_default_profile`. This test pins the
    // system-prompt portion: the child must be materially (and strictly)
    // cheaper, and a regression that bloats the Minimal profile above 85% of
    // the Default composed prompt must fail here. The bar is 85% rather
    // than 80% because lean cuts to the parent (shared contract/operating
    // lines) narrow the ratio without bloating the child.
    assert!(
        child_tokens < parent_tokens,
        "default subagent composed prompt ({child_tokens}) must be strictly cheaper \
         than parent ({parent_tokens})"
    );
    assert!(
        child_tokens * 20 <= parent_tokens * 17,
        "default subagent composed prompt ({child_tokens}) must be <= 85% of parent \
         ({parent_tokens}); a bloat of the Minimal profile is a regression"
    );
}
