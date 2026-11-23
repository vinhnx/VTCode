use std::fs;

use super::normalize_direct_tool_mentions;
use super::{
    DirectToolInput, begin_direct_tool_turn, direct_subagent_spawn_args, direct_subagent_tool_name,
    direct_tool_fallback, direct_tool_skips_confirmations, parse_direct_tool_input,
};
use serde_json::json;
use tempfile::TempDir;
use vtcode_config::SubagentSource;
use vtcode_config::SubagentSpec;
use vtcode_config::core::agent::AgentHarnessConfig;
use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};
use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::registry::ToolRegistry;

#[tokio::test]
async fn direct_tool_turn_keeps_repeated_previews_visible() {
    let temp_dir = TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let harness_config = AgentHarnessConfig::default();
    let body = "v".repeat(256);
    let previews_per_turn = 40;

    let mut next_file = 0;
    let _first_turn = begin_direct_tool_turn(&harness_config);
    for _ in 0..previews_per_turn {
        let file_path = temp_dir.path().join(format!("preview-{next_file}.txt"));
        next_file += 1;
        fs::write(&file_path, &body).expect("write preview file");
        let result = registry
            .execute_tool_ref(tools::READ_FILE, &json!({ "path": file_path.to_string_lossy() }))
            .await
            .expect("read preview file");
        assert_eq!(result["content"].as_str(), Some(body.as_str()));
    }

    let exhausted_path = temp_dir.path().join(format!("preview-{next_file}.txt"));
    next_file += 1;
    fs::write(&exhausted_path, &body).expect("write exhausted preview file");
    let exhausted = registry
        .execute_tool_ref(tools::READ_FILE, &json!({ "path": exhausted_path.to_string_lossy() }))
        .await
        .expect("read exhausted preview file");
    assert!(exhausted.get("preview_budget_exhausted").is_none());
    assert_eq!(exhausted["content"].as_str(), Some(body.as_str()));

    let _second_turn = begin_direct_tool_turn(&harness_config);
    let fresh_path = temp_dir.path().join(format!("preview-{next_file}.txt"));
    fs::write(&fresh_path, &body).expect("write fresh preview file");
    let fresh = registry
        .execute_tool_ref(tools::READ_FILE, &json!({ "path": fresh_path.to_string_lossy() }))
        .await
        .expect("read fresh preview file");
    assert_eq!(fresh["content"].as_str(), Some(body.as_str()));
}

fn test_subagent_spec(name: &str) -> SubagentSpec {
    SubagentSpec {
        name: name.to_string(),
        description: "test".to_string(),
        prompt: String::new(),
        tools: Some(vec!["read_file".to_string()]),
        disallowed_tools: Vec::new(),
        model: None,
        color: None,
        reasoning_effort: None,
        permissions: AgentPermissionsConfig::new(PermissionDefault::Ask),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: vtcode_config::AgentMode::Subagent,
        max_turns: None,
        nickname_candidates: Vec::new(),
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: Vec::new(),
        source: SubagentSource::Builtin,
        file_path: None,
        warnings: Vec::new(),
        tool_policy_overrides: std::collections::BTreeMap::new(),
    }
}

#[test]
fn parses_bang_prefix_with_leading_whitespace() {
    let parsed = parse_direct_tool_input("   !echo hello").expect("direct tool");
    match parsed {
        DirectToolInput::Execute { tool_name, args, is_bang_prefix } => {
            assert_eq!(tool_name, tools::EXEC_COMMAND);
            assert_eq!(args["action"], "run");
            assert_eq!(args["command"], "echo hello");
            assert!(is_bang_prefix);
        }
        DirectToolInput::InvalidBang { .. } => {
            panic!("expected valid !-command to parse");
        }
    }
}

#[test]
fn direct_command_session_skips_confirmation_prompts() {
    assert!(direct_tool_skips_confirmations(tools::UNIFIED_EXEC));
    assert!(!direct_tool_skips_confirmations(tools::UNIFIED_FILE));
}

#[test]
fn rejects_invalid_bang_command_with_diagnosis() {
    let parsed = parse_direct_tool_input("! )(").expect("invalid command should be handled");
    match parsed {
        DirectToolInput::InvalidBang { command, diagnosis } => {
            assert_eq!(command, ")(");
            assert!(!diagnosis.trim().is_empty());
        }
        DirectToolInput::Execute { .. } => {
            panic!("expected invalid !-command to be rejected");
        }
    }
}

#[test]
fn rejects_empty_bang_command() {
    assert!(parse_direct_tool_input("!   ").is_none());
}

#[test]
fn parses_likely_run_typo_as_direct_command() {
    let parsed = parse_direct_tool_input("eun cargo check").expect("direct tool");
    match parsed {
        DirectToolInput::Execute { tool_name, args, is_bang_prefix } => {
            assert_eq!(tool_name, tools::EXEC_COMMAND);
            assert_eq!(args["command"], "cargo check");
            assert!(!is_bang_prefix);
        }
        DirectToolInput::InvalidBang { .. } => {
            panic!("expected typoed run prefix to parse as direct command");
        }
    }
}

#[test]
fn normalizes_direct_run_file_mentions_before_parsing() {
    let temp_dir = TempDir::new().expect("temp dir");
    let file_path = temp_dir.path().join("src").join("main.rs");
    fs::create_dir_all(file_path.parent().expect("parent")).expect("mkdir");
    fs::write(&file_path, "fn main() {}\n").expect("write file");

    let normalized = normalize_direct_tool_mentions("run cat @src/main.rs", temp_dir.path());
    assert_eq!(normalized, "run cat src/main.rs");

    let parsed = parse_direct_tool_input(&normalized).expect("direct tool");
    match parsed {
        DirectToolInput::Execute { args, .. } => {
            assert_eq!(args["command"], "cat src/main.rs");
        }
        DirectToolInput::InvalidBang { .. } => panic!("expected valid direct command"),
    }
}

#[test]
fn normalizes_bang_file_mentions_with_spaces() {
    let temp_dir = TempDir::new().expect("temp dir");
    let file_path = temp_dir.path().join("docs").join("file with spaces.md");
    fs::create_dir_all(file_path.parent().expect("parent")).expect("mkdir");
    fs::write(&file_path, "# doc\n").expect("write file");

    let normalized = normalize_direct_tool_mentions("!cat @\"docs/file with spaces.md\"", temp_dir.path());
    assert_eq!(normalized, "!cat 'docs/file with spaces.md'");

    let parsed = parse_direct_tool_input(&normalized).expect("direct tool");
    match parsed {
        DirectToolInput::Execute { args, is_bang_prefix, .. } => {
            assert_eq!(args["command"], "cat 'docs/file with spaces.md'");
            assert!(is_bang_prefix);
        }
        DirectToolInput::InvalidBang { .. } => panic!("expected valid bang command"),
    }
}

#[test]
fn leaves_npm_scoped_packages_unchanged_when_not_a_path() {
    let temp_dir = TempDir::new().expect("temp dir");
    let normalized = normalize_direct_tool_mentions("run bun add @types/node", temp_dir.path());
    assert_eq!(normalized, "run bun add @types/node");
}

#[test]
fn parses_direct_file_operation_read_with_mode_omitted() {
    let parsed = parse_direct_tool_input("file_operation read on /tmp/example.md with mode omitted")
        .expect("direct file_operation");
    match parsed {
        DirectToolInput::Execute { tool_name, args, .. } => {
            assert_eq!(tool_name, tools::UNIFIED_FILE);
            assert_eq!(args["action"], "read");
            assert_eq!(args["path"], "/tmp/example.md");
            assert_eq!(args["condense"], false);
        }
        DirectToolInput::InvalidBang { .. } => {
            panic!("expected file_operation read to parse");
        }
    }
}

#[test]
fn parses_run_prefixed_file_operation_read() {
    let parsed = parse_direct_tool_input("run file_operation read /tmp/example.md").expect("direct file_operation");
    match parsed {
        DirectToolInput::Execute { tool_name, args, .. } => {
            assert_eq!(tool_name, tools::UNIFIED_FILE);
            assert_eq!(args["action"], "read");
            assert_eq!(args["path"], "/tmp/example.md");
            assert_eq!(args["condense"], false);
        }
        DirectToolInput::InvalidBang { .. } => {
            panic!("expected file_operation read to parse");
        }
    }
}

#[test]
fn direct_subagent_spawn_args_defers_vague_shortcut_to_main_agent() {
    assert!(
        direct_subagent_spawn_args("run rust-engineer subagent", &[test_subagent_spec("rust-engineer")],).is_none()
    );
}

#[test]
fn direct_subagent_spawn_args_defers_single_word_follow_up_task() {
    assert!(
        direct_subagent_spawn_args("run rust-engineer subagent and report", &[test_subagent_spec("rust-engineer")],)
            .is_none()
    );
}

#[test]
fn direct_subagent_spawn_args_extracts_follow_up_task() {
    let args = direct_subagent_spawn_args(
        "use rust-engineer subagent and review code",
        &[test_subagent_spec("rust-engineer")],
    )
    .expect("direct subagent spawn");
    assert_eq!(args["agent_type"], "rust-engineer");
    assert_eq!(args["background"], false);
    assert_eq!(args["message"], "review code");
}

#[test]
fn direct_subagent_spawn_args_ignores_primary_specs() {
    let mut primary_plan = test_subagent_spec("plan");
    primary_plan.mode = vtcode_config::AgentMode::Primary;
    let child_plan = test_subagent_spec("plan");

    let args = direct_subagent_spawn_args("use plan subagent and review code", &[primary_plan, child_plan])
        .expect("direct subagent spawn");

    assert_eq!(args["agent_type"], "plan");
    assert_eq!(args["message"], "review code");
}

#[test]
fn direct_subagent_spawn_args_requires_explicit_background_request() {
    let args = direct_subagent_spawn_args(
        "spawn rust-engineer subagent in the background and review code",
        &[test_subagent_spec("rust-engineer")],
    )
    .expect("direct subagent spawn");
    assert_eq!(args["agent_type"], "rust-engineer");
    assert_eq!(args["background"], true);
    assert_eq!(args["message"], "review code");
}

#[test]
fn direct_subagent_spawn_args_uses_agent_initial_prompt_when_available() {
    let mut spec = test_subagent_spec("background-demo");
    spec.background = true;
    spec.initial_prompt = Some("Run the demo subprocess and report readiness.".to_string());
    let args = direct_subagent_spawn_args("run background-demo subagent", &[spec]).expect("direct subagent spawn");
    assert_eq!(args["agent_type"], "background-demo");
    assert_eq!(args["background"], true);
    assert_eq!(args["message"], "Run the demo subprocess and report readiness.");
}

#[test]
fn direct_subagent_spawn_args_falls_back_to_initial_prompt_for_vague_follow_up() {
    let mut spec = test_subagent_spec("background-demo");
    spec.background = true;
    spec.initial_prompt = Some("Run the demo subprocess and report readiness.".to_string());
    let args =
        direct_subagent_spawn_args("run background-demo subagent and demo", &[spec]).expect("direct subagent spawn");
    assert_eq!(args["agent_type"], "background-demo");
    assert_eq!(args["background"], true);
    assert_eq!(args["message"], "Run the demo subprocess and report readiness.");
}

#[test]
fn direct_subagent_spawn_args_defaults_background_specs_to_background_launches() {
    let mut spec = test_subagent_spec("background-demo");
    spec.background = true;
    spec.initial_prompt = Some("Run the demo subprocess and report readiness.".to_string());
    let args = direct_subagent_spawn_args("run background-demo subagent", &[spec]).expect("direct subagent spawn");
    assert_eq!(args["agent_type"], "background-demo");
    assert_eq!(args["background"], true);
}

#[test]
fn direct_subagent_tool_name_routes_background_specs_to_background_subprocess() {
    let mut spec = test_subagent_spec("background-demo");
    spec.background = true;
    let args = serde_json::json!({
        "agent_type": "background-demo",
        "message": "run demo",
        "background": true
    });

    assert_eq!(direct_subagent_tool_name(&args, &[spec]), tools::SPAWN_BACKGROUND_SUBPROCESS);
}

#[test]
fn direct_subagent_tool_name_keeps_foreground_launches_on_spawn_agent() {
    let mut spec = test_subagent_spec("background-demo");
    spec.background = true;
    let args = serde_json::json!({
        "agent_type": "background-demo",
        "message": "run demo",
        "background": false
    });

    assert_eq!(direct_subagent_tool_name(&args, &[spec]), tools::SPAWN_AGENT);
}

#[test]
fn direct_tool_fallback_reads_latest_tool_payload() {
    let history = vec![uni::Message::tool_response(
        "direct_spawn_agent_1".to_string(),
        serde_json::json!({
            "error": "boom",
            "fallback_tool": tools::SPAWN_BACKGROUND_SUBPROCESS,
            "fallback_tool_args": {
                "agent_type": "background-demo",
                "message": "run demo"
            }
        })
        .to_string(),
    )];

    assert_eq!(
        direct_tool_fallback(&history, "direct_spawn_agent_1", tools::SPAWN_AGENT),
        Some((
            tools::SPAWN_BACKGROUND_SUBPROCESS.to_string(),
            serde_json::json!({
                "agent_type": "background-demo",
                "message": "run demo"
            }),
        ))
    );
}
